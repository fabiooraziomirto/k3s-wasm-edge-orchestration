#!/bin/bash

set -e

echo "========================================="
echo "  Wasmbed K3S Deployment Script"
echo "========================================="
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check K3S
echo "=== Verifica K3S ==="
if ! kubectl get nodes &>/dev/null; then
    echo -e "${RED}❌ K3S non configurato.${NC}"
    echo ""
    echo "Esegui prima:"
    echo "  curl -sfL https://get.k3s.io | sh -s - --write-kubeconfig-mode 644"
    echo "  mkdir -p ~/.kube"
    echo "  sudo cp /etc/rancher/k3s/k3s.yaml ~/.kube/config"
    echo "  sudo chown \$(id -u):\$(id -g) ~/.kube/config"
    exit 1
fi
echo -e "${GREEN}✅ K3S attivo${NC}"
echo ""

# Check Docker
if ! command -v docker &>/dev/null; then
    echo -e "${RED}❌ Docker non trovato${NC}"
    exit 1
fi

# Start local registry
echo "=== Avvio Registry Locale ==="
if docker ps | grep -q wasmbed-registry; then
    echo -e "${YELLOW}⚠️  Registry già in esecuzione${NC}"
else
    docker run -d -p 5000:5000 --restart=always --name wasmbed-registry registry:2 2>&1 | tail -1
    sleep 3
    echo -e "${GREEN}✅ Registry avviato su localhost:5000${NC}"
fi
echo ""

# Build and push images
echo "=== Build e Push Immagini ==="
cd "$(dirname "$0")/.."

IMAGES=(
    "api-server:Dockerfile.api-server"
    "gateway:Dockerfile.gateway"
    "dashboard:Dockerfile.dashboard"
    "device-controller:Dockerfile.device-controller"
    "application-controller:Dockerfile.application-controller"
    "gateway-controller:Dockerfile.gateway-controller"
)

for img in "${IMAGES[@]}"; do
    NAME="${img%%:*}"
    DOCKERFILE="${img##*:}"
    echo "Building wasmbed/${NAME}..."
    docker build -t localhost:5000/wasmbed/${NAME}:latest -f ${DOCKERFILE} . -q
    docker push localhost:5000/wasmbed/${NAME}:latest 2>&1 | tail -1
done

echo -e "${GREEN}✅ Tutte le immagini buildare e pushate${NC}"
echo ""

# Create namespace
echo "=== Setup Kubernetes ==="
kubectl create namespace wasmbed 2>/dev/null || echo "Namespace già esistente"

# Apply CRDs
echo "Applico CRDs..."
kubectl apply -f k8s/crds/device-crd.yaml
kubectl apply -f k8s/crds/application-crd.yaml
kubectl apply -f k8s/crds/gateway-crd.yaml

# Apply RBAC
echo "Applico RBAC..."
kubectl apply -f k8s/rbac/ &>/dev/null

# Generate TLS certificates
echo "Genero certificati TLS..."
cd /tmp
openssl req -x509 -newkey rsa:4096 -keyout ca-key.pem -out ca-cert.pem -days 365 -nodes -subj "/CN=Wasmbed-CA" &>/dev/null
openssl req -x509 -newkey rsa:4096 -keyout server-key.pem -out server-cert.pem -days 365 -nodes -subj "/CN=wasmbed-gateway" &>/dev/null

kubectl delete secret gateway-certificates -n wasmbed 2>/dev/null || true
kubectl create secret generic gateway-certificates -n wasmbed \
    --from-file=ca-cert.pem=/tmp/ca-cert.pem \
    --from-file=server-cert.pem=/tmp/server-cert.pem \
    --from-file=server-key.pem=/tmp/server-key.pem

cd - > /dev/null
echo -e "${GREEN}✅ Setup Kubernetes completato${NC}"
echo ""

# Update deployment images
echo "=== Deploy Services ==="
find k8s/deployments -name "*.yaml" -exec sed -i 's|image: wasmbed/|image: localhost:5000/wasmbed/|g' {} \;
find k8s/deployments -name "*.yaml" -exec sed -i 's|image: wasmbed-|image: localhost:5000/wasmbed/|g' {} \;

kubectl apply -f k8s/deployments/

echo "Attendo pods ready..."
sleep 20

kubectl get pods -n wasmbed

echo ""
echo -e "${GREEN}✅ Deployment completato${NC}"
echo ""

# Create Gateway CRD
echo "=== Creo Gateway ==="
cat > /tmp/gateway-1.yaml <<'EOF'
apiVersion: wasmbed.io/v1
kind: Gateway
metadata:
  name: gateway-1
  namespace: wasmbed
spec:
  endpoint: "wasmbed-gateway.wasmbed.svc.cluster.local:8080"
  port: 8080
  tlsPort: 8081
  capabilities: ["TLS", "HTTP"]
EOF

kubectl apply -f /tmp/gateway-1.yaml 2>/dev/null || echo "Gateway già esistente"

echo ""
echo "========================================="
echo "  DEPLOYMENT COMPLETATO"
echo "========================================="
echo ""
echo "Risorse create:"
echo "  - Pods: $(kubectl get pods -n wasmbed --no-headers 2>/dev/null | wc -l)"
echo "  - Services: $(kubectl get svc -n wasmbed --no-headers 2>/dev/null | wc -l)"
echo "  - Gateway: $(kubectl get gateways.wasmbed.io -n wasmbed --no-headers 2>/dev/null | wc -l)"
echo ""
echo "Endpoints:"
echo "  - Dashboard: http://\$(kubectl get nodes -o jsonpath='{.items[0].status.addresses[0].address}'):3000"
echo "  - API: kubectl port-forward -n wasmbed svc/wasmbed-api-server 3000:3001"
echo "  - Registry: localhost:5000"
echo ""
echo "Test device enrollment:"
echo "  kubectl port-forward -n wasmbed svc/wasmbed-api-server 3000:3001 &"
echo "  curl -X POST http://localhost:3000/api/v1/devices -H 'Content-Type: application/json' \\"
echo "    -d '{\"name\":\"test-device\",\"deviceType\":\"MCU\",\"mcuType\":\"Stm32F746gDisco\",\"gatewayId\":\"gateway-1\"}'"
echo ""
echo "========================================="
