#!/bin/bash
# =============================================================================
# setup-renode-net.sh
# Configura la rete host per l'emulazione Renode/Wasmbed.
# Va eseguito DOPO che Renode ha creato l'interfaccia tap0.
#
# Topologia:
#   Zephyr (Renode) ←→ tap0 (192.168.1.1/24) ←→ [NAT/forward] ←→ cni0 (k3s pods 10.42.x.x)
#
# Uso: sudo ./scripts/setup-renode-net.sh
# =============================================================================

set -e

TAP_IP="192.168.1.1"
DEVICE_SUBNET="192.168.1.0/24"
K3S_IFACE="cni0"    # bridge k3s (pod network 10.42.0.0/16)
WAN_IFACE="ens18"   # interfaccia fisica
GATEWAY_TLS_PORT="30443"
GATEWAY_TLS_DST="127.0.0.1:30443"

echo "=== Wasmbed network setup ==="

# 1. Verifica che tap0 esista
if ! ip link show tap0 &>/dev/null; then
  echo "ERRORE: tap0 non trovata. Avvia prima Renode (Connect dal API/Dashboard)."
  exit 1
fi

# 2. Configura tap0
ip addr flush dev tap0 2>/dev/null || true
ip addr add ${TAP_IP}/24 dev tap0
ip link set tap0 up
echo "✅ tap0: ${TAP_IP}/24"

# 3. IP forwarding
sysctl -w net.ipv4.ip_forward=1 > /dev/null

# 4. Regole iptables forwarding tap0 ↔ k3s
iptables -C FORWARD -i tap0 -o ${K3S_IFACE} -j ACCEPT 2>/dev/null || \
  iptables -I FORWARD -i tap0 -o ${K3S_IFACE} -j ACCEPT
iptables -C FORWARD -i ${K3S_IFACE} -o tap0 -m state --state ESTABLISHED,RELATED -j ACCEPT 2>/dev/null || \
  iptables -I FORWARD -i ${K3S_IFACE} -o tap0 -m state --state ESTABLISHED,RELATED -j ACCEPT
echo "✅ iptables: forwarding tap0 ↔ ${K3S_IFACE}"

# 5. NAT per traffico da device verso rete esterna
iptables -t nat -C POSTROUTING -s ${DEVICE_SUBNET} -o ${WAN_IFACE} -j MASQUERADE 2>/dev/null || \
  iptables -t nat -A POSTROUTING -s ${DEVICE_SUBNET} -o ${WAN_IFACE} -j MASQUERADE
echo "✅ NAT masquerade: ${DEVICE_SUBNET} → ${WAN_IFACE}"

# 5b. DNAT per inoltrare il traffico TLS del device verso il port-forward locale del gateway
iptables -t nat -C PREROUTING -d ${TAP_IP} -p tcp --dport ${GATEWAY_TLS_PORT} -j DNAT --to-destination ${GATEWAY_TLS_DST} 2>/dev/null || \
  iptables -t nat -A PREROUTING -d ${TAP_IP} -p tcp --dport ${GATEWAY_TLS_PORT} -j DNAT --to-destination ${GATEWAY_TLS_DST}
echo "✅ DNAT: ${TAP_IP}:${GATEWAY_TLS_PORT} → ${GATEWAY_TLS_DST}"

# 6. Avvia dnsmasq per DHCP su tap0
pkill dnsmasq 2>/dev/null || true
sleep 1
dnsmasq --conf-file=/etc/dnsmasq.d/wasmbed-tap.conf --pid-file=/tmp/wasmbed-dnsmasq.pid
echo "✅ dnsmasq DHCP avviato (192.168.1.100-200, GW=192.168.1.1, DNS=10.43.0.10)"

echo ""
echo "=== Rete pronta. Il device Renode riceverà IP 192.168.1.x via DHCP ==="
echo "    Gateway TLS raggiungibile su: $(kubectl get pod -n wasmbed -l app=gateway-1 -o jsonpath='{.items[0].status.podIP}' 2>/dev/null || echo '10.42.0.x'):8443"
