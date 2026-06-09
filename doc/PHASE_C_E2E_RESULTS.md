# Phase C — End-to-End Test Results (2026-05-08)

## Obiettivo

Verificare il ciclo completo Cloud → Edge:

1. Device Zephyr (STM32F746G Discovery) si connette via TLS al gateway
2. Enrollment CBOR completato — device riconosciuto dalla CRD K8s esistente
3. Gateway invia un modulo WASM al device via TLS/CBOR
4. WAMR carica, istanzia ed esegue il modulo
5. Device invia `DeployAck(success=true)` → gateway aggiorna Application CRD

**Setup**: STM32F746G Discovery emulato in Renode, gateway Rustls in K3S,
`hello-wasi` (97 byte, `proc_exit(0)`) come modulo WASM di prova.

---

## Bug risolti

### Bug 1 — `Device::find()` non trovava mai il device

**File**: `crates/wasmbed-k8s-resource/src/device_client.rs`, riga 21

**Sintomo**: ogni reconnessione del firmware creava una nuova CRD con nome hash
invece di ricoinciare con `stm32-wasi-test`. Il gateway non riconosceva mai
un device già enrolled.

**Causa**: il confronto usava `public_key.to_string()`, che attraverso il trait
`Display` (derivato da `derive_more`) produce `"PublicKey(<base64>)"` — con il
wrapper — mentre la CRD memorizza la chiave come base64 puro.

```rust
// Prima (ERRATO):
if device.spec.public_key == public_key.to_string()  // "PublicKey(abc...)" != "abc..."

// Dopo (CORRETTO):
if device.spec.public_key == public_key.to_base64()  // "abc..." == "abc..."
```

---

### Bug 2 — Encoding base64 incoerente nella creazione CRD

**File**: `crates/wasmbed-gateway/src/main.rs`, riga 505

**Sintomo**: un device appena enrolled non veniva trovato al successivo
reconnect (anche con il Bug 1 corretto), perché la chiave era stata scritta
nella CRD con un encoding diverso da quello usato per cercarla.

**Causa**: `create_device_crd` usava `base64::STANDARD` (include padding `=`),
mentre `PublicKey::to_base64()` — usato in `Device::find()` — usa
`URL_SAFE_NO_PAD` (nessun padding).

```rust
// Prima (ERRATO — STANDARD produce "abc...=="):
let public_key_b64 = base64::Engine::encode(
    &base64::engine::general_purpose::STANDARD, public_key);

// Dopo (CORRETTO — URL_SAFE_NO_PAD = stesso encoding di to_base64()):
// must use URL_SAFE_NO_PAD to match PublicKey::to_base64() used in Device::find()
let public_key_b64 = base64::Engine::encode(
    &base64::engine::general_purpose::URL_SAFE_NO_PAD, public_key);
```

---

### Bug 3 — WAMR falliva l'istanziazione per preopen WASI di "/"

**File**: `zephyr-app/src/wamr_integration.c`, riga 189

**Sintomo**:
```
<err> fs: invalid file or dir name!!
<err> wamr_integration: Failed to instantiate WASM module:
      error inserting preopen fd 3 (directory /) into fd table
```

**Causa**: `wasm_runtime_set_wasi_args()` riceveva `dir_list = {"/"}`.
WAMR tenta di aprire ogni directory in `dir_list` tramite la platform FS API.
Zephyr con `CONFIG_FILE_SYSTEM=y` non ha alcun filesystem montato su `/`, quindi
`fs_stat("/")` fallisce e l'istanziazione viene abortita.

**Fix**: passare `NULL, 0` al posto di `{"/"}` — i moduli WASM che usano solo
`proc_exit` / `fd_write` non necessitano di preopen di directory.

```c
// Prima (ERRATO):
static const char *wasi_dir_list[] = {"/"};
wasm_runtime_set_wasi_args(module, wasi_dir_list, 1, ...);

// Dopo (CORRETTO):
/* No dir preopens: Zephyr's FS layer rejects "/" (no real mount),
 * and WASI modules that only use proc_exit/fd_write don't need them. */
wasm_runtime_set_wasi_args(module, NULL, 0, ...);
```

---

### Bug 4 — `proc_exit(0)` classificato erroneamente come errore

**File**: `zephyr-app/src/wamr_integration.c`, riga 323

**Sintomo**:
```
<err> wamr_integration: WASI _start failed: Exception: wasi proc exit
<wrn> wasmbed_protocol: WASM run() returned error
```
Gateway riceveva `DeployAck(success=false)`.

**Causa**: WAMR rappresenta qualsiasi `proc_exit(N)` — incluso exit 0 (successo)
— come eccezione con la stringa `"wasi proc exit"`. Il codice cercava la
stringa `"proc_exit(0)"` (formato errato) e, non trovandola, trattava l'uscita
come errore.

**Fix**: cercare `"wasi proc exit"` e usare `wasm_runtime_get_wasi_exit_code()`
per distinguere exit 0 (successo) da exit N≠0 (errore).

```c
// Prima (ERRATO — stringa mai trovata):
if (ex != NULL && strstr(ex, "proc_exit(0)") != NULL) { ... }

// Dopo (CORRETTO):
if (ex != NULL && strstr(ex, "wasi proc exit") != NULL) {
    uint32_t exit_code = wasm_runtime_get_wasi_exit_code(instance);
    wasm_runtime_clear_exception(instance);
    if (exit_code == 0) {
        LOG_INF("WASI module exited cleanly (proc_exit 0)");
        return 0;
    }
    LOG_ERR("WASI module exited with error (proc_exit %u)", exit_code);
    return -1;
}
```

---

## Test effettuati

### Test 1 — Enrollment con device pre-esistente

**Procedura**:
1. Renode avviato con firmware Zephyr per STM32F746G Discovery
2. Device ottiene IP via DHCP su tap0 (192.168.1.112)
3. Device apre connessione TLS verso `192.168.100.179:30443` (NodePort gateway)
4. Handshake TLS completato (TLS 1.2, ECDHE-ECDSA, certificato gateway ECDSA P-256)
5. Device invia CBOR `EnrollmentRequest` (0x81 0x01)
6. Gateway risponde `EnrollmentAccepted` (0x81 0x01)
7. Device invia `PublicKey` (32 byte, chiave statica di test: 32×0xAB)
8. Gateway cerca CRD con `public_key = "q6urq6urq6urq6urq6urq6urq6urq6urq6urq6urq6s"` (URL_SAFE_NO_PAD)
9. Gateway trova `stm32-wasi-test` → percorso reconnect
10. Gateway risponde `DeviceUuid` (16 byte UUID)
11. Device invia `EnrollmentAcknowledgment`

**Risultato UART**:
```
<inf> wasmbed_protocol: Received DeviceUuid from gateway
<inf> wasmbed_protocol: Sent EnrollmentAcknowledgment
<inf> wasmbed_protocol: Enrollment completed successfully!
```

**Risultato gateway**:
```
INFO wasmbed_gateway: Device stm32-wasi-test reconnected (previously enrolled)
INFO wasmbed_gateway: Enrollment completed successfully
```

### Test 2 — Deploy e esecuzione WASM end-to-end

**Modulo WASM usato**: `hello-wasi` (97 byte)
```
(module
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (func $_start  call $proc_exit (i32.const 0))
  (memory 1)
  (export "_start" (func $_start))
  (export "memory" (memory 0))
)
```

**Procedura**:
```bash
curl -X POST "http://localhost:8080/api/v1/devices/stm32-wasi-test/deploy" \
  -H "Content-Type: application/json" \
  -d '{"app_id": "hello-wasi", "name": "hello-wasi", "wasm_bytes": ""}'
```

**Flusso osservato** (log UART Renode):
```
<inf> wamr_integration: Loading WASM module (size: 97 bytes)...
<inf> wamr_integration: WASM module loaded (module_id: 1)
<inf> wamr_integration: Instantiating WASM module (module_id: 1)...
<inf> wamr_integration: WASM module instantiated (instance_id: 1)
<inf> wasmbed_protocol: WASM deployed: app_id=hello-wasi module_id=1 instance_id=1
<inf> wamr_integration: Calling WASI _start (instance 1)
<inf> wamr_integration: WASI module exited cleanly (proc_exit 0)
```

**Risultato gateway**:
```
INFO wasmbed_gateway::http_api: Received deployment request for device stm32-wasi-test: app_id=hello-wasi
INFO wasmbed_gateway::http_api: TLS connection found for device stm32-wasi-test, proceeding with deployment
INFO wasmbed_gateway::http_api: Sending message to device stm32-wasi-test: DeployApplication { ... wasm_bytes: [0, 97, 115, 109, ...] }
INFO wasmbed_gateway::http_api: Successfully sent deployment command for app hello-wasi to device stm32-wasi-test
INFO wasmbed_gateway: Received deployment acknowledgment for hello-wasi: success=true
```

**Esito**: `success=true` — ciclo completo verificato.

---

## Stato infrastruttura durante i test

| Componente | Stato |
|---|---|
| K3S cluster | Running |
| `wasmbed-gateway` pod | `gateway-1-deployment-5ff5ccbfbf-7ztlf` |
| `wasmbed-api-server` pod | Running |
| Renode container | `wasmbed-renode-stm32-wasi-test` |
| Device CRD | `stm32-wasi-test` (namespace `wasmbed`) |
| Application CRD | `hello-wasi` (namespace `wasmbed`) |
| tap0 IP | `192.168.1.1/24` (host) — device `192.168.1.112` (DHCP) |
| Gateway NodePort | `192.168.100.179:30443` (TLS) / `localhost:8080` (HTTP) |

---

## File modificati

| File | Modifica |
|---|---|
| `crates/wasmbed-k8s-resource/src/device_client.rs` | `to_string()` → `to_base64()` nel confronto `Device::find()` |
| `crates/wasmbed-gateway/src/main.rs` | Encoding base64 `STANDARD` → `URL_SAFE_NO_PAD` in `create_device_crd` |
| `zephyr-app/src/wamr_integration.c` | Rimosso preopen `"/"` da `wasi_dir_list` |
| `zephyr-app/src/wamr_integration.c` | Corretta detection `proc_exit` con `wasm_runtime_get_wasi_exit_code()` |

---

## Note per sviluppi futuri

- Il firmware usa ancora una **chiave statica di test** (32×0xAB). Serve implementare
  la generazione/storage di una chiave Ed25519 persistente in flash (o in
  Renode-injected memory) per produzione.
- `CONFIG_FILE_SYSTEM=y` è necessario per la compilazione di WAMR (`zephyr_file.c`)
  ma nessun filesystem è effettivamente montato a runtime. Se in futuro si vogliono
  WASM module che accedono a file, sarà necessario montare un filesystem (es. LittleFS
  su flash) e aggiungere la directory alla `dir_list`.
- Il modulo `hello-wasi` non stampa nulla perché non usa `fd_write`. Un modulo più
  realistico dovrebbe includere output su stdout (fd 1) per verificare anche il path
  WASI I/O.
- Il Device CRD mostra `phase: Disconnected` a causa del device controller che
  sovrascrive lo stato del gateway durante la riconciliazione: da indagare nel
  `wasmbed-device-controller`.
