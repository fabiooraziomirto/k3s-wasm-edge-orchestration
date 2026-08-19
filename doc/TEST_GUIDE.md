# Platform test guide

Step-by-step validation of the Kubernetes-native WASM edge orchestration stack.

All commands assume the repository root:

```bash
cd k8s-wasm-edge-orchestration   # or your clone path
```

---

## 1. Build the Rust workspace

```bash
cargo check --workspace
cargo build --workspace
```

Expected: no compile errors.

---

## 2. Deploy on K3s

```bash
./scripts/deploy-k3s.sh
kubectl get pods -n wasmbed
```

Expected: API server, gateway, controllers, and dashboard pods reach `Running`.

Details: [K3S_DEPLOYMENT.md](K3S_DEPLOYMENT.md).

---

## 3. Runtime port-forwarding

```bash
./scripts/ensure-experiment-runtime.sh
curl -s http://127.0.0.1:3001/api/v1/devices | head
curl -s http://127.0.0.1:8080/api/v1/devices | head
```

Expected: HTTP 200 from API (3001) and gateway (8080).

---

## 4. Start emulated device

```bash
# 1. Prepare the host network first: bridge + one persistent TAP per device.
sudo ./scripts/setup-renode-net.sh native-sim-1
# 2. Then start the emulation.
curl -X POST http://127.0.0.1:3001/api/v1/devices/native-sim-1/renode/start \
  -H 'Content-Type: application/json' -d '{}'
```

Expected: Renode container running, the device's TAP (`wtap-<hash>`) up and enslaved to
`wasmbr0`, firmware connects via TLS to gateway.

Multiple devices at once (fleet) — the one-command entry point, which also runs the
preflight checks, rebuilds the api-server image (where the per-device emulation identity
is generated), sets up port-forwards and prints diagnostics on failure:

```bash
./scripts/run-e2e-fleet.sh --check     # prerequisites only
./scripts/run-e2e-fleet.sh -n 3        # full run
./scripts/run-e2e-fleet.sh --cleanup -n 3
```

The test itself, if the environment is already up:

```bash
./scripts/test-fleet-scalability.sh 3
```

Expected: 3 devices in phase `Connected`, **3 distinct TLS sessions** in the gateway,
3 distinct DHCP leases, and a WASM deploy reaching all 3.

Details: [RENODE_TLS_DEPLOY_VERIFICATION.md](RENODE_TLS_DEPLOY_VERIFICATION.md).

---

## 5. TLS enrollment smoke test

```bash
python3 scripts/test_enrollment.py
```

Expected: enrollment accepted, device visible in gateway HTTP API with fresh heartbeat. Wire format: [TLS_CONNECTION.md](TLS_CONNECTION.md), [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md#device-enrollment).

---

## 6. Deploy minimal WASM application

```bash
./scripts/verify-tls-and-deploy.sh
```

Expected: `Application` CRD `status.phase` reaches `Running`. Flow: [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md#application-deployment), [RENODE_TLS_DEPLOY_VERIFICATION.md](RENODE_TLS_DEPLOY_VERIFICATION.md).

---

## 7. Linux edge client (optional, no cluster)

Build and smoke-test the non-Zephyr edge daemon:

```bash
cargo build -p wasmbed-edge-client
openssl genpkey -algorithm ed25519 -out /tmp/device-key.pem
PUBKEY_HEX=$(openssl pkey -in /tmp/device-key.pem -pubout -outform DER | tail -c 32 | xxd -p -c 32)
RUST_LOG=info ./target/debug/wasmbed-edge-client --gateway 127.0.0.1:30443 --public-key "$PUBKEY_HEX"
```

With gateway down, expect a TCP connection error (not a panic).

---

## 8. Experiment campaign

```bash
TRIALS=100 ./scripts/run_experiment_campaign.sh
```

Details: [EXPERIMENTS.md](EXPERIMENTS.md).

---

## Troubleshooting

| Symptom | Check |
|---------|--------|
| Device stays `Unreachable` | Its TAP (`wtap-<hash>`, see `setup-renode-net.sh`), gateway TLS port-forward 30443 |
| Only one device connects out of N | TAP/MAC/publicKey must be per-device: check the generated `.resc` and `spec.publicKey` |
| Deploy timeout | Application controller logs, gateway “Waiting for TLS” messages |
| API unreachable | `ensure-experiment-runtime.sh`, pod status in `wasmbed` |
