# Wasmbed K3S Deployment Guide

**Last Updated**: 2026-01-11

This guide covers the deployment of Wasmbed on a pure K3S Kubernetes cluster.

## Table of Contents
- [Prerequisites](#prerequisites)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [Deployment Process](#deployment-process)
- [Verification](#verification)
- [Troubleshooting](#troubleshooting)

## Prerequisites

### System Requirements
- Linux system (Ubuntu 24.04 LTS tested)
- 4GB+ RAM
- 20GB+ disk space
- Root/sudo access

### Software Requirements
- **K3S** - Lightweight Kubernetes
- **Docker** - Container runtime
- **kubectl** - Kubernetes CLI

### K3S Installation

```bash
# Install K3S
curl -sfL https://get.k3s.io | sh -s - --write-kubeconfig-mode 644

# Configure kubectl
mkdir -p ~/.kube
sudo cp /etc/rancher/k3s/k3s.yaml ~/.kube/config
sudo chown $(id -u):$(id -g) ~/.kube/config

# Verify installation
kubectl get nodes
```

## Quick Start

```bash
cd /home/lucadag/18_10_23_retrospect/retrospect

# Deploy entire system
./scripts/deploy-k3s.sh

# Wait for pods to be ready (~2 minutes)
kubectl get pods -n wasmbed -w
```

## Architecture

### Components

```
Wasmbed System Architecture (K3S)
=====================================

┌─────────────────────────────────────────────────────┐
│              K3S Control Plane                      │
│                                                     │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐   │
│  │ Dashboard  │  │ API Server │  │  Gateway   │   │
│  │ (React)    │  │  (Rust)    │  │  (Rust)    │   │
│  │  :3000     │  │  :3001     │  │ :8080/8081 │   │
│  └────────────┘  └────────────┘  └────────────┘   │
│                                                     │
│  ┌────────────────────────────────────────────┐   │
│  │         Controllers (Rust)                 │   │
│  │  • Device Controller                       │   │
│  │  • Application Controller                  │   │
│  │  • Gateway Controller                      │   │
│  └────────────────────────────────────────────┘   │
│                                                     │
│  ┌────────────────────────────────────────────┐   │
│  │         Custom Resources (CRDs)            │   │
│  │  • devices.wasmbed.github.io               │   │
│  │  • applications.wasmbed.github.io          │   │
│  │  • gateways.wasmbed.io                     │   │
│  └────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
                      │
                      │ Docker Socket
                      │ /var/run/docker.sock
                      ▼
         ┌──────────────────────────┐
         │   Renode Containers      │
         │  (Device Emulation)      │
         │  • Zephyr RTOS           │
         │  • WAMR Runtime          │
         │  • TLS Client            │
         └──────────────────────────┘
```

### Networking

- **CNI**: Flannel (K3S default)
- **Service Network**: 10.43.0.0/16
- **Pod Network**: 10.42.0.0/16
- **External Access**: LoadBalancer (dashboard), Port-Forward (API)

### Storage

- **Registry**: Local Docker registry on `localhost:5000`
- **Firmware**: Embedded in API server image
- **Certificates**: Kubernetes secrets
- **Device Data**: Kubernetes CRDs

## Deployment Process

### Phase 1: Infrastructure Setup

```bash
# The deploy script performs:
# 1. K3S verification
# 2. Docker check
# 3. Local registry startup on port 5000
```

### Phase 2: Image Building

```bash
# Images built and pushed to localhost:5000:
# - wasmbed/api-server:latest
# - wasmbed/gateway:latest
# - wasmbed/dashboard:latest
# - wasmbed/device-controller:latest
# - wasmbed/application-controller:latest
# - wasmbed/gateway-controller:latest
```

### Phase 3: Kubernetes Resources

```bash
# 1. Namespace creation
kubectl create namespace wasmbed

# 2. CRD installation
kubectl apply -f k8s/crds/

# 3. RBAC configuration
kubectl apply -f k8s/rbac/

# 4. TLS certificates
#    Auto-generated and stored in secret "gateway-certificates"

# 5. Service deployment
kubectl apply -f k8s/deployments/
```

### Phase 4: Gateway Configuration

```bash
# Create initial gateway resource
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
```

## Verification

### Check All Pods

```bash
kubectl get pods -n wasmbed

# Expected output (7 pods):
# - wasmbed-api-server-xxx
# - wasmbed-gateway-xxx
# - wasmbed-dashboard-xxx
# - wasmbed-device-controller-xxx
# - wasmbed-application-controller-xxx
# - wasmbed-gateway-controller-xxx
# - gateway-1-deployment-xxx
```

### Check Services

```bash
kubectl get svc -n wasmbed

# Expected services:
# - wasmbed-api-server (ClusterIP :3001)
# - wasmbed-gateway (ClusterIP :8080,:8081)
# - wasmbed-dashboard (LoadBalancer :3000)
```

### Check CRDs

```bash
# Verify Custom Resource Definitions
kubectl get crd | grep wasmbed

# Expected:
# applications.wasmbed.github.io
# devices.wasmbed.github.io
# gateways.wasmbed.io
```

### Check Registry

```bash
# List images in local registry
curl -s http://localhost:5000/v2/_catalog | jq
```

### Test Device Enrollment

```bash
# Port-forward API server
kubectl port-forward -n wasmbed svc/wasmbed-api-server 3000:3001 &

# Create test device
curl -X POST http://localhost:3000/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name":"test-device",
    "deviceType":"MCU",
    "mcuType":"Stm32F746gDisco",
    "gatewayId":"gateway-1"
  }'

# Check device created
kubectl get devices.wasmbed.github.io -n wasmbed
```

### Test Renode Emulation

```bash
# Start Renode for device
curl -X POST http://localhost:3000/api/v1/devices/test-device/renode/start

# Verify Renode container
docker ps --filter "name=renode-test-device"

# Check Renode logs
docker logs renode-test-device
```

## Troubleshooting

### Pods in CrashLoopBackOff

```bash
# Check pod description
kubectl describe pod -n wasmbed <pod-name>

# Check logs
kubectl logs -n wasmbed <pod-name>

# Common causes:
# - Image pull errors (check registry)
# - Missing secrets (gateway-certificates)
# - RBAC permission issues
```

### ImagePullBackOff

```bash
# Verify registry is running
docker ps | grep wasmbed-registry

# Check if image exists
curl http://localhost:5000/v2/wasmbed/api-server/tags/list

# Rebuild and push image
docker build -t localhost:5000/wasmbed/api-server:latest -f Dockerfile.api-server .
docker push localhost:5000/wasmbed/api-server:latest

# Restart pod
kubectl delete pod -n wasmbed -l app=wasmbed-api-server
```

### Gateway Not Ready

```bash
# Check certificates secret exists
kubectl get secret gateway-certificates -n wasmbed
kubectl describe secret gateway-certificates -n wasmbed

# Should contain: ca-cert.pem, server-cert.pem, server-key.pem

# Check gateway logs
kubectl logs -n wasmbed -l app=wasmbed-gateway

# Verify TLS port
kubectl exec -n wasmbed deployment/wasmbed-gateway -- netstat -tlnp | grep 8081
```

### Renode Container Not Starting

```bash
# Check API server logs
kubectl logs -n wasmbed -l app=wasmbed-api-server --tail=50

# Common issues:
# 1. Firmware not found in image
docker run --rm localhost:5000/wasmbed/api-server:latest \
  ls /app/zephyr-workspace/build/

# 2. Docker socket not accessible
kubectl exec -n wasmbed deployment/wasmbed-api-server -- ls -l /var/run/docker.sock

# 3. Permission issues
kubectl get deployment wasmbed-api-server -n wasmbed -o yaml | grep -A 5 securityContext
```

### Port-Forward Connection Refused

```bash
# Check service exists
kubectl get svc wasmbed-api-server -n wasmbed

# Check endpoints
kubectl get endpoints wasmbed-api-server -n wasmbed

# Test direct to pod
POD=$(kubectl get pod -n wasmbed -l app=wasmbed-api-server -o name)
kubectl exec -n wasmbed $POD -- curl -s http://localhost:3001/health
```

### Network Issues Between Pods

```bash
# Check CoreDNS
kubectl get pods -n kube-system | grep coredns

# Test DNS resolution from pod
kubectl exec -n wasmbed deployment/wasmbed-api-server -- \
  nslookup wasmbed-gateway.wasmbed.svc.cluster.local

# Check Flannel CNI
kubectl get pods -n kube-system | grep flannel
```

## Performance Tuning

### Resource Limits

Edit deployments to adjust resources:

```yaml
resources:
  requests:
    memory: "256Mi"
    cpu: "100m"
  limits:
    memory: "512Mi"
    cpu: "500m"
```

### Renode Performance

```bash
# Limit concurrent Renode containers
# Edit API server configuration:
MAX_CONCURRENT_RENODE=5
```

## Cleanup

### Remove Wasmbed Only

```bash
./scripts/cleanup-k3s.sh
```

### Complete K3S Uninstall

```bash
# Stop all Wasmbed components
./scripts/cleanup-k3s.sh

# Uninstall K3S
sudo /usr/local/bin/k3s-uninstall.sh
```

## Best Practices

1. **Always use the deployment script** - Ensures consistent setup
2. **Monitor resource usage** - K3S + Renode can be resource-intensive
3. **Backup CRDs** - Export important device/application definitions
4. **Use local registry** - Avoids external dependencies
5. **Check logs regularly** - Catch issues early

## Known Issues

### API Server Not Responding (2026-01-11)

**Issue**: API server pod runs but doesn't accept HTTP connections

**Symptoms**:
- Pod status: Running
- No HTTP server startup logs
- `curl` to localhost:3001 returns empty response

**Investigation**: Ongoing

**Workaround**: None currently - system deployment 95% complete

## References

- [TLS Connection Documentation](./TLS_CONNECTION.md)
- [Sequence Diagrams](./SEQUENCE_DIAGRAMS.md)
- [K3S Documentation](https://docs.k3s.io/)
- [Renode Documentation](https://renode.readthedocs.io/)
- [Zephyr RTOS Documentation](https://docs.zephyrproject.org/)

## Support

For issues or questions:
1. Check logs: `kubectl logs -n wasmbed <pod-name>`
2. Verify resources: `kubectl get all -n wasmbed`
3. Review this documentation
4. Check container status: `docker ps -a`
