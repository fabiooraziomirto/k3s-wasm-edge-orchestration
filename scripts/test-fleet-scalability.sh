#!/usr/bin/env bash
# =============================================================================
# test-fleet-scalability.sh
# Test di scalabilità della fleet: N device Renode devono mantenere N sessioni
# TLS distinte verso il gateway e ricevere un deploy WASM ciascuno.
#
# È il test che in §8.2 falliva: con tap0/MAC fissi e chiave pubblica statica
# condivisa, N device collassavano su una sola sessione TLS.
#
# Uso:
#   ./scripts/test-fleet-scalability.sh [N]            # default N=3
#   DRY_RUN=1 ./scripts/test-fleet-scalability.sh 3    # solo verifiche statiche
#
# Prerequisiti (senza DRY_RUN): cluster k3s con namespace wasmbed, gateway e
# api-server in esecuzione, Docker con immagine antmicro/renode:nightly, root
# per la configurazione di rete.
# =============================================================================

set -uo pipefail

N="${1:-3}"
NAMESPACE="${NAMESPACE:-wasmbed}"
API_BASE="${API_BASE:-http://127.0.0.1:3001}"
GATEWAY_HTTP="${GATEWAY_HTTP:-http://127.0.0.1:9080}"
MCU_TYPE="${MCU_TYPE:-Stm32F746gDisco}"
PREFIX="${PREFIX:-fleet-device}"
CONNECT_TIMEOUT="${CONNECT_TIMEOUT:-180}"
DRY_RUN="${DRY_RUN:-0}"

PASS=0; FAIL=0
ok()   { echo "  ✅ $*"; PASS=$((PASS+1)); }
ko()   { echo "  ❌ $*"; FAIL=$((FAIL+1)); }
step() { echo ""; echo "--- $* ---"; }

DEVICES=(); for i in $(seq 1 "$N"); do DEVICES+=("${PREFIX}-${i}"); done
echo "=== Fleet scalability test: ${#DEVICES[@]} device (${DEVICES[*]}) ==="

tap_name() { echo "wtap-$(printf '%s' "$1" | sha256sum | cut -c1-8)"; }
mac_addr() { local h; h=$(printf '%s' "$1" | sha256sum | cut -c1-10); echo "02:${h:0:2}:${h:2:2}:${h:4:2}:${h:6:2}:${h:8:2}"; }

# --- 0. Identità di rete distinte (verifica statica, sempre eseguita) --------
step "0) Identità di rete distinte per device"
uniq_taps=$(for d in "${DEVICES[@]}"; do tap_name "$d"; done | sort -u | wc -l)
uniq_macs=$(for d in "${DEVICES[@]}"; do mac_addr "$d"; done | sort -u | wc -l)
[ "$uniq_taps" -eq "${#DEVICES[@]}" ] && ok "TAP distinte: $uniq_taps/${#DEVICES[@]}" || ko "TAP duplicate: $uniq_taps/${#DEVICES[@]}"
[ "$uniq_macs" -eq "${#DEVICES[@]}" ] && ok "MAC distinti: $uniq_macs/${#DEVICES[@]}" || ko "MAC duplicati: $uniq_macs/${#DEVICES[@]}"
for d in "${DEVICES[@]}"; do echo "     $d → $(tap_name "$d") / $(mac_addr "$d")"; done

if [ "$DRY_RUN" = "1" ]; then
  echo ""
  echo "DRY_RUN: verifiche statiche completate. PASS=$PASS FAIL=$FAIL"
  [ "$FAIL" -eq 0 ] && exit 0 || exit 1
fi

command -v kubectl >/dev/null || { echo "kubectl mancante"; exit 2; }
kubectl get ns "$NAMESPACE" >/dev/null 2>&1 || { echo "namespace $NAMESPACE non raggiungibile: avvia il cluster"; exit 2; }

# --- 1. Device CRD con chiavi pubbliche distinte ----------------------------
step "1) Creazione Device CRD con chiave pubblica distinta"
declare -A KEYS
for d in "${DEVICES[@]}"; do
  # spec.publicKey = base64 url-safe senza padding dei 32 byte grezzi
  # (stesso formato di PublicKey::to_base64, usato da Device::find nel gateway).
  key=$(openssl rand 32 | base64 | tr '+/' '-_' | tr -d '=\n')
  KEYS[$d]="$key"
  kubectl apply -f - >/dev/null <<CRD
apiVersion: wasmbed.github.io/v0
kind: Device
metadata:
  name: $d
  namespace: $NAMESPACE
spec:
  publicKey: "$key"
  mcuType: $MCU_TYPE
CRD
  if [ $? -eq 0 ]; then ok "$d creato (publicKey ${key:0:12}…)"; else ko "$d non creato"; fi
done
uniq_keys=$(printf '%s\n' "${KEYS[@]}" | sort -u | wc -l)
[ "$uniq_keys" -eq "${#DEVICES[@]}" ] && ok "chiavi pubbliche distinte: $uniq_keys" || ko "chiavi duplicate"

# --- 2. Rete host: una TAP per device ---------------------------------------
step "2) Setup rete host (bridge + TAP per device)"
sudo ./scripts/setup-renode-net.sh "${DEVICES[@]}" || ko "setup-renode-net.sh fallito"
for d in "${DEVICES[@]}"; do
  t=$(tap_name "$d")
  if ip link show "$t" >/dev/null 2>&1; then ok "$t presente"; else ko "$t mancante"; fi
done

# --- 3. Avvio emulazione ----------------------------------------------------
step "3) Avvio emulazione Renode ($N container)"
for d in "${DEVICES[@]}"; do
  code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API_BASE/api/v1/devices/$d/connect" -H 'Content-Type: application/json' -d '{}')
  [ "$code" = "200" ] && ok "connect $d (HTTP $code)" || ko "connect $d (HTTP $code)"
done

running=$(docker ps --filter "name=wasmbed-renode-" --format '{{.Names}}' | wc -l)
[ "$running" -ge "$N" ] && ok "container Renode attivi: $running" || ko "container Renode attivi: $running (attesi $N)"

# Ogni .resc generato deve puntare alla propria TAP e al proprio MAC.
RESC_DIR="${ZEPHYR_WORKSPACE:-/home/ubuntu/retrospect/zephyr-workspace}/renode-scripts"
for d in "${DEVICES[@]}"; do
  f="$RESC_DIR/$d.resc"
  if [ -f "$f" ]; then
    grep -q "CreateTap \"$(tap_name "$d")\"" "$f" && grep -q "MAC \"$(mac_addr "$d")\"" "$f" \
      && ok "$d.resc usa TAP/MAC propri" || ko "$d.resc non usa TAP/MAC propri"
    grep -q "WriteDoubleWord 0x20002000" "$f" \
      && ok "$d.resc inietta la chiave pubblica" || ko "$d.resc non inietta la chiave pubblica"
  else
    ko "$f non generato"
  fi
done

# --- 4. Sessioni TLS distinte (il cuore del test) ---------------------------
step "4) Sessioni TLS distinte nel gateway (timeout ${CONNECT_TIMEOUT}s)"
# Tutti i conteggi sono limitati ai NOSTRI device: il namespace contiene anche
# device di run precedenti, che altrimenti gonfiano i numeri (es. 4/3).
count_phase_connected() {
  local n=0
  for d in "${DEVICES[@]}"; do
    [ "$(kubectl get device "$d" -n "$NAMESPACE" -o jsonpath='{.status.phase}' 2>/dev/null)" = "Connected" ] && n=$((n+1))
  done
  echo "$n"
}

# Sessioni TLS realmente attive nel gateway. Il payload espone "connected"
# (non "tls_connected") e va filtrato per device_id: se i device condividono
# un'identità, il gateway ne mostra uno solo — che era esattamente il bug.
count_gateway_sessions() {
  curl -s "$GATEWAY_HTTP/api/v1/devices" 2>/dev/null | python3 -c '
import json, sys
names = set(sys.argv[1:])
try:
    devices = json.load(sys.stdin).get("devices", [])
except Exception:
    print(0); sys.exit()
print(sum(1 for d in devices if d.get("device_id") in names and d.get("connected")))
' "${DEVICES[@]}"
}

deadline=$((SECONDS + CONNECT_TIMEOUT)); connected=0; gw_connected=0
while [ $SECONDS -lt $deadline ]; do
  connected=$(count_phase_connected)
  gw_connected=$(count_gateway_sessions)
  [ "${gw_connected:-0}" -ge "$N" ] && [ "$connected" -ge "$N" ] && break
  sleep 5
done
[ "$connected" -ge "$N" ] && ok "device in phase Connected: $connected/$N" || ko "device in phase Connected: $connected/$N"
[ "${gw_connected:-0}" -ge "$N" ] && ok "sessioni TLS attive nel gateway: $gw_connected/$N" \
                                   || ko "sessioni TLS attive nel gateway: $gw_connected/$N (era il bug: 1)"

# Un IP distinto per device: se i MAC coincidessero, il DHCP darebbe un solo lease.
leases=0
for d in "${DEVICES[@]}"; do
  mac=$(mac_addr "$d")
  ip neigh show dev "${BRIDGE:-wasmbr0}" 2>/dev/null | grep -qi "$mac" && leases=$((leases+1))
done
[ "$leases" -ge "$N" ] && ok "indirizzi IP distinti sul bridge: $leases/$N" \
                        || ko "indirizzi IP distinti sul bridge: $leases/$N (MAC attesi: $(for d in "${DEVICES[@]}"; do printf '%s ' "$(mac_addr "$d")"; done))"

hb=0
for d in "${DEVICES[@]}"; do
  [ -n "$(kubectl get device "$d" -n "$NAMESPACE" -o jsonpath='{.status.last_heartbeat}' 2>/dev/null)" ] && hb=$((hb+1))
done
[ "$hb" -ge "$N" ] && ok "device con heartbeat: $hb/$N" || ko "device con heartbeat: $hb/$N"

# --- 5. Deploy WASM su tutta la fleet ---------------------------------------
step "5) Deploy WASM su tutti i device"
APP="fleet-test-$(date +%s)"
DEV_JSON=$(printf '"%s",' "${DEVICES[@]}"); DEV_JSON="[${DEV_JSON%,}]"
# Modulo WASM minimo valido (header + versione).
WASM_B64=$(printf '\x00\x61\x73\x6d\x01\x00\x00\x00' | base64 | tr -d '\n')
curl -s -X POST "$API_BASE/api/v1/applications" -H 'Content-Type: application/json' \
  -d "{\"name\":\"$APP\",\"description\":\"fleet scalability test\",\"wasmBytes\":\"$WASM_B64\",\"targetDevices\":{\"deviceNames\":$DEV_JSON}}" >/dev/null
curl -s -X POST "$API_BASE/api/v1/applications/$APP/deploy" -H 'Content-Type: application/json' -d '{}' >/dev/null
sleep 20
deployed=$(kubectl get application "$APP" -n "$NAMESPACE" -o jsonpath='{.status.deviceStatuses}' 2>/dev/null | grep -o 'Running' | wc -l)
[ "$deployed" -ge "$N" ] && ok "deploy Running su $deployed/$N device" || ko "deploy Running su $deployed/$N device"

# --- Riepilogo --------------------------------------------------------------
echo ""
echo "=== Riepilogo: PASS=$PASS FAIL=$FAIL ==="
kubectl get devices -n "$NAMESPACE" -o wide 2>/dev/null
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
