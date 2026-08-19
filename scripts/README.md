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
| `setup-renode-net.sh` | Bridge + one TAP per device, DHCP, NAT/DNAT (sudo). Run **before** starting emulation |

```bash
./scripts/ensure-experiment-runtime.sh
sudo ./scripts/setup-renode-net.sh native-sim-1     # bridge + per-device TAP
curl -X POST http://127.0.0.1:3001/api/v1/devices/native-sim-1/renode/start -d '{}' 
```

## Validation

| Script | Purpose |
|--------|---------|
| `verify-tls-and-deploy.sh` | TLS liveness + WASM deploy smoke test |
| `test-fleet-scalability.sh` | N devices → N distinct TLS sessions + fleet deploy (`DRY_RUN=1` for static checks) |
| `run-e2e-fleet.sh` | One-command E2E: preflight, build/deploy, port-forwards, network, fleet test, diagnostics |
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

### Energy metrics (optional)

`collect_experiment_metrics.py` also queries Prometheus/Kepler (see
[k8s/monitoring/](../k8s/monitoring/)) and attaches a `measurement_scope` +
`energy` field to every trial record, additive to the existing
latency/success-rate output — no existing field is removed or renamed.

```bash
kubectl -n wasmbed-monitoring port-forward svc/prometheus 9090:9090 &
python3 scripts/collect_experiment_metrics.py --trials 100 --output-dir /tmp/campaign
# or, if k8s/monitoring/ isn't deployed:
python3 scripts/collect_experiment_metrics.py --trials 100 --output-dir /tmp/campaign --no-energy
```

Every record's `measurement_scope` is `"host_shared_with_renode"` — these
are whole-host readings that include the `wasmbed-renode` container, not an
isolated per-process measurement. See
[doc/energy-tracking-assessment.md](../doc/energy-tracking-assessment.md)
before quoting them.

## Port-forward reference

| Local | Service | Remote |
|-------|---------|--------|
| 3001 | wasmbed-api-server | 3001 |
| 8080 | gateway-1-service | 8080 (HTTP) |
| 30443 | gateway-1-service | 8443 (TLS) |
| 3000 | wasmbed-dashboard | 3000 |
| 9090 | prometheus (wasmbed-monitoring ns) | 9090 |
