# TLS Connection Between End Device and Gateway

## Overview

This document describes how the TLS connection is established and maintained between emulated devices (running in Renode) and the Wasmbed gateway.

## Problem Statement

Previously, devices used a TCP bridge at `127.0.0.1:40029` to connect to the gateway. This approach had several issues:
- Not persistent across restarts
- Required additional TCP bridge processes
- Complicated the architecture
- Not suitable for production

## Solution: Direct TLS Connection

Devices now connect directly to the gateway pod IP using TLS on port 8081.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  Kubernetes Cluster                          │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Gateway Pod                                           │ │
│  │  IP: 10.42.0.12 (dynamic, resolved at runtime)        │ │
│  │  Ports:                                                │ │
│  │    - 8080: HTTP API                                    │ │
│  │    - 8081: TLS (device connections)                    │ │
│  └────────────────────────────────────────────────────────┘ │
│                          ▲                                   │
└──────────────────────────┼───────────────────────────────────┘
                           │
                           │ TLS Connection
                           │ (persistent)
                           │
┌──────────────────────────┼───────────────────────────────────┐
│                  Host Docker                                 │
│                          │                                   │
│  ┌───────────────────────┼────────────────────────────────┐ │
│  │  Renode Container                                      │ │
│  │  Network: --net=host                                   │ │
│  │                       │                                │ │
│  │  ┌────────────────────┼─────────────────────────────┐ │ │
│  │  │  Zephyr RTOS       │                             │ │ │
│  │  │                    │                             │ │ │
│  │  │  1. Reads endpoint from memory (0x20001000)     │ │ │
│  │  │     → "10.42.0.12:8081"                         │ │ │
│  │  │                    │                             │ │ │
│  │  │  2. Parses host and port                        │ │ │
│  │  │     → host: "10.42.0.12"                        │ │ │
│  │  │     → port: 8081                                │ │ │
│  │  │                    │                             │ │ │
│  │  │  3. Creates TLS socket                          │ │ │
│  │  │     → zsock_socket(AF_INET, SOCK_STREAM, TCP)  │ │ │
│  │  │     → zsock_setsockopt(SOL_TLS, TLS_HOSTNAME)  │ │ │
│  │  │                    │                             │ │ │
│  │  │  4. Connects to gateway                         │ │ │
│  │  │     → zsock_connect(addr, port)                 │ │ │
│  │  │     → TLS handshake                             │ │ │
│  │  │                    └─────────────────────────────┘ │ │
│  │  └──────────────────────────────────────────────────┘ │ │
│  └───────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

## Implementation Details

### 1. Gateway Endpoint Resolution (API Server)

When a device is created, the API Server resolves the gateway pod IP:

```rust
// File: crates/wasmbed-qemu-manager/src/lib.rs

// Extract host part (remove http:// prefix and port)
let host_part = gateway_ep
    .replace("http://", "")
    .replace("https://", "")
    .split(':')
    .next()
    .map(|s| s.to_string())
    .unwrap_or_else(|| gateway_ep.clone());

// Check if endpoint is a local address (old TCP bridge)
if host_part == "127.0.0.1" || host_part == "localhost" {
    // Use first available gateway pod directly
    eprintln!("Warning: Gateway endpoint {} is a local address, \
               will use first available gateway pod", gateway_ep);
    gateway_name = String::new();
}

// Get gateway pod IP using kubectl
let pod_ip_output = Command::new("kubectl")
    .args(&["get", "pods", "-n", "wasmbed", 
            "-l", "app=wasmbed-gateway", 
            "-o", "jsonpath={.items[0].status.podIP}"])
    .output();

// TLS port is 8081 (gateway's TLS port)
let tls_port = 8081;
let gateway_pod_endpoint = format!("{}:{}", pod_ip, tls_port);
// Result: "10.42.0.12:8081"
```

### 2. Writing Endpoint to Memory (Renode Script)

The endpoint is written to memory before Zephyr starts:

```rust
// File: crates/wasmbed-qemu-manager/src/lib.rs

let endpoint_bytes = gateway_endpoint_str.as_bytes();
let mut endpoint_write_commands = String::new();

// Write length at 0x20001000
endpoint_write_commands.push_str(&format!(
    "\nsysbus WriteDoubleWord 0x20001000 0x{:08x}", 
    endpoint_bytes.len()
));

// Write endpoint bytes (4 bytes at a time)
for (i, chunk) in endpoint_bytes.chunks(4).enumerate() {
    let mut word: u32 = 0;
    for (j, &byte) in chunk.iter().enumerate() {
        word |= (byte as u32) << (j * 8);
    }
    endpoint_write_commands.push_str(&format!(
        "\nsysbus WriteDoubleWord 0x{:08x} 0x{:08x}", 
        0x20001004 + (i as u32 * 4), 
        word
    ));
}
```

Example for `"10.42.0.12:8081"` (15 bytes):

```bash
sysbus WriteDoubleWord 0x20001000 0x0000000f  # Length: 15
sysbus WriteDoubleWord 0x20001004 0x342e3031  # "10.4" (ASCII)
sysbus WriteDoubleWord 0x20001008 0x2e302e32  # "2.0."
sysbus WriteDoubleWord 0x2000100c 0x383a3231  # "12:8"
sysbus WriteDoubleWord 0x20001010 0x00313830  # "081\0"
```

### 3. Reading Endpoint from Memory (Zephyr)

Zephyr reads the endpoint at boot:

```c
// File: zephyr-app/src/wasmbed_protocol.c

#define GATEWAY_ENDPOINT_ADDR 0x20001000

static int read_gateway_endpoint(void)
{
    // Read length from first 4 bytes
    uint32_t *length_ptr = (uint32_t *)GATEWAY_ENDPOINT_ADDR;
    uint32_t length = *length_ptr;
    
    if (length == 0 || length >= sizeof(gateway_endpoint)) {
        LOG_ERR("Invalid endpoint length: %u", length);
        return -1;
    }
    
    // Read endpoint string from memory
    char *endpoint_ptr = (char *)(GATEWAY_ENDPOINT_ADDR + 4);
    memcpy(gateway_endpoint, endpoint_ptr, length);
    gateway_endpoint[length] = '\0';
    
    LOG_INF("Read gateway endpoint from memory: %s (length: %u)", 
            gateway_endpoint, length);
    return 0;
}
```

### 4. Parsing and Connecting (Zephyr)

```c
// File: zephyr-app/src/wasmbed_protocol.c

int wasmbed_protocol_init(void)
{
    // Read gateway endpoint from memory
    if (read_gateway_endpoint() != 0) {
        LOG_ERR("Failed to read gateway endpoint from memory");
        return -1;
    }
    
    // Parse endpoint (format: "host:port")
    char host[64];
    uint16_t port;
    if (parse_endpoint(gateway_endpoint, host, sizeof(host), &port) == 0) {
        LOG_INF("Connecting to gateway with TLS: %s:%u", host, port);
        
        // Connect to gateway with TLS
        if (network_connect_tls(host, port) == 0) {
            gateway_connected = true;
            LOG_INF("Connected to gateway via TLS");
        } else {
            LOG_ERR("Failed to connect to gateway with TLS");
        }
    }
    
    return 0;
}
```

### 5. TLS Connection (Zephyr Network Stack)

```c
// File: zephyr-app/src/network_handler.c

int network_connect_tls(const char *host, uint16_t port)
{
    // Create TCP socket
    socket_fd = zsock_socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    
    // Configure TLS before connecting
    // Set TLS hostname for SNI (Server Name Indication)
    int ret = zsock_setsockopt(socket_fd, SOL_TLS, TLS_HOSTNAME, 
                                host, strlen(host) + 1);
    
    // Setup address structure
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);
    
    // Resolve hostname to IP
    if (net_addr_pton(AF_INET, host, &addr.sin_addr) < 0) {
        LOG_ERR("Invalid IP address: %s", host);
        return -1;
    }
    
    // Connect to server (TLS handshake happens during connect)
    if (zsock_connect(socket_fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        LOG_ERR("Failed to connect: %d", errno);
        return -1;
    }
    
    LOG_INF("Connected to gateway with TLS: %s:%u", host, port);
    return 0;
}
```

## Persistence

The TLS connection is persistent:

1. **Endpoint Resolution**: Done once at device creation
2. **Storage**: Endpoint stored in device object in memory
3. **Renode Restarts**: Endpoint written to memory each time Renode starts
4. **Zephyr Reads**: Zephyr reads the same endpoint on each boot
5. **Connection**: TLS connection established and maintained until device stops

### Verification

Check that the endpoint is correct:

```bash
# 1. Check gateway pod IP
kubectl get pods -n wasmbed -l app=wasmbed-gateway -o wide

# 2. Check API server logs for endpoint resolution
API_POD=$(kubectl get pods -n wasmbed -l app=wasmbed-api-server \
          -o jsonpath='{.items[0].metadata.name}')
kubectl logs -n wasmbed $API_POD | grep "Resolved gateway endpoint"

# 3. Check endpoint written to memory
kubectl exec -n wasmbed $API_POD -- \
    cat /tmp/gateway_endpoint_memory_DEVICE_ID.txt

# 4. Check Renode script
kubectl exec -n wasmbed $API_POD -- \
    cat /tmp/renode_prewrite_DEVICE_ID.resc | grep "sysbus WriteDoubleWord"
```

Expected output:
```
Resolved gateway endpoint for device DEVICE_ID: 10.42.0.12:8081
Writing gateway endpoint to memory for device DEVICE_ID: 10.42.0.12:8081

sysbus WriteDoubleWord 0x20001000 0x0000000f
sysbus WriteDoubleWord 0x20001004 0x342e3031
sysbus WriteDoubleWord 0x20001008 0x2e302e32
sysbus WriteDoubleWord 0x2000100c 0x383a3231
sysbus WriteDoubleWord 0x20001010 0x00313830
```

## Security Considerations

### Development Mode
- Gateway uses self-signed certificates
- Devices skip certificate verification
- Suitable for testing and development

### Production Mode
To enable proper TLS security:

1. **Generate CA Certificate**:
   ```bash
   openssl req -x509 -newkey rsa:4096 -keyout ca-key.pem \
       -out ca-cert.pem -days 365 -nodes
   ```

2. **Generate Gateway Certificate**:
   ```bash
   openssl req -newkey rsa:4096 -keyout gateway-key.pem \
       -out gateway-csr.pem -nodes
   openssl x509 -req -in gateway-csr.pem -CA ca-cert.pem \
       -CAkey ca-key.pem -CAcreateserial -out gateway-cert.pem -days 365
   ```

3. **Configure Zephyr** to verify certificates:
   ```c
   // Add CA certificate to TLS credentials
   tls_credential_add(CA_CERTIFICATE_TAG, TLS_CREDENTIAL_CA_CERTIFICATE,
                      ca_cert, sizeof(ca_cert));
   
   // Set security tag list
   sec_tag_t sec_tag_list[] = { CA_CERTIFICATE_TAG };
   zsock_setsockopt(socket_fd, SOL_TLS, TLS_SEC_TAG_LIST,
                    sec_tag_list, sizeof(sec_tag_list));
   ```

## Troubleshooting

### Device Not Connecting

1. **Check gateway pod IP**:
   ```bash
   kubectl get pods -n wasmbed -l app=wasmbed-gateway -o wide
   ```

2. **Verify gateway is listening on port 8081**:
   ```bash
   GATEWAY_POD=$(kubectl get pods -n wasmbed -l app=wasmbed-gateway \
                 -o jsonpath='{.items[0].metadata.name}')
   kubectl logs -n wasmbed $GATEWAY_POD | grep "Starting TLS server"
   ```

3. **Test TLS connection from host**:
   ```bash
   curl -k https://10.42.0.12:8081/health
   ```

4. **Check Zephyr logs** (if UART is working):
   ```bash
   docker logs renode-DEVICE_ID 2>&1 | grep "Connecting to gateway"
   ```

### Endpoint Still Shows 127.0.0.1

This means the endpoint resolution failed. Check:

1. **API server has kubectl access**:
   ```bash
   kubectl exec -n wasmbed $API_POD -- kubectl get pods -n wasmbed
   ```

2. **ServiceAccount permissions**:
   ```bash
   kubectl get rolebinding -n wasmbed wasmbed-api-server -o yaml
   ```

3. **Gateway pod is running**:
   ```bash
   kubectl get pods -n wasmbed -l app=wasmbed-gateway
   ```

### TLS Handshake Fails

1. **Check gateway certificate**:
   ```bash
   openssl s_client -connect 10.42.0.12:8081 -showcerts
   ```

2. **Verify Zephyr TLS configuration**:
   - Check `prj.conf` has `CONFIG_NET_SOCKETS_SOCKOPT_TLS=y`
   - Check `CONFIG_MBEDTLS=y` is enabled

3. **Check gateway logs for TLS errors**:
   ```bash
   kubectl logs -n wasmbed $GATEWAY_POD | grep -i "tls\|handshake"
   ```

## Performance

### Connection Establishment Time
- DNS resolution: N/A (direct IP)
- TCP handshake: ~1-5ms (within cluster)
- TLS handshake: ~10-50ms (depending on CPU)
- **Total**: ~15-60ms

### Throughput
- TLS overhead: ~5-10% compared to plain TCP
- Typical throughput: 50-100 MB/s (within cluster)

### Latency
- Round-trip time: ~1-2ms (within cluster)
- TLS adds: ~0.5-1ms per message

## References

- [Zephyr TLS Sockets](https://docs.zephyrproject.org/latest/connectivity/networking/api/sockets.html#tls-sockets)
- [mbedTLS Documentation](https://tls.mbed.org/api/)
- [Renode Documentation](https://renode.readthedocs.io/)
- [Kubernetes Service DNS](https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/)
