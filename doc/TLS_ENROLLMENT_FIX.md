# Fix TLS e Enrollment Firmware-Gateway

Questo documento descrive i bug trovati e le correzioni applicate per far
funzionare la connessione TLS e il protocollo di enrollment tra il firmware
Zephyr (lato dispositivo) e il Gateway (lato cluster Kubernetes).

---

## Contesto

Il flusso atteso è:

```
Firmware Zephyr (Renode)          Gateway (Kubernetes pod)
       │                                    │
       │  ─── TLS handshake ──────────────► │  :8443
       │                                    │
       │  ─── EnrollmentRequest ──────────► │  CBOR: 81 01
       │  ◄── EnrollmentAccepted ────────── │  CBOR: 81 01
       │  ─── PublicKey { key } ──────────► │  CBOR: 82 02 58 20 <32 bytes>
       │  ◄── DeviceUuid { uuid } ───────── │  CBOR: 82 03 50 <16 bytes>
       │  ─── EnrollmentAcknowledgment ───► │  CBOR: 81 03
       │  ◄── EnrollmentCompleted ───────── │  CBOR: 81 04
       │                                    │
       │  ─── Heartbeat ─────────────────► │  CBOR: 81 00  (ogni 25s)
       │  ◄── HeartbeatAck ──────────────── │  CBOR: 81 00
```

Tutti i messaggi usano il **wire format**: 4 byte big-endian (lunghezza payload) + payload CBOR.

Il Gateway ascolta su `:8443` (TLS) e `:8080` (HTTP API admin).  
Pairing mode va abilitata prima dell'enrollment: `POST /api/v1/admin/pairing-mode {"enabled": true}`.

---

## Bug trovati e corretti

### Bug 1 — Socket TLS sbagliato nel firmware

**File:** `zephyr-app/src/network_handler.c`

`network_connect_tls()` creava il socket con `IPPROTO_TCP` invece di
`IPPROTO_TLS_1_2`: il TLS non veniva mai attivato. La `setsockopt` per
`TLS_HOSTNAME` era anche incorretta (applicata a socket non-TLS).

```c
// PRIMA (rotto)
socket_fd = zsock_socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
zsock_setsockopt(socket_fd, SOL_TLS, TLS_HOSTNAME, host, strlen(host) + 1);

// DOPO (corretto)
socket_fd = zsock_socket(AF_INET, SOCK_STREAM, IPPROTO_TLS_1_2);
int verify = TLS_PEER_VERIFY_NONE;
zsock_setsockopt(socket_fd, SOL_TLS, TLS_PEER_VERIFY, &verify, sizeof(verify));
```

`TLS_PEER_VERIFY_NONE` è appropriato in fase di sviluppo (il gateway usa un
certificato self-signed). In produzione sostituire con `TLS_PEER_VERIFY_REQUIRED`
e caricare il CA cert tramite `TLS_SEC_TAG_LIST`.

---

### Bug 2 — Sequenza di enrollment mancante nel firmware

**File:** `zephyr-app/src/wasmbed_protocol.c`

Dopo la connessione TLS il firmware si limitava ad aspettare i messaggi del
gateway, senza mai inviare `EnrollmentRequest`. Il gateway quindi non sapeva
che il device voleva fare l'enrollment e non rispondeva.

È stata aggiunta la funzione `do_enrollment()` chiamata subito dopo
`network_connect_tls()`:

```c
if (network_connect_tls(host, port) == 0) {
    gateway_connected = true;
    if (do_enrollment() != 0) {
        LOG_WRN("Enrollment failed - will continue with heartbeats only");
    }
}
```

`do_enrollment()` invia l'intera sequenza:
1. Manda `EnrollmentRequest` (`0x81 0x01`)
2. Attende `EnrollmentAccepted` (`0x81 0x01`)
3. Legge la chiave pubblica del device da `0x20002000` (scritta da Renode/qemu-manager),
   con fallback a 32 byte `0xAB` se non disponibile
4. Manda `PublicKey { key }` (`0x82 0x02 0x58 0x20 <32 bytes>`)
5. Attende `DeviceUuid` (`0x82 0x03 0x50 <16 bytes>`)
6. Manda `EnrollmentAcknowledgment` (`0x81 0x03`)
7. Attende `EnrollmentCompleted` (`0x81 0x04`) — tollerato se assente

---

### Bug 3 — Gateway rifiutava la chiave pubblica durante enrollment

**File:** `crates/wasmbed-gateway/src/main.rs`

Nel handler `ClientMessage::PublicKey`, il gateway confrontava la chiave TLS
del client (`ctx.client_public_key()`) con la chiave inviata nel messaggio.
Poiché il server TLS è configurato con `with_no_client_auth()` (nessuna
autenticazione mTLS), la chiave TLS del client è sempre un vettore vuoto
→ il confronto falliva sempre → `EnrollmentRejected`.

```rust
// PRIMA (rotto)
if tls_public_key_obj != message_public_key {
    // inviava sempre EnrollmentRejected
}

// DOPO (corretto)
if !tls_public_key_bytes.is_empty() && tls_public_key_obj != message_public_key {
    // il check viene saltato quando il client non ha un certificato TLS
}
```

---

### Bug 4 — `mcuType` in minuscolo/formato sbagliato

**File:** `crates/wasmbed-gateway/src/main.rs`, funzione `create_device_crd()`

Il Device CRD veniva creato con `mcuType: "mps2-an385"`, ma il validatore
Kubernetes richiede un valore PascalCase dall'enum (`"Stm32F746gDisco"`, ecc.).

```rust
// PRIMA
mcu_type: Some("mps2-an385".to_string()),

// DOPO
mcu_type: Some("Stm32F746gDisco".to_string()),
```

---

### Bug 5 — TLS connection loop non funzionante (bug architetturale)

**File:** `crates/wasmbed-tls-utils/src/lib.rs`, `handle_tls_connection()`

Tre problemi combinati nella gestione della connessione:

1. **Il canale `rx` veniva droppato immediatamente**: `on_connection_ready`
   riceveva l'unica copia del sender `tx`. Se non c'era un `device_id` noto
   (caso enrollment — device nuovo), il sender veniva scartato e il loop
   si chiudeva.

2. **`ctx.reply_fn` non veniva mai impostato**: il handler del gateway chiamava
   `ctx.reply(msg)` ma la `reply_fn` era `None` → le risposte sparivano
   silenziosamente.

3. **Messaggi letti senza strippare il prefisso 4-byte di lunghezza**: il CBOR
   decode falliva perché leggeva `00 00 00 02 81 01` invece di `81 01`.

La riscrittura separa il loop di lettura dal loop di scrittura tramite
`tokio::io::split`, usa framing corretto e imposta `reply_fn` su ogni context:

```rust
// Split stream: reader e writer indipendenti
let (mut reader, mut writer) = tokio::io::split(tls_stream);

// Writer task: consuma la rx queue e manda al client
tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
        let cbor = minicbor::to_vec(&msg)?;
        writer.write_all(&(cbor.len() as u32).to_be_bytes()).await?;
        writer.write_all(&cbor).await?;
    }
});

// Reader loop: legge frame (4-byte len + CBOR) e imposta reply_fn
loop {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let payload = read_exact(msg_len).await?;
    let mut ctx = MessageContextWithKey::new(...);
    ctx.set_reply_fn(Box::new(move |msg| tx_for_read.try_send(msg)));
    (on_client_message)(ctx).await;
}
```

---

### Bug 6 — Renode container non apriva la porta TCP del monitor

**File:** `crates/wasmbed-qemu-manager/src/lib.rs`, `ensure_renode_container_running()`

Il comando di avvio usava `--disable-gui` insieme a `-P 9999`.
Dalla documentazione Renode: *"`--disable-gui` automatically sets HideMonitor"*,
che disabilita il TCP monitor — quindi la porta 9999 non veniva mai aperta.

```rust
// PRIMA (rotto)
"sleep infinity | renode --console --disable-gui -P 9999"

// DOPO (corretto)
"renode -P 9999"
```

`-P` da solo dice a Renode di aprire il monitor su TCP invece che aprire una
finestra, senza toccare il flag `HideMonitor`.

---

## Verifica

Per testare l'enrollment end-to-end senza avviare Renode si può usare lo
script `scripts/test_enrollment.py`:

```bash
# 1. Abilita pairing mode sul gateway
curl -X POST http://<gateway-http-svc>:8080/api/v1/admin/pairing-mode \
     -H 'Content-Type: application/json' -d '{"enabled":true}'

# 2. Esegui il test
python3 scripts/test_enrollment.py
```

Output atteso:
```
TLS handshake complete
Step 1: EnrollmentRequest        → sent (2 CBOR bytes): 8101
Step 2: Waiting for accepted...  ← recv: 8101  → EnrollmentAccepted ✓
Step 3: Send PublicKey           → sent (36 CBOR bytes)
Step 4: Waiting for DeviceUuid   ← recv: 820350...  → DeviceUuid: <uuid> ✓
Step 5: EnrollmentAcknowledgment → sent
Step 6: EnrollmentCompleted      ← recv: 8104  ✓
Step 7: Heartbeat                → sent / ← HeartbeatAck ✓
```

Lo script richiede solo la stdlib Python (nessuna dipendenza esterna).

---

## File modificati

| File | Tipo di fix |
|------|-------------|
| `zephyr-app/src/network_handler.c` | Bug 1: socket TLS |
| `zephyr-app/src/wasmbed_protocol.c` | Bug 2: sequenza enrollment |
| `crates/wasmbed-gateway/src/main.rs` | Bug 3: check chiave vuota + Bug 4: mcuType |
| `crates/wasmbed-tls-utils/src/lib.rs` | Bug 5: connection loop |
| `crates/wasmbed-qemu-manager/src/lib.rs` | Bug 6: Renode startup |
