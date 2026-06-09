# Guida ai Test — OCRE/Edge Integration

## Scope

Questo documento copre la procedura di test per le modifiche introdotte nelle
sessioni 1-3 del lavoro OCRE/Edge:

| Area | Modifica | Dove |
|---|---|---|
| Firmware MCU | `WAMR_BUILD_LIBC_WASI 0 → 1` | `zephyr-app/CMakeLists.txt:25` |
| CRD estese | `TargetRuntime`, `DeviceClass`, `LinuxArm64/X86_64/RiscV` | `wasmbed-k8s-resource`, `wasmbed-qemu-manager` |
| Propagazione config | `ApplicationConfig` CRD → CBOR | `wasmbed-gateway/src/http_api.rs` |
| Nuovo daemon Linux | `wasmbed-edge-client` | `crates/wasmbed-edge-client/` |

Le fasi sono indipendenti: la Fase A non richiede cluster K8s, la Fase C
richiede Zephyr SDK installato.

---

## Prerequisiti comuni

```bash
cd /home/ubuntu/Thesis/retrospect

# Verifica compilazione workspace (deve finire senza errori)
cargo check --workspace
```

---

## FASE A — Test `wasmbed-edge-client` (nessun cluster necessario)

### A1. Build del binario

```bash
cargo build -p wasmbed-edge-client
# Binario: target/debug/wasmbed-edge-client
```

**Verifica**: nessun errore di compilazione. Il crate usa `wasmtime 18.0` con
la nuova API `preview2`; se compare un errore su `add_to_linker_sync` o
`WasiPreview1View` significa che la versione di wasmtime nel Cargo.lock è
diversa dalla 18.0 — verificare con `cargo tree -p wasmtime`.

### A2. Genera una chiave Ed25519 di test

Il daemon accetta la chiave pubblica Ed25519 raw a 32 byte codificata in
esadecimale. Per generarla:

```bash
# Genera keypair Ed25519
openssl genpkey -algorithm ed25519 -out /tmp/device-key.pem
openssl pkey -in /tmp/device-key.pem -pubout -out /tmp/device-pub.pem

# Estrai i 32 byte raw della chiave pubblica
# (il DER di una chiave pubblica Ed25519 ha 12 byte di header + 32 byte di chiave)
PUBKEY_HEX=$(openssl pkey -in /tmp/device-pub.pem -pubin -outform DER \
  | tail -c 32 | xxd -p -c 32)
echo "Public key (hex): $PUBKEY_HEX"
```

Nota: `tail -c 32` funziona perché OpenSSL Ed25519 SubjectPublicKeyInfo DER ha
sempre esattamente 44 byte (12 header + 32 raw key). Se vuoi verifica:
`openssl pkey -in /tmp/device-pub.pem -pubin -outform DER | wc -c` deve dare 44.

### A3. Test connessione rifiutata (smoke test)

Con il gateway non in ascolto, verifica che il binario si avvii e tenti la
connessione:

```bash
RUST_LOG=info ./target/debug/wasmbed-edge-client \
  --gateway 127.0.0.1:8081 \
  --public-key "$PUBKEY_HEX"
```

**Output atteso**:
```
WARN TLS: skipping server certificate verification (development mode)
ERROR TCP connect to 127.0.0.1:8081: Connection refused (os error 111)
```

Il fatto che esca con errore TCP (e non un panic o errore di parsing) conferma
che argomenti e bootstrap funzionano correttamente.

---

## FASE B — Enrollment device Linux su cluster K8s

### B1. Deploy piattaforma Wasmbed

Richiede Docker e registry locale su porta 5000:

```bash
# Avvia registry locale se non attivo
docker run -d -p 5000:5000 --name registry registry:2 2>/dev/null || true

# Deploy completo (build immagini + manifest K8s)
bash scripts/deploy-k3s.sh
```

**Atteso**: tutti i pod in `Running` entro ~2 minuti.

```bash
kubectl get pods -n wasmbed
# NAME                                         READY   STATUS    RESTARTS
# wasmbed-infrastructure-xxx                   1/1     Running   0
# wasmbed-gateway-xxx                          1/1     Running   0
# wasmbed-api-server-xxx                       1/1     Running   0
# wasmbed-device-controller-xxx                1/1     Running   0
# wasmbed-application-controller-xxx           1/1     Running   0
# wasmbed-gateway-controller-xxx               1/1     Running   0
```

### B2. Estrai il certificato CA del gateway

Il daemon edge-client usa questo certificato per verificare il gateway TLS in
modalità produzione. In sviluppo puoi omettere `--ca-cert` e usare il
connettore no-verify.

```bash
kubectl get secret -n wasmbed gateway-certificates \
  -o jsonpath='{.data.ca\.crt}' | base64 -d > /tmp/gateway-ca.crt
```

Se il secret si chiama diversamente:
```bash
kubectl get secrets -n wasmbed | grep -i cert
```

### B3. Trova la porta TLS del gateway

```bash
# Se il gateway ha un NodePort:
GATEWAY_PORT=$(kubectl get svc -n wasmbed wasmbed-gateway \
  -o jsonpath='{.spec.ports[?(@.name=="tls")].nodePort}')
echo "Gateway TLS port: $GATEWAY_PORT"

# Se non ha NodePort, usa port-forward in un terminale separato:
# kubectl port-forward -n wasmbed svc/wasmbed-gateway 8081:8081
# e poi usa --gateway 127.0.0.1:8081
```

### B4. Crea il Device CRD per il device Linux

Sostituisci `<PUBKEY_HEX>` con la chiave generata in A2:

```bash
cat > /tmp/linux-device.yaml <<EOF
apiVersion: wasmbed.io/v1alpha1
kind: Device
metadata:
  name: linux-edge-01
  namespace: wasmbed
spec:
  publicKey: "$(echo $PUBKEY_HEX)"
  mcuType: LinuxArm64
  deviceClass: MpuRich
EOF

kubectl apply -f /tmp/linux-device.yaml
kubectl get device -n wasmbed linux-edge-01
```

**Atteso**: device in fase `Pending` o `Registered`.

### B5. Avvia il daemon edge-client

```bash
RUST_LOG=info ./target/debug/wasmbed-edge-client \
  --gateway 127.0.0.1:${GATEWAY_PORT} \
  --public-key "$PUBKEY_HEX" \
  --ca-cert /tmp/gateway-ca.crt
```

In modalità sviluppo (senza CA verificata):

```bash
RUST_LOG=info ./target/debug/wasmbed-edge-client \
  --gateway 127.0.0.1:${GATEWAY_PORT} \
  --public-key "$PUBKEY_HEX"
```

**Output atteso** (log in ordine):
```
INFO  TLS: skipping server certificate verification (development mode)
INFO  TLS connected to 127.0.0.1:<port>
INFO  Enrollment: sending EnrollmentRequest
INFO  Enrollment: accepted by gateway
INFO  Enrollment: sending PublicKey (32 bytes)
INFO  Enrollment: received UUID <uuid>
INFO  Enrollment: sending EnrollmentAcknowledgment
INFO  Enrollment: completed successfully
INFO  Enrolled successfully
DEBUG Sent heartbeat        ← ogni 25 secondi
```

### B6. Verifica stato CRD aggiornato

```bash
kubectl get device -n wasmbed linux-edge-01 -o yaml | grep -A5 "status:"
# phase: Enrolled   ← o Connected, a seconda della logica gateway
```

### B7. Test deploy applicazione WASM sul device Linux

Prepara un WASM minimo (hello-world WASI):

```bash
# Se hai Rust + wasm32-wasi target installato:
cat > /tmp/hello/src/main.rs <<'EOF'
fn main() {
    println!("Hello from WASM on Linux edge!");
}
EOF
# Compila e ottieni .wasm...

# Oppure usa il tool di test già presente nel workspace:
cargo build -p wasmbed-protocol-tool
```

Tramite API Server:
```bash
# Port-forward API server
kubectl port-forward -n wasmbed svc/wasmbed-api-server 3001:3001 &

# Deploy applicazione
curl -X POST http://localhost:3001/api/v1/applications \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello-wasm",
    "targetDevices": ["linux-edge-01"],
    "wasmBinary": "<BASE64_WASM>"
  }'
```

**Atteso nei log del daemon**:
```
INFO  Deploying application 'hello-wasm' (<app-id>)
INFO  Running WASM app '<app-id>'
Hello from WASM on Linux edge!
INFO  WASM app '<app-id>' finished
```

---

## FASE C — Firmware Zephyr con `WAMR_BUILD_LIBC_WASI=1`

Richiede: Zephyr SDK installato, variabile `ZEPHYR_BASE` configurata.

### C1. Verifica modifica CMakeLists.txt

```bash
grep "WAMR_BUILD_LIBC_WASI" zephyr-app/CMakeLists.txt
# set(WAMR_BUILD_LIBC_WASI 1)   ← deve essere 1
```

### C2. Build firmware

```bash
cd zephyr-app
west build -p always -b stm32f746g_disco -- -DCONF_FILE=prj.conf
```

**Risultati effettivi (sessione 2026-05-07)**:
```bash
size build/zephyr/zephyr.elf
#    text    data     bss     dec
#  395440   10044  243642  649126
```

| Sezione | Valore | Limite | Stato |
|---|---|---|---|
| `.text` (Flash) | 387 KB | 1 MB | ✓ OK (38.67%) |
| `.data + .bss` (RAM) | 248 KB | 256 KB | ✓ OK (91.72%) — tight ma funzionale |

**Nota RAM**: il 91.72% include il WAMR heap pool (64 KB) e MbedTLS heap
(16 KB) pre-allocati come BSS — non competono con lo heap di sistema a runtime.

### C3. Avvia su Renode via API Server

Con il cluster K8s attivo:

```bash
curl -X POST http://localhost:3001/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "stm32-wasi-test",
    "mcuType": "Stm32F746gDisco",
    "gatewayEndpoint": "192.168.100.179:30443"
  }'
```

**Atteso**: device enroll → log Zephyr mostrano `[WAMR] WASI initialized` o
simile, il device raggiunge lo stato `Connected`.

### C4. Verifica WASI runtime nel firmware

Dopo enrollment, deploya un WASM che usa socket WASI:

```bash
# Il hello-wasm in MasterThesis/hello-wasm/ usa socket TCP WASI
# Compila e deploya sullo STM32:
curl -X POST http://localhost:3001/api/v1/applications \
  -H "Content-Type: application/json" \
  -d '{
    "name": "tcp-hello",
    "targetDevices": ["stm32-wasi-test"],
    "wasmBinary": "<BASE64_WASM_TCP>"
  }'
```

---

## Tabella riassuntiva verifica

| Test | Comando verifica | Risultato atteso |
|---|---|---|
| A1 build edge-client | `cargo build -p wasmbed-edge-client` | 0 errori |
| A3 smoke test | `./wasmbed-edge-client --gateway 127.0.0.1:8081 ...` | `Connection refused` (non panic) |
| B1 deploy cluster | `kubectl get pods -n wasmbed` | Tutti `Running` |
| B5 enrollment Linux | log daemon | `Enrolled successfully` |
| B6 status CRD | `kubectl get device ... -o yaml` | `phase: Enrolled` |
| B7 deploy WASM | log daemon | `WASM app finished` |
| C2 build Zephyr | `west build` | 0 errori, ELF size OK | ✓ text=387 KB, RAM=248 KB |
| C4 WASI socket | log Renode | app WASM TCP eseguita |

---

## Cleanup ambiente K8s (pre-requisito per test ripetibili)

Il deploy script crea sia un deployment statico `wasmbed-gateway` che uno
gestito dal gateway-controller (`gateway-1-deployment`). Entrambi condividono
il label `app: wasmbed-gateway` e causano load-balancing non deterministico.

Fix applicato (permanente nei manifest):
- Rimosso il deployment statico `wasmbed-gateway` da `k8s/deployments/wasmbed-deployments.yaml`
- Il service `wasmbed-gateway` ora punta solo al pod del controller
- Corretto `targetPort` del service da 8081 a 8443 (il controller-managed pod ascolta su 8443)

Per ricreare l'ambiente da zero in modo pulito:
```bash
bash scripts/deploy-k3s.sh
# Verifica: deve esserci UN SOLO pod gateway
kubectl get pods -n wasmbed | grep gateway
# gateway-1-deployment-xxxxx   1/1   Running   ← solo questo
```

Abilitare pairing mode prima di ogni enrollment:
```bash
kubectl port-forward -n wasmbed svc/wasmbed-gateway 8080:8080 8081:8081 &
curl -s -X POST http://localhost:8080/api/v1/admin/pairing-mode \
  -H "Content-Type: application/json" -d '{"enabled": true}'
```

---

## Problemi trovati durante i test Fase C — Zephyr 3.5 + WAMR_BUILD_LIBC_WASI=1

| Problema | Causa | Fix applicato |
|---|---|---|
| `IP_TTL` / `IP_MULTICAST_TTL` undeclared | Non definiti in Zephyr 3.5 `<net/socket.h>` | `add_compile_definitions(IP_TTL=2 IP_MULTICAST_TTL=33)` in CMakeLists.txt |
| `IP_MULTICAST_LOOP`, `ip_mreq`, `ip_mreqn` undeclared | Non presenti in Zephyr 3.5 socket headers | Stub `#ifndef` + definizione struct in `zephyr_socket.c` |
| `lfs.h: No such file or directory` | WAMR include `<zephyr/fs/littlefs.h>` incondizionalmente | `#ifdef CONFIG_FILE_SYSTEM_LITTLEFS` guard in `zephyr_file.c` |
| `CONFIG_ZVFS_OPEN_MAX undeclared` | Macro introdotta in Zephyr 3.6, non presente in 3.5 | `#ifndef CONFIG_ZVFS_OPEN_MAX / #define CONFIG_ZVFS_OPEN_MAX 16` in `zephyr_file.c` |
| `invalid use of incomplete typedef 'os_timespec'` | `struct timespec` non definita in picolibc senza `<time.h>` | `#include <time.h>` aggiunto in `platform_internal.h` prima della typedef |
| `fs_stat / fs_open undefined reference` | Layer file system Zephyr non abilitato | `CONFIG_FILE_SYSTEM=y` in `prj.conf` |
| `struct timespec` incomplete in `posix.c` | `CONFIG_POSIX_API=n` bloccava la definizione completa | `CONFIG_POSIX_API=y` in `prj.conf` |

**Tutti risolti**: build riuscito senza errori (solo warning non critici).

---

## Problemi trovati durante i test (sessione 2026-05-07)

| Problema | Causa | Fix applicato |
|---|---|---|
| `NoCipherSuitesInCommon` durante TLS handshake | Gateway configurato con cipher ECDSA (`TLS_ECDHE_ECDSA`) ma certificati RSA | Fix in `wasmbed-tls-utils/src/lib.rs`: aggiunto `TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256` |
| Port-forward al service fa round-robin tra due pod | `gateway-1-deployment` e `wasmbed-gateway` condividono label `app: wasmbed-gateway` | Usare `kubectl port-forward pod/<nome-pod>` invece del service |
| `EnrollmentRejected: Pairing mode disabled` | Il gateway non accetta nuovi device in modalità normale | Chiamare `POST /api/v1/admin/pairing-mode {"enabled": true}` sul pod corretto prima dell'enrollment |
| `application-crd.yaml` con output cargo nelle prime righe | Il file era stato generato con stdout del tool incluso | Rimosso con `tail -n +3` |
| `zephyr-workspace/build/*/zephyr.elf` mancanti | Dockerfile.api-server richiede firmware ELF pre-compilati | Creati file stub `touch zephyr.elf` nelle directory attese |

---

## Troubleshooting

### Enrollment fallisce con "EnrollmentRejected"

Il gateway non trova un Device CRD con la chiave pubblica fornita. Verifica:
```bash
kubectl get device -n wasmbed -o yaml | grep publicKey
```
La chiave deve essere la stessa stringa hex passata a `--public-key`.

### TLS handshake fallisce

- Se usi `--ca-cert`: verifica che il PEM sia il CA del gateway, non il suo
  certificato server.
- Senza `--ca-cert`: il connettore no-verify dovrebbe sempre funzionare se
  il gateway è in ascolto.

### TLS handshake eof (server chiude prima di rispondere)

Il service K8s fa round-robin tra più pod. Se c'è un pod con il vecchio codice
(es. `gateway-1-deployment`) la connessione può finire sul pod sbagliato.
Soluzione: port-forward diretto al pod, non al service:

```bash
# Trova il pod corretto (il deployment principale, non gateway-controller)
kubectl get pods -n wasmbed | grep wasmbed-gateway
# Porta entrambe le porte allo stesso pod
kubectl port-forward -n wasmbed pod/<nome-pod> 8080:8080 &
kubectl port-forward -n wasmbed pod/<nome-pod> 8081:8081 &
```

### `WasiPreview1View` non trovato (build edge-client)

La versione di `wasmtime-wasi` nel Cargo.lock potrebbe non essere 18.0. Forza:
```bash
cargo update -p wasmtime-wasi --precise 18.0.4
```

### Firmware Zephyr — heap insufficiente con WASI=1

WAMR WASI aggiunge ~10-20 KB di memoria di lavoro. Se il device va in fault:
aumentare `wamr_heap_buf` in `zephyr-app/src/wamr_integration.c` da 48 KB a
64 KB e fare un nuovo build.

### `mcuType: LinuxArm64` non accettato dalla CRD

Verificare che la CRD sia la versione rigenerata:
```bash
kubectl get crd devices.wasmbed.io -o jsonpath='{.spec.versions[0].schema}' \
  | python3 -m json.tool | grep -A5 "mcuType"
```
Se mancano le varianti Linux, rigenera:
```bash
cargo run -p wasmbed-k8s-resource-tool -- crd device > k8s/crds/device-crd.yaml
kubectl apply -f k8s/crds/device-crd.yaml
```
