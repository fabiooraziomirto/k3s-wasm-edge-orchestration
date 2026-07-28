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

---

## 6. Aggiornamento stesso giorno: validazione su k3s reale (macchina diversa)

Sessione successiva, stesso giorno (2026-07-28), macchina diversa: **VM
KVM** (non container LXC — `systemd-detect-virt` → `kvm`), Ubuntu 24.04,
16 thread, CPU **`QEMU Virtual CPU version 2.5+`**. Non uno scratch box:
k3s v1.35.5 già attivo da 60 giorni con altri workload di produzione
(Stack4Things/IoTronic, Keycloak, Keystone, Istio, Crossplane, MetalLB,
cert-manager, InfluxDB). Tutte le operazioni sono state fatte con cautela
per non impattare quei workload (niente `curl|sh` di k3s senza prima
verificare che fosse già installato; prune Docker solo su immagini/
container non riferiti da nulla in uso).

### 6.1 Il blocco del §2.1 è risolto qui — ma emerge un limite diverso

k3s funziona **normalmente** su questa macchina: nessun problema
`/dev/kmsg`, kubelet parte, nodo `Ready`. Il blocco ambientale del §2.1
era specifico del container LXC Proxmox, non un problema del progetto.

**Limite nuovo, non anticipato**: questa VM **non espone RAPL**
(`/sys/class/powercap/` vuoto) — CPU virtuale QEMU senza passthrough MSR.
Kepler lo rileva da solo e logga esplicitamente:

```
acpi.go:71  Could not find any ACPI power meter path. Is it a VM?
power.go:73  using none to obtain power
process_energy.go:114  Using the Ratio/DynPower Power Model to estimate ...
node_platform_energy.go:52  Using the Regressor/AbsPower Power Model to estimate Node Platform Power
```

Quindi: **le due macchine usate finora sono complementari, non
sovrapposte**. La macchina del §1-5 aveva RAPL reale ma non poteva
eseguire k3s. Questa macchina esegue k3s reale ma produce solo stime
model-based, mai misure hardware dirette. Nessuna delle due, da sola,
copre l'intero percorso "k3s reale + RAPL reale" — resta un gap per una
terza macchina (bare metal o VM con RAPL passthrough) se serve quel
risultato combinato per il paper.

### 6.2 Kepler DaemonSet reale: attribuzione per-namespace confermata

Deploy di `k8s/monitoring/` completo (namespace, RBAC, DaemonSet Kepler,
Prometheus) su k3s reale, poi deploy di tutto `wasmbed` (6 immagini
Docker buildate e pushate su registry locale, 6 pod applicativi +
`gateway-1-deployment` creato dal gateway-controller, tutti `1/1
Running`).

**Confermato con dati reali ciò che il §2.2 poteva solo predire in
standalone Docker**: ogni pod del namespace `wasmbed` (incluso
`gateway-1-deployment`, creato dal controller e quindi **con** Pod
object kubelet) porta `container_namespace="wasmbed"` nelle metriche
Kepler:

```
kepler_container_core_joules_total{container_name="gateway-controller",container_namespace="wasmbed",...}
kepler_container_core_joules_total{container_name="gateway",container_namespace="wasmbed",pod_name="gateway-1-deployment-...",...}
kepler_container_core_joules_total{container_name="api-server",container_namespace="wasmbed",...}
```

E, separatamente, un container avviato con `docker run` grezzo (stesso
meccanismo di `wasmbed-renode`, replicato con un container Alpine di
prova per non dipendere dal toolchain Zephyr) **conferma** l'ipotesi
del `k8s/monitoring/README.md`: nessuna label `wasmbed`, bucket
sintetico Kepler:

```
kepler_container_joules_total{container_name="system_processes",container_namespace="system",pod_name="system_processes",...}
```

### 6.3 Due bug reali trovati nelle recording rule, mai visibili prima

Testando le regole di `prometheus-rules.yaml` contro Kepler reale sotto
carico (un container Alpine `while :; do md5sum /dev/urandom; done` come
analogo di Renode), è emerso che **`wasmbed:kepler_unattributed_watts:approx`
poteva restituire valori negativi** — fisicamente impossibili e mai
osservati nella sessione precedente (dove l'errore si mascherava, vedi
sotto).

1. **Mismatch di `mode`**: le regole sommavano
   `kepler_container_joules_total` su entrambi `mode="dynamic"` e
   `mode="idle"` (idle ~33× più grande), mentre
   `kepler_node_platform_joules_total` espone **solo** `mode="dynamic"`
   quando Kepler non ha una fonte di potenza hardware (VM senza RAPL).
   Misurato: `unattributed` = -229 W.
2. **Contaminazione da pseudo-namespace**: `all_pods` non filtrava
   `container_namespace=~"system|kernel"` — i bucket sintetici Kepler
   dove finisce esattamente il container Renode (raw `docker run`, non
   gestito da kubelet). Sotto carico reale: 9.75 W di 10.12 W sommati da
   `all_pods` erano in realtà nel bucket `system` (96%), non pod reali.
   Questo fa sì che `unattributed` (il numero che il README dice di
   citare come stima del costo Renode) **cancelli quasi interamente
   Renode con sé stesso**: misurato 0.33 W dove il valore corretto era
   10.08 W — un sotto-stima di 30×.

**Perché non era emerso prima**: sulla macchina del §1-5 (RAPL reale),
sia il lato host che il lato container esponevano probabilmente
entrambi i `mode`, mascherando il mismatch (469.20 W vs 469.03 W, una
differenza plausibile ma già di per sé sospetta — vedi "Tentativo 1" al
§4, dove la variazione trascurabile era stata correttamente segnalata
come sospetta ma attribuita al carico debole, non al bug).

**Fix applicato e verificato**: ogni regola ora fissa `mode="dynamic"`
esplicitamente e `all_pods` esclude `container_namespace=~"system|kernel"`;
aggiunto anche un `clamp_min(...,0)` di sicurezza con commento che
spiega quando un valore molto negativo prima del clamp indica una
regressione, non un caso normale. Verificato dal vivo: da -212.79 W a
+45.03 W con lo stesso carico non-pod attivo (99% del dynamic host
watts correttamente attribuito a `unattributed`). Vedi
`k8s/monitoring/prometheus-rules.yaml` per i dettagli e i numeri esatti.

### 6.4 Stringa d'eccezione WAMR: confermata SENZA build del toolchain

Il punto aperto del §2.4 (verificare `"instruction limit"` contro il
sorgente WAMR reale) è stato chiuso leggendo il sorgente upstream
(`bytecodealliance/wasm-micro-runtime`, sparse-checkout, senza bisogno
di SDK Zephyr/ARM):

- `WAMR_BUILD_INSTRUCTION_METERING=1` → `config_common.cmake` →
  `-DWASM_ENABLE_INSTRUCTION_METERING=1`
- Guardia risultante in `wasm_interp_classic.c`/`wasm_interp_fast.c`:
  `wasm_set_exception(module, "instruction limit exceeded")`
- `wasm_runtime_get_exception` antepone `"Exception: "` → stringa finale
  `"Exception: instruction limit exceeded"`

La costante `WAMR_INSTRUCTION_LIMIT_EXCEPTION_SUBSTR = "instruction limit"`
in `zephyr-app/src/wamr_integration.c` **è corretta**, verificata contro
il sorgente reale — non serve modificarla. Resta comunque non verificato
il comportamento *a runtime* su firmware reale (nessun Zephyr SDK/west/
WAMR clone disponibile neanche su questa macchina), ma il rischio
principale (stringa sbagliata) è escluso con certezza.

### 6.5 Bug reali trovati e corretti testando con dati veri (oltre al §6.3)

Come nella sessione precedente, testare con infrastruttura reale ha
scoperto bug che nessuna verifica statica avrebbe trovato:

1. **`Dockerfile.api-server`, heredoc BuildKit-only**: `cat <<'EOF' >
   file` in una riga `RUN` è sintassi heredoc, supportata solo da
   BuildKit. Questa macchina ha Docker 29.3.1 **senza** il plugin
   `buildx` → builder legacy attivo di default → l'heredoc viene
   eseguito con stdin vuoto, scrivendo un `Cargo.toml` di **zero byte**
   senza alcun errore a quello step; il build falliva più avanti con un
   messaggio fuorviante (`missing either a [package] or a [workspace]`).
   Corretto sostituendo con `printf` (portabile su entrambi i builder),
   verificato su entrambi (test isolato + build reale).
2. **`Dockerfile.api-server`, COPY firmware con wildcard**: `COPY
   zephyr-workspace/.../zephyr.elf*` fallisce con "no source files were
   specified" se `zephyr-workspace/` non esiste (ogni clone pulito: la
   directory è in `.gitignore`, nessun firmware Zephyr è mai stato
   buildato qui). Il commento originale nel Dockerfile ("if available")
   e la sezione troubleshooting di `doc/K3S_DEPLOYMENT.md` (che tratta
   "firmware not found" come verifica **a runtime**, `docker run ... ls
   /app/zephyr-workspace/build/`) confermano che l'intento era un build
   sempre riuscito, indipendente dal firmware. Corretto: `COPY` della
   directory (non wildcard) in `Dockerfile.api-server`, più `mkdir -p`
   delle directory vuote in `scripts/deploy-k3s.sh` prima del build loop
   (un `COPY` di directory richiede comunque che la directory esista nel
   build context, anche vuota).
3. **Ambiguità nomi risorsa `kubectl` bare (bug più serio)**: 21 punti
   in `crates/wasmbed-api-server` + `crates/wasmbed-qemu-manager` (più 7
   in `scripts/collect_experiment_metrics.py`) invocavano `kubectl get/
   patch/delete device|devices|application|applications|gateway` senza
   qualificare il gruppo API. Su un cluster che ospita **anche** un'altra
   risorsa chiamata `devices` (`devices.iot.s4t.crossplane.io`, dal
   provider Stack4Things/Crossplane già presente su questa macchina) e
   3 CRD diverse chiamate `gateways`, questo è genuinamente ambiguo.
   Verificato: `kubectl get devices` risolveva **deterministicamente**
   all'altra CRD (cluster-scoped, quindi ignorava anche `-n wasmbed`) —
   403 Forbidden per il ServiceAccount di api-server (correttamente
   scoperto solo su `wasmbed.github.io`), ma un **risultato vuoto
   silenzioso e sbagliato** con un kubeconfig più permissivo (il mio,
   usato per debug). In `wasmbed-qemu-manager`, lo stesso problema era
   già silenziosamente inghiottito in un fallback
   (`if let Ok(output) = ...`), quindi il sintomo visibile era solo un
   log `WARN` — mai un errore hard. Corretto qualificando ogni
   occorrenza (`devices.wasmbed.github.io`, `applications.wasmbed.github.io`,
   `gateways.wasmbed.io`) in entrambi i crate Rust e nello script Python;
   verificato con `cargo check` pulito, `py_compile` pulito, e una
   chiamata reale `GET /api/v1/devices` che prima falliva con 500 e ora
   restituisce il device reale enrolled.
4. **`run_experiment_campaign.sh`, abort su `sudo` mancante**: la
   cattura diagnostica di rete (`sudo iptables -t nat -L PREROUTING`)
   girava sotto `set -euo pipefail` senza tolleranza di fallimento —
   su un host senza `sudo` passwordless (questa macchina), l'intero
   script abortiva a `capture_env()`, **prima di eseguire un solo
   trial**. Confermato con un caso di test ridotto (exit 1 riprodotto
   isolatamente). Corretto con `sudo -n ... || true`.
5. **`collect_experiment_metrics.py`, durata PromQL frazionaria (bug
   più impattante per il paper)**: `query_energy_window()` formattava
   la finestra di tempo con precisione al millisecondo
   (`f"avg_over_time({rule}[{window_s:.3f}s])"`, es. `[11.13s]`) — ma
   PromQL **non accetta decimali** nei letterali di durata
   (`parse error: unknown unit "." in duration "11.13s"`). Risultato:
   **ogni** trial arricchito con energia falliva silenziosamente a
   `energy.unavailable: true`, per ogni finestra non fortuitamente
   intera — cioè quasi sempre. Questo bug esisteva dalla Fase 3
   originale e non era mai stato scoperto perché lo script non era mai
   stato eseguito contro un Prometheus reale con trial reali (la
   sessione precedente lo aveva solo validato in isolamento, senza
   stack completo — vedi §3). Corretto arrotondando a un intero di
   secondi per la sola stringa PromQL (`window_s` originale, preciso,
   resta invariato nel JSON di output per l'analisi). Verificato:
   prima/dopo il fix, stesso trial, stessi tre stage
   (enrollment/heartbeat/deployment) passano da `unavailable: true` a
   valori reali (`namespace_watts: 0.66 W`, `all_pods_watts: 1.06 W`,
   `host_watts: 12.03 W`, `unattributed_watts_approx: 10.97 W` —
   coerente col §6.3: il non-pod domina come atteso).

Tutti i fix sono minimi/mirati (nessun redesign), commentati inline con
il "perché" e la data della scoperta, seguendo lo stesso pattern già in
uso nel repo. **Non ancora pushati** — in attesa di conferma esplicita
data la portata (in particolare il fix #3, 28 punti di invocazione
totali tra Rust e Python).

### 6.6 Enrollment e deployment: pipeline applicativa confermata end-to-end

Con le immagini corrette deployate: enrollment di un device reale
(`POST /api/v1/devices`) → CRD Device creato → gateway risolto →
tentativo di avvio Renode fallisce **solo** per firmware Zephyr assente
(`Zephyr firmware not found for STM32F746G Discovery`, atteso, stesso
gap del §2.4/§6.4 — non un bug nuovo). Deploy di una vera Application
CR (`test-wasm-app`, modulo WASM minimale) reconciliato correttamente
dal controller, stato `Failed` per `"Device not connected"` — di nuovo,
esattamente il comportamento atteso senza firmware reale, non un
fallimento della pipeline applicativa/K8s. **Non validabile qui**:
l'esecuzione WASM reale e quindi l'esercizio pratico del fuel/instruction
metering durante un'invocazione reale — richiede lo stesso toolchain
Zephyr del §2.4.

### 6.7 Smoke test dello script di raccolta metriche: eseguito, con successo, per la prima volta

`scripts/run_experiment_campaign.sh` **non è stato eseguito** (oltre al
fix del punto 4 sopra, richiede `.venv-paper/bin/python` e
`RETROSPECT_submission/paper_ieee_inginf05/generate_figures.py`, **nessuno
dei due presente in questo repository, né tracciato né in
`.gitignore`** — è una dipendenza esterna, verosimilmente un repo/venv
del paper mai commitato qui; non è stato fabbricato per non inventare
scope). Eseguito invece direttamente `collect_experiment_metrics.py
--trials 3` (`TRIALS` basso, come richiesto, dati di solo smoke test):

- Corre in modo pulito end-to-end contro il device reale enrolled,
  Prometheus reale, Kepler reale.
- `deployment`: 3/3 successo (crea/reconcilia l'Application CR
  realmente).
- `enrollment`/`heartbeat`: 0/3 successo, **atteso** — nessuna
  connessione firmware reale (§6.6).
- **Ogni** record porta un blocco `energy` con valori reali (vedi §6.5
  punto 5) e il nuovo campo `energy_power_provenance` (aggiunto in
  questa sessione, pattern identico a `is_synthetic`):
  ```json
  {
    "platform_power_source": "none",
    "components_power_source": "estimator",
    "is_estimated": true,
    "detail": "MODEL ESTIMATE -- no hardware power meter (platform_power_source='none'); watts are regression output, not measurements"
  }
  ```
  Questo è il campo che distingue, per ogni dataset raccolto, se i watt
  sono misure RAPL reali o stime del modello Kepler — assente prima di
  questa sessione, necessario perché un dataset raccolto su questa VM
  sarebbe altrimenti indistinguibile byte-per-byte da uno raccolto su
  hardware reale strumentato.

Dati di questo smoke test: scartati (3 trial, `test-device` sintetico
enrollato a mano, nessun firmware reale) — stesso trattamento del §4,
non hanno valore per il paper.

### 6.8 Riepilogo aggiornato

| Componente | Stato dopo questa sessione |
|---|---|
| Deploy k3s reale | **Validato** — nodo Ready, 60gg uptime, altri workload intatti |
| Deploy Kepler DaemonSet + Prometheus via k8s | **Validato** — non più solo standalone Docker |
| Attribuzione per-namespace `wasmbed` via label k8s reali | **Validato** — confermato su tutti e 6 i pod applicativi |
| Distinzione Renode-vs-wasmbed via k8s reale | **Validato** — replicato con container Alpine equivalente (nessun Renode reale, ma stesso meccanismo Docker-vs-kubelet) |
| Recording rule Prometheus | **2 bug aggiuntivi trovati e corretti** (mode mismatch, contaminazione pseudo-namespace) — vedi §6.3 |
| Provenienza hardware-vs-stima nei dataset | **Aggiunto** (`energy_power_provenance`), assente prima |
| WAMR instruction-limit string | **Confermata** contro sorgente upstream reale, nessuna build necessaria |
| Enrollment + deploy applicativo reale | **Validato** end-to-end (limitato solo dall'assenza di firmware Zephyr) |
| `collect_experiment_metrics.py` con energia | **Eseguito con successo per la prima volta** contro stack reale — 1 bug bloccante trovato e corretto (durata PromQL frazionaria) |
| WAMR instruction metering (esecuzione reale) | Ancora non validato — richiede toolchain Zephyr/ARM (nessuna macchina finora lo ha) |
| RAPL + k3s sulla stessa macchina | Ancora non raggiunto — questa VM non ha RAPL, l'altra non aveva k3s |
| `run_experiment_campaign.sh` (script completo) | Bloccato da dipendenza esterna mancante (`.venv-paper`/`RETROSPECT_submission`), non dal codice di questo repo |

### Prima di lanciare una campagna reale per il paper (aggiornato)

1. ~~Sblocca k3s~~ — fatto, su questa macchina.
2. ~~Applica `k8s/monitoring/` su un cluster funzionante~~ — fatto,
   attribuzione per-namespace confermata.
3. Trova (o predisponi) una macchina con **sia** k3s **sia** RAPL reale
   per numeri hardware-misurati end-to-end — nessuna delle due macchine
   usate finora li ha entrambi. In assenza, ogni numero energetico va
   dichiarato esplicitamente come stima modellistica (`is_estimated:
   true`), mai come misura.
4. Costruisci il toolchain Zephyr/WAMR/Renode (`doc/K3S_DEPLOYMENT.md`,
   `doc/FIRMWARE.md`) — unico modo per validare instruction metering ed
   eseguire moduli WASM reali end-to-end.
5. Fornisci/recupera `.venv-paper` e
   `RETROSPECT_submission/paper_ieee_inginf05/` (dipendenza esterna, non
   in questo repo) prima di poter lanciare `run_experiment_campaign.sh`
   per intero — oppure usa direttamente `collect_experiment_metrics.py`
   (ora verificato funzionante end-to-end) come base per una raccolta
   dati equivalente senza la generazione automatica delle figure.
6. Solo allora, TRIALS bassi come smoke test prima di una campagna vera.

---

## 7. Aggiornamento stesso giorno: tentativo firmware ARM reale + proxy computazionale Wasmtime

Sessione di continuazione, stessa macchina del §6. Obiettivo cambiato:
il paper è già scritto e validato da un secondo autore — il compito
diventa produrre dati energetici reali per il claim di sostenibilità,
non validare ulteriormente la pipeline in astratto. Contesto aggiuntivo:
il lavoro è destinato a una Special Issue ScienceDirect su
"Sustainable Digital Research Infrastructures for the Edge-to-Cloud
Continuum", scadenza 30 agosto 2026 — che richiede esplicitamente
"reproducible, evidence-based approaches", coerente con l'approccio già
seguito in questo documento.

### 7.1 Tentativo di build firmware Zephyr/WAMR reale — 3 bug reali trovati, poi sospeso

Costruito da zero il toolchain mai assemblato in nessuna sessione
precedente: Zephyr SDK 0.16.8 (solo toolchain ARM, installer minimale),
west workspace (Zephyr v3.7.0, shallow update mirato ai soli moduli
necessari per risparmiare spazio disco), WAMR (shallow clone). Tool di
build mancanti (`ninja`, `gperf`, `dtc`, `ccache`) recuperati senza
privilegi root via `apt-get download` + `dpkg -x` in una prefix utente
(nessun sudo passwordless disponibile).

**3 bug reali trovati e corretti durante il primo vero tentativo di
build contro questo `prj.conf`/`CMakeLists.txt`** (mai eseguito prima
in nessuna sessione, per assenza di toolchain):

1. **`zephyr-app/prj.conf`: 4 simboli Kconfig mbedtls inesistenti**
   (`MBEDTLS_HASH_SHA384_ENABLED`, `MBEDTLS_HASH_SHA512_ENABLED`,
   `MBEDTLS_MAC_SHA384_ENABLED`, `MBEDTLS_MAC_SHA512_ENABLED`) —
   verificato contro il Kconfig reale di Zephyr v3.7.0
   (`zephyr/modules/mbedtls/Kconfig.tls-generic`): i simboli corretti
   sono `MBEDTLS_SHA384`/`MBEDTLS_SHA512` (senza suffisso `_ENABLED`,
   nessuna distinzione HASH/MAC). Assegnare un valore a un simbolo
   Kconfig inesistente è un errore fatale di build in Zephyr
   ("Aborting due to Kconfig warnings"), non un no-op silenzioso.
   Corretto.
2. **`zephyr-app/prj.conf`: mancavano `CONFIG_REQUIRES_FULL_LIBC=y` e
   `CONFIG_FILE_SYSTEM(_LITTLEFS)=y`**, entrambi richiesti da
   `WAMR_BUILD_LIBC_WASI=1` (già impostato in
   `zephyr-app/CMakeLists.txt`) su Zephyr. Senza il primo, il layer WASI
   di WAMR (`posix.c`) non compila (`struct timespec` è un tipo
   incompleto con la libc minimale di Zephyr). Senza il secondo,
   `zephyr_file.c` di WAMR fallisce (`lfs.h: No such file or
   directory`) perché include incondizionatamente
   `<zephyr/fs/littlefs.h>`. Confermato confrontando con il sample
   ufficiale WAMR `product-mini/platforms/zephyr/simple-file/prj.conf`,
   che usa esattamente questi due flag per la stessa combinazione
   WASI+`POSIX_API=n`. Corretto.
3. **`wamr/core/shared/platform/zephyr/zephyr_socket.c` (dipendenza
   esterna, non in questo repo): `IP_MULTICAST_LOOP` non definito** da
   Zephyr v3.7.0 (`net/socket.h` definisce solo `IP_MULTICAST_TTL`).
   Patch locale minimale (stesso pattern già usato dal codice per
   `IPPROTO_IPV6` mancante: `#ifdef`/fallback `EAFNOSUPPORT`) — **non
   committata al repo wasmbed**, perché `wamr/` è un clone esterno
   gitignored; è un workaround locale per questa sessione, non un fix
   del progetto.

**Sospeso dopo il bug 3** — non per esaurimento del tempo, ma perché
l'ispezione di `wamr/.github/workflows/compilation_on_zephyr.yml`
(CI ufficiale WAMR) ha rivelato che questa combinazione — ARM
Cortex-M/STM32 reale, sample `simple-file` con WASI abilitato — **non è
testata dalla CI upstream di WAMR stessa** (che copre solo QEMU ARC e
QEMU x86-32, e solo i sample `simple`/`user-mode`). È quindi territorio
genuinamente non validato a monte, con tempo di risoluzione incerto.
Consultato l'utente: deciso di **non continuare** su questo percorso e
pivotare sul proxy computazionale Wasmtime, già funzionante. Il
workspace (`zephyr-workspace/`, `~/zephyr-sdk-0.16.8`, `wamr/`, ~3GB
totali) è stato lasciato intatto sulla macchina per un'eventuale ripresa
futura.

### 7.2 Bug reale trovato in `wasmbed-wasm-runtime`: leak del contatore istanze

Per collegare il runtime Wasmtime (fuel metering, Fase 2 — mai
collegato a un binario in esecuzione, vedi assessment doc) a un carico
reale, creato `crates/wasmbed-wasm-runtime/src/bin/fuel_load_probe.rs`:
esegue ciclicamente fasi idle/carico, nella fase di carico crea
un'istanza WASM, esegue una funzione con lavoro aritmetico reale (somma
0..200000 in un loop, non un no-op), legge il fuel consumato, rimuove
l'istanza, ripete.

**Bug reale trovato al primo run**: `WasmRuntime::create_instance`
incrementa `context.active_instances` (`runtime.rs`) ma
`remove_instance` non lo decrementava mai — solo `shutdown()` lo
azzerava. Risultato osservato: le prime ~5 istanze create/rimosse in un
loop stretto avevano successo, poi **ogni** `create_instance`
successiva falliva permanentemente per limite istanze raggiunto (36
milioni di errori in 3 secondi, verificato). Mai scoperto prima perché
(a) i test esistenti creano al massimo 1-2 istanze per test, mai un
ciclo ripetuto, e (b) il crate non era mai stato eseguito sotto carico
sostenuto essendo scollegato da qualunque binario in esecuzione. Fix:
un decremento (`saturating_sub`) in `remove_instance`. Verificato:
7503 chiamate riuscite in 3s dopo il fix (contro 5 prima), e la suite
`cargo test -p wasmbed-wasm-runtime --lib` resta 15/15 verde.

### 7.3 Correlazione fuel-Wasmtime vs watt-Kepler: proxy computazionale solido, watt non significativi su questa macchina

Containerizzato il probe (`Dockerfile.energy-probe`), deployato come
`Deployment` nel namespace `wasmbed`
(`k8s/test-resources/energy-probe-deployment.yaml`, cicli da 30s idle +
30s carico) così Kepler lo attribuisce correttamente
(`container_namespace="wasmbed"`, confermato). Raccolti 6 cicli
completi, ognuno con ~80-85 mila invocazioni WASM reali fuel-metered,
correlati contro le stesse recording rule di Prometheus del §6.3
(corrette).

**Il proxy computazionale è solido e riproducibile**:

| Metrica | Valore |
|---|---|
| Chiamate per ciclo | 83.176 ± 1.708 (CV 2.05%) |
| Fuel totale per ciclo | 2.163e11 ± 4.44e9 (CV 2.05%) |
| Fuel per chiamata | **2.600.010, costante al centesimo su tutti e 6 i cicli (0.00% varianza)** |

**La correlazione in watt, su questa macchina, non è statisticamente
significativa**: differenza media `namespace_watts` (carico − idle) su
6 cicli = **-0.213 W**, intervallo di confidenza 95% = **[-0.824,
+0.398] W** (include lo zero), solo 1 ciclo su 6 con la direzione
fisicamente attesa (carico > idle). Causa più probabile: rumore da
altri tenant di produzione sulla stessa VM condivisa (IoTronic,
Keycloak, Istio, Crossplane, ecc. — vedi §6, ambiente non dedicato),
aggravato dal fatto che Kepler qui stima (nessun RAPL, vedi §6.1) invece
di misurare. Stesso identico problema metodologico già incontrato nel
"Tentativo 1" del §4 (segnale reale ma piccolo, sommerso dal rumore di
un host condiviso) — non un artefatto nuovo, ma la stessa lezione
confermata due volte in due sessioni diverse.

**Implicazione diretta per il claim di sostenibilità**: il fuel/
instruction count è una metrica difendibile, deterministica, misurata
qui con dati reali. Il watt stimato da Kepler su questa macchina
condivisa e senza RAPL **non lo è** — non va citato nel paper come prova
del costo energetico di un'invocazione WASM specifica senza dichiarare
esplicitamente questo limite, o senza ripetere la misura su un host
dedicato (idealmente con RAPL reale, vedi §6.8) e/o con un campione
molto più grande per estrarre statisticamente un eventuale segnale
piccolo dal rumore.

Il pod `energy-probe` è stato lasciato attivo per accumulare
ulteriori cicli in background, se un campione più ampio serve in
futuro.
