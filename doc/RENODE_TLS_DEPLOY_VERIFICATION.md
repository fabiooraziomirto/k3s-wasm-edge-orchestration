# Verifica: Device Renode → Gateway TLS → Kubernetes e Deploy WASM

Questo documento descrive come verificare che (1) i device emulati su Renode si connettono al Gateway via TLS e risultano attivi su Kubernetes, e (2) il deploy dell’applicazione e l’esecuzione WASM funzionino correttamente.

---

## 1. Flusso atteso

### 1.1 Connessione device → Gateway (TLS)

1. **API Server / Dashboard**: l’utente avvia l’emulazione del device (es. "Connect" o "Start emulation"). L’API Server chiama il Gateway `POST /api/v1/devices/{device_id}/connect` e poi avvia il Renode (RenodeManager) passando l’endpoint del Gateway (es. `http://wasmbed-gateway.wasmbed.svc.cluster.local:8080`).
2. **RenodeManager** (in API Server):
   - Risolve l’IP del pod del Gateway: `kubectl get pods -n wasmbed -l app=wasmbed-gateway -o jsonpath={.items[0].status.podIP}` e forma `{pod_ip}:8081` (porta TLS).
   - Scrive l’endpoint in memoria nel device a `0x20001000`: prima 4 byte (lunghezza), poi i byte della stringa (es. `10.42.0.12:8081`).
   - Avvia Renode con la piattaforma corretta (es. STM32F746) e il firmware Zephyr.
3. **Firmware Zephyr** (in Renode):
   - Legge l’endpoint da `0x20001000` (lunghezza + stringa host:port).
   - Connette al Gateway sulla porta TLS (8081) tramite `network_connect_tls(host, port)`.
   - Esegue enrollment (EnrollmentRequest → PublicKey → EnrollmentAcknowledgment) e heartbeat.
4. **Gateway** (porta 8081 TLS):
   - Alla connessione TLS verifica il certificato client e trova il Device CRD tramite public key.
   - Aggiorna il Device CRD: `DeviceStatusUpdate::mark_connected()` (phase: Connected, gateway, lastHeartbeat).
   - Alla ricezione del primo messaggio chiama `mark_device_tls_connected(device_id)` (device visibile come “connesso” per il deploy).
   - Su Heartbeat aggiorna `DeviceStatusUpdate::update_heartbeat()` e `http_server.update_heartbeat()`.

**Verifica connessione e K8s:**

- Dopo aver avviato un device (Connect / Start emulation) con Renode e firmware che si connettono al Gateway:
  - `kubectl get devices -n wasmbed` deve mostrare il device con **phase: Connected** (e status.gateway compilato).
  - I log del Gateway devono mostrare “TLS client certificate verification successful” e “Marked device X as having active TLS connection”.
  - La dashboard (lista devices) deve mostrare il device come connesso (dati letti da K8s).

### 1.2 Deploy applicazione e runtime WASM

1. **Dashboard**: l’utente fa Deploy su un’application con target devices che includono il device connesso.
2. **API Server**: `POST /api/v1/applications/:id/deploy` → per ogni target device chiama il Gateway `POST {gateway}/api/v1/devices/{device_id}/deploy` (body: app_id, name, wasm_bytes).
3. **Gateway**:
   - Legge l’Application CRD (GET) per ottenere i byte WASM (da spec.wasmBytes, base64).
   - Registra il deploy in memoria (`register_application`).
   - Attende che il device abbia `tls_connected == true` (fino a 30 s).
   - Fa PATCH sull’Application CRD: phase Deploying, deviceStatuses[device_id] Deploying.
   - Invia al device via TLS il messaggio **ServerMessage::DeployApplication** { app_id, name, wasm_bytes, config }.
   - Il messaggio viene serializzato in CBOR (minicbor) e inviato con prefisso lunghezza 4 byte (big-endian u32) + payload CBOR.
4. **Device (firmware)**:
   - Deve leggere 4 byte (len), poi `len` byte CBOR.
   - Deve parsare il CBOR come ServerMessage: per DeployApplication è un array a 5 elementi: tag=5 (u32), app_id (str), name (str), wasm_bytes (bytes), config (null o oggetto).
   - Deve chiamare `wamr_load_module(wasm_bytes, len, &module_id)`, `wamr_instantiate(module_id, &instance_id)` e opzionalmente `wamr_call_function()` per eseguire il modulo.
   - Deve inviare **ClientMessage::ApplicationDeployAck** { app_id, success, error } (CBOR con prefisso lunghezza come da protocollo client).
5. **Gateway** (su ricezione DeployAck):
   - Aggiorna l’Application CRD: phase Running o Failed, deviceStatuses[device_id], lastUpdated, error (in caso di fallimento).

**Verifica deploy e WASM:**

- Dopo un deploy dalla dashboard per un device con TLS attiva:
  - In Kubernetes: `kubectl get applications -n wasmbed -o yaml` deve mostrare `status.phase: Running` (o Failed) e `status.deviceStatuses.<device_id>` aggiornato.
  - La dashboard (lista applications) deve mostrare lo status e i deployed devices coerenti con K8s.
  - Sul device, il modulo WASM deve essere stato caricato ed eseguito (WAMR); in assenza di parsing CBOR nel firmware, il Gateway invia comunque il messaggio ma il device non risponde con DeployAck e non esegue WASM.

---

## 2. Stato implementativo (codebase)

### 2.1 Già implementato

| Componente | Dettaglio |
|------------|-----------|
| **RenodeManager** | Risoluzione gateway pod IP, scrittura endpoint a 0x20001000, avvio Renode con script e piattaforma corretta. |
| **Gateway TLS** | Server TLS sulla porta 8081, verifica certificato client, lookup Device per public key, aggiornamento Device CRD (Connected, Enrolling, Enrolled, heartbeat). |
| **Gateway HTTP** | `POST .../devices/:id/connect` registra il device in memoria; deploy attende `tls_connected` e invia `ServerMessage::DeployApplication` via TLS (length-prefix + CBOR). |
| **Application CRD status** | Il Gateway fa PATCH di phase e deviceStatuses su deploy e su DeployAck/StopAck/ApplicationStatus. |
| **Zephyr firmware** | Lettura endpoint da 0x20001000, connessione TLS al Gateway, invio messaggi (enrollment, heartbeat); **wasmbed_protocol_handle_message()** riceve i byte ma non decodifica ancora CBOR. |
| **WAMR (firmware)** | `wamr_integration.c`: `wamr_init()`, `wamr_load_module()`, `wamr_instantiate()`, `wamr_call_function()` disponibili. |

### 2.2 Da completare / verificare

| Componente | Stato | Azione |
|------------|--------|--------|
| **Firmware: parsing CBOR** | In `zephyr-app/src/wasmbed_protocol.c`, `wasmbed_protocol_handle_message()` non decodifica CBOR (solo log dei primi 32 byte). | Aggiungere decoder CBOR minimale (o libreria) per: lettura prefisso 4 byte + payload; riconoscere messaggio tipo DeployApplication (tag 5) ed estrarre app_id, wasm_bytes. |
| **Firmware: gestione DeployApplication** | Non implementata. | Dopo il parsing: chiamare `wamr_load_module()` e `wamr_instantiate()`; inviare `ApplicationDeployAck` (success/error) in CBOR con prefisso lunghezza come usato dal client. |
| **Firmware: encoding DeployAck** | Il device deve inviare ClientMessage in formato CBOR (come fa il Gateway per ServerMessage). | Implementare encoding CBOR per ApplicationDeployAck (tag 5, app_id, success, error) e invio con length-prefix se il protocollo client lo richiede. |
| **E2E con Renode reale** | Documentato come “parzialmente funzionale” in DEVELOPMENT_STATUS. | Eseguire: avvio cluster, Gateway, API Server, Renode con device; verificare phase Connected su K8s; poi deploy e verificare status Application e, quando il firmware sarà pronto, esecuzione WASM. |

---

## 3. Formato wire (per implementazione firmware)

- **Gateway → device**: per ogni ServerMessage:
  - 4 byte: lunghezza payload (big-endian u32).
  - N byte: CBOR di ServerMessage (minicbor).
- **DeployApplication** in CBOR: array di 5 elementi: `[5, app_id_str, name_str, wasm_bytes_bstr, null]` (tag 5 = SERVER_DEPLOY_APPLICATION). Se config non è null, il quinto elemento è un oggetto (memory_limit, cpu_time_limit, map env, array args).
- **Device → Gateway**: ClientMessage in CBOR; il Gateway si aspetta lo stesso schema length-prefix + CBOR per i messaggi client (vedi `tls_utils`: legge `buffer[..n]` e fa `minicbor::decode::<ClientMessage>`). Quindi il device deve inviare: 4 byte (len) + CBOR(ApplicationDeployAck). ApplicationDeployAck: array di 4 elementi: tag 5 (CLIENT_APPLICATION_DEPLOY_ACK), app_id (str), success (bool), error (null o str).

---

## 4. Checklist verifica pratica

### Connessione TLS e Kubernetes

1. [ ] Cluster K8s attivo, namespace wasmbed, CRD Device/Application/Gateway installati.
2. [ ] Gateway in esecuzione con certificati e TLS su 8081.
3. [ ] API Server in esecuzione; RenodeManager può avviare Renode (ambiente con Docker/kubectl se necessario).
4. [ ] Creare un Device CRD con public key coerente con il certificato usato dal firmware (o usare pairing mode e enrollment).
5. [ ] Avviare l’emulazione del device (Connect / Start emulation) e attendere che il firmware si connetta al Gateway.
6. [ ] Verificare: `kubectl get devices -n wasmbed` mostra il device con **phase: Connected** e gateway compilato.
7. [ ] Verificare: log del Gateway mostrano connessione TLS e device marcato come TLS connected.

### Deploy e WASM

1. [ ] Creare un’Application CRD con spec.wasmBytes (base64) e targetDevices che includono il device connesso.
2. [ ] Eseguire deploy dalla dashboard (o POST .../applications/:id/deploy).
3. [ ] Verificare: in K8s l’Application ha status.phase e status.deviceStatuses aggiornati (Deploying poi Running/Failed).
4. [x] Il firmware implementa CBOR + WAMR + DeployAck: verificare che il modulo WASM venga caricato/istanziato e che il Gateway riceva DeployAck e aggiorni lo status (phase: Running).

---

## 5. Verifica connessione TLS mantenuta e deploy WASM

### 5.1 Connessione TLS mantenuta

- **Firmware**: invia `ClientMessage::Heartbeat` (CBOR `[0]`) ogni 25 s tramite `wasmbed_protocol_tick()` (chiamato dal main loop in `main.c`). Payload: 4 byte length (big-endian) + `0x81 0x00`.
- **Gateway**: alla ricezione di `ClientMessage::Heartbeat` aggiorna `DeviceStatusUpdate::update_heartbeat()` (Device CRD `status.last_heartbeat`) e `http_server.update_heartbeat(device_id)` (in-memory `last_heartbeat`).
- **Monitor**: il task `check_heartbeat_timeouts` nel Gateway (periodo 30 s, timeout default 90 s) controlla `status.last_heartbeat` dei Device; se superato il timeout, marca il device come unreachable (phase aggiornata).
- **Recovery automatico**:
  - **Gateway**: alla ricezione di un Heartbeat da un device in phase Unreachable, il Gateway applica `mark_connected(gateway_reference).last_heartbeat(now)` e riporta il device a Connected (vedi `wasmbed-gateway/src/main.rs`).
  - **Device Controller**: per i device in phase Unreachable, `handle_unreachable` chiama il Gateway `POST .../devices/{id}/connect` per ri-registrare il device; al successivo Heartbeat TLS il Gateway può aggiornare lo status a Connected (vedi `wasmbed-device-controller/src/main.rs`).
- **Verifica**: con un device Renode connesso e firmware che invia heartbeat, `kubectl get devices -n wasmbed -o jsonpath='{.items[*].status.last_heartbeat}'` deve mostrare timestamp recenti; i log del Gateway mostrano "Heartbeat from ..." e aggiornamento heartbeat.

### 5.2 Deploy WASM su dispositivi Renode

- **Flusso**: Dashboard/API `POST .../applications/:id/deploy` → API Server chiama Gateway `POST .../devices/:id/deploy` → Gateway invia `ServerMessage::DeployApplication` via TLS (length + CBOR) al device.
- **Firmware**: `wasmbed_protocol_handle_message()` riconosce DeployApplication (0x85 0x05), decodifica app_id, wasm_bytes (CBOR minimale), copia wasm in buffer, chiama `wamr_load_module()` e `wamr_instantiate()`, poi invia `ApplicationDeployAck` (success/error) con `send_deploy_ack()`.
- **Gateway**: alla ricezione di `ClientMessage::ApplicationDeployAck` aggiorna Application CRD (phase: Running o Failed, deviceStatuses[device_id]).
- **Verifica**: dopo deploy, `kubectl get applications -n wasmbed -o yaml` deve mostrare `status.phase: Running` e `status.deviceStatuses.<device_id>.status: Running` quando il device ha inviato DeployAck con success. L’applicazione WASM è caricata e istanziata sul device (WAMR); l’esecuzione di funzioni esportate è opzionale tramite `wamr_call_function()`.

---

## 6. Riferimenti nel codice

- Scrittura endpoint in memoria: `wasmbed-qemu-manager/src/lib.rs` (build_renode_args, 0x20001000).
- Lettura endpoint e TLS: `zephyr-app/src/wasmbed_protocol.c` (read_gateway_endpoint, network_connect_tls).
- Heartbeat e deploy (firmware): `zephyr-app/src/wasmbed_protocol.c` (wasmbed_protocol_tick, heartbeat_packet, handle_deploy_application, send_deploy_ack, cbor_read_text/cbor_read_bytes), `main.c` (wasmbed_protocol_tick nel loop).
- Gateway: connessione e aggiornamento Device/Application: `wasmbed-gateway/src/main.rs` (callbacks TLS, Heartbeat, ApplicationDeployAck), `http_api.rs` (deploy, device_status, patch status).
- Protocollo CBOR: `wasmbed-protocol/src/cbor.rs` (tag e formato ServerMessage/ClientMessage).
- Invio TLS con length-prefix: `wasmbed-tls-utils/src/lib.rs` (minicbor::to_vec, len.to_be_bytes(), write_all).
