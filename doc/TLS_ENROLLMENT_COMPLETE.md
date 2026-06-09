# Connessione TLS e Enrollment Firmware-Gateway: guida completa

Questo documento descrive tutte le modifiche effettuate per far funzionare
la connessione TLS e il protocollo di enrollment tra il firmware Zephyr (che
gira in Renode) e il Gateway (che gira come pod Kubernetes). Il documento e'
pensato per chi deve riprendere il lavoro o capire cosa e' stato cambiato.

---

## Il flusso di comunicazione

Il firmware Zephyr, una volta avviato in Renode, esegue questa sequenza:

```
Firmware (Renode/STM32F746G)      Gateway (pod K8s, porta 8443)
        |                                      |
        |  --- TLS handshake ----------------> |
        |                                      |
        |  --- EnrollmentRequest  -----------> |  CBOR: 81 01
        |  <-- EnrollmentAccepted ------------ |  CBOR: 81 01
        |  --- PublicKey { 32 bytes } -------> |  CBOR: 82 02 58 20 <32 byte>
        |  <-- DeviceUuid { 16 bytes } -------- |  CBOR: 82 03 50 <16 byte>
        |  --- EnrollmentAcknowledgment ------> |  CBOR: 81 03
        |  <-- EnrollmentCompleted ------------ |  CBOR: 81 04
        |                                      |
        |  --- Heartbeat (ogni 25s) ----------> |  CBOR: 81 00
        |  <-- HeartbeatAck  ----------------- |  CBOR: 81 00
```

Tutti i messaggi seguono il wire format: 4 byte big-endian (lunghezza del
payload CBOR) + payload CBOR.

Il Gateway ascolta su due porte:
- `:8443` per le connessioni TLS dai dispositivi (NodePort esterno: 30443)
- `:8080` per le API HTTP di amministrazione (NodePort esterno: 31834)

---

## Prerequisiti per la rete emulata

Il device emulato gira in un container Docker con accesso alla rete host
tramite un'interfaccia TAP. Sul sistema host devono essere attive le seguenti
configurazioni (vanno ripetute dopo ogni riavvio del sistema o del container):

```bash
# Aggiungere IP all'interfaccia tap0 (creata da Renode all'avvio del container)
sudo ip addr add 10.0.86.1/24 dev tap0 2>/dev/null
sudo ip link set tap0 up

# Abilitare il forwarding IP
sudo sysctl -w net.ipv4.ip_forward=1

# NAT e forwarding tra tap0 e l'interfaccia del cluster (cni0 o simile)
sudo iptables -t nat -A POSTROUTING -s 10.0.86.0/24 -o cni0 -j MASQUERADE
sudo iptables -A FORWARD -i tap0 -o cni0 -j ACCEPT
sudo iptables -A FORWARD -i cni0 -o tap0 -m state --state RELATED,ESTABLISHED -j ACCEPT
```

Il DHCP per il device viene fornito da dnsmasq, avviato automaticamente dal
container Renode (configurato con `--dhcp-option=3,10.0.86.1` per il gateway
IP). Il device riceve un indirizzo nella subnet `10.0.86.100-200/24`.

Il container Renode si avvia con questi parametri:

```bash
docker run -dt \
  --net=host \
  --cap-add=NET_ADMIN \
  --device=/dev/net/tun \
  --name wasmbed-renode-device-b341efda2 \
  -v /path/to/renode-scripts:/scripts:ro \
  -v wasmbed-firmware-store:/firmware:ro \
  antmicro/renode:nightly renode --plain /scripts/device-<id>.resc
```

---

## Pairing mode

Prima di fare l'enrollment di un nuovo dispositivo, il gateway deve avere la
pairing mode abilitata. Si abilita con una chiamata HTTP:

```bash
curl -X POST http://<host>:31834/api/v1/admin/pairing-mode \
  -H 'Content-Type: application/json' \
  -d '{"enabled": true}'
```

La pairing mode permette al gateway di accettare dispositivi nuovi (che non
hanno ancora una chiave registrata). Senza di essa, il gateway rifiuta
l'enrollment con un errore.

---

## Certificati TLS del Gateway

### Problema originale

Il gateway usava un certificato RSA a 4096 bit. MbedTLS sul firmware (Zephyr)
ha un heap dedicato di 16 KB: durante l'handshake TLS con RSA-4096, MbedTLS
esauriva la memoria e l'handshake falliva con errore `-0x7100`
(`MBEDTLS_ERR_SSL_CONN_EOF`). Il firmware riceveva la chiusura della
connessione prima ancora di completare il TLS.

### Soluzione: migrazione a ECDSA P-256

Sono stati generati nuovi certificati ECDSA P-256 (curva `prime256v1`). Questo
tipo di chiave e' circa 10 volte piu' piccolo di RSA-4096 e richiede molta meno
memoria durante l'handshake.

Procedura di generazione (da eseguire solo se si devono rigenerare i certificati):

```bash
# CA ECDSA P-256
openssl ecparam -name prime256v1 -genkey -noout -out ca-key.pem
openssl req -new -x509 -key ca-key.pem -out ca-cert.pem -days 3650 \
  -subj "/CN=Wasmbed CA"

# Chiave server ECDSA P-256
openssl ecparam -name prime256v1 -genkey -noout -out server-key-ec.pem
# Conversione in PKCS8 (richiesta da rustls, la libreria TLS del gateway)
openssl pkcs8 -topk8 -nocrypt -in server-key-ec.pem -out server-key.pem

# Certificato server firmato dalla CA, con i SAN richiesti
openssl req -new -key server-key.pem -out server.csr \
  -subj "/CN=wasmbed-gateway"
# Creare il file v3.ext con i Subject Alternative Names
cat > v3.ext << EOF
[v3_server]
subjectAltName = DNS:localhost, DNS:wasmbed-gateway, IP:127.0.0.1, IP:192.168.100.179, IP:10.42.0.1
EOF
openssl x509 -req -in server.csr -CA ca-cert.pem -CAkey ca-key.pem \
  -CAcreateserial -out server-cert.pem -days 365 -sha256 \
  -extfile v3.ext -extensions v3_server
cp ca-cert.pem client-ca.pem
```

I certificati risultanti sono in `config/certs/`:
- `ca-key.pem` — chiave privata della CA (non va distribuita)
- `ca-cert.pem` — certificato CA
- `server-key.pem` — chiave privata del server in formato PKCS8
- `server-cert.pem` — certificato server firmato dalla CA
- `client-ca.pem` — copia di `ca-cert.pem` usata per la validazione client

### Aggiornamento del secret Kubernetes

Dopo la rigenerazione, il secret va ricaricato nel cluster:

```bash
kubectl delete secret gateway-certificates -n wasmbed
kubectl create secret generic gateway-certificates -n wasmbed \
  --from-file=server-cert.pem=config/certs/server-cert.pem \
  --from-file=server-key.pem=config/certs/server-key.pem \
  --from-file=ca-cert.pem=config/certs/ca-cert.pem
kubectl rollout restart deployment/gateway-1-deployment -n wasmbed
```

### Verifica del certificato attivo

```bash
openssl s_client -connect 192.168.100.179:30443 -showcerts </dev/null 2>/dev/null \
  | openssl x509 -noout -text | grep -E "PKEY|sigalg|Subject:"
# Risultato atteso: id-ecPublicKey, 256 bit, ecdsa-with-SHA256
```

---

## Modifiche al Gateway (Rust)

### Cipher suite TLS

**File:** `crates/wasmbed-tls-utils/src/lib.rs`

Il gateway usa rustls con una cipher suite esplicita. Cambiata da RSA a ECDSA:

```rust
// Prima
TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256

// Dopo
TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
```

### Gestione connessioni anonime

**File:** `crates/wasmbed-gateway/src/main.rs`

Prima dell'enrollment, il device si connette senza certificato TLS client
(il server e' configurato con `with_no_client_auth()`), quindi la chiave
pubblica estratta dalla connessione TLS e' vuota. Il callback `on_connect`
rifiutava le connessioni con chiave vuota (`Unauthorized`).

Correzione: le connessioni anonime (chiave vuota) vengono accettate. Il device
e' identificato nell'enrollment successivo tramite CBOR.

```rust
// Adesso: chiave vuota = accettato come connessione anonima
if public_key.is_empty() {
    debug!("Anonymous TLS connection accepted; awaiting CBOR enrollment");
    return AuthorizationResult::Authorized;
}
```

### Sender in pending durante enrollment

**File:** `crates/wasmbed-gateway/src/http_api.rs`

Problema precedente: al momento dell'enrollment il canale `mpsc::Sender` per
inviare messaggi al device veniva scartato perche' non c'era ancora un
`device_id` noto. Il gateway non riusciva a rispondere all'enrollment.

Soluzione: il sender viene sempre archiviato in una mappa `pending_senders`
indicizzata per `connection_id` (indirizzo IP:porta). Quando arriva l'enrollment
e il `device_id` viene stabilito, il sender viene spostato dalla mappa pending
alla mappa permanente.

```rust
// In on_connection_ready: archiviato subito, prima che il device_id sia noto
http_server.store_pending_sender(&connection_id, sender.clone()).await;

// Il reply_fn del context usa il sender della connessione corrente
ctx.set_reply_fn(Box::new(move |msg| {
    tx_for_read.try_send(msg).map_err(|e| anyhow!("{}", e))
}));
```

### Dipendenza tracing-log

**File:** `crates/wasmbed-gateway/Cargo.toml`

Aggiunta la dipendenza `tracing-log = "0.2"` e la chiamata
`tracing_log::LogTracer::init()` all'avvio: permette al gateway di catturare i
log emessi da crate che usano il crate `log` (come `rustls`, `kube`) e
mostrarli attraverso il sistema `tracing`.

---

## Modifiche al firmware Zephyr

### Configurazione MbedTLS (prj.conf)

**File:** `zephyr-app/prj.conf`

Aggiunte le opzioni per la suite ECDSA:

```kconfig
# Suite ECDSA per la connessione con il gateway ECDSA P-256
CONFIG_MBEDTLS_KEY_EXCHANGE_ECDHE_ECDSA_ENABLED=y
CONFIG_MBEDTLS_ECDH_C=y
CONFIG_MBEDTLS_ECDSA_C=y
CONFIG_MBEDTLS_ECP_C=y
CONFIG_MBEDTLS_ECP_DP_SECP256R1_ENABLED=y
CONFIG_MBEDTLS_CIPHER_GCM_ENABLED=y
```

Le suite RSA sono state tenute abilitate (`CONFIG_MBEDTLS_KEY_EXCHANGE_RSA_ENABLED=y`,
`CONFIG_MBEDTLS_KEY_EXCHANGE_ECDHE_RSA_ENABLED=y`) perche' la rimozione causa
un crash del firmware all'avvio per ragioni legate all'ordine di inizializzazione
di Zephyr/MbedTLS.

Altre configurazioni importanti e il motivo per cui non vanno cambiate:

| Parametro | Valore | Motivo |
|-----------|--------|--------|
| `CONFIG_MBEDTLS_HEAP_SIZE` | 16384 (16 KB) | Sufficiente per ECDSA P-256; aumentare causa problemi MPU in Renode |
| `CONFIG_MAIN_STACK_SIZE` | 2048 (2 KB) | Valore originale; 4096 causa MPU fault in Renode/STM32F7 |
| `CONFIG_NET_TCP_WORKQ_STACK_SIZE` | 4096 (4 KB) | Necessario per le operazioni ECDHE nel workqueue TCP |

### Ricezione dei frame TLS (wasmbed_protocol.c)

**File:** `zephyr-app/src/wasmbed_protocol.c`

Problema: `zsock_recv` su Zephyr puo' restituire meno byte di quelli
disponibili nel frame. Un singolo `recv` restituiva solo i primi 4 byte
(l'header di lunghezza) senza il payload CBOR. Il firmware interpretava
questo come un frame incompleto e l'enrollment falliva.

Soluzione: aggiunta la funzione statica `recv_frame()` nella stessa
translation unit. La funzione fa il loop su piu' chiamate `network_receive`
fino ad aver accumulato prima i 4 byte dell'header, poi il numero di byte
del payload indicato nell'header. Usa `k_uptime_get()` per il timeout.

```c
static int recv_frame(uint8_t *buf, uint32_t buf_len,
                      uint32_t *total_len, int timeout_ms)
{
    uint32_t got = 0;
    int64_t deadline = k_uptime_get() + timeout_ms;

    // Accumula i 4 byte dell'header
    while (got < 4) { ... }

    // Legge la lunghezza del payload dall'header BE
    uint32_t payload_len = (buf[0]<<24)|(buf[1]<<16)|(buf[2]<<8)|buf[3];

    // Accumula il payload
    while (got < 4 + payload_len) { ... }

    *total_len = 4 + payload_len;
    return 0;
}
```

Tutti e tre i punti di ricezione in `do_enrollment()` usano ora questa
funzione al posto della singola chiamata `network_receive`.

Nota: esiste anche `network_receive_framed()` in `network_handler.c` con la
stessa logica, ma causava sporadici MPU fault nella simulazione Renode/STM32F7.
L'implementazione inline in `wasmbed_protocol.c` non presenta questo problema.

### Script Renode per il device (device-b341efda21704392bc3f8d6794c7a0f2.resc)

**File:** `zephyr-workspace/renode-scripts/device-b341efda21704392bc3f8d6794c7a0f2.resc`

Lo script scrive in memoria del device prima dell'avvio:
- `0x20001000`: indirizzo del gateway (`192.168.100.179:30443`)
- `0x20002000`: chiave pubblica del device (usata nell'enrollment)

Il firmware legge queste informazioni alla partenza.

---

## Come buildare e deployare

### Build firmware Zephyr

```bash
cd /home/ubuntu/retrospect/zephyr-workspace
source /home/ubuntu/retrospect/.venv/bin/activate
west build -p always -b stm32f746g_disco /home/ubuntu/retrospect/zephyr-app
```

### Deploy firmware nel volume Docker

```bash
docker run --rm \
  -v wasmbed-firmware-store:/firmware \
  -v /home/ubuntu/retrospect/zephyr-workspace/build/zephyr:/src:ro \
  alpine cp /src/zephyr.elf \
    /firmware/device-b341efda21704392bc3f8d6794c7a0f2/zephyr.elf
```

### Avvio container Renode

```bash
docker rm -f wasmbed-renode-device-b341efda2 2>/dev/null
docker run -dt \
  --net=host --cap-add=NET_ADMIN --device=/dev/net/tun \
  --name wasmbed-renode-device-b341efda2 \
  -v /home/ubuntu/retrospect/zephyr-workspace/renode-scripts:/scripts:ro \
  -v wasmbed-firmware-store:/firmware:ro \
  antmicro/renode:nightly renode --plain \
    /scripts/device-b341efda21704392bc3f8d6794c7a0f2.resc
```

### Build e deploy Gateway

```bash
cd /home/ubuntu/retrospect
cargo build -p wasmbed-gateway --release
docker build -f Dockerfile.gateway -t localhost:5000/wasmbed/gateway:latest .
docker save localhost:5000/wasmbed/gateway:latest | sudo k3s ctr images import -
kubectl rollout restart deployment/gateway-1-deployment -n wasmbed
```

### Sequenza completa di test enrollment

```bash
# 1. Configurare la rete host (dopo ogni riavvio)
sudo ip addr add 10.0.86.1/24 dev tap0 2>/dev/null
sudo ip link set tap0 up

# 2. Abilitare pairing mode sul gateway
curl -X POST http://192.168.100.179:31834/api/v1/admin/pairing-mode \
  -H 'Content-Type: application/json' -d '{"enabled": true}'

# 3. Attendere ~30 secondi per boot, DHCP e TLS

# 4. Verificare i log firmware
docker logs wasmbed-renode-device-b341efda2 2>&1 \
  | grep "usart1" | grep -E "TLS|enrollment|Enrollment|completed"

# 5. Verificare che il device sia stato registrato in Kubernetes
kubectl get devices -n wasmbed
```

---

## Risultato finale verificato

Con tutte le modifiche applicate, il flusso completo funziona:

```
[firmware] Connected to gateway via TLS: 192.168.100.179:30443
[firmware] Sent EnrollmentRequest
[gateway]  Received enrollment request
[firmware] Enrollment accepted by gateway
[firmware] Sent PublicKey (32 bytes)
[gateway]  Received public key during enrollment: 32 bytes
[gateway]  Created Device CRD: device-bcdcc271230147188f087e80336ff404
[firmware] Received DeviceUuid from gateway
[firmware] Sent EnrollmentAcknowledgment
[gateway]  Enrollment completed successfully
[firmware] Enrollment completed successfully!
[firmware] Wasmbed Application Ready
[gateway]  Updated heartbeat for device device-bcdcc271230147188f087e80336ff404
```

Il device appare nella lista Kubernetes:

```bash
kubectl get devices -n wasmbed
# NAME                                        AGE
# device-bcdcc271230147188f087e80336ff404    <tempo>
```

---

## Riepilogo file modificati

| File | Tipo modifica |
|------|---------------|
| `zephyr-app/prj.conf` | Aggiunte opzioni cipher ECDSA, tolte dipendenze RSA non necessarie |
| `zephyr-app/src/wasmbed_protocol.c` | Aggiunta `recv_frame()`, sostituita la ricezione in `do_enrollment()` |
| `zephyr-app/src/network_handler.c` | Aggiunta `network_receive_framed()` (non usata — vedi nota sopra) |
| `zephyr-app/src/network_handler.h` | Aggiunta dichiarazione `network_receive_framed()` |
| `crates/wasmbed-tls-utils/src/lib.rs` | Cipher suite ECDSA; callback con `connection_id`; reader/writer loop TLS |
| `crates/wasmbed-gateway/src/main.rs` | Connessioni anonime; handler enrollment; idempotenza CRD |
| `crates/wasmbed-gateway/src/http_api.rs` | `pending_senders`, `connection_to_device`, `resolve_device_id` |
| `crates/wasmbed-gateway/Cargo.toml` | Aggiunto `tracing-log = "0.2"` |
| `config/certs/` | Nuovi certificati ECDSA P-256 (sostituiscono RSA-4096) |
