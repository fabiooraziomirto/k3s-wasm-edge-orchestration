#!/bin/bash
# =============================================================================
# setup-renode-net.sh
# Configura la rete host per l'emulazione Renode/Wasmbed.
#
# Topologia (multi-device):
#   Zephyr dev-1 (Renode) ←→ wtap-xxxxxxxx ─┐
#   Zephyr dev-2 (Renode) ←→ wtap-yyyyyyyy ─┼→ wasmbr0 (192.168.1.1/24)
#   Zephyr dev-3 (Renode) ←→ wtap-zzzzzzzz ─┘        ↕ [NAT/DNAT]
#                                              k3s pods (10.42.x.x) / gateway TLS
#
# Ogni device ha la PROPRIA interfaccia TAP e il proprio MAC: con una tap0
# condivisa e un MAC fisso solo un device per volta riusciva a raggiungere il
# gateway (tutti gli altri restavano senza L2 e senza lease DHCP).
# Il nome della TAP è derivato dal device id con lo stesso schema usato da
# wasmbed-qemu-manager (device_tap_name / device_mac_address in lib.rs):
#   tap = "wtap-" + sha256(device_id)[0..4]      (8 cifre esadecimali)
#   mac = "02:" + sha256(device_id)[0..5]        (locally administered, unicast)
#
# Uso:
#   sudo ./scripts/setup-renode-net.sh                  # legge i device dai CRD
#   sudo ./scripts/setup-renode-net.sh dev-1 dev-2 ...  # device espliciti
#   sudo ./scripts/setup-renode-net.sh --down           # rimuove taps e bridge
#
# Va eseguito PRIMA di avviare Renode: le TAP sono persistenti e Renode si
# limita ad aprirle (`emulation CreateTap`).
# =============================================================================

set -e

BRIDGE="${BRIDGE:-wasmbr0}"
BRIDGE_IP="${BRIDGE_IP:-192.168.1.1}"
DEVICE_SUBNET="${DEVICE_SUBNET:-192.168.1.0/24}"
DHCP_RANGE_START="${DHCP_RANGE_START:-192.168.1.100}"
DHCP_RANGE_END="${DHCP_RANGE_END:-192.168.1.200}"
K3S_IFACE="${K3S_IFACE:-cni0}"     # bridge k3s (pod network 10.42.0.0/16)
WAN_IFACE="${WAN_IFACE:-ens18}"    # interfaccia fisica
GATEWAY_TLS_PORT="${GATEWAY_TLS_PORT:-30443}"
GATEWAY_TLS_DST="${GATEWAY_TLS_DST:-127.0.0.1:30443}"
CLUSTER_DNS="${CLUSTER_DNS:-10.43.0.10}"
NAMESPACE="${NAMESPACE:-wasmbed}"
DNSMASQ_CONF="/etc/dnsmasq.d/wasmbed-tap.conf"

# --- Derivazione identità di rete (deve restare allineata a lib.rs) ---------
tap_name() { echo "wtap-$(printf '%s' "$1" | sha256sum | cut -c1-8)"; }
mac_addr() {
  local h; h=$(printf '%s' "$1" | sha256sum | cut -c1-10)
  echo "02:${h:0:2}:${h:2:2}:${h:4:2}:${h:6:2}:${h:8:2}"
}

# --- Teardown ---------------------------------------------------------------
if [ "${1:-}" = "--down" ]; then
  echo "=== Teardown rete Renode ==="
  for tap in $(ip -o link show 2>/dev/null | awk -F': ' '{print $2}' | grep '^wtap-' || true); do
    ip link del "$tap" 2>/dev/null && echo "  rimossa $tap" || true
  done
  ip link del "$BRIDGE" 2>/dev/null && echo "  rimosso bridge $BRIDGE" || true
  pkill -F /tmp/wasmbed-dnsmasq.pid 2>/dev/null || true
  echo "✅ Teardown completato"
  exit 0
fi

echo "=== Wasmbed network setup (multi-device) ==="

# --- 1. Elenco dei device ---------------------------------------------------
DEVICES=("$@")
if [ ${#DEVICES[@]} -eq 0 ]; then
  if command -v kubectl &>/dev/null; then
    mapfile -t DEVICES < <(kubectl get devices -n "$NAMESPACE" -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}' 2>/dev/null | grep -v '^$' || true)
  fi
fi
if [ ${#DEVICES[@]} -eq 0 ]; then
  echo "ERRORE: nessun device. Passa gli id come argomenti o crea i Device CRD nel namespace $NAMESPACE."
  exit 1
fi
echo "Device: ${DEVICES[*]}"

# --- 2. Bridge --------------------------------------------------------------
if ! ip link show "$BRIDGE" &>/dev/null; then
  ip link add name "$BRIDGE" type bridge
fi
ip addr flush dev "$BRIDGE" 2>/dev/null || true
ip addr add "${BRIDGE_IP}/24" dev "$BRIDGE"
ip link set "$BRIDGE" up
# Il traffico fra device sullo stesso bridge non deve passare da iptables/netfilter.
sysctl -qw net.bridge.bridge-nf-call-iptables=0 2>/dev/null || true
echo "✅ bridge $BRIDGE: ${BRIDGE_IP}/24"

# --- 3. Una TAP per device --------------------------------------------------
for dev in "${DEVICES[@]}"; do
  tap=$(tap_name "$dev")
  mac=$(mac_addr "$dev")
  if ! ip link show "$tap" &>/dev/null; then
    ip tuntap add dev "$tap" mode tap
  fi
  ip link set "$tap" master "$BRIDGE"
  ip link set "$tap" up
  echo "✅ $dev → $tap (MAC atteso dal firmware: $mac)"
done

# --- 4. Forwarding e NAT ----------------------------------------------------
sysctl -qw net.ipv4.ip_forward=1

iptables -C FORWARD -i "$BRIDGE" -o "$K3S_IFACE" -j ACCEPT 2>/dev/null || \
  iptables -I FORWARD -i "$BRIDGE" -o "$K3S_IFACE" -j ACCEPT
iptables -C FORWARD -i "$K3S_IFACE" -o "$BRIDGE" -m state --state ESTABLISHED,RELATED -j ACCEPT 2>/dev/null || \
  iptables -I FORWARD -i "$K3S_IFACE" -o "$BRIDGE" -m state --state ESTABLISHED,RELATED -j ACCEPT
echo "✅ iptables: forwarding $BRIDGE ↔ $K3S_IFACE"

iptables -t nat -C POSTROUTING -s "$DEVICE_SUBNET" -o "$WAN_IFACE" -j MASQUERADE 2>/dev/null || \
  iptables -t nat -A POSTROUTING -s "$DEVICE_SUBNET" -o "$WAN_IFACE" -j MASQUERADE
echo "✅ NAT masquerade: $DEVICE_SUBNET → $WAN_IFACE"

# DNAT: traffico TLS dei device verso il port-forward locale del gateway.
iptables -t nat -C PREROUTING -d "$BRIDGE_IP" -p tcp --dport "$GATEWAY_TLS_PORT" -j DNAT --to-destination "$GATEWAY_TLS_DST" 2>/dev/null || \
  iptables -t nat -A PREROUTING -d "$BRIDGE_IP" -p tcp --dport "$GATEWAY_TLS_PORT" -j DNAT --to-destination "$GATEWAY_TLS_DST"
# Il DNAT verso 127.0.0.1 richiede route_localnet sul bridge.
sysctl -qw "net.ipv4.conf.${BRIDGE}.route_localnet=1" 2>/dev/null || true
echo "✅ DNAT: ${BRIDGE_IP}:${GATEWAY_TLS_PORT} → ${GATEWAY_TLS_DST}"

# --- 5. DHCP ----------------------------------------------------------------
mkdir -p /etc/dnsmasq.d
cat > "$DNSMASQ_CONF" <<CONF
# Generato da scripts/setup-renode-net.sh — non modificare a mano.
interface=${BRIDGE}
bind-interfaces
except-interface=lo
dhcp-range=${DHCP_RANGE_START},${DHCP_RANGE_END},12h
dhcp-option=3,${BRIDGE_IP}
dhcp-option=6,${CLUSTER_DNS}
log-dhcp
CONF

pkill -F /tmp/wasmbed-dnsmasq.pid 2>/dev/null || pkill dnsmasq 2>/dev/null || true
sleep 1
dnsmasq --conf-file="$DNSMASQ_CONF" --pid-file=/tmp/wasmbed-dnsmasq.pid
echo "✅ dnsmasq DHCP su $BRIDGE (${DHCP_RANGE_START}-${DHCP_RANGE_END}, GW=${BRIDGE_IP}, DNS=${CLUSTER_DNS})"

echo ""
echo "=== Rete pronta: ${#DEVICES[@]} device, un IP distinto ciascuno via DHCP ==="
echo "    Lease attivi:  ip neigh show dev $BRIDGE"
echo "    Gateway TLS:   ${BRIDGE_IP}:${GATEWAY_TLS_PORT}"
