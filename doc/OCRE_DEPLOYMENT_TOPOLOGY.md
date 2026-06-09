# OCRE Deployment Topology — Analisi Architetturale

> **Documento complementare a `OCRE_INTEGRATION_ANALYSIS.md`.**
> Quello analizza *come* integrare OCRE nel firmware Zephyr (livello firmware).
> Questo analizza *dove* collocare OCRE nella topologia Cloud-Fog-Edge e se il
> ruolo del Gateway debba cambiare (livello architetturale).

---

## DECISIONE ARCHITETTURALE — 2026-05-07

> **Opzione A adottata: "Edge eterogeneo, Gateway invariato".**

Il Gateway resta un hub stateless TLS+CBOR. L'edge si biforca in MCU
constrained (Zephyr+WAMR) e MPU ricchi (Linux+WAMR+OCRE), entrambi con lo
stesso protocollo di base. Le opzioni B e C rimangono come riferimento storico
in questo documento ma **non saranno implementate** nella baseline della tesi.

**Documenti prodotti da questa sessione:**
- [`GATEWAY_TRANSLATION_GAPS.md`](GATEWAY_TRANSLATION_GAPS.md) — catalogo
  completo dei gap di traduzione K8s ↔ CBOR (incluso bug `ApplicationConfig`
  non propagata, canali assenti, stub non collegati).
- [`DEVICE_CLASS_DIFFERENTIATION.md`](DEVICE_CLASS_DIFFERENTIATION.md) —
  proposta di differenziazione MCU/MPU: tassonomia, due modelli di transport
  (CBOR uniforme vs bridge passthrough), modifiche CRD/protocollo necessarie
  (non ancora implementate).

**Prossimi passi immediati** (in ordine di priorità):
1. Fix bug `ApplicationConfig` mai propagata al device (`http_api.rs:423`).
2. Fase 1 roadmap: Zephyr+OCRE su MCU (vedi `OCRE_INTEGRATION_ANALYSIS.md`).
3. Fase 2: aggiungere `spec.targetRuntime` alla Application CRD.

---

## 1. Punto di partenza e problema

### Stato attuale di RETROSPECT

```
CLOUD       Kubernetes (API Server + Controllers + Dashboard)
               ↕  HTTP/K8s API
FOG         Gateway (wasmbed-gateway)
               ↕  TLS 1.3 + CBOR southbound
EDGE        Zephyr RTOS + WAMR + app WASM
            [STM32, ESP32, RISC-V, Cortex-R8 — tutti MCU constrained]
```

Il Gateway è un hub **stateless** e **single-responsibility**: riceve intent da
K8s (spec CRD) e li traduce in messaggi CBOR verso device connessi via TLS.
Non esegue workload. Non ospita runtime.

### Il problema nuovo

OCRE (Open Container Runtime for Embedded) non è confinato a Zephyr. Il
progetto supporta anche target **Linux POSIX**. Questo significa che un edge
node può essere un device Linux-based (Raspberry Pi, gateway industriale ARM,
VM edge) che esegue OCRE con le stesse API container ma sopra un kernel Linux.

Domande che questo documento risponde:

1. Dove va OCRE nella topologia a tre layer?
2. Il Gateway deve cambiare ruolo per supportare edge Linux?
3. Qual è l'architettura più pulita e con il rapporto complessità/valore migliore?

---

## 2. Le tre opzioni architetturali

### Opzione A — "Edge eterogeneo, Gateway invariato"

Il Gateway rimane esattamente quello che è. L'edge si biforca in due famiglie
di device che parlano **lo stesso protocollo** (TLS 1.3 + CBOR):

```
CLOUD       Kubernetes
               ↕  K8s API
FOG         Gateway (invariato)
             /           \
           TLS+CBOR     TLS+CBOR
           /                 \
EDGE    Zephyr+WAMR+OCRE    Linux+WAMR+OCRE
        [MCU constrained]    [Raspberry, ARM board, VM]
```

Il Gateway non sa (e non deve sapere) quale OS gira sotto il device. La
distinzione `ocre_zephyr` vs `ocre_linux` è codificata nella Application CRD
(`spec.targetRuntime`) e nel record device registrato. Il Gateway seleziona il
payload CBOR corretto (`DeployApplication`) in base al target registrato.

**Cosa serve di nuovo:**
- Un **client CBOR Linux** (`wasmbed-edge-client` come binario systemd o
  daemon) che implementa lo stesso protocollo del firmware Zephyr. La logica
  enrollment + heartbeat + deploy è identica; cambia solo il trasporto
  sottostante (rustls su Linux vs mbedTLS su Zephyr).
- Campo `spec.targetRuntime` nella Application CRD.
- Estensione di `McuType` o `DeviceSpec` per registrare device Linux nel
  Gateway registry.

**Pro:**
- Tre layer netti, separation of concerns intatta.
- Il Gateway non accumula stato o responsabilità di runtime.
- Scalabile: aggiungere nuovi tipi di edge (RISC-V Linux, x86 VM) senza
  toccare il control-plane.
- Narrativa tesi pulita: "stesso protocollo sicuro per device eterogenei".
- TLS + Ed25519 funziona identicamente su Linux/rustls — nessun porting
  crittografico.

**Contro:**
- Serve un nuovo componente (`wasmbed-edge-client`) che è essenzialmente un
  porting del firmware Zephyr in Rust POSIX.
- CBOR espone un sottoinsieme stretto delle capability di un device Linux
  (manca: filesystem, processi multipli, healthcheck strutturati, log
  streaming). Linux viene trattato come un "MCU grosso" — le sue capacità
  extra restano non esposte al control-plane.
- OCRE-on-Linux è meno maturo di OCRE-on-Zephyr; le host functions GPIO via
  `/dev/gpiochip` sono in upstream ma con meno testing.

**Approfondimenti:**
- Catalogo completo dei gap di traduzione: [`GATEWAY_TRANSLATION_GAPS.md`](GATEWAY_TRANSLATION_GAPS.md)
- Differenziazione MCU/MPU e transport passthrough per Linux: [`DEVICE_CLASS_DIFFERENTIATION.md`](DEVICE_CLASS_DIFFERENTIATION.md)

---

### Opzione B — "Gateway-as-Edge" (Gateway con OCRE embedded)

Il Gateway esegue esso stesso applicazioni OCRE/WASM oltre a fare da hub.
Diventa simultaneamente fog router e edge ricco.

```
CLOUD       Kubernetes
               ↕  K8s API
FOG/EDGE    Gateway + OCRE runtime
            [esegue workload locali via K8s Pod]
               ↕  TLS+CBOR
EDGE        Zephyr+WAMR+OCRE [MCU constrained, invariati]
```

I workload "edge ricco" non escono mai dal cluster K8s: il Gateway Pod esegue
WASM via OCRE in-process o in sidecar. Gli MCU constrained restano connessi
al Gateway come oggi.

**Pro:**
- Per workload "edge ricco" non serve un device fisico aggiuntivo — il Gateway
  Pod è già un processo long-running con risorse allocate.
- Latenza minima per workload che devono elaborare dati prima di inviarli al
  cloud (fog computing classico).
- Riusa K8s scheduler, probes, logs per il runtime OCRE locale.

**Contro:**
- **Rompe la separation of concerns fondamentale.** Il Gateway è l'unico
  choke point del sistema — ogni device passa da lì. Caricarlo con
  responsabilità di runtime stateful lo trasforma nel componente più critico
  e più difficile da scalare.
- Aumenta la superficie di attacco del nodo che già detiene tutte le
  connessioni TLS client.
- HA diventa complessa: se il Gateway Pod viene ricreato, tutti i device
  devono riconnettersi *e* i workload locali vanno riavviati.
- **Non è "edge"** nel senso topologico: i workload girano nel cluster K8s,
  non sul device fisico lontano. La narrazione "continuum Cloud-Fog-Edge" si
  sgonfia.
- Effort alto per beneficio discutibile: i casi d'uso di fog computing
  locale si coprono meglio con un nodo K8s dedicato (vedi Opzione C).

**Verdetto: scartata.** Il Gateway deve restare un hub leggero e stateless.

---

### Opzione C — "Sidecar Linux: nodi K8s come edge ricco"

Edge ricco = un **nodo K8s aggiuntivo** (k3s agent su Raspberry o ARM board).
Il Raspberry entra nel cluster come worker node standard. Le Application vengono
deployate su di esso come **Pod nativi Kubernetes**, con o senza OCRE/WASM.
Il Gateway continua a gestire solo gli MCU constrained via CBOR.

```
CLOUD       Kubernetes control plane
             /                    \
           K8s API              K8s API
           /                         \
FOG      Gateway                  k3s worker (Raspberry)
           ↕ TLS+CBOR             [Pod nativi: WASM+OCRE o binari Linux]
EDGE     Zephyr+WAMR+OCRE
         [MCU constrained]
```

L'Application Controller discrimina: device constrained → Gateway API,
device Linux-k8s → kubectl apply Pod.

**Pro:**
- Riusa K8s end-to-end per Linux (kubectl, probes, logs, resource limits,
  rollout nativi) — zero nuovo codice di control-plane.
- Standard industriale: k3s edge nodes è il pattern dominante (KubeEdge,
  Akri, ecc.).
- Nessuna modifica al Gateway o al protocollo CBOR.
- Raspberry Pi è già un worker node k3s documentato e supportato.

**Contro:**
- **Due percorsi di deploy radicalmente diversi** (CBOR/TLS vs K8s Pod) —
  l'Application Controller deve discriminare e gestire entrambi; la CRD
  `Application` deve astrarre su percorsi incompatibili.
- "Edge" diventa ambiguo: un nodo K8s nel cluster è ancora "edge"? O è
  semplicemente un fog node con footprint ridotto? La narrativa a tre layer
  si sfuma.
- Perde l'unità architetturale: non tutto passa dal Gateway, il protocollo
  comune (CBOR) non copre più tutti i device, il registry è spezzato tra
  Gateway registry e K8s node registry.
- OCRE su questo percorso diventa opzionale/secondario — il Pod Linux può
  girare qualsiasi processo, non necessariamente WASM.

**Verdetto: valida come evoluzione futura, non come architettura iniziale.**
Conviene solo se il requisito esplicito è integrare device Linux già in un
cluster K8s esistente.

---

## 3. Tabella comparativa

| Dimensione | A — Edge eterogeneo | B — Gateway-as-Edge | C — K8s sidecar |
|---|---|---|---|
| Complessità Gateway | Invariata (bassa) | Alta (diventa stateful) | Invariata |
| Copertura device | MCU + Linux edge | MCU + fog locale | MCU + K8s worker |
| Unità protocollo | TLS+CBOR per tutti | CBOR (MCU) + K8s API | CBOR (MCU) + K8s API |
| Coerenza narrativa tesi | Alta | Media | Media |
| Effort implementativo | Medio (nuovo client) | Alto (refactor Gateway) | Basso (k3s join) |
| Scalabilità | Alta | Bassa (choke point) | Alta |
| Sicurezza (superficie) | Gateway invariato | Gateway più esposto | Gateway invariato |
| OCRE come requisito | Sì, su MCU e Linux | Opzionale | Opzionale |
| "Tutto passa dal Gateway" | Sì | Sì | No |

---

## 4. Vincoli del protocollo CBOR su Linux

`wasmbed-protocol` espone oggi:

```
ClientMessage: Heartbeat | EnrollmentRequest | Telemetry | Ack
ServerMessage: Deploy | Stop | Configure | EnrollmentResponse
```

Su un MCU Zephyr questo è sufficiente e completo. Su un device Linux questo
è un sottoinsieme stretto delle sue capability:

| Capability | CBOR oggi | CBOR esteso | K8s Pod nativo |
|---|---|---|---|
| Deploy WASM | Sì | Sì | Sì (come container) |
| Stop workload | Sì | Sì | Sì |
| Heartbeat/telemetry | Sì | Sì | K8s probes |
| Accesso filesystem | No | Estendibile | Sì (volume mount) |
| Log streaming | No | Estendibile | Sì (stdout → kubectl logs) |
| Multi-process | No | No | Sì (multi-container Pod) |
| Resource limits | Manifesto OCRE | Manifesto OCRE | K8s requests/limits |
| Update hotpatch | No | Estendibile | K8s rolling update |

Se il device Linux è trattato come "MCU grosso" (Opzione A), CBOR basta per
il caso d'uso principale (deploy WASM sandboxato). Le capability avanzate
rimangono non esposte ma non sono necessarie per un edge node WASM-only.

Se invece si vogliono sfruttare appieno le capability Linux, Opzione C è più
appropriata ma rinuncia all'unità del protocollo.

---

## 5. Raccomandazione: Opzione A come baseline

### Architettura raccomandata

```
CLOUD       Kubernetes
            ┌─────────────────────────────────┐
            │  API Server + Controllers        │
            │  Application CRD                 │
            │    spec.targetRuntime:           │
            │      wamr_raw | ocre_zephyr |    │
            │      ocre_linux                  │
            └────────────┬────────────────────┘
                         │ K8s API
                         ↓
FOG         wasmbed-gateway (invariato)
            ┌─────────────────────────────────┐
            │  TLS 1.3 southbound (port 8081)  │
            │  HTTP northbound (port 8080)     │
            │  Device registry: MCU + Linux    │
            │  Routing: payload per runtime    │
            └────────────┬────────────────────┘
                    TLS+CBOR (identico)
                   /              \
EDGE        Zephyr+WAMR          Linux+WAMR+OCRE
            +OCRE (MCU)          (Raspberry, ARM, VM)
            [constrained]        [wasmbed-edge-client daemon]
```

### Perché questa scelta

**Il Gateway resta il punto di controllo unico.** Tutti i device, indipendentemente
dall'OS, passano dallo stesso hub TLS. Il control-plane (K8s + CRD) non sa
come è fatto il device — sa solo che esiste, è registrato, e ha un
`targetRuntime`. Il Gateway traduce l'intent in payload CBOR corretto.

**La differenza MCU/Linux è un dettaglio del device, non del control-plane.**
Il record device nel Gateway registry include già `mcu_type` e `capabilities`.
Aggiungere `runtime_type: LinuxArm64` è un'estensione naturale, non un cambio
architetturale.

**Il client CBOR Linux riusa il protocollo esistente.** `wasmbed-edge-client`
non è un porting da zero: è lo stesso protocollo CBOR implementato su
rustls/tokio invece di mbedTLS/Zephyr threads. La struttura `Envelope<ClientMessage>`
è identica — `wasmbed-protocol` è già `no_std`, quindi funziona su entrambi
i target.

**L'Opzione B è anti-pattern.** Il Gateway è il single point of failure del
sistema. Trasformarlo in un runtime stateful aumenta il blast radius di ogni
incidente e complica qualsiasi forma di HA.

**L'Opzione C è un'evoluzione futura valida** se si vuole integrare device Linux
già in un cluster K8s (caso d'uso: raspberry che fa anche da k3s worker node).
Non è necessaria per la baseline e introduce discontinuità nel modello mentale.

---

## 6. Roadmap implementativa

### Fase 1 — Zephyr+OCRE (invariante architetturale)

Descritta in `OCRE_INTEGRATION_ANALYSIS.md`. Prerequisito: abilitare WASI su
WAMR puro, poi aggiungere OCRE su STM32F746G. Non cambia nulla nel Gateway
o nel control-plane — il device resta un MCU che parla CBOR.

### Fase 2 — Estensione CRD per runtime multipli

**File:** `crates/wasmbed-k8s-resource/src/application.rs`

```rust
pub enum TargetRuntime {
    WamrRaw,        // default attuale
    OcreZephyr,     // MCU con OCRE
    OcreLinux,      // Linux device con OCRE
}

pub struct ApplicationSpec {
    // campi esistenti...
    pub target_runtime: Option<TargetRuntime>,
}
```

**File:** `crates/wasmbed-protocol/src/lib.rs` — aggiungere `runtime` a
`DeployApplication` (opzionale, retrocompatibile con device che ignorano il campo).

### Fase 3 — `wasmbed-edge-client` per Linux

Nuovo crate (o binario standalone) che implementa:
- TLS 1.3 client con rustls + certificato Ed25519 device.
- Loop CBOR: enrollment → heartbeat → ricezione Deploy → invocazione OCRE API.
- Systemd unit file per avvio automatico.
- Stesso `Envelope<ClientMessage>` del firmware Zephyr.

Struttura suggerita:

```
crates/
  wasmbed-edge-client/        # nuovo
    src/
      main.rs                 # binary: args, config, loop principale
      tls.rs                  # rustls client, replica zephyr tls_client.c
      cbor_loop.rs            # enrollment + heartbeat + deploy handler
      ocre_runner.rs          # invoca OCRE API Linux o WAMR raw
    Cargo.toml
```

### Fase 4 — Registrazione device Linux nel Gateway

**File:** `crates/wasmbed-qemu-manager/src/lib.rs`

Aggiungere `McuType::LinuxArm64` (o variante generica `LinuxDevice`) per
permettere la registrazione di device non-emulati. Il Renode Manager non
lancia container per questi device — la registrazione avviene direttamente
dall'edge client via `POST /api/v1/board/register`.

### Fase 5 (opzionale, futuro) — Opzione C

Se emerge il requisito di device Linux già in un cluster K8s, aggiungere al
Application Controller un secondo percorso che usa `kubectl apply` invece di
Gateway API. Richiede discriminazione esplicita nell'Application Controller
e una CRD aggiornata.

---

## 7. Domande aperte

| Domanda | Impatto | Stato |
|---|---|---|
| Il Raspberry entra nel registry via Renode Manager o direttamente via `board/register`? | Cambia Fase 4 | Da decidere |
| OCRE-on-Linux: GPIO via `/dev/gpiochip` è stabile upstream? | Scope di `ocre_runner.rs` | Da verificare su OCRE repo |
| `wasmbed-edge-client` è un crate nel workspace Rust o un progetto separato? | Struttura repo | Da decidere (workspace consigliato) |
| I lint workspace (`no_unwrap`, `checked_arithmetic`) si applicano anche al client Linux? | Build CI | Sì — workspace Cargo.toml si applica a tutti |
| Serve backward compatibility: device che non supportano `TargetRuntime` ignorano il campo? | Protocollo CBOR | Sì — campo opzionale, default `WamrRaw` |

---

## 8. Relazione con gli altri documenti

| Documento | Scope | Relazione |
|---|---|---|
| `OCRE_INTEGRATION_ANALYSIS.md` | Firmware Zephyr: *come* integrare OCRE su MCU | Prerequisito per Fase 1 |
| `ARCHITECTURE.md` | Architettura generale RETROSPECT | Questo doc estende la sezione Edge Layer |
| `MCU_SUPPORT.md` | Lista MCU emulati Renode | Fase 4 aggiunge `LinuxArm64` |
| `TLS_CONNECTION.md` | Flusso TLS device↔Gateway | `wasmbed-edge-client` replica quel flusso su POSIX |
| `SEQUENCE_DIAGRAMS.md` | Diagrammi enrollment e deploy | Da aggiornare dopo Fase 3 |
