#!/usr/bin/env bash
# =============================================================================
# run-e2e-fleet.sh
# Esegue l'intero test end-to-end multi-device con un solo comando:
#   preflight → build/deploy → port-forward → rete → test fleet → diagnostica
#
# Verifica che N device Renode mantengano N sessioni TLS DISTINTE nel gateway
# e ricevano un deploy WASM ciascuno (il test che falliva in §8.2).
#
# Uso:
#   ./scripts/run-e2e-fleet.sh                 # 3 device, build+deploy incrementale
#   ./scripts/run-e2e-fleet.sh -n 5            # 5 device
#   ./scripts/run-e2e-fleet.sh --check         # solo preflight, non tocca nulla
#   ./scripts/run-e2e-fleet.sh --skip-build    # riusa le immagini già deployate
#   ./scripts/run-e2e-fleet.sh --full-deploy   # deploy completo (deploy-k3s.sh)
#   ./scripts/run-e2e-fleet.sh --cleanup       # solo teardown (device, container, rete)
#
# Richiede: k3s, docker, sudo (per la rete), firmware Zephyr compilato.
# =============================================================================

set -uo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# --- Configurazione ---------------------------------------------------------
N="${N:-3}"
NAMESPACE="${NAMESPACE:-wasmbed}"
API_BASE="${API_BASE:-http://127.0.0.1:3001}"
GATEWAY_HTTP="${GATEWAY_HTTP:-http://127.0.0.1:8080}"
PREFIX="${PREFIX:-fleet-device}"
RENODE_IMAGE="${RENODE_IMAGE:-antmicro/renode:nightly}"
REGISTRY="${REGISTRY:-localhost:5000}"

MODE="run"; SKIP_BUILD=0; FULL_DEPLOY=0
while [ $# -gt 0 ]; do
  case "$1" in
    -n|--devices) N="$2"; shift 2 ;;
    --check)       MODE="check"; shift ;;
    --cleanup)     MODE="cleanup"; shift ;;
    --skip-build)  SKIP_BUILD=1; shift ;;
    --full-deploy) FULL_DEPLOY=1; shift ;;
    -h|--help)     sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "Opzione sconosciuta: $1 (usa --help)"; exit 2 ;;
  esac
done

G='\033[0;32m'; R='\033[0;31m'; Y='\033[1;33m'; B='\033[0;34m'; NC='\033[0m'
phase() { echo ""; echo -e "${B}=== $* ===${NC}"; }
ok()    { echo -e "  ${G}✅${NC} $*"; }
warn()  { echo -e "  ${Y}⚠️ ${NC} $*"; }
err()   { echo -e "  ${R}❌${NC} $*"; }
die()   { err "$*"; echo ""; echo "Interrotto."; exit 1; }

# --- Workspace Zephyr ------------------------------------------------------
# Gli script .resc vengono scritti in ZEPHYR_WORKSPACE, che l'api-server monta
# come hostPath: se il valore qui non coincide con quello del deployment, Renode
# legge script vecchi o non li trova. Fonte di verità = il deployment stesso.
WORKSPACE_SOURCE="default"
if [ -n "${ZEPHYR_WORKSPACE:-}" ]; then
  WORKSPACE_SOURCE="variabile d'ambiente"
else
  ZEPHYR_WORKSPACE=$(kubectl -n "$NAMESPACE" get deploy wasmbed-api-server \
    -o jsonpath='{.spec.template.spec.containers[0].env[?(@.name=="ZEPHYR_WORKSPACE")].value}' 2>/dev/null)
  if [ -n "$ZEPHYR_WORKSPACE" ]; then
    WORKSPACE_SOURCE="deployment api-server"
  else
    for candidate in /home/ubuntu/retrospect/zephyr-workspace /opt/k8s-wasm-edge/zephyr-workspace; do
      [ -d "$candidate" ] && { ZEPHYR_WORKSPACE="$candidate"; break; }
    done
    ZEPHYR_WORKSPACE="${ZEPHYR_WORKSPACE:-/home/ubuntu/retrospect/zephyr-workspace}"
  fi
fi
MCU_BUILD="${MCU_BUILD:-stm32f746g_disco}"
FIRMWARE_ELF="${FIRMWARE_ELF:-$ZEPHYR_WORKSPACE/build/$MCU_BUILD/zephyr/zephyr.elf}"

DEVICES=(); for i in $(seq 1 "$N"); do DEVICES+=("${PREFIX}-${i}"); done

# --- Immagini e deployment --------------------------------------------------
# Repliche disponibili di un deployment (0 se non esiste).
deploy_available() {
  local n
  n=$(kubectl -n "$NAMESPACE" get deploy "$1" -o jsonpath='{.status.availableReplicas}' 2>/dev/null)
  echo "${n:-0}"
}

# Ricostruisce l'immagine di un deployment e la rende visibile al cluster.
# Il tag viene letto dal deployment stesso: su questo cluster i tag sono stati
# fissati a mano (es. gateway:fixhb2) e imagePullPolicy è Never, quindi ricostruire
# ":latest" non aggiornerebbe nulla.
sync_image() {
  local deploy="$1" dockerfile="$2" img
  img=$(kubectl -n "$NAMESPACE" get deploy "$deploy" -o jsonpath='{.spec.template.spec.containers[0].image}' 2>/dev/null)
  [ -n "$img" ] || { err "deployment $deploy non trovato"; return 1; }

  # Dockerfile.api-server COPYs these directories from the build context; sono
  # gitignorate, quindi su un clone pulito non esistono e la build fallisce.
  # deploy-k3s.sh fa lo stesso prima di buildare.
  mkdir -p zephyr-workspace/build/nrf52840dk/nrf52840/zephyr \
           zephyr-workspace/build/stm32f4/zephyr \
           zephyr-workspace/build/stm32f746g_disco/zephyr

  echo "  Build $img da $dockerfile (compila il Rust dentro l'immagine, richiede qualche minuto)..."
  docker build -q -t "$img" -f "$dockerfile" . >/dev/null || { err "docker build fallito per $img"; return 1; }

  # Il registry locale è best-effort: con imagePullPolicy Never non viene comunque usato.
  docker push "$img" >/dev/null 2>&1 && ok "$img pushata nel registry" || warn "push nel registry non riuscita (ignorabile)"

  # k3s usa containerd, non il daemon Docker: senza import il pod non vede la nuova immagine.
  if command -v k3s >/dev/null 2>&1; then
    docker save "$img" | k3s ctr images import - >/dev/null 2>&1 \
      && ok "$img importata in containerd" || warn "import in containerd non riuscito"
  fi

  kubectl -n "$NAMESPACE" rollout restart "deploy/$deploy" >/dev/null || { err "rollout restart di $deploy fallito"; return 1; }
  kubectl -n "$NAMESPACE" rollout status "deploy/$deploy" --timeout=300s >/dev/null || { err "$deploy non è tornato pronto"; return 1; }
  ok "$deploy aggiornato e pronto"
}

# --- Teardown ---------------------------------------------------------------
cleanup() {
  phase "Teardown"
  for d in "${DEVICES[@]}"; do
    docker rm -f "wasmbed-renode-${d:0:16}" >/dev/null 2>&1 && ok "container di $d rimosso"
    kubectl delete device "$d" -n "$NAMESPACE" >/dev/null 2>&1 && ok "Device CRD $d rimosso"
  done
  for app in $(kubectl get applications -n "$NAMESPACE" -o name 2>/dev/null | grep 'fleet-test-'); do
    kubectl delete "$app" -n "$NAMESPACE" >/dev/null 2>&1 && ok "${app#*/} rimossa"
  done
  sudo ./scripts/setup-renode-net.sh --down 2>/dev/null | sed 's/^/  /' || warn "teardown rete saltato"
  ok "Teardown completato"
}

if [ "$MODE" = "cleanup" ]; then cleanup; exit 0; fi

echo "========================================================"
echo "  E2E fleet test — $N device ($NAMESPACE)"
echo "========================================================"

# --- Fase 1: preflight ------------------------------------------------------
phase "1/6 Preflight"
FATAL=0
for c in kubectl docker curl openssl sudo; do
  if command -v "$c" >/dev/null 2>&1; then ok "$c presente"; else err "$c mancante"; FATAL=1; fi
done

if kubectl get nodes >/dev/null 2>&1; then
  ok "cluster raggiungibile ($(kubectl get nodes --no-headers 2>/dev/null | wc -l) nodo/i)"
else
  err "cluster non raggiungibile (kubectl get nodes)"
  echo "     Avvia k3s: sudo systemctl start k3s   —   serve un host con accesso privilegiato al kernel"
  FATAL=1
fi

if docker info >/dev/null 2>&1; then ok "docker attivo"; else err "docker non attivo o permessi mancanti"; FATAL=1; fi

if docker image inspect "$RENODE_IMAGE" >/dev/null 2>&1; then
  ok "immagine Renode presente ($RENODE_IMAGE)"
else
  warn "immagine Renode assente: docker pull $RENODE_IMAGE"
fi

ok "workspace Zephyr: $ZEPHYR_WORKSPACE (fonte: $WORKSPACE_SOURCE)"
DEPLOY_WS=$(kubectl -n "$NAMESPACE" get deploy wasmbed-api-server \
  -o jsonpath='{.spec.template.spec.containers[0].env[?(@.name=="ZEPHYR_WORKSPACE")].value}' 2>/dev/null)
if [ -n "$DEPLOY_WS" ] && [ "$DEPLOY_WS" != "$ZEPHYR_WORKSPACE" ]; then
  err "workspace diverso da quello montato dall'api-server ($DEPLOY_WS): i .resc finirebbero dove Renode non li legge"
  FATAL=1
fi

if [ -f "$FIRMWARE_ELF" ]; then
  ok "firmware Zephyr: $FIRMWARE_ELF"
else
  err "firmware Zephyr non trovato: $FIRMWARE_ELF"
  echo "     cd $ZEPHYR_WORKSPACE && west build -b $MCU_BUILD ../zephyr-app --pristine --build-dir build/$MCU_BUILD"
  FATAL=1
fi

if sudo -n true 2>/dev/null; then ok "sudo disponibile senza password"; else warn "sudo chiederà la password (serve per la rete)"; fi

if [ "$MODE" = "check" ]; then
  echo ""
  [ "$FATAL" -eq 0 ] && { echo -e "${G}Preflight OK: puoi lanciare ./scripts/run-e2e-fleet.sh -n $N${NC}"; exit 0; }
  echo -e "${R}Preflight fallito: risolvi i punti ❌ sopra.${NC}"; exit 1
fi
[ "$FATAL" -eq 0 ] || die "preflight fallito"

# --- Fase 2: build e deploy -------------------------------------------------
phase "2/6 Build e deploy"
if [ "$SKIP_BUILD" = "1" ]; then
  warn "build saltata (--skip-build): assicurati che l'api-server deployato contenga la fix multi-device"
elif [ "$FULL_DEPLOY" = "1" ] || ! kubectl get deploy wasmbed-api-server -n "$NAMESPACE" >/dev/null 2>&1; then
  echo "  Deploy completo con scripts/deploy-k3s.sh..."
  ./scripts/deploy-k3s.sh 2>&1 | sed 's/^/  /' || die "deploy-k3s.sh fallito"
  ok "deploy completo eseguito"
else
  # La fix vive in wasmbed-qemu-manager, che gira dentro l'api-server:
  # ricostruire quell'immagine è obbligatorio, un restart del pod non basta.
  # Il codice Rust viene compilato DENTRO l'immagine (i Dockerfile usano rust:1.88),
  # quindi non serve una toolchain aggiornata sull'host.
  sync_image wasmbed-api-server Dockerfile.api-server || die "aggiornamento api-server fallito"

  # Il deployment del gateway è creato dal gateway-controller, non da un manifest:
  # se non è disponibile, ricostruiscilo con lo stesso tag che il deployment richiede.
  if [ "$(deploy_available gateway-1-deployment)" -lt 1 ]; then
    warn "gateway non disponibile: ricostruisco anche la sua immagine"
    sync_image gateway-1-deployment Dockerfile.gateway || warn "aggiornamento gateway fallito"
  fi
fi

for d in wasmbed-api-server gateway-1-deployment; do
  if [ "$(deploy_available "$d")" -ge 1 ]; then ok "$d disponibile"; else err "$d NON disponibile"; fi
done

# --- Fase 3: port-forward ---------------------------------------------------
phase "3/6 Port-forward"
./scripts/ensure-experiment-runtime.sh 2>&1 | sed 's/^/  /' || die "ensure-experiment-runtime.sh fallito"
curl -sf -o /dev/null "$API_BASE/api/v1/devices"     && ok "API server risponde ($API_BASE)"     || die "API server non raggiungibile"
curl -sf -o /dev/null "$GATEWAY_HTTP/api/v1/devices" && ok "gateway risponde ($GATEWAY_HTTP)"    || warn "gateway HTTP non raggiungibile su $GATEWAY_HTTP"

# --- Fase 4: stato pulito ---------------------------------------------------
phase "4/6 Pulizia stato precedente"
for d in "${DEVICES[@]}"; do
  docker rm -f "wasmbed-renode-${d:0:16}" >/dev/null 2>&1 || true
  kubectl delete device "$d" -n "$NAMESPACE" >/dev/null 2>&1 || true
done
ok "device e container residui rimossi"

# --- Fase 5: test fleet -----------------------------------------------------
phase "5/6 Test fleet ($N device)"
NAMESPACE="$NAMESPACE" API_BASE="$API_BASE" GATEWAY_HTTP="$GATEWAY_HTTP" PREFIX="$PREFIX" \
  ./scripts/test-fleet-scalability.sh "$N"
RESULT=$?

# --- Fase 6: esito e diagnostica -------------------------------------------
phase "6/6 Esito"
if [ "$RESULT" -eq 0 ]; then
  echo -e "${G}✅ E2E FLEET TEST SUPERATO: $N device, $N sessioni TLS distinte, deploy su tutta la fleet.${NC}"
  echo ""
  echo "Teardown quando hai finito:  ./scripts/run-e2e-fleet.sh --cleanup -n $N"
  exit 0
fi

echo -e "${R}❌ E2E FLEET TEST FALLITO (exit $RESULT). Diagnostica:${NC}"

echo ""
echo "--- Device CRD ---"
kubectl get devices -n "$NAMESPACE" -o wide 2>/dev/null

echo ""
echo "--- Script Renode generati (TAP/MAC/chiave devono differire fra device) ---"
for d in "${DEVICES[@]}"; do
  f="$ZEPHYR_WORKSPACE/renode-scripts/$d.resc"
  if [ -f "$f" ]; then
    echo "  $d: $(grep -o 'CreateTap "[^"]*"' "$f" | head -1) $(grep -o 'MAC "[^"]*"' "$f" | head -1) key=$(grep -c '0x2000200' "$f") righe"
  else
    echo "  $d: $f NON generato"
  fi
done
echo "  Se TAP/MAC/chiave coincidono fra i device, sta girando ancora la vecchia immagine api-server:"
echo "    ./scripts/run-e2e-fleet.sh --full-deploy -n $N"

echo ""
echo "--- Interfacce sul bridge ---"
ip link show master wasmbr0 2>/dev/null | grep -o 'wtap-[0-9a-f]*' || echo "  nessuna TAP su wasmbr0 (rete non preparata?)"

echo ""
echo "--- Ultimi log api-server ---"
kubectl logs -n "$NAMESPACE" deploy/wasmbed-api-server --tail=25 2>/dev/null | sed 's/^/  /'

echo ""
echo "--- Ultimi log gateway ---"
kubectl logs -n "$NAMESPACE" deploy/gateway-1 --tail=25 2>/dev/null | sed 's/^/  /'

echo ""
echo "--- UART del primo device ---"
tail -15 "/tmp/uart-${DEVICES[0]}.log" 2>/dev/null | sed 's/^/  /' || echo "  /tmp/uart-${DEVICES[0]}.log assente"

echo ""
echo "Teardown: ./scripts/run-e2e-fleet.sh --cleanup -n $N"
exit "$RESULT"
