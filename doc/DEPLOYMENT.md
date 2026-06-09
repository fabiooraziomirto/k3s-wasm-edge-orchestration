# Deployment Guide

## Prerequisites

### Software Required

- **Kubernetes**: Kubernetes cluster 1.24+ (K3S recommended)
- **Docker**: For building and containerization
- **kubectl**: Configured to access the cluster
- **Rust**: Toolchain 1.70+ (for building components)
- **Zephyr SDK**: For firmware compilation (optional for basic deployment)

### Hardware/Emulation

- **Renode**: For device emulation (via Docker)
- **Cluster Resources**: Minimum 4 CPU, 8GB RAM

## Initial Setup

### 1. Clone Repository

```bash
git clone <repository-url>
cd retrospect
```

### 2. Install K3S (if not already installed)

```bash
curl -sfL https://get.k3s.io | sh -s - --write-kubeconfig-mode 644
mkdir -p ~/.kube
sudo cp /etc/rancher/k3s/k3s.yaml ~/.kube/config
sudo chown $(id -u):$(id -g) ~/.kube/config
```

### 3. Deploy System

The easiest way to deploy is using the automated script:

```bash
./scripts/deploy-k3s.sh
```

This script will:
- Build all Docker images
- Set up local Docker registry
- Deploy all Kubernetes components
- Generate TLS certificates
- Create initial Gateway CRD

### 4. Setup Zephyr Workspace (Optional, for firmware compilation)

Zephyr e firmware sono descritti in **doc/FIRMWARE.md**. In sintesi: configurare `ZEPHYR_BASE` (workspace Zephyr) e SDK, poi in `zephyr-app/` usare `west build` per la board desiderata (es. `west build -b stm32f4_disc1 .`).

### 5. Certificati Gateway (per avvio Gateway in locale)

```bash
./scripts/generate-gateway-certs.sh
```

Genera certificati X.509 v3 in `config/certs/` (o in una directory passata come argomento). Usare `--private-key`, `--certificate`, `--client-ca` per avviare il Gateway.

## Manual Deployment (Alternative to deploy-k3s.sh)

If you prefer manual deployment or need to customize the process:

### 1. Deploy Namespace

```bash
kubectl apply -f k8s/namespace.yaml
```

### 2. Deploy CRDs

```bash
kubectl apply -f k8s/crds/
```

Custom Resource Definitions:
- Device (`devices.wasmbed.github.io`)
- Application (`applications.wasmbed.github.io`)
- Gateway (`gateways.wasmbed.io`)

### 3. Deploy RBAC

```bash
kubectl apply -f k8s/rbac/
```

Permissions for controllers and API server.

### 4. Generate TLS Certificates

Usare lo script (raccomandato):

```bash
./scripts/generate-gateway-certs.sh
# Crea config/certs/ (ca-cert.pem, server-cert.pem, server-key.pem)
```

Oppure manualmente con OpenSSL (salvare in `config/certs/`):

```bash
mkdir -p config/certs
# Generate CA certificate
openssl genrsa -out config/certs/ca-key.pem 4096
openssl req -new -x509 -days 365 -key config/certs/ca-key.pem -out config/certs/ca-cert.pem -subj "/CN=wasmbed-ca"

# Generate server certificate
openssl genrsa -out config/certs/server-key.pem 4096
openssl req -new -key config/certs/server-key.pem -out config/certs/server-csr.pem -subj "/CN=wasmbed-gateway"
openssl x509 -req -days 365 -in config/certs/server-csr.pem -CA config/certs/ca-cert.pem -CAkey config/certs/ca-key.pem -CAcreateserial -out config/certs/server-cert.pem

# Create Kubernetes secret
kubectl create secret generic gateway-certificates -n wasmbed \
  --from-file=ca-cert.pem=config/certs/ca-cert.pem \
  --from-file=server-cert.pem=config/certs/server-cert.pem \
  --from-file=server-key.pem=config/certs/server-key.pem
```

### 5. Build and Push Docker Images

```bash
# Build images
docker build -t localhost:5000/wasmbed/api-server:latest -f Dockerfile.api-server .
docker build -t localhost:5000/wasmbed/gateway:latest -f Dockerfile.gateway .
docker build -t localhost:5000/wasmbed/dashboard:latest -f Dockerfile.dashboard .
# ... build other images

# Push to local registry
docker push localhost:5000/wasmbed/api-server:latest
docker push localhost:5000/wasmbed/gateway:latest
docker push localhost:5000/wasmbed/dashboard:latest
# ... push other images
```

### 6. Deploy Components

```bash
kubectl apply -f k8s/deployments/
```

Deploys:
- API Server
- Gateway
- Controllers (Device, Application, Gateway)
- Dashboard

### 7. Verify Deployment

```bash
kubectl get pods -n wasmbed
kubectl get svc -n wasmbed
kubectl get crds | grep wasmbed
```

## Configurazione

### Config File

File principale: `config/wasmbed-config.yaml`

```yaml
api:
  bind_addr: "0.0.0.0:8080"
  
gateway:
  bind_addr: "0.0.0.0:40029"
  http_addr: "0.0.0.0:8080"
  
tls:
  ca_cert: "config/certs/ca-cert.pem"
  server_cert: "config/certs/server-cert.pem"
  server_key: "config/certs/server-key.pem"
```

### Environment Variables

Componenti principali utilizzano variabili d'ambiente:

**API Server:**
- `WASMBED_API_BIND_ADDR`
- `WASMBED_API_NAMESPACE`

**Gateway:**
- `WASMBED_GATEWAY_BIND_ADDR`
- `WASMBED_GATEWAY_PRIVATE_KEY`
- `WASMBED_GATEWAY_CERTIFICATE`
- `WASMBED_GATEWAY_CLIENT_CA`

## Accessing Services

### Dashboard

```bash
kubectl port-forward -n wasmbed svc/wasmbed-dashboard 3000:3000
```

Access: http://localhost:3000

### API Server

```bash
kubectl port-forward -n wasmbed svc/wasmbed-api-server 3001:3001
```

API: http://localhost:3001

### Gateway

```bash
# HTTP API
kubectl port-forward -n wasmbed svc/wasmbed-gateway 8080:8080

# TLS endpoint for devices
kubectl port-forward -n wasmbed svc/wasmbed-gateway 8081:8081
```

## Device Management

### Create Device

**Via Dashboard**:
1. Navigate to "Device Management"
2. Click "Create Device"
3. Enter device name
4. Select MCU type (e.g., `Stm32F746gDisco`)
5. Select target gateway
6. Click "Create"

**Via API**:
```bash
curl -X POST http://localhost:3001/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-device",
    "deviceType": "MCU",
    "mcuType": "Stm32F746gDisco",
    "gatewayId": "gateway-1"
  }'
```

**Via Kubernetes CRD**:
```yaml
apiVersion: wasmbed.github.io/v0
kind: Device
metadata:
  name: test-device
  namespace: wasmbed
spec:
  deviceType: MCU
  architecture: ARM_CORTEX_M
  mcuType: Stm32F746gDisco
  preferredGateway: gateway-1
```

### Start Device Emulation

```bash
curl -X POST http://localhost:3001/api/v1/devices/test-device/renode/start
```

### Check Device Status

```bash
kubectl get devices -n wasmbed
kubectl describe device test-device -n wasmbed
```

### View Device Logs

```bash
# Renode container logs
docker logs renode-test-device

# API server logs (device management)
kubectl logs -n wasmbed -l app=wasmbed-api-server | grep test-device
```

## Application Deployment

### Create Application

**Via Dashboard**:
1. Navigate to "Application Management"
2. Click "Create Application"
3. Enter application name and description
4. Upload WASM module or compile from Rust source
5. Configure target devices
6. Click "Create"

**Via API**:
```bash
curl -X POST http://localhost:3001/api/v1/applications \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-app",
    "description": "Test application",
    "wasmBytes": "<base64-encoded-wasm>",
    "targetDevices": {
      "deviceNames": ["test-device"]
    }
  }'
```

### Deploy Application

**Via Dashboard**:
1. Select Application
2. Click "Deploy"
3. Select target devices
4. Click "Deploy"

**Via API**:
```bash
curl -X POST http://localhost:3001/api/v1/applications/test-app/deploy
```

### Check Application Status

```bash
kubectl get applications -n wasmbed
kubectl describe application test-app -n wasmbed
```

## Monitoring

### Component Status

```bash
# All pods
kubectl get pods -n wasmbed

# All services
kubectl get svc -n wasmbed

# All CRDs
kubectl get devices -n wasmbed
kubectl get applications -n wasmbed
kubectl get gateways -n wasmbed
```

### Logs

```bash
# API Server
kubectl logs -n wasmbed -l app=wasmbed-api-server

# Gateway
kubectl logs -n wasmbed -l app=wasmbed-gateway

# Controllers
kubectl logs -n wasmbed -l app=wasmbed-device-controller
kubectl logs -n wasmbed -l app=wasmbed-application-controller
kubectl logs -n wasmbed -l app=wasmbed-gateway-controller

# Dashboard
kubectl logs -n wasmbed -l app=wasmbed-dashboard
```

### Dashboard Monitoring

The dashboard provides:
- Device status and health
- Application execution status
- Network topology visualization
- System health monitoring
- Infrastructure component status
- Real-time logs

Access at: http://localhost:3000 (after port-forwarding)

## Troubleshooting

### Pods Not Starting

```bash
# Check pod status
kubectl describe pod <pod-name> -n wasmbed

# Check logs
kubectl logs <pod-name> -n wasmbed

# Check events
kubectl get events -n wasmbed --sort-by=.metadata.creationTimestamp
```

### Device Not Connecting

1. **Check Gateway Endpoint**:
   ```bash
   kubectl get pods -n wasmbed -l app=wasmbed-gateway -o wide
   # Verify endpoint in device memory matches gateway pod IP
   ```

2. **Check TLS Certificates**:
   ```bash
   kubectl get secret gateway-certificates -n wasmbed
   ```

3. **Check Renode Container**:
   ```bash
   docker ps --filter "name=renode-"
   docker logs renode-<device-id>
   ```

4. **Check Device Status**:
   ```bash
   kubectl describe device <device-name> -n wasmbed
   ```

### Application Not Executing

1. **Verify WASM Format**:
   ```bash
   # Check application CRD
   kubectl get application <app-name> -n wasmbed -o yaml
   ```

2. **Check Device Logs**:
   ```bash
   docker logs renode-<device-id>
   ```

3. **Check Gateway Logs**:
   ```bash
   kubectl logs -n wasmbed -l app=wasmbed-gateway | grep <app-name>
   ```

### Network Issues

1. **Check Service Endpoints**:
   ```bash
   kubectl get endpoints -n wasmbed
   ```

2. **Check Network Policies**:
   ```bash
   kubectl get networkpolicies -n wasmbed
   ```

3. **Test Connectivity**:
   ```bash
   # From API server pod
   kubectl exec -n wasmbed <api-server-pod> -- curl http://wasmbed-gateway.wasmbed.svc.cluster.local:8080/health
   ```

## Scaling

### Gateway Scaling

```bash
kubectl scale deployment wasmbed-gateway -n wasmbed --replicas=3
```

### HPA Configuration

File: `k8s/gateway-hpa.yaml`

```bash
kubectl apply -f k8s/gateway-hpa.yaml
```

## Cleanup

### Complete Cleanup

Use the cleanup script:

```bash
./scripts/cleanup-k3s.sh
```

This will:
- Stop all port-forwards
- Remove Renode containers
- Remove Docker volumes
- Delete wasmbed namespace
- Optionally stop local registry

### Manual Cleanup

```bash
# Delete namespace (removes all resources)
kubectl delete namespace wasmbed

# Remove Renode containers
docker stop $(docker ps -q --filter "name=renode-")
docker rm $(docker ps -aq --filter "name=renode-")

# Remove Docker volumes
docker volume rm $(docker volume ls -q --filter "name=firmware-")
```

## Production Considerations

### Security

- Use proper CA with certificate rotation
- Configure network policies
- Enable full RBAC
- Use Kubernetes secrets for credentials
- Implement proper certificate provisioning for devices
- Use VPN or secure network for device-to-gateway communication

### Performance

- Configure resource limits for all pods
- Use HPA for auto-scaling
- Monitor metrics and set up alerting
- Optimize Renode container startup time
- Use persistent volumes for device state

### Backup

- Backup CRDs (Device, Application, Gateway)
- Backup configuration files
- Backup TLS certificates and keys
- Backup Kubernetes secrets

### High Availability

- Deploy multiple gateway instances
- Use load balancing for gateways
- Implement gateway health monitoring
- Set up automatic failover

## Additional Resources

- [K3S Deployment Guide](K3S_DEPLOYMENT.md) - K3S-specific instructions
- [Architecture Documentation](ARCHITECTURE.md) - System architecture details
- [Development Status](DEVELOPMENT_STATUS.md) - Current status and known issues
- [Real Device Integration](REAL_DEVICE_INTEGRATION.md) - Hardware integration guide
