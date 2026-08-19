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
ZEPHYR_WORKSPACE="${ZEPHYR_WORKSPACE:-/opt/k8s-wasm-edge/zephyr-workspace}"
FIRMWARE_ELF="${FIRMWARE_ELF:-$ZEPHYR_WORKSPACE/build/stm32f746g_disco/zephyr/zephyr.elf}"
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

DEVICES=(); for i in $(seq 1 "$N"); do DEVICES+=("${PREFIX}-${i}"); done

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

if [ -f "$FIRMWARE_ELF" ]; then
  ok "firmware Zephyr: $FIRMWARE_ELF"
else
  err "firmware Zephyr non trovato: $FIRMWARE_ELF"
  echo "     cd $ZEPHYR_WORKSPACE && west build -b stm32f746g_disco ../zephyr-app --pristine --build-dir build/stm32f746g_disco"
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
  echo "  Rebuild dell'immagine api-server (contiene la fix multi-device)..."
  cargo build --workspace 2>&1 | tail -3 | sed 's/^/  /' || die "cargo build fallito"
  docker build -q -t "$REGISTRY/wasmbed/api-server:latest" -f Dockerfile.api-server . | sed 's/^/  /' || die "docker build fallito"
  docker push "$REGISTRY/wasmbed/api-server:latest" 2>&1 | tail -1 | sed 's/^/  /' || die "docker push fallito"
  kubectl -n "$NAMESPACE" rollout restart deploy/wasmbed-api-server >/dev/null || die "rollout restart fallito"
  kubectl -n "$NAMESPACE" rollout status deploy/wasmbed-api-server --timeout=180s | sed 's/^/  /' || die "api-server non pronto"
  ok "api-server aggiornato"
fi

not_running=$(kubectl get pods -n "$NAMESPACE" --no-headers 2>/dev/null | grep -vc "Running")
[ "${not_running:-1}" -eq 0 ] && ok "tutti i pod Running" || warn "$not_running pod non Running (kubectl get pods -n $NAMESPACE)"

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
