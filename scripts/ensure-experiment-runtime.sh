#!/bin/bash
# =============================================================================
# ensure-experiment-runtime.sh
# Rende ripetibile l'ambiente runtime per esperimenti Wasmbed:
# - verifica prerequisiti kubectl
# - riavvia in modo pulito i port-forward necessari
# - verifica porte in ascolto
#
# Uso: ./scripts/ensure-experiment-runtime.sh
# =============================================================================

set -euo pipefail

NAMESPACE="wasmbed"
LOG_DIR="/tmp"

API_SERVICE="wasmbed-api-server"
GW_SERVICE="gateway-1-service"

API_LOCAL_PORT="3001"
API_REMOTE_PORT="3001"
GW_HTTP_LOCAL_PORT="8080"
GW_HTTP_REMOTE_PORT="8080"
GW_TLS_LOCAL_PORT="30443"
GW_TLS_REMOTE_PORT="8443"

API_LOG="${LOG_DIR}/pf-api.log"
GW_HTTP_LOG="${LOG_DIR}/pf-gw-http.log"
GW_TLS_LOG="${LOG_DIR}/pf-gw-tls.log"

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "ERRORE: comando non trovato: $1"
		exit 1
	fi
}

ensure_kubectl_ready() {
	kubectl version --client >/dev/null
	kubectl get ns "${NAMESPACE}" >/dev/null
}

stop_pf() {
	local pattern="$1"
	pkill -f "$pattern" 2>/dev/null || true
}

start_pf() {
	local service="$1"
	local local_port="$2"
	local remote_port="$3"
	local log_file="$4"

	local cmd="kubectl -n ${NAMESPACE} port-forward svc/${service} ${local_port}:${remote_port} --address 127.0.0.1"
	echo "Avvio: ${cmd}"
	nohup bash -lc "${cmd}" >"${log_file}" 2>&1 &
}

wait_port() {
	local port="$1"
	local max_wait_s=10
	local waited=0

	while ! ss -tln "sport = :${port}" | grep -q LISTEN; do
		sleep 1
		waited=$((waited + 1))
		if [[ ${waited} -ge ${max_wait_s} ]]; then
			echo "ERRORE: porta ${port} non in ascolto dopo ${max_wait_s}s"
			return 1
		fi
	done
}

echo "=== Ensure Wasmbed experiment runtime ==="

require_cmd kubectl
require_cmd ss
ensure_kubectl_ready

stop_pf "kubectl -n ${NAMESPACE} port-forward svc/${API_SERVICE} ${API_LOCAL_PORT}:${API_REMOTE_PORT}"
stop_pf "kubectl -n ${NAMESPACE} port-forward svc/${GW_SERVICE} ${GW_HTTP_LOCAL_PORT}:${GW_HTTP_REMOTE_PORT}"
stop_pf "kubectl -n ${NAMESPACE} port-forward svc/${GW_SERVICE} ${GW_TLS_LOCAL_PORT}:${GW_TLS_REMOTE_PORT}"

start_pf "${API_SERVICE}" "${API_LOCAL_PORT}" "${API_REMOTE_PORT}" "${API_LOG}"
start_pf "${GW_SERVICE}" "${GW_HTTP_LOCAL_PORT}" "${GW_HTTP_REMOTE_PORT}" "${GW_HTTP_LOG}"
start_pf "${GW_SERVICE}" "${GW_TLS_LOCAL_PORT}" "${GW_TLS_REMOTE_PORT}" "${GW_TLS_LOG}"

wait_port "${API_LOCAL_PORT}"
wait_port "${GW_HTTP_LOCAL_PORT}"
wait_port "${GW_TLS_LOCAL_PORT}"

echo "✅ Port-forward attivi: ${API_LOCAL_PORT}, ${GW_HTTP_LOCAL_PORT}, ${GW_TLS_LOCAL_PORT}"
echo "   Log: ${API_LOG}, ${GW_HTTP_LOG}, ${GW_TLS_LOG}"
echo ""
echo "Nota: per rendere persistente anche la rete tap0 + DNAT, eseguire con sudo:"
echo "  ./scripts/setup-renode-net.sh"
