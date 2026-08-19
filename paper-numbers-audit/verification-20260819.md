# Seconda verifica indipendente — 2026-08-19

Rimisurazione sull'host di laboratorio `192.168.100.183`, repo a `336efc7`, per
chiudere i punti lasciati aperti da [`README.md`](README.md). Nuova campagna
completamente indipendente da quella dell'audit: stesso testbed, stesso commit,
1 device Renode (`fleet-device-1`) connesso in TLS.

Dati grezzi in [`raw/verify-20260819/`](raw/verify-20260819/).

---

## 1. Latenze — la coda del deployment è reale, non rumore

n=100, 1 device, `scripts/collect_experiment_metrics.py`.

| Stadio | Paper | Audit (1ª rimisura) | **Verifica (2ª rimisura)** |
|---|---|---|---|
| Enrollment, media | 583.9 | 619.6 | **609.5** |
| Enrollment, p95 | 773.4 | 862.3 | **839.6** |
| Heartbeat obs., media | 4.43 | 4.1 | **4.08** |
| Deployment, media | 601.9 | 662.0 | **676.5** |
| Deployment, p95 | 1031.3 | 1076.5 | **1021.8** |
| Deployment, p99 | 1295.7 | — | **1145.2** |
| Deployment, max | 1423.7 | 2727.3 | **3000.8** |
| End-to-end, media | 1185.8 | 1281.6 | **1286.0** |
| End-to-end, p95 | 1503.1 | — | **1661.5** |
| End-to-end, p99 | 1789.6 | — | **1974.3** |
| End-to-end, 95% CI | [1152.8, 1218.7] | [1225.0, 1338.3] | **[1226.4, 1345.6]** |

**Conclusioni.**

- Le due rimisurazioni concordano tra loro entro l'1% sulle medie e i loro CI
  dell'end-to-end si sovrappongono quasi esattamente. La differenza rispetto al
  paper non è variabilità: i CI del paper e quelli rimisurati **non si
  sovrappongono** (1152.8–1218.7 contro 1226.4–1345.6). Il valore corrente è
  ~1286 ms, +8.4% rispetto a 1185.8 ms.
- **La coda del deployment è confermata** (punto 2 dell'audit): il massimo è
  2727 ms e 3001 ms in due campagne indipendenti, contro i 1423.7 ms del paper.
  Non è un outlier isolato dell'host.
- Il **p95** del deployment invece regge (1021.8 contro 1031.3): la coda si
  manifesta oltre il p99. p95 e p99 dell'end-to-end sono però più alti del paper.

## 2. Tassi di successo — invariati

| Stadio | Paper | Verifica |
|---|---|---|
| Enrollment | 100/100 | **100/100** |
| Heartbeat | 100/100 | **100/100** |
| Deployment | 100/100 | **100/100** |
| Transazionale (tutti gli stadi) | 100/100 | **100/100** |
| Coerenza dell'evidenza | 100/100 | **100/100** |

Wilson 95% LB per 100/100 ricalcolato: **96.30%**, esattamente il 96.3% del paper.
Aritmetica del traffico heartbeat ricontrollata: 12 B × (3600/25) = **1728 B/h** ✓.

## 3. CPU-time per fase — punto 3 chiuso, ma i numeri cambiano

n=5, `scripts/cpu_time_per_phase.py` (nuovo, aggiunto con questa verifica).
CPU-secondi, medie.

| Stadio | Pod | Paper | **Verifica** |
|---|---|---|---|
| Enrollment | gateway | 0.000923 | **0.000350** |
| Enrollment | api-server | 0.592647 | **0.143510** |
| Enrollment | app-controller | 0.000543 | **0.036625** |
| Heartbeat | gateway | 0.000612 | **0.000413** |
| Heartbeat | api-server | 0.099574 | **0.020483** |
| Heartbeat | app-controller | 0.000022 | **0.010088** |
| Deployment | gateway | 0.005530 | **0.006338** |
| Deployment | api-server | 1.049292 | **0.564172** |
| Deployment | app-controller | 0.011233 | **0.057557** |

**La nota del paper è obsoleta.** Il `.tex` (righe 246 e 264) spiega a lungo che
l'enrollment «non soddisfa il criterio `phase=Connected` per i device
provisionati attraverso questo endpoint»: nella verifica l'enrollment fa
**5/5**, con `phase=Connected` letto dal CRD in tutti i trial. Vanno rimossi sia
il paragrafo `\revC{...}` di riconciliazione sia la clausola nella caption.

Da correggere anche il testo che accompagna la tabella: dice «Zephyr 3.7.0»
mentre la tabella del testbed dice 3.5.0 (punto 4 dell'audit, ancora aperto).

## 4. Risorse dei pod — punto 1 chiuso: i valori del paper non sono riproducibili

n=5 campioni presi **durante** la campagna n=100, quindi sotto carico. Due
metriche affiancate per togliere di mezzo l'ambiguità: working set
(`kubectl top`) e cgroup v2 (`memory.current`, `anon` di `memory.stat`).

| Pod | Paper CPU | Verifica CPU | Paper RAM | Verifica RAM (top) | Verifica RAM (cgroup current / anon) |
|---|---|---|---|---|---|
| Gateway | 3.6 mc | **3 mc** | 18.2 MiB | **4–5 Mi** | 5.60 MB / 3.82 MB |
| API server | 91.0 mc | **149–180 mc** | 201.0 MiB | **30–43 Mi** | 32.3–33.0 MB / 5.92 MB |
| App controller | 44.8 mc | **33–45 mc** | 6.0 MiB | **12 Mi** | 13.3 MB / 11.4 MB |

- La CPU del **gateway** e dell'**app controller** combacia con il paper. Quella
  dell'**API server** è più alta, coerentemente col fatto che questi campioni
  sono presi sotto il carico della campagna.
- La RAM del gateway e dell'API server **non è riproducibile con nessuna delle
  due metriche**: 18.2 MiB contro 4–5 Mi misurati, 201.0 MiB contro 30–43 Mi.
  Non è una questione di working set contro RSS — entrambe le metriche danno lo
  stesso ordine di grandezza, molto sotto il paper. Il valore 18.2 MiB compare
  anche **negli highlights, nell'abstract e nella tabella `arch-compare`**
  («${\approx}$18 MiB», «Fog mediation RAM 18.2 MiB») e in §10 («sub-20 MiB»):
  vanno corretti tutti insieme.
- L'app controller va nella direzione opposta (6.0 → 12 Mi).

**Registro a 50 board**: 50/50 registrazioni riuscite attraverso
`/api/v1/board/register`; la RAM del gateway è rimasta **identica** (5 Mi,
`memory.current` invariato al byte). Il claim del paper «non oltre 19.0 MiB»
resta vero ma è ancorato a una baseline sbagliata: il numero corretto è ~5 Mi.

## 5. Footprint firmware — confermato l'audit

Build corrente in `/home/ubuntu/retrospect/zephyr-workspace/build/stm32f746g_disco/zephyr`.

| Voce | Paper | Verifica |
|---|---|---|
| `zephyr.bin` | 403,360 B (≈394 KiB) | **406,568 B (≈397 KiB)** |
| RAM statica (data+bss) | ≈270,000 B (≈264 KiB) | **270,238 B** (data 9,384 + bss 260,854) |
| `zephyr.elf` | 8,030,556 B (7.66 MiB) | **8,095,176 B (7.72 MiB)** |
| Modulo WASM | 33 B | invariato |

`text` = 397,172. La crescita di +3.2 KB è il codice aggiunto per l'identità per
device (lettura chiave all'init, iniezione e programmazione del MAC).

## 6. Energia — rimisurata, e due punti su quattro non erano riproducibili

Campagna con Kepler + Prometheus ridistribuiti sul cluster e il probe WASM
(`fuel_load_probe`) sotto QoS Guaranteed a 2 vCPU, come dichiara il paper.
Nuovo script: [`scripts/energy_campaign.py`](../scripts/energy_campaign.py).

### 6.1 Curva CPU-time per taglia del workload

| N | n | Paper (CPU-s/call) | **Verifica** | Δ |
|---|---|---|---|---|
| 50.000 | 10 | 8.9×10⁻⁵ | **9.59×10⁻⁵** | +7.8% |
| 200.000 | 39 | 3.53×10⁻⁴ | **3.77×10⁻⁴** | +6.9% |
| 1.000.000 | 10 | 1.76×10⁻³ | **1.87×10⁻³** | +6.2% |
| 5.000.000 | 10 | 8.84×10⁻³ | **9.36×10⁻³** | +5.9% |

Lo scostamento è **sistematico e nella stessa direzione** su tutte e quattro le
taglie (+6–8%), non rumore: gli IC sono strettissimi (a N=200.000,
[3.761, 3.784]×10⁻⁴) e due passate indipendenti a quella taglia hanno dato
3.7725×10⁻⁴ e 3.7726×10⁻⁴. La linearità in N regge: ×3.93, ×4.96, ×5.01 per
incrementi di ×4, ×5, ×5.

**Due dei quattro punti non erano riproducibili.** A N≥10⁶ il profilo MCU del
runtime impone un budget Wasmtime di 5×10⁶ unità di fuel
([`config.rs:105`](../crates/wasmbed-wasm-runtime/src/config.rs)); il loop lo
esaurisce, la chiamata va in trap e il probe registra solo errori — 41.391
errori e **zero chiamate completate** in una finestra da 30 s. I due punti alti
del paper sono ottenibili solo alzando il budget. Ora è un parametro
(`FUEL_BUDGET`) e il `.tex` lo dichiara.

### 6.2 Cicli idle/attivo (N=200.000, n=39)

| Voce | Paper | Verifica |
|---|---|---|
| CPU-s finestra attiva | 29.9859 ± 0.0022 | **29.9289 ± 0.0189** |
| CPU-s finestra idle | 0.0012 ± 0.0004 | **0.000116 ± 0.000053** |
| Differenza appaiata, IC 95% | [29.9839, 29.9855] | **[29.9230, 29.9354]** |
| Potenza Kepler attiva | 4.042 ± 0.153 W | **3.952 W** |
| Potenza Kepler idle | 0.256 ± 0.032 W | **0.000 W** |

La potenza attiva combacia entro il 2.2%, il che è notevole per una stima
modellistica. L'idle no: Kepler attribuisce al pod **esattamente zero** watt
dinamici nelle finestre di riposo. Non è un dato mancante — è quanto il modello
produce quando il pod non consuma CPU. Il `.tex` ora lo dice esplicitamente,
come ulteriore motivo per trattare il wattaggio come segnale secondario.

Nota di metodo: la prima passata dava 0.068 CPU-s nelle finestre idle. Era un
artefatto del driver, non del sistema: interpolando il contatore `cpu.stat` sui
bordi della finestra, un campione preso durante il load contaminava l'idle
successivo. La misura corretta usa solo i campioni interni alla finestra.

### 6.3 Energia e carbonio derivati

Ricalcolati dai valori rimisurati: ≈119 J attribuibili alla finestra attiva
(≈1.5 mJ per invocazione su ≈79.300 invocazioni), ≈33·I µgCO₂eq, e con
l'intensità ISPRA di 199.61 gCO₂eq/kWh ≈6.6 mgCO₂eq per finestra. I
coefficienti W/vCPU (4.84 da TDP, 4.72 da RAPL di terzi) sono invariati: sono
una scelta di fonte, non una misura.

## 7. Figure

Cinque delle sette figure incluse nel paper dipendono da numeri cambiati e sono
state rigenerate con [`scripts/generate_paper_figures.py`](../scripts/generate_paper_figures.py)
(prima non esisteva uno script che le producesse):

- `latency_boxplot.png`, `latency_cdf.png`, `latency_trial_scatter.png` — dai
  100 trial della campagna di verifica;
- `pod_resources_bar.png` — dai 5 campioni sotto carico;
- `energy_workload_size_curve.png`, `energy_idle_active_kepler.png` — dalla
  campagna energia.

`wire_sizes_bar.png` non è stata toccata: il formato CBOR non è cambiato.

## Cosa resta non verificato

- **Ablation** (n=30, application controller scalato a zero): non rifatta, ma
  non è materiale del paper — `main.tex` non include una sezione di ablation e
  le sue figure non sono referenziate.
- **`835/27.021` periodi di throttling** citati a giustificazione del QoS a
  2 vCPU: non rimisurati; il manifest ora fissa requests = limits = 2 vCPU,
  quindi la condizione descritta è quella effettiva.
- **Registro 50 board**: verificato come stato del registro (50/50, RAM
  invariata), non come 50 sessioni TLS — la stessa scoping del paper.

## Correzioni applicate al `.tex`

Applicate dopo questa verifica, sui valori rimisurati nella campagna del
2026-08-19.

| File | Correzione |
|---|---|
| `08_methodology.tex` | Tab. `quantitative-latency`: tutte e quattro le righe ai valori rimisurati; aggiunta una frase sulla coda del deployment (CV 0.43, max 3000.8 ms). |
| `08_methodology.tex` | Footprint firmware: 403,360 → 406,568 B; 8,030,556 → 8,095,176 B; RAM statica esplicitata come 9,384 + 260,854 = 270,238 B. |
| `08_methodology.tex` | Risorse dei pod: valori rimisurati e **metrica dichiarata** (working set di `metrics-server`, con riscontro su `memory.current` e `anon`); il registro 50 board ora dice «RAM invariata» invece di «non oltre 19.0 MiB». |
| `08_methodology.tex` | Riga «Device networking» della tabella testbed: bridge `wasmbr0` e una TAP per device, DNAT verso il pod del gateway. |
| `08_methodology.tex` | «Zephyr 3.7.0» → 3.5.0, coerente con la tabella testbed. |
| `08_methodology.tex` | Tab. `cpu-time-per-phase`: tutte e nove le righe rimisurate; caption e paragrafo dicono 5/5 su tutti gli stadi; **rimosso** il paragrafo `\revC{...}` che riconciliava l'anomalia dell'enrollment, non più osservabile. |
| `08_methodology.tex` | Tab. `arch-compare`: «≈18 MiB» e «18.2 MiB» → «< 5 MiB». |
| `00_abstract.tex` | 1185.8 → 1286.0 ms, CI [1226.4, 1345.6]; 18.2 MiB → «sotto 5 MiB»; 3.6 → 3 millicore; 394 → 397 KiB di flash. Allineato anche l'abstract alternativo («single-digit MiB», «entro 400 KiB»). |
| `01_highlights.tex` | 1185.8 → 1286.0 ms; 18.2 MiB → «sotto 5 MiB». |
| `13_conclusion.tex` | «≈1.19 s» → «≈1.29 s». |
| `10_discussion.tex` | «sub-20 MiB gateway mediation» → «single-digit-MiB». |

Il PDF ricompila senza errori e senza riferimenti indefiniti.

**Non toccati**: i tassi di successo (100/100, Wilson LB 96.3%), le dimensioni
sul filo, il modulo WASM da 33 B, la tabella del testbed a parte la riga rete, e
la sottosezione sulla concorrenza — tutti confermati dalla rimisurazione.

### Correzioni alla sezione energia

| File | Correzione |
|---|---|
| `08_methodology.tex` | Tab. `workload-cpu-time`: tutti e quattro i CPU-s/call e i J/call ricalcolati; aggiunta la dichiarazione del fuel budget alzato per N ≥ 10⁶. |
| `08_methodology.tex` | Paragrafo idle/attivo: CPU-s attivi e idle, IC della differenza appaiata, e potenza Kepler ai valori rimisurati; aggiunta la nota che l'idle stimato è esattamente zero. |
| `08_methodology.tex` | Curva: «da 89 µs a 8.84 ms» → «da 95.9 µs a 9.36 ms». |
| `08_methodology.tex` | Carbonio: ≈110 → ≈119 J, ≈1.3 → ≈1.5 mJ/invocazione, ≈30·I → ≈33·I µgCO₂eq, ≈6 → ≈6.6 mgCO₂eq. |
| `08_methodology.tex` | Caption della figura idle/attivo: n=38 cicli con lettura di potenza, e il caveat sull'attribuzione nulla. |

### Modifiche al codice richieste dalla verifica

| File | Modifica |
|---|---|
| `crates/wasmbed-wasm-runtime/src/bin/fuel_load_probe.rs` | `BURN_N` e `FUEL_BUDGET` da ambiente: la taglia del workload era hardcoded a 200.000 e il budget non era sovrascrivibile, quindi la curva del paper non era riproducibile. |
| `k8s/test-resources/energy-probe-deployment.yaml` | QoS Guaranteed a 2 vCPU (requests = limits), come dichiara il paper; il manifest era fermo a 1 vCPU con requests più basse. |
| `scripts/energy_campaign.py` | Nuovo: esegue la campagna energia. |
| `scripts/cpu_time_per_phase.py` | Nuovo: esegue la misura del CPU-time per fase. |
| `scripts/generate_paper_figures.py` | Nuovo: rigenera le cinque figure misurate. |
