# Analisi repository RETROSPECT

Analisi di elementi **utili**, **inutili** (rimossi o da rimuovere) e **non implementati** o non integrati.

---

## 1. Elementi utili (mantenere)

### Crate e binari in uso
- **wasmbed-api-server**: API REST, dashboard, integrazione K8s e Gateway.
- **wasmbed-gateway**: HTTP API (8080), TLS (8081), enrollment, deploy, heartbeat, recovery.
- **wasmbed-device-controller**: Reconcile Device CRD, handle Unreachable/Disconnected, Renode.
- **wasmbed-application-controller**: Reconcile Application CRD, deploy verso Gateway.
- **wasmbed-gateway-controller**: Reconcile Gateway CRD, gestione pod Gateway.
- **wasmbed-qemu-manager**: Renode manager (avvio device, scrittura endpoint in memoria, board register).
- **wasmbed-protocol**: Tipi e CBOR ClientMessage/ServerMessage.
- **wasmbed-tls-utils**: TLS server/client, callbacks Gateway.
- **wasmbed-k8s-resource**: CRD Device/Application/Gateway, client K8s, status update.
- **wasmbed-types**: GatewayReference, PublicKey, tipi condivisi.
- **wasmbed-cert**: Usato da gateway e protocol-server (certificati).
- **wasmbed-config**: Configurazione condivisa.
- **wasmbed-test-utils**, **wasmbed-wasm-runtime**: Test e runtime WASM (host-side).

### Tool CLI (utili per sviluppo/debug)
- **wasmbed-cert-tool**: Generazione/gestione certificati.
- **wasmbed-protocol-tool**: Encode/decode protocollo CBOR.
- **wasmbed-k8s-resource-tool**: Operazioni su risorse K8s (CRD).

### Documentazione
- **README.md**: Documentazione principale.
- **doc/DEVELOPMENT_STATUS.md**: Stato sviluppo aggiornato.
- **doc/ARCHITECTURE.md**, **doc/SEQUENCE_DIAGRAMS.md**: Architettura e flussi.
- **doc/RENODE_TLS_DEPLOY_VERIFICATION.md**, **doc/DASHBOARD_API_K8S_VERIFICATION.md**: Verifiche.
- **doc/FIRMWARE.md**, **doc/TLS_CONNECTION.md**, **doc/MCU_SUPPORT.md**, **doc/DEPLOYMENT.md**, **doc/K3S_DEPLOYMENT.md**, **doc/WASMBED_CAPABILITIES.md**, **doc/REAL_DEVICE_INTEGRATION.md**: Riferimenti tecnici.

### Script e config
- **scripts/deploy-k3s.sh**, **scripts/cleanup-k3s.sh**: Deploy e cleanup K3s.
- **scripts/generate-gateway-certs.sh**: Certificati X.509 v3 per Gateway.
- **scripts/verify-tls-and-deploy.sh**: Verifica TLS e deploy.
- **scripts/README.md**: Istruzioni script.
- **config/wasmbed-config.yaml**: Configurazione esempio.

### K8s
- **k8s/crds/**, **k8s/deployments/**, **k8s/rbac/**, **k8s/namespace.yaml**, **k8s/ingress/**, **k8s/gateways/**, **k8s/gateway-hpa.yaml**: Deploy produzione.
- **k8s/devices/**, **k8s/test-resources/**: Device e risorse di test.

### Dockerfile (in uso)
- **Dockerfile.api-server**, **Dockerfile.dashboard**, **Dockerfile.gateway**, **Dockerfile.device-controller**, **Dockerfile.application-controller**, **Dockerfile.gateway-controller**: Build immagini servizi.
- **crates/wasmbed-gateway/Dockerfile**: Alternativa per solo gateway.

### Firmware e emulazione
- **zephyr-app/**: App Zephyr (protocollo, WAMR, TLS, heartbeat, deploy).
- **renode-scripts/*.resc**: Script Renode (es. arduino, nrf, stm32f4); `test_ethernet_connection.resc` aggiornato per non dipendere da binario inesistente.
- **wamr/**: Submodule WAMR (runtime WASM su device).

---

## 2. Elementi inutili (rimossi o da non usare)

### File rimossi (operazione completata)
- **.dashboard.pid**: File temporaneo (PID); già in `.gitignore` (*.pid). **Eliminato.**
- **doc/CURRENT_STATUS.md**: Obsoleto; stato aggiornato in **DEVELOPMENT_STATUS.md**. **Eliminato.**
- **README_TO_BE_DONE.MD**: Duplicato/draft del README; contenuto coperto da **README.md**. **Eliminato.**
- **Dockerfile.device**: Buildava il binario `wasmbed-device-runtime`, che **non esiste** nel workspace (crate rimosso). **Eliminato.**
- **scripts/TEST_REPORT.md**, **scripts/API_TEST_REPORT.md**: Report di test storici; stato test e step verificati sono in **doc/DEVELOPMENT_STATUS.md**. **Eliminati.**

### Codice morto rimosso
- **wasmbed-gateway/src/http_api.rs**: Rimossi `CborTlsHandler`, `start_cbor_tls_listener`, `send_cbor_tls_message`, campi `tls_config` e `cbor_tls_listener` da `HttpApiServer` (TLS reale è in wasmbed-tls-utils).
- **wasmbed-device-controller**: Rimosso `create_device_pod` e campo `pods`; Renode è gestito da RenodeManager.

---

## 3. Non implementato o non integrato

### Crate rimosso
- **wasmbed-protocol-server**: Rimosso dal workspace e eliminata la directory del crate (nessun altro crate lo usava; non era integrato).

### Riferimenti a binario inesistente
- **wasmbed-device-runtime**: Rimosso dal workspace (commento in `Cargo.toml`). Riferimenti residui:
  - **renode-scripts/test_ethernet_connection.resc**: Usava `wasmbed-device-runtime` come firmware; aggiornato per usare path firmware Zephyr o messaggio chiaro se assente.

### Funzionalità parziali (documentate in DEVELOPMENT_STATUS)
- **E2E TLS device → Gateway**: Firmware Zephyr si connette e invia heartbeat; deploy WASM con CBOR + WAMR + DeployAck implementato; test E2E con Renode dipendono da build firmware e rete.
- **WASM su device**: Caricamento e istanziazione WAMR ok; chiamata a funzioni esportate (es. `wamr_call_function`) opzionale nel main loop.

### wasmbed-infrastructure
- Binario **wasmbed-infrastructure** (CA, logging, monitoring, secret store): presente nel repo ma **non deployato** in K8s (commento in `k8s/deployments/wasmbed-deployments.yaml`). L’API server lo tratta come **opzionale**: se `WASMBED_API_SERVER_INFRASTRUCTURE_ENDPOINT` è vuoto (default), monitoring usa metriche fallback e `/api/v1/infrastructure/health` restituisce `"status": "not_configured"`. I suggerimenti kubectl nel dashboard puntano a `wasmbed-gateway` invece di wasmbed-infrastructure.

---

## 4. Riepilogo azioni

| Azione | Elemento |
|--------|----------|
| **Eliminati** | `.dashboard.pid`, `doc/CURRENT_STATUS.md`, `README_TO_BE_DONE.MD`, `Dockerfile.device` |
| **Aggiornato** | `renode-scripts/test_ethernet_connection.resc` (riferimento wasmbed-device-runtime sostituito con path firmware Zephyr / messaggio) |
| **Completato** | Codice morto gateway e device-controller rimosso; wasmbed-protocol-server rimosso dal workspace ed eliminato. |
| **Completato** | API server: infrastructure opzionale (endpoint vuoto = fallback metrics, health `not_configured`); suggerimenti kubectl aggiornati (gateway, application-controller). |
| **Completato** | Dashboard: rimosso `App.tsx` duplicato (entry point è `App.js`). Root `Dockerfile.gateway`: EXPOSE corretto a 8080 8081. |
