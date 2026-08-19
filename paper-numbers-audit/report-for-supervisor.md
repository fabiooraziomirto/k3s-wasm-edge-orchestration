# Scalabilità della fleet: verifica, esito e stato dei numeri

Riferimento: l'osservazione sul test di scalabilità («tre macchine Renode, una sola
sessione TLS attiva nel gateway») e quella sulle fonti dei coefficienti energetici.

---

## 1. Esito in breve

Il risultato negativo non era un limite del banco di prova ma una causa identificabile
nel codice, ora rimossa. Con tre device Renode concorrenti, misurato il 2026-08-19:

| Evidenza | Fonte | Prima | Dopo |
|---|---|---|---|
| Sessioni TLS distinte nel gateway | vista runtime del gateway, heartbeat freschi | 1 su 3 | **3 su 3** |
| Device in `phase: Connected` | Device CRD, scritto dal gateway | — | **3 su 3** |
| Identità di rete distinte | lease DHCP e indirizzi separati | 1 IP condiviso | **3 su 3** |
| Deploy WASM sull'intera fleet | `deviceStatuses` dell'Application CRD | non riuscito | **3 su 3 `Running`** |

I tre deploy sono l'evidenza più stringente: un comando di deployment viaggia solo sulla
sessione TLS del device di destinazione, quindi tre acknowledgment distinti provano tre
canali indipendenti e non uno riusato.

Riproducibile con un comando: `./scripts/run-e2e-fleet.sh -n 3` (24 controlli, 0 falliti,
due esecuzioni consecutive).

## 2. Perché tre device diventavano uno

Quattro identificatori che dovevano essere per-device erano condivisi. Ciascuno da solo
era sufficiente a far collassare la fleet.

| Identificatore | Cosa accadeva | Conseguenza |
|---|---|---|
| Chiave pubblica | il firmware la leggeva a `0x20002000` circa 1,6 s dopo il boot, quando quella RAM è già riusata; ripiegava su una chiave statica condivisa | il gateway indicizza le connessioni per chiave: ogni nuova sessione sovrascriveva la precedente |
| MAC | il driver Ethernet manteneva il proprio indirizzo di default, identico in ogni istanza | un solo lease DHCP e un solo IP per tutti |
| Interfaccia TAP | tutti gli script Renode creavano `tap0`, mentre i container condividono il namespace di rete dell'host | una sola macchina poteva agganciarla |
| Percorso TLS | i device raggiungevano il gateway attraverso un `kubectl port-forward` | sessioni concorrenti cadute e riaperte di continuo; i deploy che arrivavano in quelle finestre andavano in timeout |

## 3. Cosa è cambiato nel codice

21 commit, +1336 / −208 righe sui sorgenti.

| Area | File | Modifica |
|---|---|---|
| Emulazione | `crates/wasmbed-qemu-manager/src/lib.rs` | chiave pubblica presa dal Device CRD e iniettata; TAP e MAC derivati dal device id; test unitari e di integrazione |
| Firmware | `zephyr-app/src/wasmbed_protocol.c` | la chiave viene copiata all'init, quando quella memoria è ancora valida |
| Firmware | `zephyr-app/src/network_handler.c`, `prj.conf` | MAC per device programmato via `NET_REQUEST_ETHERNET_SET_MAC_ADDRESS` prima del DHCP |
| Rete host | `scripts/setup-renode-net.sh` | bridge `wasmbr0` con una TAP per device; DNAT diretto al pod del gateway |
| Control plane | `crates/wasmbed-api-server/src/main.rs` | l'endpoint di connect riporta l'esito reale dell'avvio dell'emulazione; `phase` e `lastHeartbeat` non vengono più scritti dall'API server |
| Gateway | `crates/wasmbed-gateway/src/http_api.rs` | `connected_since` riferito alla sessione corrente |
| Verifica | `scripts/run-e2e-fleet.sh`, `scripts/test-fleet-scalability.sh` | esecuzione end-to-end in un comando, con diagnostica |

## 4. Cosa è cambiato nei risultati

Campagna n=100 ripetuta nelle stesse condizioni del paper (un device).

| Voce | Prima | Dopo | Nota |
|---|---|---|---|
| Successo transazionale | 100/100 | **100/100** | invariato |
| Enrollment, media | 583,9 ms | 619,6 ms | +6,1% |
| Deployment, media | 601,9 ms | 662,0 ms | +10,0% |
| End-to-end, media | 1185,8 ms | 1281,6 ms | +8,1% |
| Flash firmware | 403.360 B | 406.568 B | +3,2 KB di codice |
| RAM statica firmware | ≈270.000 B | 270.238 B | invariata |
| Dimensioni sul filo, traffico heartbeat | 6/87/90 B, 1728 B/h | invariati | protocollo non toccato |

L'aumento delle latenze medie ha una spiegazione precisa: l'enrollment ora attende
un'evidenza osservata dal gateway, mentre prima lo stato veniva scritto dall'API server
al momento della richiesta. Si misura un evento reale invece di una risposta sincrona.
I tassi di successo restano 100/100 e ora poggiano su evidenza che il control plane non
può attribuirsi da sé.

## 5. Cosa resta aperto

Quattro punti, dichiarati come tali e non risolti:

1. **RAM dei pod.** Il paper riporta 18,2 MiB per il gateway e 201,0 MiB per l'API server;
   la rimisurazione con `kubectl top` dà 4 Mi e 13–14 Mi. Le CPU invece coincidono. Nessuna
   delle modifiche fatte può spiegare la differenza: le due misure contano quasi certamente
   grandezze diverse. Va scelta la metrica e rimisurato. 18,2 MiB compare negli highlights.
2. **Coda del deployment.** Il valore massimo raddoppia fra le campagne (1424 → 2727 ms),
   mentre mean e p95 restano confrontabili. Prima di pubblicare un p99 va ripetuta la misura.
3. **Tabella CPU-time per fase (n=5).** Non rimisurata. La nota che accompagna la riga
   dell'enrollment descrive un comportamento che non esiste più.
4. **Coerenza del testo.** La tabella del testbed descrive ancora la rete a TAP singola, e
   la versione di Zephyr compare come 3.5.0 in un punto e 3.7.0 in un altro.

## 6. Modifiche al paper già applicate

- **§8, nuova sottosezione "Concurrent devices"**: il risultato sulla concorrenza con la
  sua evidenza e il suo perimetro (tre device su un solo host; nessuna estrapolazione a
  fleet-scale, multi-gateway o hardware fisico).
- **§10**: la frase sulla singola sessione TLS sostituita dal risultato; le 50 board
  restano dichiarate come test dello stato del registro, non di sessioni concorrenti.
- **Fonti energetiche**: coefficiente W/vCPU con la specifica AMD ufficiale come fonte
  primaria (155 W TDP su 32 thread → 4,84 W/vCPU) e la misura RAPL di terzi come
  corroborazione secondaria dichiarata (4,72 W/vCPU, entro il 3%); indicati SPECpower e la
  coefficient table di Cloud Carbon Footprint come vie per un coefficiente specifico.
  Intensità di carbonio: rimossa la fonte non istituzionale, adottato il fattore ISPRA di
  consumption mix (199,61 gCO₂eq/kWh, 2025 preliminare) e reso esplicito il calcolo.
- **Disclaimer**: la cautela sul coefficiente non è più solo nelle conclusioni; il
  paragrafo si apre dichiarando che è l'unico passaggio della pipeline non misurato
  su questa piattaforma.

## 7. Come verificarlo

```bash
./scripts/run-e2e-fleet.sh --check     # prerequisiti
./scripts/run-e2e-fleet.sh -n 3        # test completo
```

Prerequisiti non negoziabili: host con accesso privilegiato al kernel (k3s non parte in
container non privilegiati), Docker, immagine Renode, firmware Zephyr compilato.

Dati grezzi delle due campagne e confronto voce per voce: `paper-numbers-audit/`.
