#!/usr/bin/env bash
# Verify TLS connection maintenance (heartbeat) and WASM deployment to Renode devices.
# Prerequisites: K8s cluster with wasmbed namespace, Gateway and API Server running,
#                at least one Device in phase Connected (Renode + firmware with heartbeat).
# Usage: ./scripts/verify-tls-and-deploy.sh [API_BASE_URL] [GATEWAY_HTTP_URL]

set -e

API_BASE="${1:-http://127.0.0.1:3001}"
GATEWAY_HTTP="${2:-}"  # Optional: e.g. http://127.0.0.1:9080
NAMESPACE="${NAMESPACE:-wasmbed}"

echo "=== Verify TLS maintenance and WASM deploy ==="
echo "API_BASE=$API_BASE NAMESPACE=$NAMESPACE"

# 1) TLS maintenance: devices with last_heartbeat (Gateway updates this on Heartbeat from device)
echo ""
echo "--- 1) Devices and last_heartbeat (TLS maintenance) ---"
if command -v kubectl &>/dev/null; then
    kubectl get devices -n "$NAMESPACE" -o wide 2>/dev/null || true
    echo "Device status (last_heartbeat when device sends Heartbeat every ~25s):"
    kubectl get devices -n "$NAMESPACE" -o jsonpath='{range .items[*]}{.metadata.name}: phase={.status.phase} last_heartbeat={.status.last_heartbeat}{"\n"}{end}' 2>/dev/null || true
else
    echo "kubectl not found; skip K8s checks."
fi

# 2) Applications status (after deploy, phase should become Running when device sends DeployAck)
echo ""
echo "--- 2) Applications status ---"
if command -v kubectl &>/dev/null; then
    kubectl get applications -n "$NAMESPACE" -o wide 2>/dev/null || true
    echo "Application status (phase, deviceStatuses updated by Gateway on DeployAck):"
    kubectl get applications -n "$NAMESPACE" -o jsonpath='{range .items[*]}{.metadata.name}: phase={.status.phase}{"\n"}{end}' 2>/dev/null || true
fi

# 3) Gateway HTTP API (devices list; optional if GATEWAY_HTTP set)
if [ -n "$GATEWAY_HTTP" ]; then
    echo ""
    echo "--- 3) Gateway HTTP API ($GATEWAY_HTTP) ---"
    curl -s -o /dev/null -w "GET /api/v1/devices: %{http_code}\n" "$GATEWAY_HTTP/api/v1/devices" || true
    curl -s "$GATEWAY_HTTP/api/v1/devices" 2>/dev/null | head -c 500
    echo ""
fi

# 4) API Server deploy endpoint (smoke: POST deploy returns 200 with body; may fail if Gateway unreachable)
echo ""
echo "--- 4) API Server deploy (smoke) ---"
APPLICATIONS=$(curl -s "$API_BASE/api/v1/applications" 2>/dev/null | sed -n 's/.*"name":"\([^"]*\)".*/\1/p' | head -1)
if [ -n "$APPLICATIONS" ]; then
    APP_ID="$APPLICATIONS"
    echo "Triggering deploy for application: $APP_ID"
    DEPLOY_RESP=$(curl -s -w "\n%{http_code}" -X POST "$API_BASE/api/v1/applications/$APP_ID/deploy" 2>/dev/null) || true
    HTTP_CODE=$(echo "$DEPLOY_RESP" | tail -1)
    BODY=$(echo "$DEPLOY_RESP" | sed '$d')
    echo "POST deploy response: HTTP $HTTP_CODE"
    echo "$BODY" | head -c 300
    echo ""
else
    echo "No applications found; create one and set targetDevices to a Connected device, then re-run."
fi

# 5) Deploy flow verification (create app + deploy if API available)
echo ""
echo "--- 5) Deploy flow (create app + POST deploy) ---"
MINIMAL_WASM_B64="AGFzbQEAAAAB"
DEVICE_NAME=$(kubectl get devices -n "$NAMESPACE" -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)
if [ -n "$DEVICE_NAME" ]; then
    APP_NAME="verify-deploy-$(date +%s)"
    CREATE_RESP=$(curl -s -X POST "$API_BASE/api/v1/applications" -H "Content-Type: application/json" \
      -d "{\"name\":\"$APP_NAME\",\"description\":\"Verify deploy\",\"wasmBytes\":\"$MINIMAL_WASM_B64\",\"targetDevices\":{\"deviceNames\":[\"$DEVICE_NAME\"]}}" 2>/dev/null) || true
    if echo "$CREATE_RESP" | grep -q '"success":true'; then
        echo "Created application: $APP_NAME"
        DEPLOY_RESP=$(curl -s -X POST "$API_BASE/api/v1/applications/$APP_NAME/deploy" -H "Content-Type: application/json" -d '{}' 2>/dev/null) || true
        echo "Deploy response: $(echo "$DEPLOY_RESP" | head -c 400)"
        echo ""
        echo "Application status in K8s:"
        kubectl get application "$APP_NAME" -n "$NAMESPACE" -o jsonpath='  phase={.status.phase} deviceStatuses={.status.deviceStatuses}{"\n"}' 2>/dev/null || true
    fi
else
    echo "No devices in namespace; skip deploy flow."
fi

echo ""
echo "=== Done ==="
echo "Recovery: Gateway recovers Unreachable->Connected on Heartbeat; Device Controller re-registers Unreachable devices with Gateway."
echo "TLS maintenance: firmware sends Heartbeat every 25s; Gateway updates Device last_heartbeat; monitor marks unreachable after 90s."
echo "WASM deploy: API POST deploy -> Gateway POST .../devices/:id/deploy -> Gateway sends DeployApplication via TLS; firmware sends DeployAck -> Gateway sets Application phase Running."
