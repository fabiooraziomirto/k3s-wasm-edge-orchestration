# Kubernetes-native WASM orchestration for embedded IoT (cloud–fog–edge)

Research prototype for orchestrating WebAssembly workloads on constrained endpoints through a K3s control plane, a TLS/CBOR gateway, and Renode-emulated Zephyr firmware.

The stack covers device enrollment, heartbeat supervision, declarative deployment via Kubernetes CRDs, and end-to-end experiment reproducibility.

## Repository layout

| Path | Contents |
|------|----------|
| `crates/` | Rust workspace (API server, gateway, controllers, protocol, Renode manager) |
| `k8s/` | Kubernetes manifests and CRDs |
| `scripts/` | Deploy, networking, and experiment harness |
| `zephyr-app/` | Zephyr firmware |
| `dashboard-react/` | Web dashboard |
| `doc/` | Technical documentation |
| `experiments/` | Measurement artifacts (100-trial campaign + ablation) |

## Quick start

```bash
# Prerequisites: Docker, K3s/kubectl, Rust toolchain
docker run -d -p 5000:5000 --name registry registry:2
./scripts/deploy-k3s.sh
./scripts/ensure-experiment-runtime.sh
curl -X POST http://127.0.0.1:3001/api/v1/devices/native-sim-1/renode/start -d '{}'
sudo ./scripts/setup-renode-net.sh
```

## Experiments

Reproduce the validated campaign:

```bash
TRIALS=100 ./scripts/run_experiment_campaign.sh
```

Artifacts: `experiments/20260609-070246/`

## External dependencies (not vendored)

- [Zephyr SDK](https://github.com/zephyrproject-rtos/sdk-ng) and `west` workspace
- [WAMR](https://github.com/bytecodealliance/wasm-micro-runtime)
- Renode (`antmicro/renode:nightly` via Docker)

See `doc/` for firmware build, TLS enrollment, and deployment topology.

## License

AGPL-3.0 — see [LICENSE](LICENSE).
