# MCU Support in Wasmbed

## Overview

Wasmbed now supports multiple MCU types with emphasis on boards that have Ethernet or WiFi connectivity for reliable network testing.

## Supported MCU Types

### Ethernet-Enabled Boards (Recommended)

#### 1. STM32F746G Discovery (`Stm32F746gDisco`)
- **CPU**: ARM Cortex-M7 @ 216 MHz
- **RAM**: 320 KB
- **Flash**: 1 MB
- **Network**: Ethernet 10/100 Mbps
- **Renode Platform**: `stm32f7_discovery-bb`
- **Zephyr Board**: `stm32f746g_disco`
- **UART**: `usart1`
- **Status**: Firmware compilato e testato; flusso Enrolled → Connected verificato (feb 2026)
- **Recommended**: **Yes** - Best choice for network testing

#### 2. FRDM-K64F (`FrdmK64f`)
- **CPU**: ARM Cortex-M4 @ 120 MHz
- **RAM**: 256 KB
- **Flash**: 1 MB
- **Network**: Ethernet 10/100 Mbps
- **Renode Platform**: `frdm_k64f`
- **Zephyr Board**: `frdm_k64f`
- **UART**: `uart0`
- **Status**: Code ready, firmware compilation pending
- **Recommended**: **Yes** - Well supported alternative

### WiFi-Enabled Boards

#### 3. ESP32 DevKitC (`Esp32DevkitC`)
- **CPU**: Xtensa LX6 @ 240 MHz (dual-core)
- **RAM**: 520 KB SRAM
- **Flash**: 4 MB (external)
- **Network**: WiFi 802.11 b/g/n, Bluetooth 4.2
- **Renode Platform**: `esp32`
- **Zephyr Board**: `esp32_devkitc_wroom`
- **UART**: `uart0`
- **Status**: Code ready, firmware compilation pending
- **Recommended**: **Maybe** - WiFi support in Renode may be limited

### No Network Boards

#### 4. STM32F4 Discovery (`Stm32F4Disco`)
- **CPU**: ARM Cortex-M4 @ 168 MHz
- **RAM**: 192 KB
- **Flash**: 1 MB
- **Network**: None
- **Renode Platform**: `stm32f4_discovery`
- **Zephyr Board**: `stm32f4_discovery`
- **UART**: `usart2`
- **Status**: Code ready
- **Recommended**: **No** - No network support

#### 5. nRF52840 DK (`Nrf52840DK`)
- **CPU**: ARM Cortex-M4 @ 64 MHz
- **RAM**: 256 KB
- **Flash**: 1 MB
- **Network**: BLE 5.0 only (no Ethernet/WiFi)
- **Renode Platform**: `nrf52840dk_nrf52840`
- **Zephyr Board**: `nrf52840dk_nrf52840`
- **UART**: `uart0`
- **Status**: Code ready, firmware exists
- **Recommended**: **No** - BLE only, no Ethernet/WiFi

### Zephyr + Renode virtual boards (no network in emulator)

Official Zephyr virtual boards; sample `hello_world` firmware. The device is registered with the gateway at start by the API server (no TLS connection from firmware).

- **Riscv32Virtual** – RISCV32 virtual; Renode platform from `zephyr/boards/renode/riscv32_virtual/support/riscv32_virtual.repl`
- **CortexR8Virtual** – Cortex-R8 virtual; Renode platform from `zephyr/boards/renode/cortex_r8_virtual/support/cortex_r8_virtual.repl`

**Status:** Implemented in wasmbed-qemu-manager; Docker firmware build and E2E tests still to verify. See [scripts/README.md](../scripts/README.md) and [TEST_GUIDE.md](TEST_GUIDE.md).

### Legacy Boards

#### 6. Arduino Nano 33 BLE (`RenodeArduinoNano33Ble`)
- Legacy support for backward compatibility
- Maps to nRF52840 firmware

#### 7. STM32F4 Discovery Legacy (`RenodeStm32F4Discovery`)
- Legacy support for backward compatibility
- Maps to STM32F4 Discovery

#### 8. ARM MPS2-AN385 (`Mps2An385`)
- Legacy support for backward compatibility
- Maps to Arduino Nano firmware

## MCU Type Methods

All MCU types support the following methods:

```rust
// Get Renode platform file name
pub fn renode_platform(&self) -> &'static str

// Get CPU architecture
pub fn cpu_architecture(&self) -> &'static str

// Get memory size
pub fn memory_size(&self) -> &'static str

// Get display name for UI
pub fn display_name(&self) -> &'static str

// Get Rust HAL crate name (if available)
pub fn rust_hal_crate(&self) -> Option<&'static str>

// Get firmware path
pub fn get_firmware_path(&self) -> &'static str

// Get UART peripheral name
pub fn get_uart_name(&self) -> &'static str

// Check if board has Ethernet
pub fn has_ethernet(&self) -> bool

// Check if board has WiFi
pub fn has_wifi(&self) -> bool

// Check if board has any network (Ethernet or WiFi)
pub fn has_network(&self) -> bool
```

## Network Configuration

### Ethernet Configuration (Renode)

For boards with Ethernet (`Stm32F746gDisco`, `FrdmK64f`), Renode is configured with:

```renode
emulation CreateSwitch "ethernet_switch"
emulation CreateTap "tap0" "ethernet_tap"
sysbus.ethernet MAC "00:11:22:33:44:55"
connector Connect sysbus.ethernet ethernet_switch
connector Connect host.ethernet_tap ethernet_switch
host.ethernet_tap Start
```

This creates:
- A virtual Ethernet switch
- A TAP interface for host communication
- MAC address assignment
- Connection between device and host

### WiFi Configuration (Renode)

WiFi support in Renode is limited. For ESP32, a placeholder configuration is added:

```renode
# WiFi configuration for ESP32 (if supported)
```

## Firmware Compilation

### Prerequisites

1. **Zephyr SDK** (required):
   ```bash
   wget https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v0.16.5/zephyr-sdk-0.16.5_linux-x86_64.tar.xz
   tar xvf zephyr-sdk-0.16.5_linux-x86_64.tar.xz
   cd zephyr-sdk-0.16.5
   ./setup.sh
   ```

2. **Python dependencies**:
   ```bash
   python3 -m venv .venv
   source .venv/bin/activate
   pip install west pykwalify pyelftools
   ```

### Compilation Steps

#### STM32F746G Discovery

```bash
cd zephyr-workspace
source ../.venv/bin/activate
west build -b stm32f746g_disco ../zephyr-app --pristine --build-dir build/stm32f746g_disco
```

Firmware output: `build/stm32f746g_disco/zephyr/zephyr.elf`

#### FRDM-K64F

```bash
cd zephyr-workspace
source ../.venv/bin/activate
west build -b frdm_k64f ../zephyr-app --pristine --build-dir build/frdm_k64f
```

Firmware output: `build/frdm_k64f/zephyr/zephyr.elf`

#### ESP32 DevKitC

```bash
cd zephyr-workspace
source ../.venv/bin/activate
west build -b esp32_devkitc_wroom ../zephyr-app --pristine --build-dir build/esp32_devkitc_wroom
```

Firmware output: `build/esp32_devkitc_wroom/zephyr/zephyr.elf`

## Boards selectable for emulation (TLS-capable first)

The platform exposes all Renode-compatible boards via **GET /api/v1/boards**. Each board includes:

- `id`: API/CRD identifier (e.g. `Stm32F746gDisco`, `FrdmK64f`)
- `displayName`: Human-readable name
- `hasEthernet` / `hasWifi`: Network capability
- `recommendedForTls`: `true` for Ethernet and WiFi boards (use these for TLS and app deploy in emulation)

**Ethernet (recommended for TLS):** Stm32F746gDisco, FrdmK64f  
**WiFi:** Esp32DevkitC  
**No network (no TLS in emulation):** Stm32F4Disco, Nrf52840DK, legacy types

The dashboard Device creation form loads this list and shows "Recommended for TLS" for Ethernet/WiFi boards. Build firmware for at least one Ethernet board (see Compilation Steps) before building the API server image so that connect + deploy can complete TLS.

## Device Creation

### Via API

```bash
curl -X POST http://localhost:3001/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "stm32f7-device",
    "deviceType": "MCU",
    "mcuType": "Stm32F746gDisco",
    "publicKey": "ssh-ed25519 AAAA..."
  }'
```

### Via Kubernetes CRD

```yaml
apiVersion: wasmbed.dev/v1
kind: Device
metadata:
  name: stm32f7-device
  namespace: wasmbed
spec:
  deviceType: MCU
  architecture: ARM_CORTEX_M
  mcuType: Stm32F746gDisco
  publicKey: |
    ssh-ed25519 AAAA...
```

## Default MCU Type

The default MCU type is now `Stm32F746gDisco` (STM32F746G Discovery with Ethernet), which is the best choice for network testing.

## Migration from Old MCU Types

Old device configurations will be automatically mapped:

- `Mps2An385` → `Mps2An385` (legacy support maintained)
- `RenodeArduinoNano33Ble` → `RenodeArduinoNano33Ble` (legacy support maintained)
- `RenodeStm32F4Discovery` → `RenodeStm32F4Discovery` (legacy support maintained)

For new devices, use:
- `Stm32F746gDisco` (recommended for Ethernet)
- `FrdmK64f` (alternative for Ethernet)
- `Esp32DevkitC` (for WiFi, if supported)

## Testing Status

### Implemented

- [x] MCU type enum with all boards
- [x] Renode platform mapping
- [x] Firmware path mapping
- [x] UART peripheral mapping
- [x] Network capability detection (`has_ethernet()`, `has_wifi()`, `has_network()`)
- [x] Ethernet configuration in Renode scripts
- [x] API Server MCU type parsing
- [x] Device CRD MCU type serialization

### Pending

- [ ] Firmware compilation for FRDM-K64F
- [ ] Firmware compilation for ESP32 DevKitC
- [ ] Virtual-board firmware Docker build verified; E2E with Riscv32Virtual/CortexR8Virtual
- [ ] WAMR execution testing (deploy E2E)

### Done

- [x] Firmware STM32F746G Discovery compilato; TLS + Identify → Connected verificato
- [x] Supporto McuType Riscv32Virtual e CortexR8Virtual (Renode script, path firmware)

## Known Issues

1. **Zephyr SDK Required**: Firmware compilation requires Zephyr SDK 0.16.5 or later
2. **WiFi Support Limited**: Renode's WiFi support for ESP32 may be limited
3. **Firmware Size**: Ethernet boards may require larger firmware due to network stack

## Performance Comparison

| Board | CPU Speed | RAM | Network | Renode Speed | Recommended |
|-------|-----------|-----|---------|--------------|-------------|
| STM32F746G | 216 MHz | 320 KB | Ethernet | Fast | ⭐⭐⭐⭐⭐ |
| FRDM-K64F | 120 MHz | 256 KB | Ethernet | Fast | ⭐⭐⭐⭐ |
| ESP32 | 240 MHz | 520 KB | WiFi | Medium | ⭐⭐⭐ |
| STM32F4 | 168 MHz | 192 KB | None | Fast | ⭐⭐ |
| nRF52840 | 64 MHz | 256 KB | BLE only | Fast | ⭐⭐ |

## Next Steps

1. Install Zephyr SDK
2. Compile firmware for STM32F746G Discovery
3. Test Ethernet connectivity in Renode
4. Verify TLS connection to gateway
5. Test complete workflow: device enrollment → app deployment → execution
