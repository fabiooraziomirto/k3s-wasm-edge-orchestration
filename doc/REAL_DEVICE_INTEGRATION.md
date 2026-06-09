# Real Device Integration Guide

This guide provides step-by-step instructions for integrating a real hardware device with the RETROSPECT Wasmbed platform.

## Overview

The Wasmbed platform is designed to support both emulated devices (via Renode) and real hardware devices. This guide covers the process of connecting a physical embedded device to the platform.

## Prerequisites

### Hardware Requirements

- **Supported MCU**: One of the supported MCU types (see [MCU_SUPPORT.md](MCU_SUPPORT.md))
- **Network Connectivity**: Ethernet or WiFi capability (required for TLS connection)
- **Debug Interface**: SWD/JTAG interface for firmware flashing
- **Serial Interface**: UART for debugging and logs

### Software Requirements

- **Zephyr SDK**: Version 0.16.5 or later
- **West Tool**: Zephyr meta-tool
- **Python 3.8+**: With required dependencies
- **OpenOCD or J-Link**: For firmware flashing
- **Serial Terminal**: For UART debugging

### Platform Requirements

- **Kubernetes Cluster**: Wasmbed platform deployed and running
- **Gateway Accessible**: Gateway pod IP must be reachable from device network
- **TLS Certificates**: Device certificates must be provisioned

## Supported Hardware

### Recommended Boards

1. **STM32F746G Discovery**
   - Ethernet support
   - 1MB Flash, 320KB RAM
   - SWD interface
   - UART available

2. **FRDM-K64F**
   - Ethernet support
   - 1MB Flash, 256KB RAM
   - SWD interface
   - UART available

3. **ESP32 DevKitC**
   - WiFi support
   - 4MB Flash, 520KB RAM
   - USB interface
   - UART available

### Other Supported Boards

See [MCU_SUPPORT.md](MCU_SUPPORT.md) for complete list of supported MCU types.

## Step-by-Step Integration Process

### Step 1: Prepare Development Environment

#### 1.1 Install Zephyr SDK

```bash
# Download Zephyr SDK
wget https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v0.16.5/zephyr-sdk-0.16.5_linux-x86_64.tar.xz

# Extract SDK
tar xvf zephyr-sdk-0.16.5_linux-x86_64.tar.xz
cd zephyr-sdk-0.16.5

# Run setup script
./setup.sh
```

#### 1.2 Setup Python Environment

```bash
# Create virtual environment
python3 -m venv .venv
source .venv/bin/activate

# Install dependencies
pip install west pykwalify pyelftools
```

#### 1.3 Install West Tool

```bash
pip install west
```

### Step 2: Configure Zephyr Workspace

#### 2.1 Initialize Zephyr Workspace

```bash
cd /path/to/retrospect
./scripts/setup-zephyr-workspace.sh
```

This script will:
- Clone Zephyr RTOS repository
- Setup Zephyr environment
- Configure WAMR integration

#### 2.2 Verify Environment

```bash
cd zephyr-workspace
source ../.venv/bin/activate
west --version
```

### Step 3: Configure Firmware for Your Board

#### 3.1 Select Board

Choose your board from the supported list. For this example, we'll use STM32F746G Discovery:

```bash
export BOARD=stm32f746g_disco
```

#### 3.2 Configure Network

Edit `zephyr-app/prj.conf` to ensure network support is enabled:

```conf
# Network stack
CONFIG_NETWORKING=y
CONFIG_NET_IPV4=y
CONFIG_NET_TCP=y
CONFIG_NET_SOCKETS=y
CONFIG_NET_DHCPV4=y

# TLS support
CONFIG_MBEDTLS=y
CONFIG_MBEDTLS_BUILTIN=y

# Serial for debugging
CONFIG_SERIAL=y
CONFIG_UART_INTERRUPT_DRIVEN=y
```

#### 3.3 Configure Gateway Endpoint

The firmware reads the gateway endpoint from memory address `0x20001000`. For real devices, you have two options:

**Option A: Hardcode Gateway Endpoint (Development)**

Edit `zephyr-app/src/main.c` to hardcode the gateway endpoint:

```c
#define GATEWAY_ENDPOINT "10.42.0.44:8081"  // Replace with your gateway pod IP
```

**Option B: Provision via Bootloader (Production)**

Implement a bootloader that writes the gateway endpoint to memory before firmware starts.

### Step 4: Compile Firmware

#### 4.1 Build Firmware

```bash
cd zephyr-workspace
source ../.venv/bin/activate
west build -b $BOARD ../zephyr-app --pristine
```

#### 4.2 Verify Build Output

```bash
ls -lh build/$BOARD/zephyr/zephyr.elf
ls -lh build/$BOARD/zephyr/zephyr.bin
ls -lh build/$BOARD/zephyr/zephyr.hex
```

### Step 5: Generate Device Certificates

#### 5.1 Generate Device Key Pair

```bash
# Generate Ed25519 key pair
openssl genpkey -algorithm Ed25519 -out device-key.pem
openssl pkey -in device-key.pem -pubout -out device-pub.pem
```

#### 5.2 Generate Device Certificate

You'll need the CA certificate from the platform. Get it from Kubernetes:

```bash
# Extract CA certificate
kubectl get secret gateway-certificates -n wasmbed -o jsonpath='{.data.ca-cert\.pem}' | base64 -d > ca-cert.pem

# Generate device certificate request
openssl req -new -key device-key.pem -out device-csr.pem -subj "/CN=device-$(hostname)"

# Sign certificate with CA (requires CA private key - contact platform admin)
openssl x509 -req -in device-csr.pem -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial -out device-cert.pem -days 365
```

**Note**: In production, use a proper certificate provisioning process.

#### 5.3 Convert Certificates to DER Format

Zephyr uses DER format for certificates:

```bash
# Convert CA certificate
openssl x509 -in ca-cert.pem -outform DER -out ca-cert.der

# Convert device certificate
openssl x509 -in device-cert.pem -outform DER -out device-cert.der

# Convert device key
openssl pkcs8 -topk8 -nocrypt -in device-key.pem -outform DER -out device-key.der
```

### Step 6: Embed Certificates in Firmware

#### 6.1 Copy Certificates to Firmware Directory

```bash
cp ca-cert.der zephyr-app/src/
cp device-cert.der zephyr-app/src/
cp device-key.der zephyr-app/src/
```

#### 6.2 Update Firmware to Load Certificates

The firmware should load certificates from filesystem or embed them in the binary. Refer to `zephyr-app/src/network_handler.c` for certificate loading implementation.

### Step 7: Flash Firmware to Device

#### 7.1 Connect Device

- Connect SWD/JTAG interface to your computer
- Connect UART for debugging
- Power on the device

#### 7.2 Flash Firmware

**Using OpenOCD (STM32)**:

```bash
openocd -f interface/stlink.cfg -f target/stm32f7x.cfg \
  -c "program zephyr-workspace/build/stm32f746g_disco/zephyr/zephyr.elf verify reset exit"
```

**Using J-Link (nRF52840)**:

```bash
JLinkExe -device nRF52840_xxAA -if SWD -speed 4000 -autoconnect 1
loadfile zephyr-workspace/build/nrf52840dk_nrf52840/zephyr/zephyr.hex
```

### Step 8: Configure Network

#### 8.1 Ethernet Configuration

For Ethernet-enabled boards:

1. Connect Ethernet cable to device
2. Device should obtain IP via DHCP automatically
3. Verify IP assignment via UART logs

#### 8.2 WiFi Configuration

For WiFi-enabled boards (ESP32):

1. Configure WiFi credentials in firmware (or via provisioning)
2. Device should connect to WiFi network
3. Verify IP assignment via UART logs

### Step 9: Register Device in Platform

#### 9.1 Get Device Public Key

```bash
# Extract public key
cat device-pub.pem
```

#### 9.2 Create Device CRD

**Via Dashboard**:
1. Navigate to "Device Management"
2. Click "Create Device"
3. Enter device name
4. Select MCU type
5. Paste public key
6. Select target gateway
7. Click "Create"

**Via API**:

```bash
curl -X POST http://localhost:3001/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "real-device-1",
    "deviceType": "MCU",
    "mcuType": "Stm32F746gDisco",
    "publicKey": "-----BEGIN PUBLIC KEY-----\n...\n-----END PUBLIC KEY-----",
    "gatewayId": "gateway-1"
  }'
```

**Via Kubernetes CRD**:

```yaml
apiVersion: wasmbed.github.io/v0
kind: Device
metadata:
  name: real-device-1
  namespace: wasmbed
spec:
  deviceType: MCU
  architecture: ARM_CORTEX_M
  mcuType: Stm32F746gDisco
  publicKey: |
    -----BEGIN PUBLIC KEY-----
    ...
    -----END PUBLIC KEY-----
  preferredGateway: gateway-1
```

### Step 10: Verify Connection

#### 10.1 Check Device Status

```bash
kubectl get devices -n wasmbed
kubectl describe device real-device-1 -n wasmbed
```

#### 10.2 Monitor UART Logs

Connect to device UART and monitor logs:

```bash
# Using screen
screen /dev/ttyUSB0 115200

# Using minicom
minicom -D /dev/ttyUSB0 -b 115200
```

Expected logs:
- Network stack initialization
- DHCP IP assignment
- TLS connection attempt
- Gateway connection established
- Device enrollment

#### 10.3 Check Gateway Logs

```bash
kubectl logs -n wasmbed -l app=wasmbed-gateway --tail=50
```

Look for:
- TLS connection accepted
- Device enrollment request
- Device authenticated
- Device registered

### Step 11: Deploy Application

#### 11.1 Create Application

**Via Dashboard**:
1. Navigate to "Application Management"
2. Click "Create Application"
3. Upload WASM module or compile from Rust source
4. Configure application parameters
5. Click "Deploy"

**Via API**:

```bash
curl -X POST http://localhost:3001/api/v1/applications \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-app",
    "wasmBytes": "<base64-encoded-wasm>",
    "targetDevices": {
      "deviceNames": ["real-device-1"]
    }
  }'
```

#### 11.2 Monitor Application Execution

```bash
# Check application status
kubectl get applications -n wasmbed
kubectl describe application test-app -n wasmbed

# Monitor device logs
# (via UART or gateway logs)
```

## Network Configuration

### Gateway Endpoint Discovery

The device needs to know the gateway endpoint. Options:

1. **Static Configuration**: Hardcode in firmware (development)
2. **DNS Discovery**: Use Kubernetes service DNS (e.g., `wasmbed-gateway.wasmbed.svc.cluster.local:8081`)
3. **Provisioning**: Write endpoint to memory via bootloader
4. **DHCP Option**: Use DHCP custom option (if supported)

### Network Requirements

- **Device Network**: Must be able to reach Kubernetes cluster network
- **Gateway IP**: Must be routable from device network
- **Port 8081**: Must be open for TLS connections
- **DNS**: Optional, for service name resolution

### Firewall Configuration

Ensure firewall rules allow:
- Outbound TCP connections from device to gateway (port 8081)
- Inbound connections to gateway (port 8081)

## Troubleshooting

### Device Not Connecting

1. **Check Network Connectivity**:
   ```bash
   # From device (if possible)
   ping <gateway-pod-ip>
   telnet <gateway-pod-ip> 8081
   ```

2. **Verify Gateway Endpoint**:
   - Check gateway pod IP: `kubectl get pods -n wasmbed -l app=wasmbed-gateway -o wide`
   - Verify endpoint in firmware matches gateway IP

3. **Check Certificates**:
   - Verify certificates are in DER format
   - Verify CA certificate matches gateway CA
   - Check certificate expiration

4. **Monitor UART Logs**:
   - Check for network initialization errors
   - Check for TLS handshake errors
   - Check for certificate validation errors

### Certificate Issues

1. **Certificate Format**:
   - Ensure certificates are in DER format
   - Verify certificate chain is complete

2. **Certificate Validation**:
   - Check CA certificate matches gateway CA
   - Verify device certificate is signed by CA
   - Check certificate expiration dates

3. **Key Pair Mismatch**:
   - Verify device private key matches public key in CRD
   - Check key format (Ed25519)

### Network Issues

1. **DHCP Not Working**:
   - Check network cable connection (Ethernet)
   - Verify DHCP server is available
   - Check network interface configuration in firmware

2. **WiFi Connection Issues**:
   - Verify WiFi credentials
   - Check signal strength
   - Verify WiFi driver is loaded

3. **Gateway Unreachable**:
   - Check network routing
   - Verify firewall rules
   - Test connectivity from device network

## Production Considerations

### Certificate Management

- Use proper CA with certificate rotation
- Implement secure certificate provisioning
- Use hardware security modules (HSM) for key storage

### Network Security

- Use VPN or secure network for device-to-gateway communication
- Implement network segmentation
- Use firewall rules to restrict access

### Device Provisioning

- Implement secure boot process
- Use device attestation
- Implement device identity management

### Monitoring

- Set up device health monitoring
- Implement alerting for device failures
- Log all device communications

## Additional Resources

- [Zephyr RTOS Documentation](https://docs.zephyrproject.org/)
- [WAMR Documentation](https://github.com/bytecodealliance/wasm-micro-runtime)
- [TLS Connection Guide](TLS_CONNECTION.md)
- [MCU Support](MCU_SUPPORT.md)
- [Firmware Documentation](FIRMWARE.md)

## Support

For issues or questions:
- Check device logs via UART
- Check gateway logs: `kubectl logs -n wasmbed -l app=wasmbed-gateway`
- Check API server logs: `kubectl logs -n wasmbed -l app=wasmbed-api-server`
- Review [DEVELOPMENT_STATUS.md](DEVELOPMENT_STATUS.md) for known issues
