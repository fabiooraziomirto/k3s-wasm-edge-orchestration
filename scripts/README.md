# Scripts

Deployment, networking, validation, and experiment automation. Run from the repository root.

## Deployment

| Script | Purpose |
|--------|---------|
| `deploy-k3s.sh` | Build images, push to local registry, apply K8s manifests |
| `cleanup-k3s.sh` | Remove namespace, Renode containers, port-forwards |
| `generate-gateway-certs.sh` | Generate gateway TLS certificates in `config/certs/` |

```bash
./scripts/deploy-k3s.sh
kubectl get pods -n wasmbed
```

## Runtime and networking

| Script | Purpose |
|--------|---------|
| `ensure-experiment-runtime.sh` | Restart port-forwards (3001, 8080, 30443) |
| `setup-renode-net.sh` | Configure TAP, DHCP, DNAT for emulated devices (sudo) |

```bash
./scripts/ensure-experiment-runtime.sh
curl -X POST http://127.0.0.1:3001/api/v1/devices/native-sim-1/renode/start -d '{}'
sudo ./scripts/setup-renode-net.sh
```

## Validation

| Script | Purpose |
|--------|---------|
| `verify-tls-and-deploy.sh` | TLS liveness + WASM deploy smoke test |
| `test_enrollment.py` | Direct gateway TLS/CBOR enrollment test |

See [doc/RENODE_TLS_DEPLOY_VERIFICATION.md](../doc/RENODE_TLS_DEPLOY_VERIFICATION.md) and [doc/TLS_CONNECTION.md](../doc/TLS_CONNECTION.md).

## Experiments

| Script | Purpose |
|--------|---------|
| `collect_experiment_metrics.py` | Run trials, emit JSON metrics |
| `postprocess_experiment.py` | JSON → JSONL, summary tables |
| `run_experiment_campaign.sh` | Full campaign (warm-up + hardened + ablation) |
| `generate_ablation_figure.py` | Ablation comparison figure |

```bash
TRIALS=100 ./scripts/run_experiment_campaign.sh
```

See [doc/EXPERIMENTS.md](../doc/EXPERIMENTS.md).

## Port-forward reference

| Local | Service | Remote |
|-------|---------|--------|
| 3001 | wasmbed-api-server | 3001 |
| 8080 | gateway-1-service | 8080 (HTTP) |
| 30443 | gateway-1-service | 8443 (TLS) |
| 3000 | wasmbed-dashboard | 3000 |
