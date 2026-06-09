#!/bin/bash

echo "========================================="
echo "  Wasmbed K3S Cleanup Script"
echo "========================================="
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

read -p "Rimuovere TUTTO il deployment Wasmbed? (y/N): " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Operazione annullata"
    exit 0
fi

echo ""
echo "=== Pulizia in corso ==="

# Kill port-forwards
echo "Termino port-forwards..."
pkill -f "port-forward.*wasmbed" 2>/dev/null || true

# Stop Renode containers
echo "Fermo containers Renode..."
docker ps -a --filter "name=renode-" -q | xargs -r docker rm -f 2>/dev/null || true

# Remove Docker volumes
echo "Rimuovo volumi Docker..."
docker volume ls -q --filter "name=firmware-" | xargs -r docker volume rm 2>/dev/null || true

# Delete namespace (this removes all resources)
echo "Rimuovo namespace wasmbed..."
kubectl delete namespace wasmbed --grace-period=0 --force 2>/dev/null || true

# Wait for namespace deletion
echo "Attendo completamento..."
sleep 5

# Optional: stop registry
read -p "Fermare anche il registry locale? (y/N): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Fermo registry..."
    docker stop wasmbed-registry 2>/dev/null || true
    docker rm wasmbed-registry 2>/dev/null || true
fi

echo ""
echo -e "${GREEN}✅ Pulizia completata${NC}"
echo ""
echo "Verifica:"
docker ps --filter "name=renode-" --filter "name=wasmbed-registry"
echo ""
kubectl get ns wasmbed 2>&1 | grep -q "NotFound" && echo "Namespace rimosso" || echo "⚠️  Namespace ancora presente"
echo ""
echo "Per reinstallare: ./deploy-k3s.sh"
echo ""
