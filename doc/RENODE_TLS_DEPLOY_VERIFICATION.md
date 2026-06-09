# Renode device → gateway TLS → Kubernetes and WASM deploy verification

This guide explains how to verify that (1) Renode-emulated devices connect to the gateway over TLS and appear active in Kubernetes, and (2) application deployment and WASM execution work end-to-end.

Related: [TEST_GUIDE.md](TEST_GUIDE.md), [TLS_CONNECTION.md](TLS_CONNECTION.md), [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md), [scripts/README.md](../scripts/README.md).

---

## 1. Expected flow

### 1.1 Device → gateway (TLS)

1. **API server / dashboard**: the user starts device emulation (Connect / Start emulation). The API server calls the gateway `POST /api/v1/devices/{device_id}/connect` and starts Renode via RenodeManager, passing the gateway endpoint (e.g. `http://wasmbed-gateway.wasmbed.svc.cluster.local:8080`).
2. **RenodeManager** (in API server):
   - Resolves the gateway pod IP: `kubectl get pods -n wasmbed -l app=wasmbed-gateway -o jsonpath={.items[0].status.podIP}` and forms `{pod_ip}:8081` (TLS port).
   - Writes the endpoint to device memory at `0x20001000`: 4-byte length, then the host:port string (e.g. `10.42.0.12:8081`).
   - Starts Renode with the correct platform (e.g. STM32F746) and Zephyr firmware.
3. **Zephyr firmware** (in Renode):
   - Reads the endpoint from `0x20001000` (length + host:port string).
   - Connects to the gateway TLS port (8081) via `network_connect_tls(host, port)`.
   - Runs enrollment (EnrollmentRequest → PublicKey → EnrollmentAcknowledgment) and heartbeat.
4. **Gateway** (port 8081 TLS):
   - On TLS connect, verifies the client certificate and finds the Device CRD by public key.
   - Updates the Device CRD: `DeviceStatusUpdate::mark_connected()` (phase: Connected, gateway, lastHeartbeat).
   - On the first message, calls `mark_device_tls_connected(device_id)` (device visible as connected for deploy).
   - On heartbeat, updates `DeviceStatusUpdate::update_heartbeat()` and `http_server.update_heartbeat()`.

**Verification (connection + K8s):**

- After starting a device with Renode firmware that connects to the gateway:
  - `kubectl get devices -n wasmbed` should show the device with **phase: Connected** (and `status.gateway` set).
  - Gateway logs should show “TLS client certificate verification successful” and “Marked device X as having active TLS connection”.
  - The dashboard device list should show the device as connected (data from K8s).

### 1.2 Application deploy and WASM runtime

1. **Dashboard**: user deploys an application whose target devices include the connected device.
2. **API server**: `POST /api/v1/applications/:id/deploy` → for each target device, calls gateway `POST {gateway}/api/v1/devices/{device_id}/deploy` (body: app_id, name, wasm_bytes).
3. **Gateway**:
   - Reads the Application CRD (GET) for WASM bytes (`spec.wasmBytes`, base64).
   - Registers the deploy in memory (`register_application`).
   - Waits until the device has `tls_connected == true` (up to 30 s).
   - PATCHes the Application CRD: phase Deploying, `deviceStatuses[device_id]` Deploying.
   - Sends **ServerMessage::DeployApplication** { app_id, name, wasm_bytes, config } to the device over TLS.
   - Message is CBOR-serialized (minicbor) with a 4-byte big-endian u32 length prefix + CBOR payload.
4. **Device (firmware)**:
   - Reads 4 bytes (len), then `len` bytes of CBOR.
   - Parses CBOR as ServerMessage: DeployApplication is a 5-element array: tag=5 (u32), app_id (str), name (str), wasm_bytes (bytes), config (null or object).
   - Calls `wamr_load_module(wasm_bytes, len, &module_id)`, `wamr_instantiate(module_id, &instance_id)`, and optionally `wamr_call_function()`.
   - Sends **ClientMessage::ApplicationDeployAck** { app_id, success, error } (CBOR with length prefix per client protocol).
5. **Gateway** (on DeployAck):
   - Updates Application CRD: phase Running or Failed, `deviceStatuses[device_id]`, lastUpdated, error on failure.

**Verification (deploy + WASM):**

- After a dashboard deploy to a device with active TLS:
  - `kubectl get applications -n wasmbed -o yaml` should show `status.phase: Running` (or Failed) and updated `status.deviceStatuses.<device_id>`.
  - The dashboard application list should match K8s status and deployed devices.
  - On the device, the WASM module must be loaded and executed (WAMR). Without CBOR parsing in firmware, the gateway still sends the message but the device will not DeployAck or run WASM.

---

## 2. Implementation status (codebase)

### 2.1 Implemented

| Component | Details |
|-----------|---------|
| **RenodeManager** | Gateway pod IP resolution, endpoint write at 0x20001000, Renode start with correct platform script. |
| **Gateway TLS** | TLS server on 8081, client cert verification, Device lookup by public key, Device CRD updates (Connected, Enrolling, Enrolled, heartbeat). |
| **Gateway HTTP** | `POST .../devices/:id/connect` registers device in memory; deploy waits for `tls_connected` and sends `ServerMessage::DeployApplication` over TLS (length-prefix + CBOR). |
| **Application CRD status** | Gateway PATCHes phase and deviceStatuses on deploy and on DeployAck/StopAck/ApplicationStatus. |
| **Zephyr firmware** | Reads endpoint from 0x20001000, TLS connect, enrollment/heartbeat messages; **wasmbed_protocol_handle_message()** receives bytes and decodes CBOR for deploy. |
| **WAMR (firmware)** | `wamr_integration.c`: `wamr_init()`, `wamr_load_module()`, `wamr_instantiate()`, `wamr_call_function()` available. |

### 2.2 To verify / extend

| Component | Status | Action |
|-----------|--------|--------|
| **Firmware: CBOR parsing** | Implemented in `wasmbed_protocol.c` for DeployApplication. | Re-run E2E after firmware or gateway changes. |
| **Firmware: DeployApplication handling** | Loads/instantiates WASM and sends DeployAck. | Confirm `wamr_call_function()` for exported entry points if needed. |
| **E2E with real Renode** | See [TEST_GUIDE.md](TEST_GUIDE.md). | Start cluster, gateway, API server, Renode device; verify Connected phase; deploy and check Application status and WASM execution. |

---

## 3. Wire format (firmware implementation)

- **Gateway → device** (each ServerMessage):
  - 4 bytes: payload length (big-endian u32).
  - N bytes: ServerMessage CBOR (minicbor).
- **DeployApplication** CBOR: 5-element array `[5, app_id_str, name_str, wasm_bytes_bstr, null]` (tag 5 = SERVER_DEPLOY_APPLICATION). If config is not null, the fifth element is an object (memory_limit, cpu_time_limit, env map, args array).
- **Device → gateway**: ClientMessage CBOR with the same length-prefix scheme (`tls_utils` reads `buffer[..n]` and `minicbor::decode::<ClientMessage>`). Device sends: 4 bytes (len) + CBOR(ApplicationDeployAck). ApplicationDeployAck: 4 elements: tag 5 (CLIENT_APPLICATION_DEPLOY_ACK), app_id (str), success (bool), error (null or str).

---

## 4. Practical verification checklist

### TLS connection and Kubernetes

1. [ ] K8s cluster active, namespace `wasmbed`, Device/Application/Gateway CRDs installed.
2. [ ] Gateway running with certificates and TLS on 8081.
3. [ ] API server running; RenodeManager can start Renode (Docker/kubectl as needed).
4. [ ] Device CRD created with public key matching firmware certificate (or use pairing mode + enrollment).
5. [ ] Start device emulation (Connect) and wait for firmware to connect to gateway.
6. [ ] Verify: `kubectl get devices -n wasmbed` shows **phase: Connected** and gateway set.
7. [ ] Verify: gateway logs show TLS connection and device marked TLS connected.

### Deploy and WASM

1. [ ] Application CRD with `spec.wasmBytes` (base64) and targetDevices including the connected device.
2. [ ] Deploy from dashboard (or `POST .../applications/:id/deploy`).
3. [ ] Verify: Application `status.phase` and `status.deviceStatuses` update (Deploying → Running/Failed).
4. [x] Firmware implements CBOR + WAMR + DeployAck: confirm WASM load/instantiate and gateway receives DeployAck (phase: Running).

---

## 5. Sustained TLS and WASM deploy

### 5.1 Sustained TLS connection

- **Firmware**: sends `ClientMessage::Heartbeat` (CBOR `[0]`) every 25 s via `wasmbed_protocol_tick()` (main loop in `main.c`). Payload: 4-byte length (big-endian) + `0x81 0x00`.
- **Gateway**: on Heartbeat, updates `DeviceStatusUpdate::update_heartbeat()` (Device CRD `status.last_heartbeat`) and `http_server.update_heartbeat(device_id)`.
- **Monitor**: gateway task `check_heartbeat_timeouts` (30 s period, default 90 s timeout) marks unreachable devices when heartbeat expires.
- **Recovery**: gateway and device controller can re-register unreachable devices on the next successful heartbeat (see `wasmbed-gateway/src/main.rs`, `wasmbed-device-controller/src/main.rs`).
- **Verify**: with a connected Renode device, `kubectl get devices -n wasmbed -o jsonpath='{.items[*].status.last_heartbeat}'` shows recent timestamps; gateway logs show “Heartbeat from …”.

### 5.2 WASM deploy on Renode devices

- **Flow**: Dashboard/API `POST .../applications/:id/deploy` → API server → gateway `POST .../devices/:id/deploy` → gateway sends `ServerMessage::DeployApplication` over TLS.
- **Firmware**: `wasmbed_protocol_handle_message()` handles DeployApplication, decodes wasm_bytes, calls WAMR load/instantiate, sends `ApplicationDeployAck`.
- **Gateway**: on `ClientMessage::ApplicationDeployAck`, updates Application CRD (Running/Failed).
- **Verify**: after deploy, `kubectl get applications -n wasmbed -o yaml` shows `status.phase: Running` and `deviceStatuses.<device_id>.status: Running` when DeployAck succeeded.

Automated smoke: `./scripts/verify-tls-and-deploy.sh`

---

## 6. Code references

- Endpoint memory write: `wasmbed-qemu-manager/src/lib.rs` (build_renode_args, 0x20001000).
- Endpoint read + TLS: `zephyr-app/src/wasmbed_protocol.c` (read_gateway_endpoint, network_connect_tls).
- Heartbeat and deploy (firmware): `zephyr-app/src/wasmbed_protocol.c`, `main.c`.
- Gateway TLS and CRD updates: `wasmbed-gateway/src/main.rs`, `http_api.rs`.
- CBOR protocol: `wasmbed-protocol/src/cbor.rs`.
- TLS length-prefix send: `wasmbed-tls-utils/src/lib.rs`.
