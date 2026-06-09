# Platform architecture

## Overview

This platform is a distributed system for managing WebAssembly applications on embedded devices across the cloud–fog–edge continuum. It uses Kubernetes for orchestration, Renode for hardware emulation, and Zephyr RTOS for device firmware.

**Diagrams:**
- High-level component and communication views: [repository README](../README.md#architecture) (Mermaid)
- **Sequence diagrams (Mermaid):** [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md) — enrollment, heartbeat, deploy/stop, monitoring
- TLS transport details: [TLS_CONNECTION.md](TLS_CONNECTION.md)

## System Components

### 1. Control Plane (Kubernetes)

#### Gateway
- **Location**: Kubernetes Pod (wasmbed namespace)
- **IP**: Dynamic (e.g., 10.42.0.12)
- **Ports**:
  - 8080: HTTP API
  - 8081: TLS (secure device connections)
- **Responsibilities**:
  - Device enrollment and authentication
  - TLS certificate management
  - Message routing between devices and applications
  - WebAssembly module deployment

#### API Server
- **Location**: Kubernetes Pod (wasmbed namespace)
- **Port**: 3001
- **Responsibilities**:
  - RESTful API for device/application management
  - Kubernetes CRD management (Device, Application, Gateway)
  - Renode container orchestration
  - Firmware deployment

#### Dashboard
- **Location**: Kubernetes Pod (wasmbed namespace)
- **Port**: 3000 (exposed via port-forward)
- **Responsibilities**:
  - Web UI for system monitoring
  - Network topology visualization
  - Device status and logs
  - Application deployment interface

#### Infrastructure Service
- **Location**: Kubernetes Pod (wasmbed namespace)
- **Port**: 30460
- **Responsibilities**:
  - Certificate Authority (CA)
  - Secret storage
  - System monitoring

### 2. Device Emulation (Fog Layer)

#### Device Proxy (one per device)
- **Concept**: The **device proxy** is the logical object that represents one emulated device toward the control plane. It is one-per-device (or one-per-runtime when a device has multiple runtimes). Implementation may be a Docker container, a process, or a Renode machine.
- **Location**: **Single Renode container** (`wasmbed-renode`): one Docker container runs one Renode process; each device is a **machine** inside that Renode instance (one Renode, N devices). The Renode monitor is exposed on port 9999; the manager sends commands to add/pause machines.
- **Network**: Container exposes port 9999 (Renode monitor). Firmware in shared volume `wasmbed-firmware-store` (per-device subdirs).
- **Responsibilities**:
  - Hardware emulation (CPU, memory, peripherals) when implemented with Renode
  - Loads and executes Zephyr firmware
  - Provides UART analyzer for logs
  - Interfaces with the runtime (Zephyr + WAMR) for application lifecycle

#### Renode Manager (Board Provisioner)
- **Role**: Creates and manages **device proxy** instances. The name "Renode Manager" is used because the current implementation uses Renode; the function is generic (device manager / board provisioner).
- **Location**: Runs inside API Server Pod. Ensures the **single** Renode container (`wasmbed-renode`) is running, then sends monitor commands (TCP port 9999) to add or pause machines per device.
- **Responsibilities**: Start/ensure singleton Renode container, copy firmware to shared volume, send Renode monitor commands to add/start or pause machines, register/unregister boards with Gateway.

#### One Renode, N device proxies
- **Model**: A single Renode instance (Docker container `wasmbed-renode`) hosts **N device proxies**: each device is a **machine** inside that instance, not a separate container. Shared volume `wasmbed-firmware-store` has one subdirectory per device with firmware. Renode Manager talks to Renode on monitor port 9999 to start/pause machines.
- **Flow**: Start device → monitor commands create and run the machine; stop device → `mach set` + `pause`. Gateway and CRDs see N distinct devices, each with its own TLS endpoint and state. See [SEQUENCE_DIAGRAMS.md](SEQUENCE_DIAGRAMS.md) for enrollment and deploy flows.

#### Zephyr RTOS Firmware
- **Location**: Runs inside device proxy (e.g. inside Renode emulator)
- **Boards Supported**:
  - **nRF52840 DK** (Official Zephyr board) - Recommended
  - **STM32F4 Discovery** (Official Zephyr board)
  - Arduino Nano 33 BLE (Legacy)
- **Components**:
  - Network stack (TCP/IP, TLS)
  - WAMR (WebAssembly Micro Runtime)
  - Wasmbed protocol handler
- **Responsibilities**:
  - Reads gateway endpoint from memory (0x20001000)
  - Establishes TLS connection to gateway
  - Executes WebAssembly modules
  - Sends telemetry and receives commands

## Network Architecture

```mermaid
graph TB
    subgraph "Cloud Layer - Kubernetes Cluster (K3S)"
        Gateway["Gateway Pod<br/>IP: 10.42.0.44<br/>Ports: 8080 HTTP, 8081 TLS"]
        APIServer["API Server Pod<br/>Port: 3001"]
        Dashboard["Dashboard Pod<br/>Port: 3000"]
    end
    
    subgraph "Fog Layer - Host Docker"
        DevProxy["Device Proxy<br/>(one per device)<br/>Network: host<br/>Volume: firmware-{device-id}"]
    end
    
    subgraph "Edge Layer - Emulated Device (Runtime)"
        Zephyr["Zephyr RTOS<br/>Network Stack + TLS"]
        WAMR["WAMR Runtime<br/>WebAssembly"]
    end
    
    Dashboard -->|HTTP| APIServer
    APIServer -->|Manages| DevProxy
    Zephyr -->|TLS 1.3| Gateway
    Zephyr -->|Executes| WAMR
    DevProxy -->|Runs| Zephyr
```

## TLS Connection Flow

1. **Device Startup**:
   - Device proxy starts with firmware volume (e.g. Renode container)
   - Device proxy writes gateway endpoint to memory (0x20001000)
   - Format: `"10.42.0.12:8081"` (15 bytes)

2. **Zephyr Initialization**:
   - Zephyr boots and initializes network stack
   - Reads gateway endpoint from memory address 0x20001000
   - Parses host IP and port

3. **TLS Connection**:
   - Zephyr calls `network_connect_tls(host, port)`
   - Creates TCP socket with TLS options
   - Performs TLS handshake with gateway
   - Connection is persistent until device stops

4. **Communication**:
   - Device sends enrollment request
   - Gateway authenticates using public key
   - Device receives WebAssembly modules
   - WAMR executes modules
   - Device sends telemetry data

## Gateway Endpoint Resolution

### Problem Solved
Previously, devices used a TCP bridge at `127.0.0.1:40029`. Now they connect directly to the gateway pod IP.

### Resolution Process

1. **API Server** (in Pod):
   ```rust
   // When starting device emulation
   let gateway_endpoint = if is_local_address(endpoint) {
       // Resolve to actual gateway pod IP
       kubectl get pods -n wasmbed -l app=wasmbed-gateway \
           -o jsonpath='{.items[0].status.podIP}'
       // Returns: "10.42.0.12"
   }
   let tls_port = 8081; // Gateway TLS port
   let full_endpoint = format!("{}:{}", gateway_endpoint, tls_port);
   ```

2. **Renode Script Generation**:
   ```bash
   # Write endpoint to memory (called before Zephyr starts)
   sysbus WriteDoubleWord 0x20001000 0x0000000f  # Length: 15
   sysbus WriteDoubleWord 0x20001004 0x342e3031  # "10.4"
   sysbus WriteDoubleWord 0x20001008 0x2e302e32  # "2.0."
   sysbus WriteDoubleWord 0x2000100c 0x383a3231  # "12:8"
   sysbus WriteDoubleWord 0x20001010 0x00313830  # "081"
   ```

3. **Zephyr Reading**:
   ```c
   #define GATEWAY_ENDPOINT_ADDR 0x20001000
   
   uint32_t *length_ptr = (uint32_t *)GATEWAY_ENDPOINT_ADDR;
   uint32_t length = *length_ptr; // 15
   
   char *endpoint_ptr = (char *)(GATEWAY_ENDPOINT_ADDR + 4);
   memcpy(gateway_endpoint, endpoint_ptr, length);
   gateway_endpoint[length] = '\0';
   // Result: "10.42.0.12:8081"
   ```

### Persistence
- The endpoint is resolved once at device creation
- Stored in device CRD status
- Reused on subsequent Renode restarts
- No more `127.0.0.1` fallback

## Board registration protocol

When a **device proxy** (e.g. Renode container) starts, the **Renode Manager** (running in the API Server) registers the board with the **Gateway** over HTTP. When the device proxy stops, the manager unregisters it. The Gateway keeps an in-memory **board registry** (device_id → endpoint, mcu_type, capabilities).

### Endpoints (Gateway HTTP API, port 8080)

- **POST /api/v1/board/register** — Register a board. Body: `{ "device_id", "endpoint", "mcu_type", "capabilities" }` (optional: `has_ethernet`, `has_wifi`, `has_network`). Returns the stored registration.
- **GET /api/v1/board** — List all registered boards.
- **DELETE /api/v1/board/:device_id** — Unregister a board (204 No Content or 404 if not found).

### Flow

1. API Server starts a device proxy (e.g. `docker run` for Renode); after the process/container is up, it calls `POST .../board/register` with the device’s gateway endpoint, MCU type, and capabilities.
2. The Gateway adds the board to its registry. The device (Zephyr) can then connect via TLS and enroll; the Gateway can correlate TLS connections with registered boards.
3. When the API Server stops the device proxy (e.g. `docker stop`), it calls `DELETE .../board/:device_id` so the Gateway removes the board from the registry.

## Supported MCU Types

See [MCU_SUPPORT.md](MCU_SUPPORT.md) for complete list of supported MCU types.

### Ethernet-Enabled Boards (Recommended)

1. **STM32F746G Discovery** (`Stm32F746gDisco`) - **Default**
   - CPU: ARM Cortex-M7 @ 216 MHz
   - RAM: 320 KB
   - Flash: 1 MB
   - Network: Ethernet 10/100 Mbps
   - Renode platform: `stm32f7_discovery-bb.repl`

2. **FRDM-K64F** (`FrdmK64f`)
   - CPU: ARM Cortex-M4 @ 120 MHz
   - RAM: 256 KB
   - Flash: 1 MB
   - Network: Ethernet 10/100 Mbps
   - Renode platform: `frdm_k64f.repl`

### WiFi-Enabled Boards

3. **ESP32 DevKitC** (`Esp32DevkitC`)
   - CPU: Xtensa LX6 @ 240 MHz
   - RAM: 520 KB
   - Flash: 4 MB
   - Network: WiFi 802.11 b/g/n
   - Renode platform: `esp32.repl`

### Other Supported Boards

- **nRF52840 DK** (`Nrf52840DK`) - BLE only
- **STM32F4 Discovery** (`Stm32F4Disco`) - No network
- **Arduino Nano 33 BLE** (`RenodeArduinoNano33Ble`) - Legacy
- **MPS2-AN385** (`Mps2An385`) - Legacy
- And more (see [MCU_SUPPORT.md](MCU_SUPPORT.md))

## Data Flow

### Device Enrollment

```
1. Device boots → Zephyr reads endpoint from memory
2. Zephyr → TLS connect → Gateway:8081
3. Gateway ← Enrollment request (with public key)
4. Gateway → Verify public key
5. Gateway → Store device in Kubernetes CRD
6. Gateway → Respond with enrollment confirmation
```

### Application Deployment

```
1. User → Dashboard → Upload WASM module
2. Dashboard → API Server → Create Application CRD
3. Gateway watches CRD → Detects new application
4. Gateway → Device (via TLS): Deploy application
5. Device → WAMR → Load and execute WASM
6. Device → Gateway: Send execution results
```

### Gateway and Application CRD status (desired vs reported)

The Gateway is the component in the middle between the control plane and devices: it reads **desired** state from the Application CRD (spec) and writes **reported** state to the Application CRD status (phase, deviceStatuses, error). The Gateway is the owner of Application status; the Application Controller triggers deploy via the Gateway API but does not write Application status.

## Security

### TLS Configuration
- **Gateway**: Generates self-signed certificate
- **Devices**: Skip certificate verification (development mode)
- **Production**: Use proper CA and certificate validation

### Authentication
- Devices authenticated by Ed25519 public keys
- Keys stored in Kubernetes CRD
- Gateway validates signatures on messages

## Storage

### Kubernetes CRDs
- **Device**: Device metadata, status, public key
- **Application**: WASM module metadata, binary
- **Gateway**: Gateway configuration, endpoints

### Docker Volumes
- **Firmware volumes**: One per device (e.g., `firmware-nrf52840-device`)
- Contains: `zephyr.elf` (firmware binary)
- Mounted read-only in Renode container

### File System (API Server Pod)
- `/tmp/`: Renode scripts, debug logs
- `/app/zephyr-workspace/`: Firmware binaries
- `/app/qemu_devices.json`: In-memory device cache

## Deployment

### Prerequisites
- K3S cluster running (or compatible Kubernetes 1.24+)
- Docker daemon accessible
- Zephyr firmware compiled for target boards (optional for basic deployment)

### Components Startup Order
1. Infrastructure service (CA, secrets)
2. Gateway
3. API Server
4. Dashboard
5. Device CRDs creation
6. Renode containers (on-demand)

### Port Forwarding
- Dashboard: `kubectl port-forward -n wasmbed svc/wasmbed-dashboard 3000:3000`
- API Server: `kubectl port-forward -n wasmbed svc/wasmbed-api-server 3001:3001`
- Gateway HTTP: `kubectl port-forward -n wasmbed svc/wasmbed-gateway 8080:8080`
- Gateway TLS: `kubectl port-forward -n wasmbed svc/wasmbed-gateway 8081:8081`

## Monitoring

### Dashboard Views
- **Network Topology**: Visual graph of devices and gateways
- **Device Status**: Real-time status of all devices
- **System Health**: Infrastructure component health
- **Logs**: Aggregated logs from all components

### Health Endpoints
- Gateway: `http://gateway:8080/health`
- API Server: `http://api-server:3001/health`
- Infrastructure: `http://infrastructure:30460/api/v1/infrastructure/health`

## Troubleshooting

### Device Not Connecting
1. Check gateway pod IP: `kubectl get pods -n wasmbed -l app=wasmbed-gateway -o wide`
2. Verify endpoint in memory: Check API server logs for "Writing gateway endpoint"
3. Test TLS port: `curl -k https://10.42.0.12:8081/health`

### Renode Container Exits Immediately
1. Check firmware volume: `docker run --rm -v firmware-DEVICE:/firmware alpine ls -la /firmware`
2. Verify firmware path in API server logs
3. Check Renode script: `kubectl exec -n wasmbed API_POD -- cat /tmp/renode_prewrite_DEVICE.resc`

### Dashboard Shows "UNKNOWN" Status
1. Check API endpoints in NetworkTopology.js
2. Verify infrastructure service is running
3. Check browser console for errors

## Deployment Locations

### Cloud Layer
- **API Server**: Kubernetes Pod - Centralized control plane
- **Gateway**: Kubernetes Pod - Centralized security and certificate management
- **Dashboard**: Kubernetes Pod - Centralized access, no local installation
- **Controllers**: Kubernetes Pods - Kubernetes-native lifecycle management

### Fog Layer
- **Renode Containers**: Host Docker - Close to edge, efficient resource usage, isolated per device
- **Renode Manager**: Inside API Server Pod - Direct Docker API access

### Edge Layer
- **Zephyr RTOS**: Inside Renode - Represents actual embedded device behavior
- **WAMR Runtime**: Inside Zephyr - Executes applications in constrained environment
- **WASM Applications**: Loaded into WAMR - Actual edge computing workload

## Future Improvements

1. **Certificate Management**: Implement proper CA with certificate rotation
2. **Device Persistence**: Store device state in persistent volumes
3. **Scalability**: Support multiple gateway instances with load balancing
4. **Real hardware support**: physical device integration (not yet documented)
5. **Performance Optimization**: Optimize Renode startup time and API response times
6. **Monitoring**: Enhanced metrics collection and distributed tracing

### Future evolution: multi-workload per device

The current model is one Application to many target devices (each device runs one instance of that application). A possible evolution is multiple WASM workloads on the same node: several applications on one device, with intra-node (same device) and inter-node (device-to-device) communication. This is not yet modelled or implemented; the CRD and Gateway assume one deployed application context per device per Application resource.
