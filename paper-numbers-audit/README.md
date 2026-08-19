# Paper numbers audit

Ogni numero quantitativo del paper con il valore **prima** (quello nel `.tex`), il
valore **dopo** rimisurato sul codice corrente, e la **causa** della differenza.

- **Prima**: valore nel `.tex`; artefatto originale in `experiments/20260609-070246/`
  (campagna del 2026-06-09), copiato in [`raw/before/`](raw/before/).
- **Dopo**: rimisurato sull'host di laboratorio il 2026-08-19, commit `336efc7`.
  Dati grezzi in [`raw/after/`](raw/after/), procedura in [`method.md`](method.md).
- **Causa**: perché il valore cambia. «Nessuna» = il numero non dipende dalle
  modifiche fatte.

Due campagne "dopo": **1 device** (condizione identica al paper, confronto valido) e
**3 device** (condizione nuova, per la sottosezione sulla concorrenza).

---

## 1. Latenze (§8, Tab. `quantitative-latency`, n=100, ms)

| Stadio | Prima (1 dev) | Dopo (1 dev) | Δ | Dopo (3 dev) |
|---|---|---|---|---|
| Enrollment, media | 583.9 | **619.6** | +6.1% | 656.7 |
| Enrollment, p95 | 773.4 | 862.3 | +11.5% | 900.6 |
| Heartbeat obs., media | 4.43 | **4.1** | −7.4% | 4.4 |
| Deployment, media | 601.9 | **662.0** | +10.0% | 679.2 |
| Deployment, p95 | 1031.3 | 1076.5 | +4.4% | 1260.3 |
| Deployment, max | 1423.7 | 2727.3 | +92% | 2897.1 |
| End-to-end, media | 1185.8 | **1281.6** | +8.1% | 1336.0 |
| End-to-end, 95% CI | [1152.8, 1218.7] | **[1225.0, 1338.3]** | — | [1277.8, 1394.2] |

**Causa (medie).** L'enrollment ora attende un'evidenza osservata dal gateway invece
di uno stato che l'API server scriveva da sé al momento della richiesta: la stessa
transizione costa qualche decina di ms in più perché viene misurato un evento reale
anziché una risposta sincrona. L'aumento si propaga all'end-to-end.

**Causa (coda del deployment): non spiegata.** Il massimo raddoppia in entrambe le
campagne. Va ripetuto prima di pubblicare qualsiasi p99: se la coda è reale, i valori
di p95/p99 del paper vanno aggiornati; se è rumore dell'host, serve una campagna in
condizioni più controllate.

## 2. Tassi di successo (§8 riga 64, §13, highlights)

| Stadio | Prima | Dopo (1 dev) | Dopo (3 dev) | Causa |
|---|---|---|---|---|
| Enrollment | 100/100 | **100/100** | 100/100 | — |
| Heartbeat | 100/100 | **100/100** | 100/100 | — |
| Deployment | 100/100 | **100/100** | 100/100 | — |
| Transazionale | 100/100 (Wilson LB 96.3%) | **100/100 (96.3%)** | 100/100 | — |

Invariati, ma ora poggiano su evidenza che l'API server non può più scrivere da sé
(`phase` e `lastHeartbeat` arrivano dal gateway). È il numero più esposto del paper e
regge alla rimisurazione.

## 3. Footprint firmware (§8 riga 120)

| Voce | Prima | Dopo | Causa |
|---|---|---|---|
| `zephyr.bin` (flash) | 403,360 B (≈394 KiB) | **406,568 B (≈397 KiB)** | +3.2 KB di codice: lettura della chiave all'init, iniezione e programmazione del MAC, `CONFIG_NET_L2_ETHERNET_MGMT`. |
| RAM statica (`data`+`bss`) | ≈270,000 B (≈264 KiB) | **270,238 B (≈264 KiB)** | Nessuna: invariata. |
| `zephyr.elf` | 8,030,556 B (7.66 MiB) | 8,095,176 B (7.72 MiB) | Come il `.bin`, più simboli di debug. |

Dettaglio attuale: `text` 397,172 · `data` 9,384 · `bss` 260,854.
Margine: il linker riporta la regione RAM al **98.02%** dei 256 KB (più 12,544 B di
DTCM). Non è un numero del paper, ma va tenuto d'occhio prima di aggiungere codice.

## 4. Risorse control-plane (§8 riga 126, highlights)

| Pod | Prima (n=5) | Dopo (n=5, sotto carico) | Causa |
|---|---|---|---|
| Gateway CPU | 3.6 mc | 1–2 mc | coerente (granularità di `kubectl top`: 1 mc) |
| Gateway RAM | **18.2 MiB** | **4 Mi** | **Metrica diversa, da chiarire** |
| API server CPU | 91.0 mc | 92–161 mc | coerente |
| API server RAM | **201.0 MiB** | **13–14 Mi** | **Metrica diversa, da chiarire** |
| App controller CPU | 44.8 mc | 44–57 mc | coerente |
| App controller RAM | 6.0 MiB | 12 Mi | da chiarire |

Le CPU combaciano, le RAM no, e nessuna delle modifiche fatte può spiegare un calo da
201 a 13 MiB. Prima di usare uno dei due valori va dichiarata la metrica (working set
di `kubectl top` contro RSS/page cache) e rifatto il campionamento con quella.
**18.2 MiB compare anche negli highlights**, quindi è una voce da chiudere.

## 5. Dimensioni sul filo (§8 riga 107)

| Voce | Prima | Dopo | Causa |
|---|---|---|---|
| Heartbeat req/ack | 6 B ciascuno | invariato | Nessuna: formato CBOR non toccato. |
| Round trip enrollment | 87 B | invariato | Nessuna. |
| Round trip deploy (modulo 33 B) | 90 B | invariato | Nessuna. |
| Traffico heartbeat a regime | 1728 B/h per device | invariato | Nessuna: 12 B ogni 25 s. |

## 6. Testbed (§8, Tab. `testbed`)

| Voce | Prima | Dopo | Causa |
|---|---|---|---|
| Rete device | «TAP `tap0` a 192.168.1.1/24, DHCP, DNAT verso gateway TLS su porta 30443» | Bridge `wasmbr0` a 192.168.1.1/24, una TAP `wtap-<hash>` per device, DHCP, DNAT verso il pod del gateway | Percorso riprogettato per più device; il traffico TLS non passa più dal `kubectl port-forward`. **La riga della tabella va aggiornata.** |
| K3s | v1.35.4+k3s1 | v1.35.4+k3s1 | Nessuna. |
| Host | 32 vCPU, 62 GiB RAM | 32 vCPU, 62 GiB RAM | Nessuna. |
| Zephyr | 3.5.0 | 3.5.0 | Nessuna. Attenzione: §8 riga ~209 dice «Zephyr 3.7.0» per la misura CPU-time — incoerente con la tabella testbed, da uniformare. |

## 7. CPU-time per fase (§8, Tab. `cpu-time-per-phase`, n=5)

| Voce | Prima | Dopo | Causa |
|---|---|---|---|
| Tutte le righe | vedi `.tex` | **non rimisurato** | Da rifare: la nota «enrollment non soddisfa il criterio `phase=Connected` per i device provisionati via questa API» descriveva un comportamento che non esiste più. |

## 8. Energia e carbonio (§8, Tab. `workload-cpu-time` e paragrafo coefficiente)

| Voce | Prima | Dopo | Causa |
|---|---|---|---|
| CPU-s/call, J/call | vedi tabella | **non rimisurato** | Nessuna attesa: misura il costo di esecuzione WASM. |
| Coefficiente W/vCPU | 4.72 (blog RAPL, fonte primaria) | 4.84 da TDP AMD ufficiale come primaria; 4.72 come corroborazione | Cambio di **fonte**, non di misura (differenza 2.5%). |
| Intensità di carbonio | 310 gCO₂eq/kWh (Nowtricity) | **parametrico**: 30.2·I µgCO₂eq | Fonte non istituzionale rimossa; `I` da prendere da ISPRA. |

## 9. Scalabilità fleet (§10, ora anche §8 "Concurrent devices")

| Voce | Prima | Dopo | Causa |
|---|---|---|---|
| Sessioni TLS con 3 device Renode | 1 su 3 | **3 su 3** | Chiave pubblica, MAC, TAP e lease DHCP distinti per device; TLS non più via port-forward. |
| Deploy WASM sulla fleet | non riuscito | **3 su 3 `Running`**, 3 DeployAck | Stessa causa. |
| Registro 50 board | sanity check dello stato del registro | invariato | Non rimisurato. |

---

## Da chiudere prima della submission

1. **RAM dei pod** (§4): decidere la metrica e rimisurare — il valore è negli highlights.
2. **Coda del deployment** (§1): ripetere la campagna, il massimo raddoppia.
3. **CPU-time per fase** (§7): rimisurare e togliere la nota sull'anomalia dell'enrollment.
4. **Riga rete della tabella testbed** (§6) e **incoerenza Zephyr 3.5.0 / 3.7.0**.
5. **`I` da ISPRA** nel paragrafo del carbonio (marcatore rosso già nel `.tex`).
6. Riferimento rotto preesistente `fig:energy-workload-curve`: la figura è dentro un
   blocco `comment` ma il testo la cita.
