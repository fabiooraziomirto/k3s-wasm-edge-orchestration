# Validazione end-to-end del tracciamento energetico

Data: 2026-07-28. Segue l'implementazione a fasi descritta in
[doc/energy-tracking-assessment.md](energy-tracking-assessment.md) (Fase 0:
bonifica valori sintetici, Fase 1: Kepler+Prometheus, Fase 2: fuel/gas
metering, Fase 3: integrazione benchmark).

**Ambiente di test**: container LXC (Proxmox) con Docker funzionante, root,
64GB RAM, RAPL esposto (`/sys/class/powercap/intel-rapl`), CPU AMD Zen 3
(32 thread, di cui 16 assegnati a questo container). Non un sandbox
usa-e-getta: macchina persistente con un servizio già in produzione
(`open-webui`).

---

## 1. Verifica statica

### 1.1 Ricerca valori sintetici non marcati

Grep esteso a tutto il workspace (non solo ai file toccati nelle fasi
precedenti) per pattern `45.0`/`60.0`/`30.0` e varianti:

**Trovato un gap reale, non coperto dalla Fase 0**: `crates/wasmbed-gateway/src/http_api.rs`,
handler `get_system_metrics` (route `/api/v1/metrics/system`, mai istrumentato
prima perché in un crate diverso da quelli toccati in Fase 0). Fabbricava
un'intera serie storica di 24h con `rand::random()` più un blocco "current"
con valori fissi (45.2/67.8/23.1), esposta su un endpoint HTTP reale, senza
alcun marcatore.

**Esito**: corretto con lo stesso pattern della Fase 0 — `is_synthetic: true`
su ogni entry della serie, sul blocco "current", e a livello di risposta
top-level, più un `warn!()` di log. Verificato con `cargo check -p
wasmbed-gateway` (nessun errore).

Dopo la correzione: **nessun valore hardcoded rimane esposto senza marcatore
`is_synthetic`** in `crates/`. (Non toccati, perché fuori scope — sono stub
di logica applicativa non legati a metriche/energia, non "hardcoded 45.0
esposto come reale": `get_alerts`, `get_gateways`/`get_gateway` nello stesso
file, e vari commenti `// Simulate ...` sparsi in TLS server, enrollment,
certificate authority, secret store — tutti stub "funzionalità non ancora
implementata", non falsi dati energetici/di sistema.)

### 1.2 Validazione manifest k8s

Nessun cluster disponibile per `kubectl apply --dry-run=client` (richiede
comunque un server per lo schema OpenAPI in questa versione di kubectl).
Usato **kubeconform** (validazione statica offline, pensata apposta per
questo caso):

```
Summary: 13 resources found in 7 files - Valid: 13, Invalid: 0, Errors: 0, Skipped: 0
```

(confrontato con i manifest preesistenti come baseline: 33 valid, 4 skipped
per le CRD custom senza schema — comportamento atteso.)

**Trovata un'incoerenza di stile reale**: i nuovi manifest usavano la
label `app.kubernetes.io/name` (convenzione "recommended labels" upstream),
mentre **tutto il resto della repo** usa la label semplice `app: <name>`.
Corretto in tutti i file di `k8s/monitoring/` (namespace, DaemonSet Kepler,
Deployment/Service Prometheus), incluso il relabel_config Prometheus che
filtra i pod Kepler per label (`__meta_kubernetes_pod_label_app`, non più
`_app_kubernetes_io_name`).

---

## 2. Smoke test dello stack

### 2.1 k3s — BLOCCATO, causa ambientale non risolvibile da qui

`curl -sfL https://get.k3s.io | sh` installa correttamente, ma il kubelet
non parte: `open /dev/kmsg: operation not permitted`. Non è "file mancante"
(risolvibile con `mknod`, tentato e confermato inutile) ma un **diniego a
livello di policy cgroup del container LXC**, imposta dall'host Proxmox:
il container non ha il permesso di aprire il character device `1:11`
(kmsg), e questo permesso si configura nel file di configurazione LXC del
container sull'host Proxmox (`/etc/pve/lxc/<vmid>.conf`,
`lxc.cgroup2.devices.allow: c 1:11 rwm`), non dall'interno.

**Azione richiesta da te (fuori dalla mia portata in questa sessione)**: o
modificare la config LXC sull'host Proxmox per consentire l'accesso a
`/dev/kmsg`, oppure eseguire k3s su una VM Proxmox invece che su un
container LXC, oppure testare su un host diverso (bare metal o VM cloud
qualsiasi). k3s è stato fermato e disabilitato in modo pulito (non lasciato
in crash-loop).

### 2.2 Kepler — validato con dati reali, bypassando k3s

Eseguito Kepler `release-0.7.11` via `docker run` diretto (stesso identico
binario/immagine che il DaemonSet userebbe), con RAPL e cgroup montati:

- **Confermato**: Kepler legge davvero RAPL (`source="rapl-sysfs"` nei log
  e nelle metriche), CPU rilevata correttamente come "Zen 3", potenza
  platform via ACPI.
- Senza k8s, Kepler attribuisce i processi non containerizzati a bucket
  `system_processes`/`kernel_processes` (namespace sintetici `system`/
  `kernel`), e riconosce comunque i container Docker esistenti per
  `container_id` (ha visto `open-webui`) pur senza poter risolvere
  `container_namespace`/`pod_name` (richiede il kubelet). Questo conferma
  empiricamente quanto documentato in
  [k8s/monitoring/README.md](../k8s/monitoring/README.md): il container
  Renode (avviato con `docker run` grezzo, non tramite kubelet) **non**
  porterà label `container_namespace="wasmbed"` nemmeno in un cluster k3s
  reale — non è un'ipotesi, è il comportamento osservato di Kepler stesso.
- **Non validabile qui**: la distinzione "namespace wasmbed vs container
  Renode" *tramite k8s* richiede un cluster k8s funzionante con pod reali
  nel namespace `wasmbed` — bloccato dal problema 2.1.

**Bug reale trovato e corretto nelle recording rule** (`k8s/monitoring/prometheus-rules.yaml`):
le regole raggruppavano `by (node)`, ma Kepler non espone alcuna label
`node` — solo `instance`/`mode`/`source`/`container_namespace`/`pod_name`.
`by (node)` non dava errore: collassava silenziosamente tutte le serie in
una con label `node` assente. Corretto in `by (instance)`, verificato
contro un vero Prometheus (v2.55.1, via Docker) puntato al Kepler reale:
tutte e 4 le recording rule restituiscono valori numerici plausibili (vedi
§4). Commit `73c04e2`, pushato.

### 2.3 Wasmtime fuel metering — validato (già in Fase 2)

Non ripetuto qui perché già provato con un'invocazione WASM reale:
`cargo test -p wasmbed-wasm-runtime --lib` — 15/15 pass, inclusi
`test_fuel_metering_consumes_fuel_on_mcu` (chiama una funzione WASM vera,
verifica `fuel_consumed > 0`) e `test_fuel_exhaustion_is_reported` (budget
volutamente insufficiente, verifica che l'errore `FuelExhausted` venga
riportato correttamente).

### 2.4 WAMR instruction metering — NON validato

Nessun Zephyr SDK, `west`, sorgenti WAMR o toolchain ARM disponibili su
questa macchina (verificato: nessuna di queste directory/binari esiste).
Costruire l'intero toolchain da zero (SDK multi-GB, workspace west, build
firmware, immagine Renode) non è stato tentato in questa sessione di
validazione — è un lavoro a sé. La stringa usata per riconoscere
l'eccezione di instruction-limit (`"instruction limit"` in
`wamr_integration.c`) resta **non verificata** contro il sorgente WAMR
reale.

---

## 3. Esperimento di prova end-to-end

**Non eseguito.** `run_experiment_campaign.sh` richiede l'intero stack
(gateway, Renode, device emulato) che non è disponibile qui per i motivi
di §2.1 e §2.4. Non ha senso lanciarlo contro uno stack parziale: fallirebbe
su enrollment/heartbeat/deployment (nessun device reale), producendo solo
rumore, non un test valido della pipeline energia.

Verificato invece, per quanto possibile senza cluster:
- `python3 -m py_compile scripts/collect_experiment_metrics.py` — pulito.
- Smoke test locale delle funzioni `query_prometheus_instant`/
  `query_energy_window`: degrado corretto (`None`/`unavailable: true`)
  senza un Prometheus raggiungibile.
- Contro il Prometheus reale avviato per la validazione (§2.2), le stesse
  recording rule interrogate dallo script (`wasmbed:kepler_*`)
  restituiscono dati veri — vedi §4. Lo script stesso non è stato lanciato
  contro questo Prometheus perché richiede anche l'API server/gateway
  del progetto (per i trial di enrollment/deployment), non disponibili.

---

## 4. Sanity check sui dati

Test idle-vs-carico eseguito **due volte**, con esito molto diverso — il
primo tentativo è di per sé una lezione metodologica rilevante.

### Tentativo 1 (carico debole, esito inconcludente)

Carico: 16 processi bash `while true; do :; done` per 25s.

| Metrica | Idle | Sotto "carico" |
|---|---|---|
| `wasmbed:kepler_all_pods_watts:sum` | 469.20 W | 470.35 W |
| `wasmbed:kepler_host_watts:sum` | 469.03 W | 469.36 W |

Variazione trascurabile (~0.2%). **Se mi fossi fermato qui, avrei dovuto
segnalarlo come sospetto** secondo il criterio richiesto ("non deve essere
costante"). Causa identificata: un ciclo `while true; do :; done` in bash
non è un carico CPU reale — è quasi tutto branch prediction/fetch, costo
energetico marginale trascurabile su una CPU moderna — combinato con un
host multi-tenant condiviso (altri VM/container Proxmox) la cui potenza di
base (~469W, verosimile per uno chassis con CPU 32-thread) rende invisibile
un segnale così piccolo. **Non è un sintomo di dato sintetico**: è un
carico di prova inadeguato.

### Tentativo 2 (carico reale, esito netto)

Carico: 16 processi `openssl speed sha256` per 20s (lavoro ALU/hashing
reale), misurato sulla componente `kepler_node_platform_joules_total{mode="dynamic"}`
(contatore cumulativo, delta/tempo = watt medi):

| Finestra | Durata | ΔJoule | Watt medi |
|---|---|---|---|
| Idle | 20.01s | 159 J | **7.94 W** |
| Carico (16× SHA256) | 20.03s | 1425 J | **71.13 W** |

**~9× di aumento**, ordine di grandezza fisicamente plausibile per carico
SHA256 saturato su 16 thread. Numeri non tondi, non costanti, non
identici — nessun segnale di fallback sintetico. Questo è il risultato che
conta: **conferma che Kepler in questo ambiente produce dati energetici
reali e sensibili al carico**, non un placeholder.

### Limite di questo sanity check

Il carico usato (hashing puro CPU-bound) non è rappresentativo di un vero
workload WASM/gateway del progetto — non potendo eseguire quello reale
(§2.1, §2.4), non posso confermare la sensibilità di Kepler specificamente
a un'invocazione WASM/Wasmtime/WAMR, solo a carico CPU generico. È una
conferma della pipeline di raccolta, non del workload target.

---

## 5. Riepilogo finale

### Validato con successo (evidenza reale, non solo teorica)

| Componente | Come validato |
|---|---|
| Fase 0 bonifica sintetici | `cargo check` pulito su tutti i crate toccati; trovato e corretto un gap aggiuntivo (`wasmbed-gateway`) durante questa validazione |
| Manifest k8s (struttura) | `kubeconform -strict`: 13/13 validi |
| Manifest k8s (stile) | Corretto per coerenza con la convenzione label `app:` della repo |
| Kepler legge RAPL reale | Log/metriche confermano `source="rapl-sysfs"`, CPU rilevata correttamente |
| Recording rule Prometheus | Sintassi + semantica verificate contro Kepler reale; bug `by (node)` trovato e corretto |
| Dati energia variano col carico | 7.9W idle → 71W sotto carico reale (9×), non costanti/sintetici |
| Wasmtime fuel metering | `cargo test`: invocazione WASM reale, fuel consumato tracciato, esaurimento budget riportato correttamente |
| `collect_experiment_metrics.py` | Sintassi pulita, degrado corretto senza Prometheus, campi additivi verificati |

### Fallito / bloccato

| Cosa | Perché | Serve intervento tuo |
|---|---|---|
| Deploy k3s reale | `/dev/kmsg` negato dalla policy cgroup del container LXC Proxmox | Sì — fix a livello di config LXC sull'host Proxmox, o testare su altra macchina/VM |
| Deploy DaemonSet Kepler + Prometheus **via k8s** (non via Docker diretto) | Dipende dal blocco k3s sopra | Stesso |
| WAMR instruction metering | Nessun Zephyr SDK/toolchain ARM/sorgenti WAMR presenti | Sì — bring-up firmware separato (multi-GB, tempo lungo) |
| Percorso gateway→Renode→WASM reale | Nessun certificato TLS generato, nessuna immagine Renode, dipende dai due blocchi sopra | Sì |
| `run_experiment_campaign.sh` | Richiede lo stack completo sopra | Sì |
| Distinzione Renode-vs-wasmbed **via label k8s** in un cluster reale | Richiede k3s funzionante | Sì (il meccanismo è comunque confermato via Kepler standalone, §2.2) |

### Prima di lanciare una campagna reale per il paper

1. **Sblocca k3s**: fixa la policy cgroup LXC sull'host Proxmox (o usa
   un'altra macchina/VM). Senza questo, nessun altro punto è testabile
   end-to-end.
2. Applica `k8s/monitoring/` su un cluster funzionante e ripeti il
   sanity check idle/carico **con Kepler in modalità DaemonSet reale**,
   per confermare che l'attribuzione per-namespace/pod funzioni come
   atteso (qui validato solo in standalone Docker).
3. Costruisci il toolchain Zephyr/WAMR/Renode (vedi
   `doc/K3S_DEPLOYMENT.md`, `doc/FIRMWARE.md`) e verifica la stringa di
   match dell'eccezione instruction-limit contro i log reali.
4. Solo allora, lancia `run_experiment_campaign.sh` con `TRIALS` bassi
   (es. 3-5) come smoke test prima di una campagna vera.

### I dati raccolti in questa sessione

Tutti scartati/non salvati come dataset — erano su un host generico
(container/VM di sviluppo condiviso, carico sintetico SHA256, nessun
namespace `wasmbed` reale), usati solo per validare che la pipeline
produca numeri reali e reattivi. Non hanno alcun valore per il paper.
