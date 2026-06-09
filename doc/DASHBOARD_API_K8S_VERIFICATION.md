# Verifica: Dashboard → API → Kubernetes

Questo documento mappa ogni funzionalità della dashboard, le API chiamate e le modifiche attese su Kubernetes.

**Verifica eseguita il 2026-02-10**: build OK, test OK, cluster wasmbed attivo. Verificati: GET/POST/DELETE applications (lista da K8s, create con kubectl apply, DELETE via API → risorsa rimossa da K8s), GET devices, GET gateways (dati da K8s, HTTP 200). Deploy/Stop richiedono Gateway in ascolto e device TLS (non testati in questa run). Per ogni flusso si verifica che le operazioni siano implementate e che le modifiche compaiano effettivamente su K8s (lettura tramite `kubectl get` / API GET).

### Test approfonditi (step-by-step)

1. **Step 1 – Build e unit test**: `cargo build --workspace` OK; `cargo test` OK (25 test passati: protocol, qemu-manager, tls-utils, wasm-runtime).
2. **Step 2 – API Server + K8s lettura**: API Server avviato su porta 3001; GET /api/v1/applications, /api/v1/devices, /api/v1/gateways → HTTP 200, JSON con dati da Kubernetes (applications, devices, gateways).
3. **Step 3 – CRUD Application**: POST create → risorsa presente in K8s e in GET; DELETE → risorsa rimossa da K8s e lista applications vuota. Verificato.
4. **Step 4 – Deploy/Stop API**: POST .../applications/:id/deploy e POST .../applications/:id/stop → HTTP 200, JSON con success, message, initiated/stopped, errors. Da host fuori cluster il Gateway non è raggiungibile (DNS), quindi errors contengono errore di connessione; il comportamento dell’API è corretto.
5. **Step 5 – Gateway**: il binario compila e accetta gli argomenti (--help). L’avvio completo richiede certificati TLS (secret in cluster); non eseguito in questa run.

---

## Riepilogo

| Funzionalità Dashboard | API chiamata | Modifica su Kubernetes | Visibile in GET / lista |
|------------------------|-------------|-------------------------|--------------------------|
| Lista applications | GET /api/v1/applications | — (solo lettura) | Sì: legge `kubectl get applications -o json`, status da CRD |
| Creare application | POST /api/v1/applications | Crea Application CRD (kubectl apply) | Sì: compare in GET applications |
| Deploy application | POST /api/v1/applications/:id/deploy | Gateway fa PATCH status (phase, deviceStatuses) | Sì: phase e deviceStatuses visibili in GET |
| Stop application | POST /api/v1/applications/:id/stop | Gateway fa PATCH status (Stopping → Stopped/Failed) | Sì: phase e deviceStatuses aggiornati |
| Eliminare application | DELETE /api/v1/applications/:id | kubectl delete application | Sì: la risorsa sparisce dalla lista |
| Lista devices | GET /api/v1/devices | — (solo lettura) | Sì: legge Device CRD; status aggiornato da Gateway/Controller |
| Creare device | POST /api/v1/devices | Crea Device CRD + eventuale Renode | Sì: compare in GET devices |
| Eliminare device | DELETE /api/v1/devices/:id | kubectl delete device | Sì: risorsa rimossa |
| Lista gateways | GET /api/v1/gateways | — (solo lettura) | Sì: legge Gateway CRD |
| Creare gateway | POST /api/v1/gateways | Crea Gateway CRD | Sì: compare in GET gateways |
| Altre (enroll, connect, emulation, ecc.) | Varie | Device/Gateway CRD o Renode | Come da rispettivi flussi |

---

## 1. Applications

### 1.1 GET /api/v1/applications (lista)

- **Dashboard**: usa i dati per tabella e statistiche (Running, Deploying, Failed, Stopped, ecc.).
- **API Server**: `get_all_applications()` → `kubectl get applications -n wasmbed -o json`.
- **Kubernetes**: nessuna modifica; legge `status.phase`, `status.deviceStatuses`, `status.error`, `status.lastUpdated`.
- **Visibilità**: Le modifiche scritte dal Gateway (PATCH status) sono lette qui: quindi phase, deviceStatuses, failed_devices (conteggio da deviceStatuses con status Failed) e deployed_devices (chiavi di deviceStatuses) compaiono correttamente dopo deploy/stop/ApplicationStatus.

**Verifica**: Dopo un deploy o stop da dashboard, aggiornando la lista (o ricaricando) si devono vedere `status`, `deployed_devices` e `failed_devices` coerenti con `kubectl get applications -n wasmbed -o yaml`.

### 1.2 POST /api/v1/applications (crea)

- **Dashboard**: Application Management → Create application (name, description, wasmBytes, targetDevices).
- **API Server**: `create_application` → costruisce YAML Application (spec.name, wasmBytes, targetDevices.deviceNames) e fa `kubectl apply -f -`.
- **Kubernetes**: crea/aggiorna risorsa `Application` in namespace `wasmbed` (metadata.name = name dalla request).
- **Visibilità**: La nuova application compare in GET /api/v1/applications e in `kubectl get applications -n wasmbed`.

**Verifica**: Creare un’application da dashboard e controllare `kubectl get applications -n wasmbed` e la tabella in dashboard.

### 1.3 POST /api/v1/applications/:id/deploy (deploy)

- **Dashboard**: Application Management → pulsante Deploy su una riga (app_id = id).
- **API Server**: `deploy_application_by_id` → per ogni device in `target_devices` chiama il Gateway `POST {gateway}/api/v1/devices/{device_id}/deploy` (body: app_id, name, wasm_bytes).
- **Gateway**: per ogni POST: PATCH Application status (phase: Deploying, deviceStatuses[device_id]: Deploying); GET Application (legge WASM); invio deploy via TLS al device; alla risposta (DeployAck) PATCH status (phase: Running o Failed, deviceStatuses[device_id]: Running/Failed, lastUpdated, error se fallito).
- **Kubernetes**: le uniche scritture sull’Application sono i PATCH dello **status** (subresource) fatti dal Gateway. L’API Server non deve fare patch sullo status (owner unico = Gateway).
- **Visibilità**: `kubectl get applications -n wasmbed -o yaml` mostra `status.phase`, `status.deviceStatuses`, `status.lastUpdated`, `status.error`. La dashboard in GET /api/v1/applications legge questi campi e mostra status, deployed_devices, failed_devices.

**Verifica**: Cliccare Deploy su un’application con target devices; verificare che in K8s compaiano phase Deploying e poi Running/Failed e deviceStatuses aggiornati; in dashboard la lista deve riflettere lo stesso stato.

### 1.4 POST /api/v1/applications/:id/stop (stop)

- **Dashboard**: Application Management → pulsante Stop.
- **API Server**: `stop_application_by_id` → per ogni target device `POST {gateway}/api/v1/devices/{device_id}/stop/{app_id}`.
- **Gateway**: PATCH status (phase: Stopping, deviceStatuses[device_id]: Stopping); invio stop via TLS; su StopAck PATCH (phase: Stopped o Failed, deviceStatuses aggiornati).
- **Kubernetes**: solo PATCH status (Gateway).
- **Visibilità**: Come per il deploy, phase e deviceStatuses su K8s e in GET /api/v1/applications devono essere coerenti.

**Verifica**: Dopo Stop, in `kubectl get applications -n wasmbed` e nella dashboard lo status deve andare in Stopping e poi Stopped/Failed.

### 1.5 DELETE /api/v1/applications/:id (elimina)

- **Dashboard**: Application Management → Delete.
- **API Server**: `delete_application` → `kubectl delete application {id} -n wasmbed`.
- **Kubernetes**: la risorsa Application viene eliminata.
- **Visibilità**: L’application sparisce da `kubectl get applications -n wasmbed` e dalla lista in dashboard.

**Verifica**: Eliminare un’application e controllare che non compaia più in K8s e in dashboard.

---

## 2. Devices

### 2.1 GET /api/v1/devices (lista)

- **API Server**: `get_all_devices()` → `kubectl get devices -n wasmbed -o json`. Legge status.phase, status.gateway, spec, ecc.
- **Kubernetes**: nessuna modifica. Device CRD viene aggiornato da Device Controller (enrollment, gateway assignment) e dal Gateway (heartbeat, disconnect: last_heartbeat, phase).
- **Visibilità**: Le modifiche scritte da Controller e Gateway (phase Connected/Enrolled/Disconnected, last_heartbeat, gateway) compaiono in GET e in dashboard.

**Verifica**: Dopo enrollment o heartbeat da device, la lista devices in dashboard e `kubectl get devices -n wasmbed` devono mostrare phase e dati aggiornati.

### 2.2 POST /api/v1/devices (crea)

- **API Server**: crea Device CRD (kubectl apply o equivalente) e può avviare emulazione Renode.
- **Kubernetes**: crea risorsa Device in namespace wasmbed.
- **Visibilità**: Il device compare in GET /api/v1/devices e in `kubectl get devices -n wasmbed`.

### 2.3 DELETE /api/v1/devices/:id

- **API Server**: `kubectl delete device {id} -n wasmbed` (e logica collegata).
- **Kubernetes**: rimozione della risorsa Device.
- **Visibilità**: Il device sparisce da lista e da K8s.

---

## 3. Gateways

### 3.1 GET /api/v1/gateways (lista)

- **API Server**: `get_all_gateways()` → `kubectl get gateways.wasmbed.io -n wasmbed -o json`. Legge status.phase, status.connectedDevices, status.enrolledDevices, spec.endpoint.
- **Kubernetes**: nessuna modifica. Gateway CRD è aggiornato dal Gateway Controller.
- **Visibilità**: Le modifiche su K8s compaiono in GET e in dashboard.

### 3.2 POST /api/v1/gateways (crea)

- **API Server**: crea Gateway CRD (kubectl apply o equivalente).
- **Kubernetes**: crea risorsa Gateway.
- **Visibilità**: Il gateway compare in GET e in `kubectl get gateways -n wasmbed`.

---

## 4. Coerenza status Application (owner unico)

Per evitare due writer sullo status dell’Application (API Server e Gateway), l’API Server **non** deve fare PATCH sullo status in `deploy_application_by_id`. Solo il Gateway deve:

- alla ricezione di POST deploy: PATCH phase Deploying e deviceStatuses[device_id] Deploying;
- dopo DeployAck: PATCH phase Running/Failed e deviceStatuses[device_id] Running/Failed.

In questo modo le modifiche che “compaiono su Kubernetes” per le applications sono tutte e sole quelle scritte dal Gateway, e la dashboard (che legge via GET /api/v1/applications → kubectl get applications) vede esattamente quello che il Gateway ha scritto.

---

## 5. Checklist verifica manuale

1. **Applications**
   - [ ] Creare application da dashboard → compare in `kubectl get applications -n wasmbed` e in lista dashboard.
   - [ ] Deploy da dashboard → in K8s compaiono phase e deviceStatuses aggiornati; lista dashboard mostra status/deployed_devices/failed_devices coerenti.
   - [ ] Stop da dashboard → in K8s phase Stopping poi Stopped/Failed; lista dashboard aggiornata.
   - [ ] Eliminare application → sparisce da K8s e da dashboard.

2. **Devices**
   - [ ] Lista devices mostra phase e dati letti da K8s (enrollment/heartbeat aggiornano Device CRD → visibili in lista).
   - [ ] Creare device → compare in K8s e in dashboard.
   - [ ] Eliminare device → sparisce da K8s e da dashboard.

3. **Gateways**
   - [ ] Lista gateways legge Gateway CRD; creare gateway → compare in K8s e in dashboard.

Se tutti i punti sono verificati, le funzionalità e le operazioni delle API della dashboard funzionano come programmato e le relative modifiche compaiono effettivamente su Kubernetes.
