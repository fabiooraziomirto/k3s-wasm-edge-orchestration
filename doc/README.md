# Documentation index

English guides for the platform. Start with the [repository README](../README.md) for overview diagrams, then read architecture and sequence flows.

## Architecture

| Document | Description |
|----------|-------------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Cloud–fog–edge layout, components, TLS, board registration, data flows |
| [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md) | **Mermaid sequence diagrams** — enrollment, heartbeat, deploy/stop, monitoring |
| [TLS_CONNECTION.md](TLS_CONNECTION.md) | Southbound TLS transport and endpoint resolution |

## Deploy and validate

| Document | Description |
|----------|-------------|
| [K3S_DEPLOYMENT.md](K3S_DEPLOYMENT.md) | Install K3s, deploy manifests, troubleshooting |
| [RENODE_TLS_DEPLOY_VERIFICATION.md](RENODE_TLS_DEPLOY_VERIFICATION.md) | TAP networking, TLS connect, WASM deploy on emulated device |
| [TEST_GUIDE.md](TEST_GUIDE.md) | Step-by-step platform validation |
| [EXPERIMENTS.md](EXPERIMENTS.md) | Reproducing the 100-trial measurement campaign |
| [../scripts/README.md](../scripts/README.md) | Deployment and experiment scripts |

## Firmware and hardware

| Document | Description |
|----------|-------------|
| [FIRMWARE.md](FIRMWARE.md) | Zephyr firmware structure and build |
| [MCU_SUPPORT.md](MCU_SUPPORT.md) | Supported boards and Renode platforms |

## Suggested reading order

1. [../README.md](../README.md) (architecture Mermaid) → [ARCHITECTURE.md](ARCHITECTURE.md) → [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md)
2. [K3S_DEPLOYMENT.md](K3S_DEPLOYMENT.md) → [TEST_GUIDE.md](TEST_GUIDE.md) → [EXPERIMENTS.md](EXPERIMENTS.md)
