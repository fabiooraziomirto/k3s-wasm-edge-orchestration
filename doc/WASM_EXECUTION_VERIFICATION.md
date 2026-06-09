# Verifica Esecuzione WASM End-to-End — Punti 4, 5, 6

## Contesto

Questo documento descrive in dettaglio cosa è stato implementato, i problemi incontrati, i fix applicati e i risultati ottenuti nella verifica end-to-end dell'esecuzione di moduli WebAssembly su dispositivo embedded emulato (STM32F746G Discovery via Renode), coordinata da un Gateway Kubernetes.

Copre i seguenti punti del progetto:

| Punto | Descrizione | Stato |
|-------|-------------|-------|
| **4** | WASM lifecycle reale nel Gateway (invio TLS, DeployAck, esecuzione reale) | ✅ **Completato** |
| **5** | Gateway aggiorna Application CRD status (reported) in modo coerente | ✅ **Completato** |
| **6** | wasmbed-renode-sidecar → rimosso, sostituito da wasmbed-qemu-manager | ✅ **Completato** |

---

## Architettura del flusso testato

```
[API Server] ──HTTP POST /deploy──► [Gateway]
                                        │
                                  TLS (CBOR framing)
                                        │
                                        ▼
                                  [Device (Zephyr+WAMR)]
                                   su Renode (emulato)
                                        │
                              DeployAck + ApplicationStatus
                                        │
                                        ▼
                                    [Gateway]
                                        │
                               kubectl PATCH status
                                        │
                                        ▼
                              [Application CRD in K8s]
```

---

## Punto 4 — WASM lifecycle reale

### Problema trovato

Il codice preesistente in `wasmbed_protocol.c` eseguiva correttamente caricamento e istanziazione del modulo WASM (`wamr_load_module` + `wamr_instantiate`), ma **non chiamava mai** la funzione entry-point del modulo. Dopo l'istanziazione, non accadeva nulla.

```c
/* Situazione prima del fix — mancava la chiamata a wamr_call_function */
ret = wamr_instantiate(module_id, &instance_id);
if (ret != 0) { ... return -1; }
/* ← nessuna esecuzione! */
send_deploy_ack(deploy_app_id_buf, true, NULL);
```

Problema aggiuntivo: il `DeployApplication` veniva quasi sempre **silenziosamente scartato** nel main loop, per via di un bug nel modo in cui il firmware riceveva i messaggi dal gateway (vedi sezione "Bug ricezione frame").

### Fix implementati

#### 1. Chiamata effettiva a `wamr_call_function` dopo l'istanziazione

In `zephyr-app/src/wasmbed_protocol.c`, funzione `handle_deploy_application()`:

```c
ret = wamr_instantiate(module_id, &instance_id);
if (ret != 0) {
    LOG_ERR("wamr_instantiate failed");
    return -1;
}
LOG_INF("WASM deployed: app_id=%s module_id=%u instance_id=%u",
        deploy_app_id_buf, (unsigned)module_id, (unsigned)instance_id);

/* NUOVO: esecuzione dell'entry-point "run" del modulo WASM */
if (wamr_call_function(instance_id, "run", NULL, 0, NULL, 0) != 0) {
    LOG_WRN("WASM run() returned error");
}
current_instance_id = instance_id;
app_deployed        = true;
last_app_status_ms  = k_uptime_get_32();
```

`wamr_call_function` chiama `wasm_runtime_lookup_function` per trovare l'esportazione `"run"` e poi `wasm_runtime_call_wasm` per eseguirla. L'implementazione si trova in `zephyr-app/src/wamr_integration.c`.

#### 2. Stato deploy tracciato con variabili statiche

```c
static uint32_t current_instance_id = 0;
static bool     app_deployed         = false;
#define APP_STATUS_INTERVAL_MS       30000U
static uint32_t last_app_status_ms   = 0U;
```

### Modulo WASM di test

Per verificare l'esecuzione è stato creato un modulo WASM minimale (33 byte):

```wat
(module
  (func (export "run"))   ;; no-op, ritorna immediatamente
)
```

Binario hex: `0061736d01000000010401600000030201000707010372756e00000a040102000b`

Base64: `AGFzbQEAAAABBAFgAAADAgEABwcBA3J1bgAACgQBAgAL`

Il modulo è definito nel CRD K8s `k8s/test-resources/test-wasm-app.yaml`:

```yaml
apiVersion: wasmbed.github.io/v1alpha1
kind: Application
metadata:
  name: test-wasm-app
  namespace: wasmbed
spec:
  name: test-wasm-counter
  wasmBytes: "AGFzbQEAAAABBAFgAAADAgEABwcBA3J1bgAACgQBAgAL"
  targetDevices:
    allDevices: true
```

---

## Punto 5 — Application CRD status coerente

### Cosa fa il Gateway lato Rust

Quando il Gateway riceve un messaggio `ClientMessage::ApplicationStatus` dal device via TLS, esegue:

1. Traduce lo stato ricevuto in `DeviceApplicationPhase::Running` (o `Failed`)
2. Costruisce un `DeviceApplicationStatus` con `last_heartbeat = Utc::now()`
3. Chiama `ApplicationStatusUpdate::device_status(...).apply(...)` che fa `kubectl PATCH` sullo status dell'Application CRD

Il codice si trova in `crates/wasmbed-gateway/src/main.rs` (handler `ClientMessage::ApplicationStatus`):

```rust
let dev_status = DeviceApplicationStatus {
    status: dev_phase,
    last_heartbeat: Some(chrono::Utc::now().to_rfc3339()),
    metrics: metrics_opt,   // None se il device non li manda
    error: error.clone(),
    restart_count: 0,
};
ApplicationStatusUpdate::default()
    .device_status(device_id, dev_status)
    .apply(&http_server.application_api, &app)
    .await
```

### Fix firmware: invio periodico di ApplicationStatus

Prima del fix il firmware non inviava mai `ApplicationStatus` dopo il deploy. È stata aggiunta la funzione `send_application_status()` e la chiamata periodica in `wasmbed_protocol_tick()`.

**Codifica CBOR manuale** (il firmware non ha una libreria CBOR):

```
array(5) = 0x85
u32(4)   = 0x04        ← tag ClientMessage::ApplicationStatus
text(app_id)           ← nome applicazione
u32(1)   = 0x01        ← ApplicationStatus::Running
null     = 0xf6        ← campo error (assente)
null     = 0xf6        ← campo metrics (assente)
```

Formato wire finale: `[4 byte len BE] [CBOR sopra]`

**Tick periodico** in `wasmbed_protocol_tick()`:

```c
if (app_deployed && (now - last_app_status_ms >= APP_STATUS_INTERVAL_MS)) {
    send_application_status(deploy_app_id_buf, 0x01); /* Running */
    last_app_status_ms = now;
    LOG_INF("ApplicationStatus sent for %s", deploy_app_id_buf);
}
```

### Risultato verificato

Dopo deploy e avvio firmware:

```
[gateway] Received application status for test-wasm-app: Running
[gateway] Received application status for test-wasm-app: Running   ← ogni 30s
```

```bash
$ kubectl get application test-wasm-app -n wasmbed -o yaml
status:
  phase: Running
  deviceStatuses:
    device-fb144c394c384dcd9709371842c2197c:
      status: Running
      lastHeartbeat: "2026-03-08T13:52:14Z"
      metrics: null
  lastUpdated: "2026-03-08T13:52:14Z"
```

---

## Punto 6 — wasmbed-renode-sidecar rimosso

Il crate `wasmbed-renode-sidecar` è stato eliminato. La sua funzionalità (avvio istanze Renode, gestione firmware, wiring rete) è stata integrata in `wasmbed-qemu-manager`, che gestisce un singolo container Renode con N macchine virtuali indipendenti ("una Renode, N device proxy").

Questo è documentato in dettaglio in `doc/ARCHITECTURE.md`, sezione "Una Renode, N device proxy".

---

## Bug critico: ricezione frame nel main loop

### Il problema

Il main loop originale usava `network_receive()` direttamente:

```c
uint8_t recv_buffer[4096];
uint32_t received_len = 0;
if (network_receive(recv_buffer, sizeof(recv_buffer), &received_len) == 0
    && received_len > 0) {
    wasmbed_protocol_handle_message(recv_buffer, received_len);
}
```

`network_receive` è un singolo `zsock_recv` non bloccante. Il protocollo usa un framing con **4 byte di lunghezza** seguiti dal payload CBOR. Con un singolo `recv` su una connessione TLS, molto spesso arrivano solo i **4 byte dell'header** mentre il payload è ancora in transito. Il buffer conteneva quindi solo l'header, e `handle_message` scartava silenziosamente il messaggio perché `data_len < 4 + payload_len`.

Il messaggio `DeployApplication` (decine di kilobyte per il WASM) veniva perso quasi sempre.

### Il fix: `network_poll_readable` + `recv_frame`

**`network_handler.c` — nuova funzione:**

```c
int network_poll_readable(int timeout_ms)
{
    if (socket_fd < 0) return -1;
    struct zsock_pollfd pfd;
    pfd.fd      = socket_fd;
    pfd.events  = ZSOCK_POLLIN;
    pfd.revents = 0;
    return zsock_poll(&pfd, 1, timeout_ms);
}
```

Usa `zsock_poll` (l'API Zephyr equivalente di POSIX `poll`) per verificare se ci sono dati disponibili sul socket TLS entro il timeout, senza bloccare il loop principale.

**`wasmbed_protocol.c` — nuova funzione `wasmbed_protocol_recv_and_handle()`:**

```c
int wasmbed_protocol_recv_and_handle(int timeout_ms)
{
    // 1. Verifica disponibilità dati con zsock_poll (non bloccante)
    int ready = network_poll_readable(timeout_ms);
    if (ready <= 0) return (ready == 0) ? 1 : -1;

    // 2. Dati disponibili: accumulazione completa del frame con recv_frame()
    static uint8_t frame_buf[MAX_WASM_SIZE + 128];
    uint32_t total_len = 0;
    int ret = recv_frame(frame_buf, sizeof(frame_buf), &total_len, 5000);
    if (ret < 0) { gateway_connected = false; return -1; }

    return wasmbed_protocol_handle_message(frame_buf, total_len);
}
```

`recv_frame()` fa loop di `network_receive()` accumulando byte finché non ha ricevuto esattamente `4 + payload_len` byte, con timeout complessivo. In questo modo il frame è sempre completo prima di essere passato al parser CBOR.

**`main.c` — sostituzione nel loop:**

```c
/* Prima */
uint8_t recv_buffer[4096];
uint32_t received_len = 0;
if (network_receive(recv_buffer, sizeof(recv_buffer), &received_len) == 0
    && received_len > 0) {
    wasmbed_protocol_handle_message(recv_buffer, received_len);
}

/* Dopo */
wasmbed_protocol_recv_and_handle(100);
```

Il buffer da 4096 byte sullo stack è stato anche eliminato (era pericoloso su bare-metal con stack limitato); quello statico da `MAX_WASM_SIZE + 128` dentro `wasmbed_protocol_recv_and_handle` è allocato nel BSS.

---

## Riepilogo file modificati

| File | Modifica |
|------|----------|
| `zephyr-app/src/wasmbed_protocol.c` | Aggiunto: stato deploy (`current_instance_id`, `app_deployed`, `last_app_status_ms`), chiamata `wamr_call_function` in `handle_deploy_application`, funzione `send_application_status()`, chiamata periodica in `tick()`, nuova funzione `wasmbed_protocol_recv_and_handle()` |
| `zephyr-app/src/wasmbed_protocol.h` | Aggiunta dichiarazione `wasmbed_protocol_recv_and_handle(int timeout_ms)` |
| `zephyr-app/src/network_handler.c` | Aggiunta funzione `network_poll_readable(int timeout_ms)` con `zsock_poll` |
| `zephyr-app/src/network_handler.h` | Aggiunta dichiarazione `network_poll_readable(int timeout_ms)` |
| `zephyr-app/src/main.c` | Main loop: sostituito `network_receive` + `handle_message` con `wasmbed_protocol_recv_and_handle(100)` |
| `k8s/test-resources/test-wasm-app.yaml` | Nuovo: Application CRD con modulo WASM minimale di test |

---

## Procedura di test end-to-end

### Prerequisiti

- k3s attivo con namespace `wasmbed` e CRD installati
- Registry locale `localhost:5000` raggiungibile
- Zephyr SDK installato in `/home/ubuntu/retrospect/zephyr-sdk-0.16.5`
- Venv Python attivo in `/home/ubuntu/retrospect/.venv`

### Step 1 — Build firmware

```bash
export ZEPHYR_SDK_INSTALL_DIR=/home/ubuntu/retrospect/zephyr-sdk-0.16.5
export CMAKE_PREFIX_PATH=/home/ubuntu/retrospect/zephyr-sdk-0.16.5
cd /home/ubuntu/retrospect/zephyr-workspace
source /home/ubuntu/retrospect/.venv/bin/activate
west build -b stm32f746g_disco /home/ubuntu/retrospect/zephyr-app
```

Output atteso: `[362/362] Linking C executable zephyr/zephyr.elf`, `Flash: 362772 bytes`, `RAM: 220080 bytes`

### Step 2 — Avvio device emulato (Renode)

```bash
# Copia firmware nel volume condiviso
DEVICE_ID=device-fb144c394c384dcd9709371842c2197c
docker run --rm \
  -v wasmbed-firmware-store:/firmware \
  -v /home/ubuntu/retrospect/zephyr-workspace/build/zephyr:/src:ro \
  alpine cp /src/zephyr.elf /firmware/${DEVICE_ID}/zephyr.elf

# Ferma container precedente se esiste
docker rm -f wasmbed-renode-${DEVICE_ID:0:18} 2>/dev/null

# Avvia Renode
docker run -dt --net=host --cap-add=NET_ADMIN --device=/dev/net/tun \
  --name wasmbed-renode-${DEVICE_ID:0:18} \
  -v /home/ubuntu/retrospect/zephyr-workspace/renode-scripts:/scripts:ro \
  -v wasmbed-firmware-store:/firmware:ro \
  antmicro/renode:nightly renode --plain \
  /scripts/${DEVICE_ID}.resc

# Configura interfaccia TAP (rete virtuale Renode → host)
sleep 9
sudo ip addr add 10.0.86.1/24 dev tap0 2>/dev/null || true
sudo ip link set tap0 up
sudo iptables -t nat -A POSTROUTING -s 10.0.86.0/24 -o cni0 -j MASQUERADE 2>/dev/null || true
sudo iptables -A FORWARD -i tap0 -o cni0 -j ACCEPT 2>/dev/null || true
```

### Step 3 — Abilitare pairing mode e attendere enrollment

```bash
GATEWAY_IP=192.168.100.179
GATEWAY_PORT=31834

# Abilita pairing mode sul Gateway
curl -X POST http://${GATEWAY_IP}:${GATEWAY_PORT}/api/v1/admin/pairing-mode \
  -H 'Content-Type: application/json' -d '{"enabled": true}'

# Monitora enrollment (il device si connette in ~10-20s)
kubectl logs -n wasmbed -l gateway=gateway-1 -f | grep -E "enrolled|device|Enrollment"
```

### Step 4 — Deploy modulo WASM

```bash
# Ottieni il device_id dell'ultimo device enrollato
DEVICE_ID=$(kubectl get device -n wasmbed \
  -o jsonpath='{.items[-1].metadata.name}')

# Applica Application CRD
kubectl apply -f /home/ubuntu/retrospect/k8s/test-resources/test-wasm-app.yaml

# Deploy via API
curl -X POST \
  "http://${GATEWAY_IP}:${GATEWAY_PORT}/api/v1/devices/${DEVICE_ID}/deploy" \
  -H 'Content-Type: application/json' \
  -d '{
    "app_id": "test-wasm-app",
    "name": "test-wasm-counter",
    "wasm_bytes": "AGFzbQEAAAABBAFgAAADAgEABwcBA3J1bgAACgQBAgAL"
  }'
```

### Step 5 — Verifica risultati

```bash
# Controlla log Gateway: deve comparire DeployAck con success=true
kubectl logs -n wasmbed -l gateway=gateway-1 | grep -E "DeployAck|ApplicationStatus|test-wasm"

# Controlla status CRD
kubectl get application test-wasm-app -n wasmbed -o yaml | grep -A 20 status:
```

**Output atteso nei log Gateway:**
```
Received deployment acknowledgment for test-wasm-app: success=true
Received application status for test-wasm-app: Running
Received application status for test-wasm-app: Running    ← ogni 30s
```

**Output atteso dal CRD:**
```yaml
status:
  phase: Running
  deviceStatuses:
    device-<uuid>:
      status: Running
      lastHeartbeat: "2026-03-08T..."
  lastUpdated: "2026-03-08T..."
```

---

## Problemi risolti durante lo sviluppo

### 1. DeployApplication silenziosamente scartato

**Sintomo:** Il Gateway inviava il messaggio di deploy, ma il device non rispondeva mai con `DeployAck`.

**Causa:** Il main loop usava un singolo `zsock_recv` non bloccante. I messaggi larghi (WASM + header) arrivavano in più chunk TCP; il primo `recv` restituiva solo l'header 4-byte; `handle_message` vedeva `data_len < 4 + payload_len` e scartava.

**Fix:** `wasmbed_protocol_recv_and_handle()` con `zsock_poll` + `recv_frame()` ad accumulo progressivo.

### 2. Stack overflow con buffer statico da 4 KB

**Sintomo:** Potenziale corruzione stack su bare-metal.

**Causa:** `uint8_t recv_buffer[4096]` dichiarato sullo stack nel main loop; su STM32F7 con Zephyr il main thread ha stack limitato.

**Fix:** Buffer statico `static uint8_t frame_buf[MAX_WASM_SIZE + 128]` dentro la funzione dedicata (allocato nel BSS, non sullo stack).

### 3. Errore 422 nella deploy API

**Sintomo:** `curl` POST a `/api/v1/devices/.../deploy` restituiva HTTP 422.

**Causa:** Il body JSON mancava dei campi `name` e `wasm_bytes` che la struct `DeploymentRequest` richiede.

**Fix:** Aggiunto il payload completo alla chiamata curl:
```json
{"app_id": "test-wasm-app", "name": "test-wasm-counter", "wasm_bytes": "..."}
```

### 4. Two-gateway confusion (gateway-1-deployment vs wasmbed-gateway)

**Sintomo:** Dopo rebuild e push del container Gateway, le modifiche non si riflettevano.

**Causa:** Nel cluster esistono due deployment distinti che usano la stessa immagine `localhost:5000/wasmbed/gateway:latest`:
- `wasmbed-gateway` — servizi ClusterIP, raggiungibile solo internamente
- `gateway-1-deployment` — servizi NodePort 31834/30443, quello a cui i device si connettono

Il rollout veniva eseguito sul deployment sbagliato.

**Fix:** Sempre usare `kubectl rollout restart deployment/gateway-1-deployment -n wasmbed`.

### 5. Deserialization error `missing field total_devices`

**Sintomo:** Log Gateway: `missing field total_devices` alla ricezione di `ApplicationStatus`.

**Causa:** `ApplicationStatistics` aveva campi obbligatori; il CRD K8s aveva `statistics: {}` salvato senza tutti i campi.

**Fix:** `#[serde(default)]` su tutti i campi di `ApplicationStatistics` in `wasmbed-k8s-resource`, rebuild e rollout Gateway.

---

## Note sull'infrastruttura di test

| Componente | Valore |
|-----------|--------|
| Board emulata | STM32F746G Discovery (via Renode nightly) |
| Zephyr | v3.5.0, SDK 0.16.5 |
| Rust | 1.88.0 |
| k3s | v1.34.4 |
| Gateway NodePort | 31834 (HTTP+CBOR), 30443 (TLS) |
| Rete virtuale | tap0, subnet 10.0.86.0/24, gateway host 10.0.86.1 |
| TLS | ECDSA P-256, `TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256` |
| WASM runtime | WAMR (WebAssembly Micro Runtime) integrato in Zephyr |
