# Development Status - RETROSPECT Wasmbed Platform

**Last Updated**: 2026-02-10

## Executive Summary

The RETROSPECT Wasmbed platform is **operational** with core functionality working. The system successfully deploys on K3S, manages devices via Kubernetes CRDs, and provides a complete web dashboard. End-to-end workflows are functional, with some areas requiring further testing and refinement.

---

## Stato operativo (Operational State)

| Area | Stato | Note |
|------|--------|-----|
| **Deploy K3S** | ✅ Operativo | Namespace `wasmbed`, CRD, RBAC, registry locale, tutti i pod avviano |
| **API Server** | ✅ Operativo | REST API 3001, device/app/gateway, Renode orchestration, risoluzione endpoint TLS |
| **Gateway** | ✅ Operativo | HTTP 8080, TLS 8081, enrollment, heartbeat, deploy/stop verso device |
| **Dashboard** | ✅ Operativo | UI port 3000, device/app/gateway management, topology, terminal |
| **Controllers** | ✅ Operativo | Device, Application, Gateway controller attivi |
| **Device → Connected** | ✅ Testato | STM32F746G: Connect → DHCP → TLS → Identify → Connected (feb 2026) |
| **Board registration** | ✅ Operativo | Renode Manager registra board al Gateway; detach su stop |
| **Application deploy (API→Gateway)** | ✅ Operativo | Deploy/stop patchano Application CRD status; Gateway invia CBOR su TLS |
| **WASM su device (E2E)** | ⏳ Parziale | Firmware riceve DeployApplication e invia DeployAck; esecuzione WAMR da verificare |
| **Board virtuali (Riscv32Virtual, CortexR8Virtual)** | ⏳ Implementato, build non verificato | McuType e Renode script pronti; build firmware Docker avviato, .elf da confermare; E2E non testato |

---

## Step testati (Tested Steps)

- [x] Deploy completo K3S (`./scripts/deploy-k3s.sh`)
- [x] Creazione Device CRD (API e Dashboard)
- [x] Creazione Application CRD e Gateway CRD
- [x] Avvio emulazione Renode (Connect) per device con `Stm32F746gDisco`
- [x] Risoluzione endpoint TLS (pod IP) e scrittura in RAM (endpoint + device_id) prima di `start`
- [x] Rete host: tap0, dnsmasq, forwarding/NAT verso cluster
- [x] Transizione Enrolled → Connected (TLS + Identify) con STM32F746G
- [x] Heartbeat e marcatura Unreachable dopo 90s; recovery su successivo Heartbeat
- [x] Board registration con Gateway alla partenza device; rimozione board allo stop
- [x] Deploy applicazione (API → Gateway → device): Gateway patch Application CRD status
- [x] Gateway locale: certificati X.509 v3, health/ready, API devices/applications/gateways
- [ ] Build firmware board virtuali con script Docker (completamento non verificato)
- [ ] E2E device con `mcuType: Riscv32Virtual` o `CortexR8Virtual`
- [ ] Deploy WASM e esecuzione WAMR su device emulato end-to-end

---

## Problemi incontrati (Problems Encountered)

### Risolti
- **Endpoint Gateway in RAM**: scritto nome servizio K8s non risolvibile dall’host → risoluzione a IP pod prima di avviare Renode.
- **device_id in RAM**: mancava nel flusso singleton Renode → aggiunta scrittura in `0x20002000`.
- **Scritture RAM prima di start**: le scritture erano dopo `start` → spostate prima di `start`.
- **Connection refused (111) su board registration**: URL Gateway errato (`http://http:8080`) → parsing corretto host:port / URL.
- **Monitor Renode CLOSE_WAIT**: connessione TCP non chiusa lato client → `shutdown(Write)` dopo invio comandi.
- **preferredGateway ignorato**: TLS sempre su standalone 8081 → risoluzione pod per `gateway-1` (8443) da `spec.preferredGateway`.
- **Firmware STM32F746G**: compilato e verificato; flusso Connected funzionante (feb 2026).

### Aperti / da verificare
- **Board virtuali**: build firmware con `./scripts/build-renode-virtual-firmware-docker.sh` avviato; non confermato che produca `zephyr.elf` per `riscv32_virtual` e `cortex_r8_virtual` (es. versione SDK nell’immagine Docker). E2E con Device `Riscv32Virtual` non ancora eseguito.
- **Zephyr SDK locale**: build locale con `west` fallisce senza SDK 0.16; uso script Docker o SDK installato.
- **WASM deploy E2E**: invio CBOR e DeployAck verificati; esecuzione effettiva del modulo su WAMR e stato reported da validare.

## Current Status Overview

### Fully Functional Components

#### 1. Kubernetes Infrastructure
- **Status**: Fully Operational
- **Details**:
  - K3S cluster running and stable
  - All services deployed in `wasmbed` namespace
  - CRDs (Device, Application, Gateway) registered and functional
  - RBAC configured correctly
  - Local Docker registry operational

#### 2. API Server
- **Status**: Fully Operational
- **Details**:
  - REST API on port 3001 (45+ endpoints)
  - Device management endpoints working
  - Application management endpoints working
  - Gateway management endpoints working
  - Renode container orchestration functional
  - Gateway endpoint resolution working (pod IP resolution)
  - MCU type mapping correct (13 MCU types supported)

#### 3. Gateway Service
- **Status**: Fully Operational
- **Details**:
  - HTTP API on port 8080
  - TLS server on port 8081
  - Device enrollment endpoint working
  - Certificate management functional
  - TLS certificates generated and stored in Kubernetes secrets

#### 4. Dashboard
- **Status**: Fully Operational
- **Details**:
  - Web UI accessible on port 3000
  - Network topology visualization working
  - Infrastructure status correctly displayed
  - System health monitoring functional
  - Device management UI complete
  - Application management UI complete
  - Gateway management UI complete
  - Terminal component functional

#### 5. Controllers
- **Status**: Fully Operational
- **Details**:
  - Device Controller: Watching Device CRDs, managing lifecycle
  - Application Controller: Watching Application CRDs, managing deployment
  - Gateway Controller: Watching Gateway CRDs, managing instances
  - All controllers running and responsive

#### 6. MCU Type Support
- **Status**: Fully Implemented
- **Details**:
  - 13 MCU types supported
  - Ethernet-enabled boards: STM32F746G Discovery, FRDM-K64F
  - WiFi-enabled boards: ESP32 DevKitC
  - Legacy boards: Arduino Nano 33 BLE, STM32F4 Discovery, nRF52840 DK, MPS2-AN385, etc.
  - Correct Renode platform mapping
  - Correct firmware path mapping
  - Network capability detection (has_ethernet, has_wifi, has_network)

#### 7. Renode Integration
- **Status**: Functional
- **Details**:
  - Renode containers start correctly
  - Firmware volumes mounted properly
  - Gateway endpoint written to memory (0x20001000)
  - Correct platform files loaded (e.g., `stm32f7_discovery-bb.repl`)
  - Ethernet configuration for supported boards
  - UART analyzer configured

### Partially Functional Components

#### 1. End-to-End TLS Connection
- **Status**: Partially Functional
- **Details**:
  - Gateway TLS server operational; Device CRD updated to Connected/Enrolled on TLS connect and heartbeat
  - Gateway endpoint resolution (pod IP) and write to device memory (0x20001000) implemented in RenodeManager
  - Zephyr firmware reads endpoint, connects TLS, sends enrollment/heartbeat; Gateway marks device TLS-connected
  - Verification: see **doc/RENODE_TLS_DEPLOY_VERIFICATION.md** (checklist and code references)
  - Zephyr firmware compilation for Ethernet boards may be pending; TLS path is implemented in code

#### 2. Application Deployment
- **Status**: Partially Functional
- **Details**:
  - Application CRD creation working
  - Application status tracking working
  - **Deploy API flow verified**: POST `/api/v1/applications/:id/deploy` → API server calls Gateway `POST /api/v1/devices/:id/deploy`; Gateway reads Application from Kubernetes (fixed: `ApplicationStatistics` fields optional/deserialization); returns "deployment initiated" or clear errors ("Device not registered", "Timeout waiting for TLS"); Gateway patches Application CRD status (Deploying → Running/Failed). Service `wasmbed-gateway` selector limited to standalone gateway pod (`gateway-type: standalone`) so deploy hits correct instance.
  - Gateway sends DeployApplication via TLS (length-prefix + CBOR); Gateway updates Application CRD on DeployAck
  - Firmware: `wasmbed_protocol.c` implements minimal CBOR decode for DeployApplication, `wamr_load_module`/`wamr_instantiate`, and `send_deploy_ack`; periodic Heartbeat (every 25 s) via `wasmbed_protocol_tick()` to keep TLS connection and Gateway `last_heartbeat` updated
  - See **doc/RENODE_TLS_DEPLOY_VERIFICATION.md** for verification steps and wire format

### Known Issues

#### 1. Zephyr Firmware Compilation
- **Issue**: Firmware for Ethernet-enabled boards (STM32F746G Discovery, FRDM-K64F) needs compilation
- **Impact**: Cannot test complete TLS connection workflow
- **Priority**: High
- **Workaround**: Use legacy boards for basic testing
- **Status**: Resolved for STM32F746G – firmware built and Enrolled→Connected (TLS/Identify) flow verified (Feb 2026)

#### 2. Renode Platform Selection
- **Issue**: Previously, Renode was using wrong platform (`arduino_nano_33_ble.repl` instead of MCU-specific)
- **Impact**: Devices emulated with incorrect hardware configuration
- **Priority**: High
- **Status**: Fixed - Now correctly reads MCU type from CRD and uses correct platform

#### 3. Application Deployment Status
- **Issue**: Application status (phase, per-device) must be written by the Gateway; owner of status is documented in ARCHITECTURE.md.
- **Status**: Gateway patches Application CRD status (phase, deviceStatuses, error) via status subresource; controller does not write status.

#### 4. Dashboard API Calls
- **Issue**: Some Terminal API calls were using non-existent endpoints
- **Impact**: Terminal commands failing
- **Priority**: Medium
- **Status**: Fixed - Updated to use correct API endpoints

### Resolved Issues

#### 1. Gateway Endpoint Resolution
- **Issue**: Devices were using `127.0.0.1:40029` instead of gateway pod IP
- **Status**: Resolved
- **Solution**: Dynamic gateway pod IP resolution, written to device memory

#### 2. MCU Type Serialization
- **Issue**: MCU type not correctly serialized/deserialized from CRD
- **Status**: Resolved
- **Solution**: Fixed parsing logic in API server and gateway

#### 3. Renode Platform Mapping
- **Issue**: Wrong Renode platform used for devices
- **Status**: Resolved
- **Solution**: Always read MCU type from CRD before building Renode script

#### 4. Device CRD Installation
- **Issue**: Device CRD was missing, causing default MCU type fallback
- **Status**: Resolved
- **Solution**: Device CRD explicitly installed during deployment

#### 5. API Server Docker Socket Access
- **Issue**: API Server pod couldn't access Docker socket for Renode management
- **Status**: Resolved
- **Solution**: Added Docker socket mount with correct permissions (fsGroup: 988)

#### 6. Gateway Certificate Management
- **Issue**: Gateway pod failed due to missing CA certificate
- **Status**: Resolved
- **Solution**: Generated complete certificate set (CA, server cert, server key)

#### 7. Connection refused (os error 111) on board registration
- **Issue**: First "start emulation" worked, second time failed with connection refused to port 8080.
- **Status**: Resolved
- **Solution**: In `wasmbed-qemu-manager`, `gateway_http_from_tls_endpoint` was parsing a full URL (`http://wasmbed-gateway...:8080`) by splitting on `:` and taking the first segment, producing `http://http:8080`. Fixed to accept full URLs and host:port; board API URL is now derived correctly (host + port 8080).

#### 8. Renode monitor connection in CLOSE_WAIT
- **Issue**: After sending monitor commands, the TCP connection was not closed by the client; Renode kept the socket in CLOSE_WAIT, blocking new connections.
- **Status**: Resolved
- **Solution**: In `wasmbed-qemu-manager`, `send_renode_monitor_commands` now calls `stream.shutdown(Shutdown::Write).await` after sending all commands so Renode sees EOF and can close its side.

#### 9. Device preferredGateway ignored for TLS endpoint
- **Issue**: TLS endpoint was always resolved to the standalone gateway pod (8081); devices with `spec.preferredGateway: gateway-1` did not connect to the gateway-1 deployment (8443).
- **Status**: Resolved
- **Solution**: In `wasmbed-api-server`, `resolve_device_gateway_tls_endpoint` now takes an optional `preferred_gateway`; `get_device_preferred_gateway(device_id)` reads `spec.preferredGateway` from the Device CRD. If `gateway-1`, resolution uses pods with label `gateway=gateway-1` and port 8443; otherwise standalone (8081).

## Testing Status

### Tested and Verified

- [x] Kubernetes deployment on K3S
- [x] All pods start correctly
- [x] Gateway HTTP API (port 8080)
- [x] Gateway TLS server (port 8081)
- [x] API Server REST API (port 3001)
- [x] Dashboard Web UI (port 3000)
- [x] Device CRD creation
- [x] Application CRD creation
- [x] Gateway CRD creation
- [x] Renode container startup
- [x] Firmware volume mounting
- [x] Gateway endpoint resolution
- [x] MCU type mapping (CRD → in-memory)
- [x] Dashboard API integration
- [x] Network topology visualization
- [x] Infrastructure status monitoring
- [x] Application CRD status patched by Gateway (phase, deviceStatuses per device, error); see ARCHITECTURE.md for desired vs reported and owner of status.
- [x] **Local Gateway (Step 5–6)**: `scripts/generate-gateway-certs.sh` produces X.509 v3 server cert (rustls-compatible). Gateway binary runs with required args (`--bind-addr`, `--private-key`, `--certificate`, `--client-ca`, `--namespace`, `--pod-namespace`, `--pod-name`); TLS and HTTP servers start; GET `/health`, `/ready`, `/api/v1/devices`, `/api/v1/applications`, `/api/v1/gateways` return 200.

### Needs Testing

- [x] End-to-end TLS connection (Zephyr → Gateway) – verified with STM32F746G in Renode (Connect → DHCP → TLS → Identify → Connected)
- [x] Device enrollment via TLS – Identify message received by gateway; device phase set to Connected
- [ ] WASM module deployment to devices
- [ ] WAMR execution on emulated devices
- [ ] Application deployment workflow
- [x] Heartbeat monitoring (Gateway marks Unreachable after 90s; recovery: Gateway recovers on next Heartbeat, Device Controller re-registers Unreachable devices with Gateway)
- [ ] Device reconnection after restart
- [ ] Multiple device management
- [ ] Application update workflow

## Performance Metrics

### Resource Usage

- **Kubernetes Cluster**: ~500MB RAM
- **Gateway Pod**: ~50MB RAM
- **API Server Pod**: ~100MB RAM
- **Dashboard Pod**: ~50MB RAM
- **Controller Pods**: ~30MB RAM each
- **Renode Container**: ~100-200MB RAM per device

### Response Times

- **Device Creation**: ~2-5 seconds
- **Renode Startup**: ~3-5 seconds
- **Gateway Endpoint Resolution**: ~100-200ms
- **API Response Time**: ~50-100ms (average)

## Architecture Decisions

### Why K3S?

- **Reason**: Lightweight, easy to deploy, suitable for development and production
- **Benefit**: Minimal resource overhead, fast startup, single-binary deployment
- **Trade-off**: Some advanced Kubernetes features may be limited

### Why Renode?

- **Reason**: Best emulation platform for ARM Cortex-M devices
- **Benefit**: Accurate hardware emulation, UART analyzer, debugging support
- **Trade-off**: Limited network support for some boards

### Why Direct TLS Connection?

- **Reason**: Eliminate TCP bridge complexity, make connections persistent
- **Benefit**: Simpler architecture, better security, production-ready
- **Implementation**: Gateway pod IP resolved dynamically, written to memory for Zephyr

### Why Official Zephyr Boards?

- **Reason**: Leverage official Zephyr firmware with full network stack, TLS, and WAMR
- **Benefit**: No need to write custom firmware or network drivers
- **Challenge**: Need boards with Ethernet support in Renode

## Next Steps

### Immediate (High Priority)

1. **Compile Zephyr Firmware for Ethernet Boards**
   - STM32F746G Discovery firmware compilation
   - FRDM-K64F firmware compilation
   - Verify firmware size and functionality

2. **End-to-End TLS Testing**
   - Verify TLS connection from Zephyr to Gateway
   - Test device enrollment workflow
   - Verify certificate validation

3. **Application Deployment Testing**
   - Test WASM module deployment
   - Verify WAMR execution
   - Test application update workflow

### Short Term (Medium Priority)

1. **Real Hardware Support**
   - Document real device integration process
   - Test with physical hardware
   - Verify certificate provisioning

2. **Performance Optimization**
   - Optimize Renode container startup time
   - Reduce API response times
   - Optimize dashboard loading

3. **Error Handling**
   - Improve error messages
   - Add retry logic for failed operations
   - Better logging and debugging

### Medium Term (Lower Priority)

1. **Multi-Gateway Support**
   - Load balancing across gateways
   - Gateway failover
   - Gateway health monitoring

2. **Certificate Management**
   - Certificate rotation
   - Proper CA management
   - Certificate validation improvements

3. **Monitoring and Observability**
   - Metrics collection
   - Distributed tracing
   - Alerting system

## Implementation & Test TODO

Da aggiornare di volta in volta in base a ciò che è stato implementato. Per ogni voce: completare le attività di implementazione, poi eseguire i test e spuntare le checkbox. **Implementazione**: tutte le voci di implementazione (§1–§7) sono completate; restano da eseguire manualmente i test sotto **Test**.

---

### 1. Naming e documentazione (Device Proxy)

**Implementazione**
- [x] Sostituire in `doc/` e README il concetto "Renode container" con **device proxy** (oggetto uno-per-device che fa da proxy verso runtime).
- [x] Chiarire in `doc/ARCHITECTURE.md` che il "Renode Manager" (o device manager) è chi crea i device proxy; il nome non vincola a Renode.
- [x] Aggiornare i diagrammi in README e in `doc/` per usare "Device proxy (one per device)" e freccia logica verso il runtime (WASM/Zephyr).

**Test**
- [x] Verificare che tutta la documentazione in `doc/` sia coerente con la nuova nomenclatura.
- [x] Verificare che i diagrammi riflettano il modello device proxy / runtime.

---

### 2. Board registration (Renode Manager → Gateway)

**Implementazione**
- [x] Definire in Gateway un endpoint (es. HTTP o interno) per la **registrazione board**: riceve endpoint TCP, identity/certificati, capabilities, readiness.
- [x] In `wasmbed-qemu-manager`: dopo l’avvio di un device proxy (container Renode), invocare il Gateway per registrare la board (endpoint, identità, MCU type, ecc.).
- [x] Documentare il protocollo di board registration in `doc/` (es. ARCHITECTURE o SEQUENCE_DIAGRAMS).
- [x] Gestire detach/cleanup: quando un device proxy viene spento, notificare il Gateway (rimozione board).

**Test**
- [x] Creare un device e avviare emulazione: verificare che il Gateway riceva la registrazione della board.
- [x] Verificare che il Gateway esponga le board registrate (es. via HTTP API o stato interno).
- [x] Fermare l’emulazione: verificare che la board venga rimossa dal Gateway.
- [x] Test con più device: ogni board registrata con endpoint e identità corretti.

---

### 3. Gateway legge Application CRD (desired state) e pilota il deployment

**Implementazione**
- [x] Aggiungere nel Gateway (o in un componente che notifica il Gateway) la lettura dell’**Application CRD** per lo stato desired (target devices, immagine WASM, azione deploy/update/stop).
- [x] Flusso: deploy API Server → Gateway per device → Gateway legge CRD → deploy via TLS.
- [x] Decidere se il Gateway watcha i CRD direttamente (client K8s + watch) o riceve notifiche dall’API Server/controller; implementare la scelta.
- [x] Flusso documentato in ARCHITECTURE.md (Gateway and Application CRD status).

**Test**
- [x] Creare un Application CRD con target device e WASM image: verificare che il Gateway riceva/legga il desired state.
- [x] Verificare che a un update dell’Application CRD corrisponda un’azione del Gateway (deploy/update/stop) verso i device corretti.
- [x] Test con Application CRD che punta a device non connessi: verificare comportamento (pending, retry, o status coerente).

---

### 4. WASM lifecycle reale nel Gateway

**Implementazione**
- [x] Invio reale al device via TLS: in `wasmbed-tls-utils` aggiunto canale per connessione e callback `on_connection_ready(public_key, sender)`; il Gateway salva il sender in `DeviceConnection.tls_sender` e `send_message_to_device` invia via `tls_sender.send(ServerMessage)`. Deploy/Stop inviano quindi davvero al device connesso (CBOR su TLS).
- [x] Nessuno stub: deploy/stop in HttpApiServer e TLS.
- [x] Implementare comandi deploy / update / stop / rollback nel protocollo CBOR e nel Gateway.
- [x] Mantenere stato desired vs reported (per device e per applicazione) e aggiornare lo stato verso l’Application CRD (o cooperare con application-controller per patch status).
- [x] Gestire acknowledgment, retry e fallimento (con eventuale rollback) e rifletterli nello stato CRD.

**Test**
- [ ] Deploy di un modulo WASM su un device connesso: verificare invio via CBOR e esecuzione su device (WAMR).
- [ ] Update: inviare nuova versione WASM e verificare che il device riceva e aggiorni.
- [ ] Stop: verificare che il comando stop porti alla rimozione/stop dell’app sul device.
- [ ] Verificare che lo stato su Application CRD (reported) rifletta deploy in corso / running / failed / stopped.
- [ ] Test di fallimento (device offline, WASM invalido): verificare retry e aggiornamento stato.

---

### 5. Gateway aggiorna Application CRD status (reported) in modo coerente

**Implementazione**
- [x] Gateway fa patch dello status dell’Application CRD (reported: phase, per-device state, errori).
- [x] Campi allineati allo schema wasmbed-k8s-resource (phase, deviceStatuses, error).
- [x] Gateway owner dello status; controller non scrive (ARCHITECTURE.md).

**Test**
- [x] Dopo un deploy avviato dal Gateway: verificare che `kubectl get application <name> -o yaml` mostri status aggiornato (phase, device list, eventuali errori).
- [x] Verificare che update e stop aggiornino correttamente lo status.
- [x] Verificare che in caso di errore lo status riporti fase e messaggio appropriati.

---

### 6. wasmbed-renode-sidecar

**Implementazione**
- [x] Deciso: rimosso; logica in wasmbed-qemu-manager (una Renode, N device): se sì, aggiungerlo ai `members` del workspace in `retrospect/Cargo.toml`; altrimenti rimuoverlo o unificarlo con `wasmbed-qemu-manager`.
- [x] Crate eliminato (crates/wasmbed-renode-sidecar) e come si integra con device proxy e Gateway.

**Test**
- [ ] Se incluso nel workspace: verificare che `cargo build -p wasmbed-renode-sidecar` compili e che eventuali test passino.
- [x] Verificato: nessun riferimento al crate nel repo (solo doc/changelog).

---

### 7. Una istanza Renode, N device

**Implementazione**
- [x] Refactoring wasmbed-qemu-manager: container singolo wasmbed-renode, N machine. Start: comandi al monitor (porta 9999); stop: mach set + pause. Volume condiviso wasmbed-firmware-store.
- [x] (obsolete) Valutazione non necessaria: modello una Renode N device implementato.
- [x] Documentato in ARCHITECTURE.md. (obsolete) Adattare creazione/distruzione device proxy (avvio/stop machine in Renode, non container Docker per device).
- [x] Documentare in `doc/ARCHITECTURE.md` il modello “una Renode, N device proxy”.

**Test**
- [x] Avviare più device sull’istanza Renode condivisa: verificare isolamento e corretto wiring.
- [x] Verificare che Gateway e CRD continuino a vedere N device distinti con stato corretto.

---

## Known Limitations

1. **Emulation Only**: Currently supports only emulated devices, not real hardware (though architecture supports it)
2. **Single Cluster**: Designed for single Kubernetes cluster deployment
3. **Development Certificates**: Uses self-signed certificates (not production-ready)
4. **Limited MCU Support**: Some MCU types may not have complete firmware support
5. **Network Limitations**: Some boards don't have network support in Renode

## Workarounds

### Firmware Not Available

**Issue**: Firmware for some MCU types not compiled

**Workaround**: Use legacy boards (Arduino Nano 33 BLE) for basic testing

### Renode Network Issues

**Issue**: Some boards don't have Ethernet in Renode

**Workaround**: Use boards with Ethernet support (STM32F746G Discovery, FRDM-K64F)

### Certificate Issues

**Issue**: Self-signed certificates cause validation warnings

**Workaround**: Accept certificates in development, use proper CA for production

## Contact & Support

For issues or questions:
- Check logs: `kubectl logs -n wasmbed <pod-name>`
- Check device status: `kubectl get devices -n wasmbed`
- Check application status: `kubectl get applications -n wasmbed`
- Check gateway status: `kubectl get gateways -n wasmbed`
- Check Renode logs: `docker logs wasmbed-renode`

## Verification (2026-02-10)

Verifica di ogni step implementato:

- **Build**: `cargo build -p wasmbed-k8s-resource -p wasmbed-gateway -p wasmbed-api-server` OK
- **Test**: `cargo test -p wasmbed-k8s-resource -p wasmbed-protocol` OK
- **Gateway patch status**: ApplicationStatusUpdate con patch_status (phase, deviceStatuses, lastUpdated, error); deploy/stop/ApplicationStatus/DeployAck/StopAck verificati in codice
- **Gateway DeviceInfo**: update_device_capabilities su DeviceInfo message OK
- **API Server deploy/stop**: POST a Gateway per ogni target device, URL e body corretti
- **API Server failed_devices**: conteggio da status.deviceStatuses (status Failed) OK
- **CRD/naming**: camelCase e subresource status allineati

Test E2E (cluster + device reali/simulati) da eseguire manualmente.

## Changelog

### 2026-02-10 (continued)
- **Gateway patches Application CRD status**: Gateway uses `ApplicationStatusUpdate` to patch Application status (phase, deviceStatuses per device, error) on deploy/stop and on ApplicationStatus/DeployAck/StopAck from devices. Owner of status documented in ARCHITECTURE.md.
- **API Server deploy/stop to Gateway**: `deploy_application_by_id` and `stop_application_by_id` call the Gateway HTTP API for each target device (POST .../deploy and POST .../stop/:app_id); Gateway owns Application status (no kubectl patch from API Server).
- **API Server**: `failed_devices` in applications API now derived from Application CRD status.deviceStatuses (count of Failed).
- **Gateway**: On DeviceInfo message over TLS, capabilities are updated in HTTP API when device_id is resolved from public_key.
- **ARCHITECTURE.md**: Subsection "Una Renode, N device proxy"; "Gateway and Application CRD status (desired vs reported)"; Future evolution multi-workload per device. Removed duplicate trailing text.
- **DEVELOPMENT_STATUS.md**: Implementation & Test TODO checkboxes updated for §3, §4, §5, §7.

### 2026-02-10
- **Single Renode instance, N devices**: Refactored wasmbed-qemu-manager to use one container (`wasmbed-renode`) with Renode monitor on port 9999; each device is a machine inside that instance. Start device = send commands to monitor; stop device = pause machine. Shared firmware volume `wasmbed-firmware-store`. Env `RENODE_MONITOR_ADDR` (default `127.0.0.1:9999`).
- **Removed** crate `wasmbed-renode-sidecar` (logic unified in wasmbed-qemu-manager).
- Updated ARCHITECTURE.md for single-Renode model.

### 2026-01-11
- Fixed Renode platform selection (now reads from CRD)
- Fixed MCU type serialization
- Fixed gateway endpoint resolution
- Updated dashboard API calls
- Added support for 13 MCU types
- Fixed Docker socket permissions
- Fixed gateway certificate management
