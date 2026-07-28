# Report: Tracciamento Energetico/Sostenibilità nel sistema Wasmbed (k3s + WASM edge)

Repository: `k3s-wasm-edge-orchestration` (nome interno progetto: **Wasmbed**), commit `1cefdc5`.

## Stato implementazione (aggiornato dopo Fasi 0-2)

- **Fase 0 — bonifica metriche sintetiche**: fatta. `crates/wasmbed-infrastructure/src/monitoring.rs` e `crates/wasmbed-api-server/src/monitoring.rs` ora taggano `cpu_usage`/`memory_usage`/`disk_usage` con `is_synthetic: bool` (default `true` se il campo manca in un payload) e loggano un `warn!()` ogni volta che vengono emessi. Verificato con `cargo check -p wasmbed-infrastructure -p wasmbed-api-server`.
- **Fase 1 — Kepler + Prometheus**: fatta. Manifest in [k8s/monitoring/](../k8s/monitoring/) (namespace, RBAC, DaemonSet Kepler, Prometheus minimale senza Grafana, recording rules per separare energia namespace `wasmbed` da host totale). YAML validato con `pyyaml`; **non testato contro un cluster reale** (nessun kubectl/k3s in questo ambiente). Vedi [k8s/monitoring/README.md](../k8s/monitoring/README.md) per la limitazione host-condiviso-con-Renode.
- **Fase 2 — proxy computazionali**:
  - **Wasmtime fuel** in `crates/wasmbed-wasm-runtime`: implementato e **verificato con test automatici** (`cargo test -p wasmbed-wasm-runtime --lib`, 15/15 pass, inclusi 3 test dedicati al fuel). Nota architetturale importante: questo crate **non è collegato a nessun binario in esecuzione** (nessun altro crate del workspace lo importa) — il path di esecuzione WASM reale usato dagli esperimenti passa dal gateway verso il firmware Zephyr/WAMR, non da qui. La fuel API resta quindi pronta ma inerte finché il crate non viene wired in un servizio reale.
  - **WAMR instruction metering** in `zephyr-app/`: implementato (CMake flag `WAMR_BUILD_INSTRUCTION_METERING`, budget fisso per istanza, rilevamento esaurimento via match su stringa d'eccezione). **Non verificato con una build reale**: l'albero sorgente WAMR non è vendorizzato nel repo e in questo ambiente non sono disponibili Zephyr SDK/toolchain ARM. La stringa usata per riconoscere l'eccezione di instruction-limit (`"instruction limit"` in `wamr_integration.c`) è basata sulla convenzione dei nomi API WAMR, non su una lettura diretta del sorgente — **da confermare al primo build reale** controllando il testo effettivo loggato da `LOG_ERR`.
- **Fase 3 — integrazione benchmark**: fatta. `scripts/collect_experiment_metrics.py` interroga Prometheus (recording rules di Fase 1) per ogni round di trial (finestra wall-clock enrollment+heartbeat+deployment insieme, non per singolo stage — con scrape interval 5s l'attribuzione per singola chiamata sub-secondo sarebbe precisione fittizia) e allega `energy` + `measurement_scope: "host_shared_with_renode"` ad ogni `TrialRecord`, additivi rispetto ai campi esistenti (verificato leggendo `postprocess_experiment.py`: accede ai record per chiave nota, i campi nuovi vengono ignorati dov'è già scritto, non rompono nulla). Flag `--no-energy` per campagne senza lo stack di monitoring. Verificato con `python3 -m py_compile` + smoke test locale (degrado a `None`/`unavailable: true` senza un Prometheus raggiungibile). **Non testato contro un Prometheus/Kepler reali** (nessun cluster disponibile in questo ambiente).

---

## 1. Esplorazione strutturale

### Struttura generale

```
crates/           workspace Rust (Cargo), ~15 crate
├── wasmbed-api-server           HTTP API + dashboard server (Axum)
├── wasmbed-gateway               Gateway TLS/CBOR verso i device (edge)
├── wasmbed-application-controller  K8s controller per CRD "Application"
├── wasmbed-device-controller      K8s controller per CRD "Device"
├── wasmbed-gateway-controller     K8s controller per CRD "Gateway"
├── wasmbed-infrastructure         servizio "infrastructure" (CA, secret store, monitoring)
├── wasmbed-wasm-runtime           runtime WASM (Wasmtime) eseguito lato device/firmware
├── wasmbed-qemu-manager           gestione emulazione QEMU
├── wasmbed-tcp-bridge, wasmbed-protocol(-tool), wasmbed-cert(-tool), wasmbed-k8s-resource(-tool), wasmbed-tls-utils, wasmbed-types, wasmbed-test-utils
k8s/               manifest K8s puri (CRD, deployment, RBAC, HPA, ingress, device/gateway CR)
dashboard-react/   dashboard React (consuma le API sopra)
zephyr-app/        firmware Zephyr RTOS per MCU reali/emulate (WAMR/wasm integration)
renode-scripts/    script Renode per emulazione hardware (STM32, nRF52840, Arduino Nano BLE)
scripts/           deploy, cleanup, cert generation, esperimenti/benchmark
doc/               documentazione architetturale
experiments/       artefatti di una campagna sperimentale già eseguita (20260609-070246)
```

### Deployment k3s/Kubernetes

- Manifest YAML puri (nessun Helm chart, nessun kustomize) in [k8s/](k8s/):
  - [k8s/crds/](k8s/crds/) — CRD custom: `Application`, `Device`, `Gateway`, `stm32f7-device`.
  - [k8s/deployments/wasmbed-deployments.yaml](k8s/deployments/wasmbed-deployments.yaml), [k8s/deployments/api-server-deployment.yaml](k8s/deployments/api-server-deployment.yaml), [k8s/deployments/dashboard-deployment.yaml](k8s/deployments/dashboard-deployment.yaml).
  - [k8s/gateway-hpa.yaml](k8s/gateway-hpa.yaml) — HorizontalPodAutoscaler basato su **CPU/memory Utilization** (metriche standard `metrics.k8s.io`, richiede `metrics-server`; nessuna metrica custom/energetica).
  - [k8s/rbac/](k8s/rbac/), [k8s/ingress/](k8s/ingress/), [k8s/devices/](k8s/devices/), [k8s/gateways/](k8s/gateways/), [k8s/test-resources/](k8s/test-resources/).
- Deploy automatizzato via [scripts/deploy-k3s.sh](scripts/deploy-k3s.sh) (build immagini, push su registry locale, apply manifest) e [scripts/cleanup-k3s.sh](scripts/cleanup-k3s.sh). Guida: [doc/K3S_DEPLOYMENT.md](doc/K3S_DEPLOYMENT.md) — target esplicito è **single-node K3s** (Ubuntu 24.04, 4GB+ RAM).

### Esecuzione WASM

- Runtime: **Wasmtime 18.0** (`wasmtime`, `wasmtime-wasi`, `wasmtime-wasi-nn`), dichiarato in [crates/wasmbed-wasm-runtime/Cargo.toml](crates/wasmbed-wasm-runtime/Cargo.toml). Non Wasmer, non WasmEdge, non Spin.
- Il runtime WASM **non gira come pod k3s a sé stante**: è integrato nel firmware Zephyr ([zephyr-app/src/wamr_integration.c](zephyr-app/src/wamr_integration.c) usa **WAMR**, non Wasmtime — quindi in realtà coesistono due motori: Wasmtime lato "device MPU/simulazione" nel crate Rust, WAMR lato firmware Zephyr reale/embedded) e viene eseguito **dentro dispositivi emulati con Renode** ([renode-scripts/](renode-scripts/), [doc/ARCHITECTURE.md](doc/ARCHITECTURE.md)), non su nodi k3s fisici.
- **Punto architetturale chiave per il paper**: i "device" edge (STM32F7, nRF52840, Arduino Nano BLE, RISC-V) sono istanze software dentro un **singolo container Renode** (`wasmbed-renode`), gestite come "machine" Renode multiple in un solo processo (vedi [doc/ARCHITECTURE.md](doc/ARCHITECTURE.md) righe ~57-72). Non risultano board fisiche collegate nella configurazione attuale.
- Limiti di risorse per il WASM sono **software-enforced applicativi**, non basati su fuel metering: [crates/wasmbed-wasm-runtime/src/context.rs](crates/wasmbed-wasm-runtime/src/context.rs) implementa `check_memory_limit` e `check_cpu_time_limit` come contatori manuali (durata stimata/accumulata), non tramite l'API fuel di Wasmtime (`Store::set_fuel`/`consume_fuel`) né tramite `epoch_deadline`. **Nessun uso della fuel API è presente nel codebase** (0 occorrenze di `fuel` in `crates/`).

### CI/CD e benchmark

- **Nessuna pipeline CI/CD**: non esiste directory `.github/workflows`, né altri file di CI (Jenkins, GitLab CI, ecc.).
- Script di benchmark/carico esistenti, orientati a **latenza e correttezza funzionale**, non a energia:
  - [scripts/collect_experiment_metrics.py](scripts/collect_experiment_metrics.py) — esegue trial di enrollment, heartbeat, deployment WASM; calcola intervalli di confidenza al 95% (t-Student, Wilson) su latenza e success-rate.
  - [scripts/run_experiment_campaign.sh](scripts/run_experiment_campaign.sh) — orchestratore campagna completa (warm-up + campagna "hardened" + ablation con controller scalato a 0 repliche).
  - [scripts/postprocess_experiment.py](scripts/postprocess_experiment.py), [scripts/generate_ablation_figure.py](scripts/generate_ablation_figure.py).
  - Artefatti già prodotti in [experiments/20260609-070246/](experiments/20260609-070246/) (raw JSONL, summary, figure PNG) e specifica in [experiments/experimental_agent_brief.md](experiments/experimental_agent_brief.md).
  - Metriche raccolte: latenza per stage (enrollment, heartbeat, deployment, end-to-end), success rate (Wilson CI), goodput, "evidence consistency". **Zero metriche energetiche o di potenza.**

---

## 2. Stato attuale del tracciamento energetico

**Esito della ricerca: nessuno strumento di energy/power monitoring è presente nel repository.**

Grep esaustivo su tutto l'albero (codice Rust, Python, shell, YAML, Markdown, TOML) per `kepler|scaphandre|powerapi|powerjoular|codecarbon|rapl|watt|joule|energy|carbon|power[_ -]?consum`: **0 risultati**. Stessa ricerca su `dashboard-react/src` (JS/JSX): **0 risultati**.

Nel dettaglio:

- **Nessun Kepler, Scaphandre, PowerAPI, PowerJoular, CodeCarbon**: non referenziati in nessun manifest, script o dipendenza.
- **Nessun accesso a RAPL** (né via `/sys/class/powercap`, né via crate Rust dedicate) in `Cargo.lock`/`Cargo.toml`.
- **Nessuna dashboard Grafana, nessuna configurazione Prometheus**: grep `prometheus|grafana` su tutto il repo → 0 risultati. Il sistema **non espone metriche in formato Prometheus** (né `/metrics` in formato OpenMetrics, né `ServiceMonitor`).
- **Nessun fuel metering Wasmtime** (vedi sopra) né altre proxy metric computazionali equivalenti (es. instruction counting, gas metering).
- **Wattmetri hardware / log di consumo**: assenti, coerente col fatto che i "device" sono emulati in Renode su un host generico, non board fisiche strumentate.

### Cosa esiste oggi come "metriche di sistema" (falsamente rassicurante)

Esiste un sottosistema di monitoring, ma **è uno stub con valori hardcoded**, non telemetria reale:

- [crates/wasmbed-infrastructure/src/monitoring.rs:43-71](crates/wasmbed-infrastructure/src/monitoring.rs) — `collect_metrics()` inserisce staticamente `cpu_usage = 45.0`, `memory_usage = 60.0` (commento nel codice: *"Simulate 60% memory usage"*), `disk_usage` fisso.
- [crates/wasmbed-api-server/src/monitoring.rs:60-94](crates/wasmbed-api-server/src/monitoring.rs) — `get_fallback_metrics()` replica gli stessi valori fissi (45.0/60.0/30.0) quando l'endpoint infrastructure non è raggiungibile o non configurato (`infrastructure_endpoint.is_empty()`), che nella pratica sembra essere il caso di default.
- Il dashboard React consuma questi endpoint (`/api/v1/metrics`, `/api/v1/monitoring/metrics`) mostrando quindi numeri non reali all'utente finale, salvo diversa configurazione non presente nei manifest.
- Esiste inoltre un endpoint `get_pod_metrics` ([crates/wasmbed-api-server/src/main.rs:2483](crates/wasmbed-api-server/src/main.rs)) che tenta `kubectl top pods` (richiede `metrics-server` in cluster) — questo è l'unico canale che *potrebbe* restituire CPU/memoria reali per pod, ma solo se `metrics-server` è effettivamente deployato (non presente nei manifest k8s/ del repo) e comunque **non fornisce dati di potenza/energia**, solo utilizzo relativo CPU/RAM.

**Conclusione sezione 2**: il sistema ha zero infrastruttura energetica, e persino le metriche di utilizzo risorse "generiche" che esistono sono in gran parte simulate/hardcoded, non raccolte da un vero collector (node-exporter, cAdvisor, metrics-server). Qualunque affermazione energetica per il paper dovrebbe partire da zero.

---

## 3. Gap analysis

Per produrre dati energetici scientificamente utilizzabili mancano, in ordine di priorità:

1. **Nessuna fonte di potenza reale, né hardware né software-proxy.**
   - Niente RAPL (potenza host), niente wattmetro esterno, niente fuel/instruction metering nel runtime WASM come proxy computazionale.
   - Anche se si aggiungesse RAPL sull'host k3s, questo misurerebbe **il consumo del server che ospita l'intero cluster + il container Renode**, non il consumo del singolo "device edge" emulato — perché i device MCU non sono hardware fisico strumentabile ma **macchine virtuali Renode nello stesso processo/container**, condiviso da N device (vedi sez. 1, "One Renode, N device proxies").

2. **Impossibilità architetturale di isolare il consumo per pod/nodo/modulo WASM così com'è oggi.**
   - Deployment single-node k3s ([doc/K3S_DEPLOYMENT.md](doc/K3S_DEPLOYMENT.md)): tutti i pod (gateway, controller, api-server, dashboard, container Renode) condividono lo stesso host fisico → RAPL a livello host darebbe un aggregato, non un breakdown per componente, senza uno strumento tipo Kepler/Scaphandre che fa attribuzione per-cgroup/per-pod.
   - I "device" WASM emulati (N per singolo container Renode) condividono lo stesso processo Renode → il consumo del **singolo modulo WASM** non è isolabile nemmeno a livello di processo OS, serve strumentazione a livello applicativo (es. fuel/instruction count come proxy, non watt reali).
   - Il runtime WASM lato "device MPU" (Wasmtime, Rust) e quello lato firmware Zephyr (WAMR, C) sono due motori distinti — un futuro instrumentation layer dovrebbe coprire entrambi separatamente.

3. **Assenza di baseline idle.**
   - Nessuno script misura il consumo del cluster k3s a riposo (nessun workload WASM attivo) prima/dopo i trial. La campagna esistente ([scripts/run_experiment_campaign.sh](scripts/run_experiment_campaign.sh)) fa un "warm-up" ma solo per stabilizzare la latenza, non per stabilire una baseline energetica.

4. **Nessuna correlazione tra metriche software esistenti e consumo reale.**
   - Le uniche metriche disponibili oggi (latenza, success rate, goodput da [scripts/collect_experiment_metrics.py](scripts/collect_experiment_metrics.py)) non sono accompagnate da timestamp allineabili a un eventuale collector energetico esterno — andrebbe aggiunto un timestamp comune (monotonic + wall clock) per poter fare join tra "trial N" e "finestra di potenza registrata".

5. **Mancanza di intensità di carbonio della rete elettrica (grid carbon intensity).**
   - Nessun riferimento a dataset o API di carbon intensity (es. ElectricityMaps, WattTime) — necessario solo se si vuole convertire Joule → CO2eq; oggi non applicabile perché manca anche il dato di base in Joule.

6. **Confusione tra "device edge simulato" e "hardware reale" nel paper.**
   - Va reso esplicito nella sezione metodologica che l'intero layer edge è **emulazione Renode su singolo host**, non un vero cloud-fog-edge continuum fisicamente distribuito. Questo è già riconosciuto nel repo stesso: [experiments/experimental_agent_brief.md:346](experiments/experimental_agent_brief.md) richiede esplicitamente una "limitation statement covering single-node K3s, emulation, and absence/presence of physical hardware" nei report generati — è un vincolo già noto dagli autori, coerente con quanto trovato.

**Valutazione architetturale**: l'architettura attuale (single-node, Renode monolitico multi-device, metriche stub) **non permette** l'isolamento energetico per pod/nodo/modulo senza refactoring. Il refactoring minimo necessario è: (a) separare i device Renode in container/cgroup distinti per abilitare attribuzione per-pod con Kepler/Scaphandre, oppure (b) accettare misure aggregate a livello host correlate a proxy metric software (fuel/CPU-time) per fare inferenza relativa, non assoluta.

---

## 4. Raccomandazioni concrete

### A. Baseline energetica a livello host/cluster (complessità: bassa)

- Integrare **Scaphandre** come DaemonSet k3s (legge RAPL da `/sys/class/powercap/intel-rapl`, non richiede kernel module custom come Kepler in molti setup). Aggiungere manifest in nuovo `k8s/monitoring/scaphandre-daemonset.yaml`.
- Alternativa più "standard paper energy-aware computing": **Kepler** (eBPF + RAPL, esporta per-pod già in formato Prometheus) — complessità **media** perché richiede supporto eBPF sul kernel host e, idealmente, `cgroup` per pod già ben isolati (vedi gap #2).
- Prerequisito comune: il nodo deve esporre RAPL (`/sys/class/powercap/intel-rapl`) — va verificato su hardware di test (spesso non disponibile in VM cloud; se il target è una VM, RAPL non funziona e serve un wattmetro esterno o CodeCarbon con modello stimato).

### B. Esposizione Prometheus (prerequisito trasversale, complessità: bassa)

- Nessun `/metrics` Prometheus esiste oggi. Aggiungere endpoint `/metrics` (crate `prometheus` o `metrics-exporter-prometheus`) a [crates/wasmbed-gateway](crates/wasmbed-gateway/src) e [crates/wasmbed-api-server](crates/wasmbed-api-server/src), sostituendo gli stub in `monitoring.rs` con dati reali (almeno CPU/memoria da `/proc` o crate `sysinfo`, poi estendibile a energia).
- Deploy `kube-prometheus-stack` (Prometheus + Grafana) come nuovo manifest k8s, es. `k8s/monitoring/prometheus-values.yaml` — complessità **bassa** (chart standard).

### C. Proxy metric computazionale lato WASM (complessità: media)

- Attivare la **fuel API di Wasmtime** in [crates/wasmbed-wasm-runtime/src/runtime.rs](crates/wasmbed-wasm-runtime/src/runtime.rs) (`Config::consume_fuel(true)`, `Store::set_fuel`, lettura fuel consumato post-esecuzione) come proxy computazionale per modulo WASM, da correlare a potenza host — utile perché è l'unico modo per stimare consumo *per singolo modulo WASM* quando l'isolamento a container/cgroup non è disponibile (gap #2).
- Se si vuole lato firmware reale, equivalente per **WAMR** in [zephyr-app/src/wamr_integration.c](zephyr-app/src/wamr_integration.c) (WAMR supporta gas/instruction metering con build flag dedicato).

### D. Esperimento riproducibile proposto

Coerente con lo schema già esistente in [scripts/run_experiment_campaign.sh](scripts/run_experiment_campaign.sh) (che produce già `experiments/<timestamp>/{raw,summary,figures,environment}`), estendere con:

1. **Fase idle baseline**: N secondi di misura Scaphandre/RAPL a cluster fermo, prima del warm-up esistente → nuovo file `environment/energy_idle_baseline.json`.
2. **Workload**: riutilizzare i trial già definiti (enrollment, heartbeat, deployment WASM) più un nuovo trial "sustained WASM execution" a carico costante per N secondi, per avere una finestra di potenza stabile da campionare (i trial attuali sono troppo brevi/burst per un power sampling significativo).
3. **Raccolta**: nuovo script `scripts/collect_energy_metrics.py` (analogo a `collect_experiment_metrics.py`) che interroga l'endpoint Prometheus di Scaphandre/Kepler ogni T secondi durante il trial, e allinea i timestamp con quelli del trial (stesso schema `TrialRecord`).
4. **Formato export**: JSON/JSONL coerente con l'esistente (`experiments/<timestamp>/raw/energy_trials.jsonl`), con campi `trial_id, stage, timestamp_start, timestamp_end, joules_total, watts_mean, watts_p95, fuel_consumed (se disponibile)`.
5. **Metriche derivate**: Joule/richiesta (enrollment, deploy), Joule/secondo idle vs attivo, eventualmente CO2eq se si aggiunge un fattore di intensità di carbonio statico da configurazione (non stimabile dinamicamente senza servizio esterno).

### E. Stima complessità totale

| Intervento | File/percorso | Complessità |
|---|---|---|
| Scaphandre DaemonSet + baseline RAPL | `k8s/monitoring/` (nuovo) | Bassa |
| Endpoint Prometheus reale (sostituire stub) | `crates/wasmbed-infrastructure/src/monitoring.rs`, `crates/wasmbed-api-server/src/monitoring.rs` | Bassa–Media |
| kube-prometheus-stack / Grafana | `k8s/monitoring/` (nuovo) | Bassa |
| Fuel metering Wasmtime | `crates/wasmbed-wasm-runtime/src/runtime.rs`, `context.rs` | Media |
| Isolamento cgroup/container per device Renode (per attribuzione Kepler per-device) | `crates/wasmbed-qemu-manager`, architettura Renode ([doc/ARCHITECTURE.md](doc/ARCHITECTURE.md)) | Alta (refactoring architetturale) |
| Script esperimento energetico + export dati | `scripts/collect_energy_metrics.py` (nuovo), estensione `run_experiment_campaign.sh` | Bassa–Media |
| Gas metering lato WAMR/firmware Zephyr | `zephyr-app/src/wamr_integration.c` | Media |

**Nota metodologica per il paper**: finché il gap #6 (device emulati su singolo host) non viene risolto con hardware fisico o almeno con isolamento container-per-device, qualsiasi numero energetico ottenuto misurerà **il costo dell'emulazione Renode + del cluster k3s**, non il consumo energetico realistico di un vero dispositivo edge MCU. Va dichiarato esplicitamente come limitazione, sulla falsariga di quanto già fatto per le altre metriche in [experiments/experimental_agent_brief.md](experiments/experimental_agent_brief.md).
