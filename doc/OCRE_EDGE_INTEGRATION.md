# OCRE Edge Integration — Modifiche e Flusso Attuale

> **Scopo**: unico documento di riferimento per tutte le modifiche apportate
> al progetto nell'ambito dell'integrazione OCRE / supporto device Linux.
> Descrive cosa è cambiato, perché, e come funziona il sistema adesso.
>
> Documenti correlati: `OCRE_DEPLOYMENT_TOPOLOGY.md` (scelte topologiche),
> `OCRE_INTEGRATION_ANALYSIS.md` (analisi costi/benefici OCRE firmware).

---

## 1. Decisione architetturale

### Opzione A — Edge eterogeneo, Gateway invariato

Fra le tre opzioni descritte in `OCRE_DEPLOYMENT_TOPOLOGY.md`, è stata adottata
**Opzione A**: il Gateway resta invariato; MCU Zephyr e MPU Linux parlano
entrambi **TLS + CBOR** (`wasmbed-protocol`). I limiti che emergono dalla
mancata traduzione di funzionalità K8s verso device CBOR vengono gestiti caso
per caso (vedi §5).

### Modello A1 — CBOR uniforme

Per i device MPU (Linux) è stato scelto il **Modello A1**: stesso identico
protocollo CBOR del firmware Zephyr, senza bridge passthrough. Il daemon Linux
`wasmbed-edge-client` usa `wasmbed-protocol` su tokio+rustls; il Gateway non
distingue MCU da MPU nel codice di routing.

Il Modello A2 (bridge passthrough con `ServerMessage::RawManifest`) rimane
un'opzione futura da valutare se serve log streaming o ConfigMap ref direttamente
sul device Linux.

---

## 2. Riepilogo modifiche al codice

### 2.1 Fix bug: ApplicationConfig mai propagata (Gateway)

**File**: `crates/wasmbed-gateway/src/http_api.rs`

**Problema**: il Gateway leggeva `ApplicationConfig` dalla CRD ma inviava
sempre `config: None` nel messaggio CBOR `DeployApplication`. Ogni configurazione
di `env_vars`, `memory_limit`, `args` specificata nel manifest K8s veniva
silenziosamente scartata.

**Fix**:
- `deploy_application_to_device` ora accetta `config: Option<wasmbed_protocol::ApplicationConfig>`
- Aggiunta `map_k8s_config_to_protocol(crd: &ApplicationConfig)` che mappa
  `memory_limit`, `cpu_time_limit`, `env_vars`, `args` dal tipo CRD al tipo
  protocollo. I campi `auto_restart` e `max_restarts` vengono omessi: sono
  policy Gateway-side (il restart è triggerato da un nuovo `DeployApplication`)
  e non hanno semantica device-side.
- `app_config` viene clonato prima della chiamata a `register_application`
  per evitare il move-before-use.

### 2.2 Nuovi campi CRD: TargetRuntime e DeviceClass

**Motivazione**: preparare il control-plane K8s per distinguere device MCU da
MPU e per indicare quale runtime WASM il device usa.

#### `crates/wasmbed-k8s-resource/src/application.rs`

Aggiunto a `ApplicationSpec`:
```rust
#[serde(default, rename = "targetRuntime")]
pub target_runtime: Option<TargetRuntime>,
```
Nuovo enum:
```rust
pub enum TargetRuntime {
    WamrRaw,      // default — WAMR puro, comportamento attuale
    OcreZephyr,   // OCRE container runtime su Zephyr (MCU)
    OcreLinux,    // OCRE container runtime su Linux (MPU)
}
```
Compatibilità: campo `Option`; manifest esistenti con `target_runtime` assente
usano il default `WamrRaw` senza cambiamenti.

#### `crates/wasmbed-k8s-resource/src/device.rs`

Aggiunto a `DeviceSpec`:
```rust
pub device_class: Option<DeviceClass>,
pub runtime_target: Option<String>,
```
Nuovo enum:
```rust
pub enum DeviceClass {
    McuConstrained,  // Zephyr+WAMR su microcontrollori
    MpuRich,         // Linux su SBC, gateway industriale, VM edge
}
```
Compatibilità: entrambi i campi sono `Option` con `skip_serializing_if`; i
Device CRD esistenti non necessitano aggiornamento.

### 2.3 McuType: varianti Linux

**File**: `crates/wasmbed-qemu-manager/src/lib.rs`

Aggiunte tre varianti all'enum `McuType`:
```rust
LinuxArm64,   // Raspberry Pi 4/5, ARM64 SBC, board industriali ARM
LinuxX86_64,  // VM edge, gateway industriale x86, mini-PC
LinuxRiscV,   // device RISC-V Linux (SiFive, ecc.)
```
Aggiunto helper:
```rust
pub fn is_emulated(&self) -> bool {
    !matches!(self, McuType::LinuxArm64 | LinuxX86_64 | LinuxRiscV)
}
```
Per le tre varianti Linux, tutti i metodi di emulazione Renode (`get_firmware_path`,
`get_uart_name`, `renode_platform`) restituiscono stringa vuota o `Err`. La
logica di avvio del container Renode deve verificare `is_emulated()` prima di
tentare l'avvio (i device Linux si registrano autonomamente via
`wasmbed-edge-client`).

### 2.4 CRD YAML rigenerati

```
k8s/crds/device-crd.yaml       → aggiunge deviceClass (McuConstrained|MpuRich), runtimeTarget
k8s/crds/application-crd.yaml  → aggiunge targetRuntime (WamrRaw|OcreZephyr|OcreLinux)
```

### 2.5 WASI abilitato nel firmware Zephyr

**File**: `zephyr-app/CMakeLists.txt` — riga 25

```cmake
# prima
set(WAMR_BUILD_LIBC_WASI 0)
# dopo
set(WAMR_BUILD_LIBC_WASI 1)
```

Abilita la WASI libc completa in WAMR (inclusi socket via
`sandboxed-system-primitives/src/posix.c`). La piattaforma Zephyr di WAMR
usa `zsock_*` internamente e **non richiede** `CONFIG_POSIX_API=y` in
`prj.conf` — le modifiche si limitano a CMakeLists.txt.

Con questa flag, le app WASM deployate sul device Zephyr possono importare
funzioni WASI standard (`fd_read`, `fd_write`, socket WASI) e il runtime le
eseguirà tramite il layer Zephyr di WAMR.

### 2.6 Nuovo crate: wasmbed-edge-client

**Percorso**: `crates/wasmbed-edge-client/`

Daemon Linux (tokio + rustls) che implementa lo stesso protocollo del firmware
Zephyr, permettendo a un device MPU (Raspberry Pi, VM, SBC ARM64) di
partecipare alla topologia Cloud-Fog-Edge senza modifiche al Gateway.

| File | Responsabilità |
|---|---|
| `Cargo.toml` | `wasmbed-protocol`, `tokio-rustls 0.26`, `rustls 0.23`, `minicbor 1.1`, `wasmtime 18.0`, `wasmtime-wasi 18.0`, `clap 4.4` |
| `src/main.rs` | CLI args (`--gateway`, `--public-key` hex, `--ca-cert`); installa rustls crypto provider |
| `src/protocol.rs` | TLS connect, framing 4-byte BE, enrollment, event loop |
| `src/wasm_runner.rs` | Esecuzione WASM con wasmtime + WASI preview1 |

---

## 3. Il protocollo CBOR in dettaglio

Il protocollo è definito in `crates/wasmbed-protocol/src/lib.rs` ed è `#![no_std]`
(compatibile sia con Zephyr firmware che con il daemon Linux).

**Wire format**: ogni messaggio viaggia come `[u32 BE length][CBOR payload]`
su TLS. Non c'è Envelope wrapper nel traffico reale — il CBOR codifica
direttamente `ClientMessage` o `ServerMessage`.

### Messaggi Client → Gateway

| Variante CBOR | Scopo |
|---|---|
| `Heartbeat` | Keepalive ogni 25 s — mantiene la connessione TLS attiva |
| `EnrollmentRequest` | Avvio handshake enrollment |
| `PublicKey { key }` | Consegna chiave Ed25519 (32 byte) al Gateway |
| `EnrollmentAcknowledgment` | Conferma completamento enrollment |
| `ApplicationDeployAck { app_id, success, error? }` | Esito del deploy WASM |
| `ApplicationStopAck { app_id, success, error? }` | Esito dello stop app |
| `DeviceInfo { available_memory, cpu_arch, wasm_features, max_app_size }` | Capacità hardware |
| `ApplicationStatus { app_id, status, error?, metrics? }` | Stato app su richiesta |

### Messaggi Gateway → Client

| Variante CBOR | Scopo |
|---|---|
| `HeartbeatAck` | Conferma heartbeat ricevuto |
| `EnrollmentAccepted` | Enrollment approvato (pairing mode attivo) |
| `EnrollmentRejected { reason }` | Enrollment rifiutato |
| `DeviceUuid { uuid }` | UUID assegnato durante enrollment |
| `EnrollmentCompleted` | Enrollment terminato |
| `DeployApplication { app_id, name, wasm_bytes, config? }` | Deploy WASM verso device |
| `StopApplication { app_id }` | Stop app sul device |
| `RequestDeviceInfo` | Gateway chiede capacità hardware |
| `RequestApplicationStatus { app_id? }` | Gateway chiede stato app |

### ApplicationConfig

```rust
pub struct ApplicationConfig {
    pub memory_limit: u64,
    pub cpu_time_limit: u64,
    pub env_vars: BTreeMap<String, String>,
    pub args: Vec<String>,
}
```

Ora propagata correttamente dal manifest K8s al device nel messaggio
`DeployApplication`. I campi CRD `auto_restart` e `max_restarts` vengono
omessi intenzionalmente (policy Gateway-side, non hanno semantica device-side).

---

## 4. Come funziona il flusso adesso

### 4.1 Deploy su device MCU (Zephyr — invariato)

```
1. Utente → Dashboard → "Deploy Application" su device MCU
2. Dashboard → API Server POST /api/v1/applications (con WASM binario)
3. API Server → K8s: crea Application CRD
   spec.targetRuntime: WamrRaw (o assente, stesso effetto)
4. Application Controller (watch loop) → rileva nuova CRD
5. Application Controller → Gateway HTTP POST /api/v1/applications/deploy
6. Gateway legge Application CRD:
   - spec.wasm_bytes (base64)
   - spec.config → map_k8s_config_to_protocol() → ApplicationConfig CBOR
7. Gateway trova il device target (TLS connection attiva per quel device)
8. Gateway invia CBOR: DeployApplication { wasm_bytes, config: Some(...) }
9. Firmware Zephyr riceve il messaggio:
   - wamr_load_module(wasm_bytes)
   - wamr_instantiate(module_id)
   - wamr_call_function(instance_id, "_start")
   [Con WAMR_BUILD_LIBC_WASI=1: le app possono ora usare syscall WASI]
10. Firmware → invia ApplicationDeployAck { success: true }
11. Gateway aggiorna Application CRD status: phase=Running
```

### 4.2 Deploy su device MPU Linux (wasmbed-edge-client — nuovo)

Il device Linux non viene avviato da Renode (nessun container firmware).
Il daemon `wasmbed-edge-client` si avvia manualmente sul device e si
connette al Gateway.

**Prerequisito**: il Device CRD deve essere creato manualmente con:
```yaml
spec:
  publicKey: "<hex chiave Ed25519 del device>"
  mcuType: "LinuxArm64"          # o LinuxX86_64, LinuxRiscV
  deviceClass: "MpuRich"
```

**Avvio del daemon**:
```bash
wasmbed-edge-client \
  --gateway <gateway-host>:8081 \
  --public-key <hex-32-byte-ed25519> \
  [--ca-cert /path/to/ca.pem]
```

**Enrollment del device Linux** (identico al firmware Zephyr):
```
1. TCP connect → gateway:8081
2. TLS handshake (no client cert — autenticazione via CBOR)
3. C→S: EnrollmentRequest
4. S→C: EnrollmentAccepted
5. C→S: PublicKey { key: [32 byte from --public-key] }
   Gateway: cerca Device CRD con quella chiave pubblica, aggiorna phase=Enrolled
6. S→C: DeviceUuid { uuid: [...] }
7. C→S: EnrollmentAcknowledgment
8. S→C: EnrollmentCompleted
   Gateway: aggiorna Device CRD phase=Connected
```

**Event loop (dopo enrollment)**:
```
Ogni 25s:           → Heartbeat  →  Gateway aggiorna last_heartbeat
DeployApplication:  → WASM bytes → wasmtime.run(_start)  → DeployAck
StopApplication:    → abort task → StopAck
RequestDeviceInfo:  → DeviceInfo { cpu_arch: "aarch64", ... }
```

**Esecuzione WASM** (wasmtime 18 + WASI preview1):
- Il runner carica il modulo con `wasmtime::Module::new`
- Aggiunge WASI preview1 tramite `wasmtime_wasi::preview2::preview1::add_to_linker_sync`
- Esegue `_start` (WASI command) o `main` in un `tokio::task::spawn_blocking`
- I log dell'applicazione WASM vanno su stdout/stderr del daemon
- Per stoppare: `AbortHandle::abort()` sul task

### 4.3 Enrollment: confronto MCU vs MPU

| Aspetto | MCU Zephyr | MPU Linux (edge-client) |
|---|---|---|
| Avvio | Container Renode lanciato dal Renode Manager | Daemon manuale (`wasmbed-edge-client`) |
| TLS | `PEER_VERIFY_NONE` (nessuna verifica server) | `--ca-cert` opzionale; skip-verify in dev |
| Client cert | Assente (no `with_client_auth`) | Assente (identico) |
| Autenticazione | `PublicKey` CBOR → Device CRD match | Identico |
| Heartbeat | Ogni 25s (hardcoded in firmware) | Ogni 25s (tokio interval) |
| WASM execution | WAMR interpreter/AOT | wasmtime (JIT) |

---

## 5. Gap noti tra K8s e CBOR

Funzionalità K8s che non hanno un canale CBOR corrispondente. Per ognuna
è indicata la strategia consigliata se si vuole implementarla.

### 5.1 Configurazione

| Funzionalità | Stato | Strategia |
|---|---|---|
| `env_vars` inline nel manifest Application | ✅ **Fix implementato** (sessione 2) | — |
| ConfigMap referenced (`envFrom: configMapRef`) | Assente | Estendi CBOR con `ConfigPush` |
| Secret referenced (`envFrom: secretRef`) | Assente | Come sopra |
| Aggiornamento config a runtime (no redeploy) | Assente | Aggiungi `ServerMessage::UpdateConfig` |

### 5.2 Osservabilità

| Funzionalità | Stato | Strategia |
|---|---|---|
| `kubectl logs` / log streaming | Assente | `LogChunk` CBOR o MPU passthrough |
| K8s Events dal device | Assente | `DeviceEvent` CBOR → K8s Event API |
| Prometheus metrics pull | Assente | Estendi `ApplicationMetrics` + endpoint |
| Health probe (liveness/readiness) | Assente | `Heartbeat` è keepalive opaco; aggiungere campo health |

### 5.3 Ciclo di vita applicazione

| Funzionalità | Stato | Strategia |
|---|---|---|
| Rolling update (zero-downtime) | Assente | `UpdateApplication` con handover |
| Delta update WASM | Assente | Patch binaria + checksum |
| Rollback automatico | Assente | Mantieni versione precedente in Gateway registry |
| Multi-app concorrenti per device | Assente | Multi-slot registry + `app_id` routing |

### 5.4 Debug remoto (solo MPU)

| Funzionalità | Stato | Strategia |
|---|---|---|
| `kubectl exec` | Assente | MPU passthrough (Modello A2) |
| `kubectl port-forward` | Assente | Come sopra |

### 5.5 Stub non collegati nel Gateway

Parti del Gateway che esistono come placeholder ma non hanno implementazione reale:

| Componente | Posizione | Stato |
|---|---|---|
| `tls_server.rs` loop | `tls_server.rs:21-29` | Solo `sleep(30s)` — il vero TLS è in `wasmbed-tls-utils` |
| `enrollment.rs` logic | `enrollment.rs:29-40` | `sleep(1s)` + UUID finto |
| `update_gateway` handler | `http_api.rs:1509-1549` | Returns success, no-op |
| `enroll_device` handler | `http_api.rs:1753-1765` | Returns success, no-op |
| DeviceInfo capabilities | `http_api.rs:569-597` | Hardcoded: 1 GB, riscv32, 1 MB |
| System metrics | `http_api.rs:1010-1112` | JSON hardcoded, non dati reali |

---

## 6. Prossimi step

| Priorità | Step | Note |
|---|---|---|
| Alta | Test build firmware su Renode con `WAMR_BUILD_LIBC_WASI=1` | Verificare incremento dimensione ELF e heap WAMR sufficiente su STM32F746G |
| Alta | Enrollment di un device LinuxArm64 via `wasmbed-edge-client` | Richiede Device CRD con `mcuType: LinuxArm64` e la chiave pubblica corretta |
| Media | Integrazione OCRE su firmware Zephyr (STM32F746G) | `wamr_integration.c` refactor, CMakeLists.txt + modulo west OCRE |
| Media | `targetRuntime` usato dal Gateway per scegliere il runtime | Oggi il campo è nella CRD ma il Gateway non lo legge |
| Bassa | Modello A2 (bridge passthrough per MPU) | Aggiunge `ServerMessage::RawManifest` al protocollo |

---

## 7. Compatibilità backward

Tutti i cambiamenti sono backward-compatible:

- **CRD**: tutti i nuovi campi (`deviceClass`, `runtimeTarget`, `targetRuntime`) sono
  `Option` con `serde(default)` o `skip_serializing_if`. I manifest YAML pre-esistenti
  continuano a deserializzare senza modifiche.
- **CBOR**: nessuna modifica al protocollo `wasmbed-protocol`. I device già
  enrollati non necessitano aggiornamento firmware.
- **Gateway**: `ApplicationConfig: None` nei DeployApplication esistenti
  (device non aggiornati) continua a funzionare; il fix di propagazione è
  additive-only.
- **McuType**: le nuove varianti Linux hanno alias `serde` (`LinuxArm64`,
  `linux_arm64`) e non impattano i device Zephyr esistenti.
