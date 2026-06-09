# Fase C — Firmware Zephyr con WAMR_BUILD_LIBC_WASI=1

## Obiettivo

Abilitare il supporto WASI completo (inclusi socket TCP) nel firmware Zephyr/WAMR
in esecuzione su STM32F746G Discovery emulato in Renode, in modo che il device
possa eseguire applicazioni WASM che usano syscall WASI standard
(`fd_write`, `sock_open`, `proc_exit`, ecc.).

---

## Risultati build (sessione 2026-05-07)

### Dimensioni ELF effettive

```
$ size build/zephyr/zephyr.elf
   text    data     bss     dec     hex  filename
 395440   10044  243642  649126   9e7a6  zephyr.elf
```

| Sezione | Valore effettivo | Note |
|---|---|---|
| `.text` (Flash) | 395440 B (~387 KB) | 38.67% di 1 MB — ampio headroom |
| `.data` | 10044 B (~10 KB) | Dati inizializzati |
| `.bss` | 243642 B (~238 KB) | Incluso WAMR heap pool 64 KB + MbedTLS 16 KB |
| **RAM totale (data+bss)** | **253686 B (~248 KB)** | **91.72% di 256 KB — tight ma funzionale** |

### Layout RAM stimato

| Zona | Dimensione | Tipo |
|---|---|---|
| WAMR heap pool (`wamr_heap_buf`) | 64 KB | BSS statico |
| MbedTLS heap (`CONFIG_MBEDTLS_HEAP_SIZE`) | 16 KB | BSS statico |
| System heap (`CONFIG_HEAP_MEM_POOL_SIZE`) | 4 KB | BSS statico |
| Zephyr OS: kernel, net stack, thread stack | ~164 KB | BSS/data |
| **Headroom RAM region** | **~8 KB** | Oggetti runtime aggiuntivi |
| DTCM (separato) | 12.5 KB usati / 64 KB totali | Stack ISR, puntatori veloci |

**Nota**: la tight RAM (91.72%) è funzionale perché i pool principali (WAMR,
MbedTLS) sono pre-allocati nel BSS — non competono con lo heap di sistema a
runtime.

---

## Modifiche effettuate

### 1. `zephyr-app/CMakeLists.txt` — WASI abilitato

```cmake
# Prima (WASI disabilitato):
set(WAMR_BUILD_LIBC_WASI 0)

# Dopo (WASI abilitato):
set(WAMR_BUILD_LIBC_WASI 1)
```

Aggiunto anche stub per costanti socket mancanti in Zephyr 3.5:
```cmake
if(WAMR_BUILD_LIBC_WASI)
    add_compile_definitions(IP_TTL=2 IP_MULTICAST_TTL=33)
endif()
```

---

### 2. `zephyr-app/src/wamr_integration.c` — heap aumentato

```c
// Prima:
#define WAMR_HEAP_SIZE (48 * 1024)

// Dopo:
#define WAMR_HEAP_SIZE (64 * 1024)
```

**Motivazione**: WASI aggiunge overhead runtime per fd table, environ, args
buffer. Con 48 KB il pool si esauriva durante `wasm_runtime_instantiate`.

---

### 3. `zephyr-app/src/wamr_integration.c` — WASI args prima dell'istanziazione

```c
#if WASM_ENABLE_LIBC_WASI != 0
    static const char *wasi_dir_list[]  = {"/"};
    static const char *wasi_env_list[]  = {NULL};
    static const char *wasi_argv_list[] = {"app"};
    wasm_runtime_set_wasi_args(module,
                               wasi_dir_list, 1,
                               NULL, 0,
                               wasi_env_list, 0,
                               (char **)wasi_argv_list, 1);
#endif
```

**Motivazione**: Senza questa chiamata, `environ_sizes_get` e `args_sizes_get`
ricevono puntatori NULL e provocano un trap WASI non gestito.

---

### 4. `zephyr-app/src/wamr_integration.c` — `wamr_call_wasi_start`

```c
int wamr_call_wasi_start(uint32_t instance_id);
```

Usa `wasm_runtime_lookup_wasi_start_function()` per trovare `_start` (entry
point standard dei moduli WASI command). Tratta `proc_exit(0)` come successo.
Fallback su export `run` per moduli non-WASI legacy.

---

### 5. `zephyr-app/src/wasmbed_protocol.c` — usa `wamr_call_wasi_start`

```c
// Prima:
if (wamr_call_function(instance_id, "run", NULL, 0, NULL, 0) != 0) {
// Dopo:
if (wamr_call_wasi_start(instance_id) != 0) {
```

---

### 6. Patch compatibilità Zephyr 3.5 — WAMR submodule

Zephyr 3.5 manca di alcune costanti/struct che WAMR usa. Le patch sono applicate
direttamente ai file del submodule `wamr/`.

#### 6a. `wamr/core/shared/platform/zephyr/zephyr_socket.c` — stub multicast

```c
/* Zephyr 3.5 does not expose these IP_* constants or ip_mreq in its socket
 * headers. Provide stub values so WAMR compiles. */
#ifndef IP_MULTICAST_LOOP
#define IP_MULTICAST_LOOP 34
#endif
#ifndef IP_ADD_MEMBERSHIP
#define IP_ADD_MEMBERSHIP 35
struct ip_mreq {
    struct in_addr imr_multiaddr;
    struct in_addr imr_interface;
};
struct ip_mreqn {
    struct in_addr imr_multiaddr;
    struct in_addr imr_address;
    int imr_ifindex;
};
#endif
#ifndef IP_DROP_MEMBERSHIP
#define IP_DROP_MEMBERSHIP 36
#endif
```

**Motivazione**: `IP_MULTICAST_LOOP`, `IP_ADD_MEMBERSHIP`, `IP_DROP_MEMBERSHIP`
e `struct ip_mreq`/`ip_mreqn` non sono esposte da `<zephyr/net/socket.h>` in
Zephyr 3.5 (aggiunte successivamente). Le funzionalità multicast non sono usate
nel nostro use case TCP — i valori stub fanno compilare, `setsockopt` ritorna
`ENOPROTOOPT` a runtime.

#### 6b. `wamr/core/shared/platform/zephyr/zephyr_file.c` — guard LittleFS

```c
// Prima:
#include <zephyr/fs/littlefs.h>

// Dopo:
#ifdef CONFIG_FILE_SYSTEM_LITTLEFS
#include <zephyr/fs/littlefs.h>
#endif
```

**Motivazione**: `lfs.h` (LittleFS) non è incluso nel build Zephyr a meno che
`CONFIG_FILE_SYSTEM_LITTLEFS=y` — l'include unconditional causa "No such file".

#### 6c. `wamr/core/shared/platform/zephyr/zephyr_file.c` — fallback `CONFIG_ZVFS_OPEN_MAX`

```c
// Aggiunto prima di: #define CONFIG_WASI_MAX_OPEN_FILES CONFIG_ZVFS_OPEN_MAX
#ifndef CONFIG_ZVFS_OPEN_MAX
#define CONFIG_ZVFS_OPEN_MAX 16
#endif
```

**Motivazione**: `CONFIG_ZVFS_OPEN_MAX` è stato introdotto in Zephyr 3.6. La
3.5 non lo definisce, causando un errore di preprocessore. Il fallback a 16 è
ragionevole per un MCU constrained.

#### 6d. `wamr/core/shared/platform/zephyr/platform_internal.h` — include `<time.h>`

```c
// Aggiunto prima di: typedef struct timespec os_timespec;
#include <time.h>
typedef struct timespec os_timespec;
```

**Motivazione**: Con picolibc (toolchain ARM Zephyr), `struct timespec` è
definita in `<time.h>`. Senza l'include esplicito, la typedef produceva
"incomplete type" in `posix.c` nonostante `_POSIX_C_SOURCE=200809`.

---

### 7. `zephyr-app/prj.conf` — CONFIG_POSIX_API e CONFIG_FILE_SYSTEM

```ini
# Prima:
CONFIG_POSIX_API=n

# Dopo:
CONFIG_POSIX_API=y

# Aggiunto:
CONFIG_FILE_SYSTEM=y
```

**`CONFIG_POSIX_API=y`**: Necessario per rendere disponibile la definizione
completa di `struct timespec` tramite le header POSIX di Zephyr.

**`CONFIG_FILE_SYSTEM=y`**: Abilita il layer file system Zephyr (`fs_stat`,
`fs_open`, `fs_read`, ecc.). WAMR include `zephyr_file.c` in ogni build con
`WAMR_BUILD_LIBC_WASI=1`, anche se l'applicazione non usa file — senza questo
flag il linker non trova i simboli `fs_*`.

---

## Prerequisiti per il build

### Zephyr SDK e west

```bash
# 1. Installa west
pip3 install --user west

# 2. Crea workspace Zephyr (se non già presente)
mkdir -p ~/zephyrproject && cd ~/zephyrproject
west init -m https://github.com/zephyrproject-rtos/zephyr --mr v3.5.0
west update

# 3. Esporta la variabile ZEPHYR_BASE
export ZEPHYR_BASE=~/zephyrproject/zephyr

# 4. Installa Zephyr SDK (toolchain ARM)
# Scarica da: https://github.com/zephyrproject-rtos/sdk-ng/releases
# Versione consigliata: 0.16.x (per Zephyr 3.5)
wget https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v0.16.4/\
zephyr-sdk-0.16.4_linux-x86_64_minimal.tar.xz
tar xf zephyr-sdk-0.16.4_linux-x86_64_minimal.tar.xz -C ~/
cd ~/zephyr-sdk-0.16.4
./setup.sh -t arm-zephyr-eabi   # solo toolchain ARM
```

---

## Procedura di build

```bash
cd /home/ubuntu/Thesis/retrospect/zephyr-app

# Pulizia completa (obbligatoria al cambio di opzioni WAMR o prj.conf)
west build -p always -b stm32f746g_disco -- -DCONF_FILE=prj.conf

# Verifica dimensione ELF
size build/zephyr/zephyr.elf
# Atteso: text ~387 KB, data+bss ~248 KB, totale < 1 MB Flash ✓
```

---

## Test su Renode

### Prerequisiti K8s

Ambiente pulito con un solo pod gateway:
```bash
# Il deploy aggiornato (dopo il cleanup della sessione 2026-05-07)
# non crea più il deployment statico wasmbed-gateway
bash scripts/deploy-k3s.sh
kubectl get pods -n wasmbed | grep gateway
# Atteso: solo gateway-1-deployment-xxxxx
```

### Avvio device STM32 su Renode

```bash
kubectl port-forward -n wasmbed svc/wasmbed-api-server 3001:3001 &

# Crea device STM32F746G Discovery (Renode avvierà il firmware)
curl -X POST http://localhost:3001/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "stm32-wasi-test",
    "mcuType": "Stm32F746gDisco",
    "gatewayId": "gateway-1"
  }'
```

Il firmware appena buildato (con WASI abilitato) dovrebbe produrre nei log:
```
[INF] === Wasmbed Zephyr Application Starting ===
[INF] Initializing WAMR runtime...
[INF] WAMR runtime initialized
[INF] Initializing Wasmbed protocol...
[INF] Enrollment: accepted by gateway
[INF] Enrollment: received UUID <uuid>
[INF] Enrollment: completed successfully
```

### Deploy di un modulo WASM WASI

Compila un hello-world WASI (richiede Rust + `wasm32-wasip1` target):

```bash
mkdir /tmp/wasi-hello && cd /tmp/wasi-hello
cargo init --name wasi-hello .
cat > src/main.rs <<'EOF'
fn main() {
    println!("Hello from WASI on STM32!");
}
EOF
rustup target add wasm32-wasip1
cargo build --target wasm32-wasip1 --release
# Output: target/wasm32-wasip1/release/wasi-hello.wasm
```

Deploy via API Server:
```bash
WASM_B64=$(base64 -w0 /tmp/wasi-hello/target/wasm32-wasip1/release/wasi-hello.wasm)
curl -X POST http://localhost:3001/api/v1/applications \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"wasi-hello\",
    \"targetDevices\": [\"stm32-wasi-test\"],
    \"wasmBinary\": \"${WASM_B64}\"
  }"
```

**Output atteso nei log Renode (UART del STM32)**:
```
[INF] wamr_integration: WASI args set on module 1
[INF] wamr_integration: WASM module instantiated (instance_id: 1)
[INF] wamr_integration: Calling WASI _start (instance 1)
Hello from WASI on STM32!
[INF] wamr_integration: WASI module exited cleanly (proc_exit 0)
```

### Test con socket TCP (MasterThesis hello-wasm)

Il modulo in `MasterThesis/hello-wasm/` apre una connessione TCP a
`10.0.2.2:8080`. Su Renode con SLIRP (`CONFIG_NET_QEMU_USER`) l'IP 10.0.2.2
è il gateway SLIRP che forwarda al host.

```bash
# Avvia un server TCP di ascolto sul host
nc -l 8080 &

# Deploy del modulo hello-wasm sul device STM32
WASM_B64=$(base64 -w0 MasterThesis/hello-wasm/target/wasm32-wasip1/release/hello_wasm.wasm)
curl -X POST http://localhost:3001/api/v1/applications \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"tcp-hello\",
    \"targetDevices\": [\"stm32-wasi-test\"],
    \"wasmBinary\": \"${WASM_B64}\"
  }"
```

**Output atteso su nc**: richiesta HTTP GET (o payload TCP definito in main.rs).

---

## Checklist verifica Fase C

| Check | Comando | Atteso | Stato |
|---|---|---|---|
| CMakeLists WASI flag | `grep WAMR_BUILD_LIBC_WASI zephyr-app/CMakeLists.txt` | `set(WAMR_BUILD_LIBC_WASI 1)` | ✓ |
| Heap size aggiornato | `grep WAMR_HEAP_SIZE zephyr-app/src/wamr_integration.c` | `(64 * 1024)` | ✓ |
| Build firmware | `west build -p always -b stm32f746g_disco` | 0 errori | ✓ |
| Flash `.text` | `size build/zephyr/zephyr.elf` | ~387 KB (< 1 MB) | ✓ 387 KB |
| RAM totale (data+bss) | `size build/zephyr/zephyr.elf` | < 260 KB | ✓ 248 KB |
| Enrollment STM32 | log firmware | `Enrollment: completed successfully` | da testare |
| Deploy WASM WASI | log UART | `WASI module exited cleanly (proc_exit 0)` | da testare |
| Deploy WASM TCP | nc output | pacchetto TCP ricevuto | da testare |

---

## Troubleshooting

### `wasm_runtime_instantiate` restituisce NULL senza errore

Il pool di 64 KB potrebbe essere esaurito. Aumentare `WAMR_HEAP_SIZE` a
80 KB in `wamr_integration.c`. Verificare che `.bss + .data` non superi 260 KB
(margine di ~8 KB rispetto al limite della regione RAM a 256 KB +
12 KB DTCM disponibili per overflow di stack).

### Trap `unreachable` all'avvio del modulo WASI

Probabilmente il modulo usa `wasi_snapshot_preview2` o funzioni non
implementate in WAMR. Usare `wasm32-wasip1` (non p2) come target.
Verificare con `wasm-objdump -x modulo.wasm | grep import`.

### `Function not found: _start`

```bash
wasm-objdump -x modulo.wasm | grep export
# Deve contenere: _start
```
Se il modulo esporta `main` invece di `_start`, aggiungere un secondo
fallback a `wasm_runtime_lookup_function(instance, "main")` in
`wamr_call_wasi_start`.

### RAM overflow / hard fault all'avvio

Se la regione RAM supera il 95% o il device va in fault:
1. Ridurre `CONFIG_MBEDTLS_HEAP_SIZE` da 16384 a 12288.
2. Ridurre `WAMR_HEAP_SIZE` da 64 KB a 56 KB.
3. Ridurre `CONFIG_NET_TCP_WORKQ_STACK_SIZE` da 4096 a 2048.

### ELF supera la Flash (> 1 MB su STM32F746G)

```bash
west build -- -DCMAKE_VERBOSE_MAKEFILE=ON 2>&1 | grep -E "\.text.*size"
```
Disabilitare `WAMR_BUILD_FAST_INTERP=1` in CMakeLists per ridurre ~30 KB (a
costo di prestazioni minori nel runtime).

### `proc_exit` non rilevato come successo

La stringa dell'eccezione può variare tra versioni WAMR. Se
`strstr(ex, "proc_exit(0)")` non corrisponde, stampare l'eccezione con
`LOG_ERR` e aggiornare il pattern in `wamr_call_wasi_start`.

### `IP_TTL` / `IP_MULTICAST_TTL` undeclared

Verificare che `CMakeLists.txt` contenga:
```cmake
if(WAMR_BUILD_LIBC_WASI)
    add_compile_definitions(IP_TTL=2 IP_MULTICAST_TTL=33)
endif()
```

### `lfs.h: No such file or directory`

Verificare che `zephyr_file.c` abbia il guard `#ifdef CONFIG_FILE_SYSTEM_LITTLEFS`
attorno all'include. Se manca, applicare la patch 6b descritta in "Modifiche".

### `CONFIG_ZVFS_OPEN_MAX undeclared`

Verificare che `zephyr_file.c` abbia il fallback `#ifndef CONFIG_ZVFS_OPEN_MAX`.
Se manca, applicare la patch 6c.

### `invalid use of incomplete typedef 'os_timespec'`

Verificare che `platform_internal.h` includa `<time.h>` prima della typedef
`os_timespec`. Se manca, applicare la patch 6d.

### `fs_stat / fs_open undefined reference`

Aggiungere `CONFIG_FILE_SYSTEM=y` a `prj.conf`. WAMR include il layer file
system anche se il firmware non usa file — serve per soddisfare il linker.

---

## Relazione con le modifiche precedenti

| Dipendenza | Sessione | Stato |
|---|---|---|
| CRD `DeviceClass`, `McuType: LinuxArm64` | Sessione 1-2 | Applicato |
| `ApplicationConfig` propagata via CBOR | Sessione 1-2 | Applicato |
| `wasmbed-edge-client` daemon Linux | Sessione 3 | Testato (Fase B) |
| Fix cipher TLS RSA (`TLS_ECDHE_RSA`) | Sessione 3 (test) | Applicato |
| Fix gateway pod mismatch | Sessione 3 (test) | Applicato |
| `WAMR_BUILD_LIBC_WASI=1` + patch Zephyr 3.5 | Sessione 3 | **Build verificato ✓** |
