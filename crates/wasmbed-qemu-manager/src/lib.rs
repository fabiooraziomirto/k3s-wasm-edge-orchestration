// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use std::process::{Command, Stdio};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use wasmbed_tcp_bridge::TcpBridge;
use base64::{engine::general_purpose, Engine};

/// Derive Gateway HTTP base URL from TLS/gateway endpoint.
/// Accepts either a full URL ("http://host:8080" or "http://host:8081") or host:port ("10.42.0.12:8081").
/// Board API is always on port 8080.
fn gateway_http_from_tls_endpoint(tls_endpoint: &str) -> String {
    let s = tls_endpoint.trim();
    if s.starts_with("http://") || s.starts_with("https://") {
        // Full URL: extract host and use port 8080 for the HTTP API
        let after_scheme = s
            .strip_prefix("http://")
            .or_else(|| s.strip_prefix("https://"))
            .unwrap_or(s);
        let host = after_scheme
            .splitn(2, ':')
            .next()
            .and_then(|h| if h.is_empty() { None } else { Some(h) })
            .unwrap_or_else(|| after_scheme.split('/').next().unwrap_or("127.0.0.1"));
        format!("http://{}:8080", host)
    } else {
        // host:port
        let host = s.splitn(2, ':').next().unwrap_or("127.0.0.1");
        format!("http://{}:8080", host)
    }
}

/// Translate a ClusterIP TLS endpoint (e.g. "10.43.192.162:8443") to a host-visible NodePort endpoint.
/// Emulated devices run on the host network (--net=host) but cannot reach ClusterIP addresses.
/// The NodePort address is read from two env vars:
///   RENODE_GATEWAY_HOST  (default: 192.168.100.179)
///   RENODE_GATEWAY_PORT  (default: 30443)
/// If the endpoint does not look like a Kubernetes ClusterIP (10.43.x.x) it is returned as-is.
fn translate_to_nodeport_endpoint(endpoint: &str) -> String {
    let s = endpoint.trim();
    // Only translate ClusterIP addresses (10.43.x.x range used by k3s services)
    let is_cluster_ip = s.starts_with("10.43.") || {
        // Also handle "hostname:port" style k8s DNS names
        let host = s.splitn(2, ':').next().unwrap_or("");
        host.ends_with(".svc.cluster.local") || host.ends_with(".wasmbed")
    };
    if is_cluster_ip {
        let host = std::env::var("RENODE_GATEWAY_HOST")
            .unwrap_or_else(|_| "192.168.1.1".to_string());
        let port = std::env::var("RENODE_GATEWAY_PORT")
            .unwrap_or_else(|_| "30443".to_string());
        format!("{}:{}", host, port)
    } else {
        s.to_string()
    }
}

/// Request body for board registration (matches Gateway's BoardRegisterRequest).
#[derive(Debug, Serialize)]
struct BoardRegisterRequest {
    device_id: String,
    endpoint: String,
    mcu_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<BoardCapabilitiesRequest>,
}

#[derive(Debug, Serialize)]
struct BoardCapabilitiesRequest {
    has_ethernet: bool,
    has_wifi: bool,
    has_network: bool,
}

/// Register the board (device proxy) with the Gateway. Called after device proxy starts.
async fn register_board_with_gateway(
    gateway_http: &str,
    device_id: &str,
    endpoint: &str,
    mcu_type: &str,
    has_ethernet: bool,
    has_wifi: bool,
) {
    let url = format!("{}/api/v1/board/register", gateway_http.trim_end_matches('/'));
    let body = BoardRegisterRequest {
        device_id: device_id.to_string(),
        endpoint: endpoint.to_string(),
        mcu_type: mcu_type.to_string(),
        capabilities: Some(BoardCapabilitiesRequest {
            has_ethernet,
            has_wifi,
            has_network: has_ethernet || has_wifi,
        }),
    };
    match reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                eprintln!("Board registered with Gateway: device_id={}", device_id);
            } else {
                eprintln!(
                    "Gateway board registration returned status {} for device_id={}",
                    resp.status(),
                    device_id
                );
            }
        }
        Err(e) => {
            eprintln!(
                "Failed to register board with Gateway for device_id={}: {}",
                device_id, e
            );
        }
    }
}

/// Unregister the board from the Gateway. Called when device proxy stops.
async fn unregister_board_from_gateway(gateway_http: &str, device_id: &str) {
    let url = format!("{}/api/v1/board/{}", gateway_http.trim_end_matches('/'), device_id);
    match reqwest::Client::new().delete(&url).send().await {
        Ok(resp) => {
            if resp.status().is_success() || resp.status().as_u16() == 204 || resp.status().as_u16() == 404 {
                eprintln!("Board unregistered from Gateway: device_id={}", device_id);
            } else {
                eprintln!(
                    "Gateway board unregister returned status {} for device_id={}",
                    resp.status(),
                    device_id
                );
            }
        }
        Err(e) => {
            eprintln!(
                "Failed to unregister board from Gateway for device_id={}: {}",
                device_id, e
            );
        }
    }
}

// --- Per-device Renode containers ---
const WASMBED_RENODE_CONTAINER_NAME: &str = "wasmbed-renode";
const WASMBED_FIRMWARE_VOLUME: &str = "wasmbed-firmware-store";

/// Build a unique, stable Docker container name for one device's Renode instance.
fn renode_container_name(device_id: &str) -> String {
    // Use a prefix of the device_id to keep names short and valid for Docker.
    let suffix = &device_id[..device_id.len().min(16)];
    format!("{}-{}", WASMBED_RENODE_CONTAINER_NAME, suffix)
}
const RENODE_MONITOR_PORT: u16 = 9999;

/// Ensure the singleton Renode container is running. Start it if not.
fn ensure_renode_container_running() -> Result<(), anyhow::Error> {
    let check = Command::new("docker")
        .args(&["ps", "-a", "--filter", &format!("name={}", WASMBED_RENODE_CONTAINER_NAME), "--format", "{{.Names}} {{.Status}}"])
        .output()?;
    let out = String::from_utf8_lossy(&check.stdout);
    let running = out.contains(WASMBED_RENODE_CONTAINER_NAME) && (out.contains("Up") || out.contains("running"));
    if running {
        return Ok(());
    }
    // Remove if exists but stopped
    let _ = Command::new("docker")
        .args(&["rm", "-f", WASMBED_RENODE_CONTAINER_NAME])
        .output();
    // Create volume if needed
    let _ = Command::new("docker")
        .args(&["volume", "create", WASMBED_FIRMWARE_VOLUME])
        .output();
    // Start singleton Renode with monitor on TCP port.
    // -t allocates a pseudo-TTY so Renode's stdin never receives EOF → process stays alive.
    // Passing args directly (not via sh -c) avoids an extra shell layer.
    let status = Command::new("docker")
        .args(&[
            "run", "-dt", "--restart=unless-stopped",
            "--net=host",
            "--name", WASMBED_RENODE_CONTAINER_NAME,
            "-v", &format!("{}:/firmware:ro", WASMBED_FIRMWARE_VOLUME),
            "antmicro/renode:nightly",
            "renode", "-P", &RENODE_MONITOR_PORT.to_string(), "--plain",
        ])
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to start Renode container"));
    }
    // Give Renode time to initialize and bind the TCP port (typically 5-8s)
    std::thread::sleep(Duration::from_secs(8));
    Ok(())
}

/// Copy firmware into the shared volume under /firmware/<device_id>/<filename>.
fn copy_firmware_to_shared_volume(device_id: &str, firmware_path: &std::path::Path, firmware_filename: &str) -> Result<(), anyhow::Error> {
    let parent = firmware_path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
    let create_and_copy = Command::new("docker")
        .args(&[
            "run", "--rm",
            "-v", &format!("{}:/firmware", WASMBED_FIRMWARE_VOLUME),
            "-v", &format!("{}:/source:ro", parent),
            "alpine:latest",
            "sh", "-c",
            &format!("mkdir -p /firmware/{} && cp /source/{} /firmware/{}/{}", device_id, firmware_filename, device_id, firmware_filename),
        ])
        .output()?;
    if !create_and_copy.status.success() {
        let stderr = String::from_utf8_lossy(&create_and_copy.stderr);
        return Err(anyhow::anyhow!("Failed to copy firmware to shared volume: {}", stderr));
    }
    Ok(())
}

/// Send monitor commands to the running Renode instance (TCP, line-based).
/// Retries the connection up to 5 times with 2s backoff (Renode may still be initialising).
async fn send_renode_monitor_commands(addr: &str, commands: &str) -> Result<(), anyhow::Error> {
    let mut last_err = anyhow::anyhow!("connection not attempted");
    for attempt in 0..5u32 {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(mut stream) => {
                // Wait briefly for Renode's welcome banner before sending commands
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let filtered: String = commands
                    .lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(|l| format!("{}\n", l))
                    .collect();
                stream.write_all(filtered.as_bytes()).await?;
                stream.flush().await?;
                return Ok(());
            }
            Err(e) => {
                eprintln!("Renode monitor not ready at {} (attempt {}): {}", addr, attempt + 1, e);
                last_err = anyhow::anyhow!("{}", e);
                if attempt < 4 {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
    Err(anyhow::anyhow!("Failed to connect to Renode monitor at {}: {}", addr, last_err))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QemuDevice {
    pub id: String,
    pub name: String,
    pub architecture: String,
    pub device_type: String,
    pub mcu_type: McuType,
    pub status: QemuDeviceStatus,
    pub process_id: Option<u32>,
    pub endpoint: String,
    pub gateway_endpoint: Option<String>, // Gateway endpoint for TLS connection
    pub wasm_runtime: Option<WasmRuntime>,
}

/// Supported MCU types with Renode compatibility
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum McuType {
    // ===== Official Zephyr Boards with Ethernet (Recommended) =====
    /// STM32F746G Discovery (ARM Cortex-M7, Ethernet) - Best for network testing
    #[serde(alias = "Stm32f746gDisco", alias = "stm32f746g_disco")]
    Stm32F746gDisco,
    
    /// FRDM-K64F (ARM Cortex-M4, Ethernet) - Well supported
    #[serde(alias = "FrdmK64f", alias = "frdm_k64f")]
    FrdmK64f,
    
    // ===== Official Zephyr Boards with WiFi =====
    /// ESP32 DevKitC (Xtensa LX6, WiFi/BLE) - WiFi support
    #[serde(alias = "Esp32DevkitC", alias = "esp32_devkitc_wroom")]
    Esp32DevkitC,
    
    // ===== Official Zephyr Boards (No Ethernet/WiFi) =====
    /// STM32F4 Discovery (ARM Cortex-M4) - No network
    #[serde(alias = "Stm32F4Disco", alias = "stm32f4_discovery")]
    Stm32F4Disco,
    
    /// nRF52840 DK (ARM Cortex-M4, BLE only) - No Ethernet/WiFi
    #[serde(alias = "Nrf52840DK", alias = "nrf52840dk_nrf52840")]
    Nrf52840DK,
    
    // ===== Legacy Boards =====
    /// Legacy: Renode Arduino Nano 33 BLE (ARM Cortex-M4)
    RenodeArduinoNano33Ble,
    
    /// Legacy: Renode STM32F4 Discovery (ARM Cortex-M4)
    RenodeStm32F4Discovery,
    
    /// Legacy: ARM MPS2-AN385 (ARM Cortex-M3) - Maps to RenodeArduinoNano33Ble
    #[serde(alias = "Mps2An385")]
    Mps2An385,

    // ===== Linux Devices (native, not Renode-emulated) =====
    /// Linux ARM64: Raspberry Pi 4/5, ARM64 SBC, industrial ARM gateway
    #[serde(alias = "LinuxArm64", alias = "linux_arm64")]
    LinuxArm64,

    /// Linux x86_64: edge VM, industrial x86 gateway, mini-PC
    #[serde(alias = "LinuxX86_64", alias = "linux_x86_64")]
    LinuxX86_64,

    /// Linux RISC-V: RISC-V Linux devices (SiFive, etc.)
    #[serde(alias = "LinuxRiscV", alias = "linux_riscv")]
    LinuxRiscV,
}

impl McuType {
    /// Returns true if this device type is emulated in Renode.
    /// Linux native devices register autonomously via board/register API.
    pub fn is_emulated(&self) -> bool {
        !matches!(self, McuType::LinuxArm64 | McuType::LinuxX86_64 | McuType::LinuxRiscV)
    }

    /// Get Renode platform name for this MCU
    pub fn renode_platform(&self) -> &'static str {
        match self {
            // Official Zephyr boards with Ethernet
            McuType::Stm32F746gDisco => "stm32f7_discovery-bb",
            McuType::FrdmK64f => "frdm_k64f",

            // Official Zephyr boards with WiFi
            McuType::Esp32DevkitC => "esp32",

            // Official Zephyr boards (no network)
            McuType::Stm32F4Disco => "stm32f4_discovery",
            McuType::Nrf52840DK => "nrf52840dk_nrf52840",

            // Legacy boards
            McuType::RenodeArduinoNano33Ble => "arduino_nano_33_ble",
            McuType::RenodeStm32F4Discovery => "stm32f4_discovery",
            McuType::Mps2An385 => "arduino_nano_33_ble",

            // Linux native devices — not managed by Renode
            McuType::LinuxArm64 | McuType::LinuxX86_64 | McuType::LinuxRiscV => "",
        }
    }

    /// Get CPU architecture for this MCU
    pub fn cpu_architecture(&self) -> &'static str {
        match self {
            // Cortex-M7 (high performance)
            McuType::Stm32F746gDisco => "cortex-m7",

            // Cortex-M4 (standard)
            McuType::FrdmK64f => "cortex-m4",
            McuType::Stm32F4Disco => "cortex-m4",
            McuType::Nrf52840DK => "cortex-m4",
            McuType::RenodeArduinoNano33Ble => "cortex-m4",
            McuType::RenodeStm32F4Discovery => "cortex-m4",

            // Xtensa (ESP32)
            McuType::Esp32DevkitC => "xtensa-lx6",

            // Cortex-M3 (legacy)
            McuType::Mps2An385 => "cortex-m3",

            // Linux native
            McuType::LinuxArm64 => "aarch64",
            McuType::LinuxX86_64 => "x86_64",
            McuType::LinuxRiscV => "riscv64",
        }
    }

    /// Get memory size for this MCU
    pub fn memory_size(&self) -> &'static str {
        match self {
            // High-end MCUs
            McuType::Stm32F746gDisco => "320K", // 320KB RAM
            McuType::FrdmK64f => "256K", // 256KB RAM
            McuType::Esp32DevkitC => "520K", // 520KB SRAM

            // Mid-range MCUs
            McuType::Nrf52840DK => "256K",
            McuType::RenodeArduinoNano33Ble => "256K",

            // Standard MCUs
            McuType::Stm32F4Disco => "192K",
            McuType::RenodeStm32F4Discovery => "192K",

            // Legacy
            McuType::Mps2An385 => "512K",

            // Linux native — actual memory reported at runtime via DeviceInfo CBOR
            McuType::LinuxArm64 | McuType::LinuxX86_64 | McuType::LinuxRiscV => "unknown",
        }
    }

    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            // Ethernet boards (recommended)
            McuType::Stm32F746gDisco => "STM32F746G Discovery (Ethernet)",
            McuType::FrdmK64f => "FRDM-K64F (Ethernet)",

            // WiFi boards
            McuType::Esp32DevkitC => "ESP32 DevKitC (WiFi)",

            // No network boards
            McuType::Stm32F4Disco => "STM32F4 Discovery",
            McuType::Nrf52840DK => "nRF52840 DK (BLE)",

            // Legacy
            McuType::RenodeArduinoNano33Ble => "Arduino Nano 33 BLE (Renode)",
            McuType::RenodeStm32F4Discovery => "STM32F4 Discovery (Renode)",
            McuType::Mps2An385 => "ARM MPS2-AN385 (Legacy)",

            // Linux native
            McuType::LinuxArm64 => "Linux ARM64 (native)",
            McuType::LinuxX86_64 => "Linux x86_64 (native)",
            McuType::LinuxRiscV => "Linux RISC-V (native)",
        }
    }

    /// Get Rust HAL crate name (if available)
    pub fn rust_hal_crate(&self) -> Option<&'static str> {
        match self {
            // STM32 boards
            McuType::Stm32F746gDisco => Some("stm32f7xx-hal"),
            McuType::Stm32F4Disco => Some("stm32f4xx-hal"),
            McuType::RenodeStm32F4Discovery => Some("stm32f4xx-hal"),

            // NXP/Freescale boards
            McuType::FrdmK64f => Some("kinetis-hal"),

            // Nordic boards
            McuType::Nrf52840DK => Some("nrf52840-hal"),
            McuType::RenodeArduinoNano33Ble => Some("nrf52840-hal"),

            // ESP32
            McuType::Esp32DevkitC => Some("esp32-hal"),

            // Legacy / Linux native — no bare-metal HAL
            McuType::Mps2An385
            | McuType::LinuxArm64
            | McuType::LinuxX86_64
            | McuType::LinuxRiscV => None,
        }
    }

    /// Get firmware path for this MCU type (relative to zephyr-workspace).
    /// Returns empty string for Linux native devices (no Zephyr firmware).
    pub fn get_firmware_path(&self) -> &'static str {
        match self {
            // Ethernet boards (recommended)
            McuType::Stm32F746gDisco => "build/stm32f746g_disco/zephyr/zephyr.elf",
            McuType::FrdmK64f => "build/frdm_k64f/zephyr/zephyr.elf",

            // WiFi boards
            McuType::Esp32DevkitC => "build/esp32_devkitc_wroom/zephyr/zephyr.elf",

            // No network boards
            McuType::Stm32F4Disco => "build/stm32f4/zephyr/zephyr.elf",
            McuType::Nrf52840DK => "build/nrf52840dk/nrf52840/zephyr/zephyr.elf",

            // Legacy
            McuType::RenodeArduinoNano33Ble => "build/arduino_nano_33_ble/zephyr/zephyr.elf",
            McuType::RenodeStm32F4Discovery => "build/stm32f4_discovery/zephyr/zephyr.elf",
            McuType::Mps2An385 => "build/mps2_an385/zephyr/zephyr.elf",

            // Linux native — no Zephyr firmware; device runs wasmbed-edge-client
            McuType::LinuxArm64 | McuType::LinuxX86_64 | McuType::LinuxRiscV => "",
        }
    }

    /// Get UART peripheral name for this MCU type
    pub fn get_uart_name(&self) -> &'static str {
        match self {
            // STM32 boards use USART
            McuType::Stm32F746gDisco => "usart1",
            McuType::Stm32F4Disco => "usart2",
            McuType::RenodeStm32F4Discovery => "usart2",

            // NXP/Freescale boards
            McuType::FrdmK64f => "uart0",

            // Nordic boards
            McuType::Nrf52840DK => "uart0",
            McuType::RenodeArduinoNano33Ble => "uart0",

            // ESP32
            McuType::Esp32DevkitC => "uart0",

            // Legacy
            McuType::Mps2An385 => "uart0",

            // Linux native — no UART peripheral applicable
            McuType::LinuxArm64 | McuType::LinuxX86_64 | McuType::LinuxRiscV => "",
        }
    }

    /// Get all supported MCU types
    pub fn all_types() -> Vec<McuType> {
        vec![
            // Ethernet boards (recommended for network testing)
            McuType::Stm32F746gDisco,
            McuType::FrdmK64f,

            // WiFi boards
            McuType::Esp32DevkitC,

            // No network boards
            McuType::Stm32F4Disco,
            McuType::Nrf52840DK,

            // Legacy boards
            McuType::RenodeArduinoNano33Ble,
            McuType::RenodeStm32F4Discovery,

            // Linux native devices
            McuType::LinuxArm64,
            McuType::LinuxX86_64,
            McuType::LinuxRiscV,
        ]
    }

    /// Check if this MCU type has Ethernet support
    pub fn has_ethernet(&self) -> bool {
        matches!(
            self,
            McuType::Stm32F746gDisco
                | McuType::FrdmK64f
                | McuType::LinuxArm64
                | McuType::LinuxX86_64
                | McuType::LinuxRiscV
        )
    }

    /// Check if this MCU type has WiFi support
    pub fn has_wifi(&self) -> bool {
        matches!(self, McuType::Esp32DevkitC)
    }
    
    /// Check if this MCU type has network support (Ethernet or WiFi)
    pub fn has_network(&self) -> bool {
        self.has_ethernet() || self.has_wifi()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QemuDeviceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmRuntime {
    pub wasm_bytes: Vec<u8>,
    pub config: WasmConfig,
    pub status: WasmRuntimeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmConfig {
    pub memory_limit: u32,
    pub execution_timeout: u32,
    pub host_functions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WasmRuntimeStatus {
    NotLoaded,
    Loading,
    Running,
    Stopped,
    Error(String),
}

#[derive(Debug)]
pub struct RenodeManager {
    devices: Arc<Mutex<HashMap<String, QemuDevice>>>,
    renode_binary: String,
    base_port: u16,
    tcp_bridges: Arc<Mutex<HashMap<String, TcpBridge>>>,
}

impl RenodeManager {
    pub fn new(renode_binary: String, base_port: u16) -> Self {
        // Don't load from file - Kubernetes is the source of truth
        // Devices will be created on-demand when needed
        // This prevents orphaned devices from previous sessions
        
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            renode_binary,
            base_port,
            tcp_bridges: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Save devices to persistent storage (optional, for debugging)
    /// Note: Kubernetes is the source of truth, this is just a cache
    async fn save_devices(&self) -> Result<(), anyhow::Error> {
        // Optional: save to file for debugging, but don't rely on it
        // Kubernetes is the source of truth
        let devices = self.devices.lock().await;
        let devices_file = "qemu_devices.json";
        let content = serde_json::to_string_pretty(&*devices)?;
        println!("Saving {} devices to {} (cache only, Kubernetes is source of truth)", devices.len(), devices_file);
        std::fs::write(devices_file, content)?;
        Ok(())
    }

    pub async fn create_device(&self, id: String, name: String, architecture: String, device_type: String, mcu_type: McuType, endpoint: Option<String>) -> Result<QemuDevice, anyhow::Error> {
        let mut devices = self.devices.lock().await;
        
        if devices.contains_key(&id) {
            return Err(anyhow::anyhow!("Device {} already exists", id));
        }

        let device = QemuDevice {
            id: id.clone(),
            name,
            architecture,
            device_type,
            mcu_type,
            status: QemuDeviceStatus::Stopped,
            process_id: None,
            endpoint: endpoint.unwrap_or_else(|| format!("127.0.0.1:{}", self.base_port + devices.len() as u16)),
            gateway_endpoint: None, // Will be set when starting device
            wasm_runtime: None,
        };

        devices.insert(id.clone(), device.clone());
        
        // Save to persistent storage
        drop(devices); // Release the lock before async call
        println!("About to save devices to persistent storage");
        match self.save_devices().await {
            Ok(_) => println!("Successfully saved devices to persistent storage"),
            Err(e) => println!("Failed to save devices to persistent storage: {}", e),
        }
        
        Ok(device)
    }

    pub async fn start_device(&self, device_id: &str, gateway_endpoint: Option<String>) -> Result<(), anyhow::Error> {
        let debug_log_path = std::env::temp_dir().join(format!("start_device_debug_{}.log", device_id));
        let _ = std::fs::write(&debug_log_path, format!("start_device called for: {}\n", device_id));
        eprintln!("DEBUG: start_device called for: {}", device_id);
        
        // ALWAYS load device from Kubernetes CRD first (CRD is source of truth)
        // This ensures we always have the correct mcuType even if device exists in memory with old data
        println!("Loading device {} from Kubernetes CRD (always, to ensure correct mcuType)", device_id);
        eprintln!("Loading device {} from Kubernetes CRD (always, to ensure correct mcuType)", device_id);
        
        let device_from_crd = {
            // Try to get device from Kubernetes
            let output = tokio::process::Command::new("kubectl")
                .args(&["get", "devices.wasmbed.github.io", device_id, "-n", "wasmbed", "-o", "json"])
                .output()
                .await;
            
            if let Ok(output) = output {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Ok(device_json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                        let spec = &device_json["spec"];
                        let metadata = &device_json["metadata"];
                        
                        let mcu_type_str = spec["mcuType"].as_str().unwrap_or("Stm32F746gDisco");
                        let mcu_type = match mcu_type_str {
                            "Stm32F746gDisco" => McuType::Stm32F746gDisco,
                            "FrdmK64f" => McuType::FrdmK64f,
                            "Esp32DevkitC" => McuType::Esp32DevkitC,
                            "Nrf52840DK" => McuType::Nrf52840DK,
                            "Stm32F4Disco" => McuType::Stm32F4Disco,
                            "RenodeArduinoNano33Ble" => McuType::RenodeArduinoNano33Ble,
                            "RenodeStm32F4Discovery" => McuType::RenodeStm32F4Discovery,
                            "Mps2An385" => McuType::Mps2An385,
                            "LinuxArm64" | "linux_arm64" => McuType::LinuxArm64,
                            "LinuxX86_64" | "linux_x86_64" => McuType::LinuxX86_64,
                            "LinuxRiscV" | "linux_riscv" => McuType::LinuxRiscV,
                            _ => {
                                eprintln!("Unknown MCU type '{}' for device {}, using Stm32F746gDisco as default", mcu_type_str, device_id);
                                McuType::Stm32F746gDisco
                            }
                        };
                        
                        let device_name = metadata["name"].as_str().unwrap_or(device_id).to_string();
                        let device_type = spec["deviceType"].as_str().unwrap_or("MCU").to_string();
                        let architecture = spec["architecture"].as_str().unwrap_or("ARM_CORTEX_M").to_string();
                        
                        println!("✅ Loaded device {} from CRD with mcuType: {:?}", device_id, mcu_type);
                        eprintln!("✅ Loaded device {} from CRD with mcuType: {:?}", device_id, mcu_type);
                        
                        Some(QemuDevice {
                            id: device_id.to_string(),
                            name: device_name,
                            architecture,
                            device_type,
                            mcu_type,
                            status: QemuDeviceStatus::Stopped,
                            process_id: None,
                            endpoint: format!("127.0.0.1:{}", self.base_port),
                            gateway_endpoint: None,
                            wasm_runtime: None,
                        })
                    } else {
                        eprintln!("Failed to parse device JSON from Kubernetes for {}", device_id);
                        None
                    }
                } else {
                    eprintln!("Device {} not found in Kubernetes CRD", device_id);
                    None
                }
            } else {
                None
            }
        };
        
        let mut devices = self.devices.lock().await;
        
        // Update device in memory with data from CRD (always, to ensure correct mcuType)
        if let Some(device_from_crd) = device_from_crd {
            let existing_status = devices.get(device_id).map(|d| d.status.clone());
            let existing_process_id = devices.get(device_id).and_then(|d| d.process_id);
            let existing_endpoint = devices.get(device_id).map(|d| d.endpoint.clone());
            
            // Preserve existing status and process_id if device is running
            let mut device = device_from_crd;
            if let Some(status) = existing_status {
                device.status = status;
            }
            if let Some(pid) = existing_process_id {
                device.process_id = Some(pid);
            }
            if let Some(ep) = existing_endpoint {
                device.endpoint = ep;
            }
            
            println!("✅ Updating device {} in memory with data from CRD (mcuType: {:?})", device_id, device.mcu_type);
            eprintln!("✅ Updating device {} in memory with data from CRD (mcuType: {:?})", device_id, device.mcu_type);
            devices.insert(device_id.to_string(), device);
        } else if !devices.contains_key(device_id) {
            return Err(anyhow::anyhow!("Device {} not found in Kubernetes CRD and not in memory", device_id));
        }
        
        let device = devices.get_mut(device_id)
            .ok_or_else(|| anyhow::anyhow!("Device {} not found after loading from CRD", device_id))?;

        if matches!(device.status, QemuDeviceStatus::Running) {
            return Err(anyhow::anyhow!("Device is already running"));
        }

        // Store current gateway_endpoint from device (might be old TCP bridge endpoint)
        let existing_gateway_endpoint = device.gateway_endpoint.clone();
        
        // Update gateway endpoint if provided (use reference to avoid moving option)
        // But don't save it yet - we'll resolve it first
        let endpoint_to_resolve = if let Some(ref gateway_ep) = gateway_endpoint {
            Some(gateway_ep.clone())
        } else {
            // If no endpoint provided, use existing one (might be old TCP bridge)
            existing_gateway_endpoint
        };
        
        // Drop lock before performing any other async operations to keep future Send
        drop(devices);
        
        // Resolve gateway endpoint to pod IP (firmware can connect directly with TLS, no TCP bridge needed)
        // The firmware uses network_connect_tls() which can connect directly to the gateway pod IP
        println!("DEBUG: start_device - gateway_endpoint parameter = {:?}", gateway_endpoint);
        eprintln!("DEBUG: start_device - gateway_endpoint parameter = {:?}", gateway_endpoint);
        println!("DEBUG: start_device - endpoint_to_resolve = {:?}", endpoint_to_resolve);
        eprintln!("DEBUG: start_device - endpoint_to_resolve = {:?}", endpoint_to_resolve);
        if let Some(gateway_ep) = endpoint_to_resolve {
            println!("DEBUG: start_device - Processing gateway endpoint: {}", gateway_ep);
            eprintln!("DEBUG: start_device - Processing gateway endpoint: {}", gateway_ep);
            
            // Extract host part first (remove http:// or https:// prefix and port)
            let host_part = gateway_ep
                .replace("http://", "")
                .replace("https://", "")
                .split(':')
                .next()
                .map(|s| s.to_string())
                .unwrap_or_else(|| gateway_ep.clone());
            
            // Check if endpoint is a local address (old TCP bridge) - if so, directly use first available gateway pod
            let gateway_name = if host_part == "127.0.0.1" || host_part == "localhost" {
                // Old TCP bridge endpoint - use first available gateway pod directly
                eprintln!("Warning: Gateway endpoint {} is a local address (old TCP bridge), will use first available gateway pod", gateway_ep);
                String::new() // Empty string to trigger fallback to first available pod
            } else if host_part.chars().all(|c| c.is_ascii_digit() || c == '.') && host_part.matches('.').count() == 3 {
                // It's a non-local IP address - try to get gateway name from device CRD
                eprintln!("Gateway endpoint {} is an IP address, trying to get gateway name from device CRD", gateway_ep);
                let device_crd_output = Command::new("kubectl")
                    .args(&["get", "devices.wasmbed.github.io", device_id, "-n", "wasmbed", "-o", "jsonpath={.status.gateway.name}"])
                    .output();
                
                if let Ok(output) = device_crd_output {
                    if output.status.success() {
                        let gateway_name_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !gateway_name_str.is_empty() {
                            gateway_name_str
                        } else {
                            eprintln!("Warning: Could not get gateway name from device CRD, will use first available gateway pod");
                            String::new() // Empty string to trigger fallback to first available pod
                        }
                    } else {
                        eprintln!("Warning: Failed to get gateway name from device CRD, will use first available gateway pod");
                        String::new() // Empty string to trigger fallback to first available pod
                    }
                } else {
                    eprintln!("Warning: Failed to execute kubectl to get gateway name, will use first available gateway pod");
                    String::new() // Empty string to trigger fallback to first available pod
                }
            } else {
                // It's a hostname - extract gateway name
                host_part
                    .split('.')
                    .next()
                    .unwrap_or(&host_part)
                    .replace("-service", "")
            };
            
            // Get TLS port (default 8443 - gateway service always uses 8443 for TLS, 8080 for HTTP)
            let tls_port = if gateway_ep.contains(":8443") {
                8443
            } else if gateway_ep.contains(":8081") {
                8081
            } else {
                // HTTP port or unknown: use 8443 for TLS
                8443
            };
            
            // Get gateway service ClusterIP (accessible from --net=host Renode container)
            eprintln!("Resolving gateway ClusterIP for gateway_name: '{}'", gateway_name);
            let svc_name = if !gateway_name.is_empty() {
                format!("{}-service", gateway_name)
            } else {
                // Try to extract service name directly from the original host_part
                if host_part.ends_with("-service") {
                    host_part.split('.').next().unwrap_or("gateway-1-service").to_string()
                } else {
                    format!("{}-service", host_part.split('.').next().unwrap_or("gateway-1"))
                }
            };
            eprintln!("Looking up ClusterIP for k8s service: {}", svc_name);
            let cluster_ip_output = Command::new("kubectl")
                .args(&["get", "svc", &svc_name, "-n", "wasmbed", "-o", "jsonpath={.spec.clusterIP}"])
                .output();
            
            if let Ok(output) = cluster_ip_output {
                if output.status.success() {
                    let cluster_ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !cluster_ip.is_empty() {
                        let gateway_pod_endpoint = format!("{}:{}", cluster_ip, tls_port);
                        println!("Resolved gateway endpoint for device {}: {} (service ClusterIP: {}, TLS port: {})", device_id, gateway_pod_endpoint, cluster_ip, tls_port);
                        eprintln!("Resolved gateway endpoint for device {}: {} (service ClusterIP: {}, TLS port: {})", device_id, gateway_pod_endpoint, cluster_ip, tls_port);
                        let _ = std::fs::write(
                            std::env::temp_dir().join(format!("gateway_endpoint_{}.txt", device_id)),
                            format!("Resolved gateway endpoint for device {}: {} (service ClusterIP: {}, TLS port: {})", device_id, gateway_pod_endpoint, cluster_ip, tls_port)
                        );
                        
                        // Store the resolved endpoint in device for later use in build_renode_args
                        let mut devices = self.devices.lock().await;
                        if let Some(device) = devices.get_mut(device_id) {
                            device.gateway_endpoint = Some(gateway_pod_endpoint);
                        }
                        drop(devices);
                    } else {
                        eprintln!("Warning: Could not resolve gateway ClusterIP for device {} (empty response), will use fallback", device_id);
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("Warning: Failed to get gateway ClusterIP for device {}: {} (status: {})", device_id, stderr, output.status);
                }
            } else {
                eprintln!("Warning: Failed to execute kubectl to get gateway ClusterIP for device {}, will use fallback", device_id);
            }
        } else {
            // No gateway endpoint provided - try to get from device CRD
            eprintln!("No gateway endpoint provided, getting gateway name from device CRD");
            let device_crd_output = Command::new("kubectl")
                .args(&["get", "devices.wasmbed.github.io", device_id, "-n", "wasmbed", "-o", "jsonpath={.status.gateway.name}"])
                .output();
            
            if let Ok(output) = device_crd_output {
                if output.status.success() {
                    let gateway_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !gateway_name.is_empty() {
                        // Use TLS port 8443 (gateway service always exposes 8443 for TLS)
                        let tls_port: u16 = 8443;
                        
                        // Get gateway service ClusterIP (accessible from --net=host Renode container)
                        let svc_name = format!("{}-service", gateway_name);
                        eprintln!("Resolving gateway ClusterIP for gateway_name: {}, service: {}", gateway_name, svc_name);
                        let cluster_ip_output = Command::new("kubectl")
                            .args(&["get", "svc", &svc_name, "-n", "wasmbed", "-o", "jsonpath={.spec.clusterIP}"])
                            .output();
                        
                        if let Ok(output) = cluster_ip_output {
                            if output.status.success() {
                                let cluster_ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                                if !cluster_ip.is_empty() {
                                    let gateway_pod_endpoint = format!("{}:{}", cluster_ip, tls_port);
                                    println!("Resolved gateway endpoint for device {}: {} (service ClusterIP: {}, TLS port: {})", device_id, gateway_pod_endpoint, cluster_ip, tls_port);
                                    eprintln!("Resolved gateway endpoint for device {}: {} (service ClusterIP: {}, TLS port: {})", device_id, gateway_pod_endpoint, cluster_ip, tls_port);
                                    let _ = std::fs::write(
                                        std::env::temp_dir().join(format!("gateway_endpoint_{}.txt", device_id)),
                                        format!("Resolved gateway endpoint for device {}: {} (service ClusterIP: {}, TLS port: {})", device_id, gateway_pod_endpoint, cluster_ip, tls_port)
                                    );
                                    
                                    // Store the resolved endpoint in device for later use in build_renode_args
                                    let mut devices = self.devices.lock().await;
                                    if let Some(device) = devices.get_mut(device_id) {
                                        device.gateway_endpoint = Some(gateway_pod_endpoint);
                                    }
                                    drop(devices);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Re-lock devices to continue with startup (no awaits after this point)
        let mut devices = self.devices.lock().await;
        let device = devices.get_mut(device_id)
            .ok_or_else(|| anyhow::anyhow!("Device {} not found", device_id))?;

        // CRITICAL: Log the device.mcu_type to verify it's correct before build_renode_args
        println!("DEBUG: Before build_renode_args - device.mcu_type = {:?}, device.id = {}", device.mcu_type, device.id);
        eprintln!("DEBUG: Before build_renode_args - device.mcu_type = {:?}, device.id = {}", device.mcu_type, device.id);
        
        // Log the device.gateway_endpoint after resolution
        println!("DEBUG: After resolution - device.gateway_endpoint = {:?}", device.gateway_endpoint);
        eprintln!("DEBUG: After resolution - device.gateway_endpoint = {:?}", device.gateway_endpoint);
        let _ = std::fs::write(
            std::env::temp_dir().join(format!("after_resolution_gateway_endpoint_{}.txt", device_id)),
            format!("device.gateway_endpoint = {:?}", device.gateway_endpoint)
        );

        device.status = QemuDeviceStatus::Starting;

        let use_docker = std::env::var("RENODE_USE_DOCKER").unwrap_or_else(|_| "true".to_string()) == "true";

        // Per-device Renode container using a .resc startup script (no TCP monitor required).
        // Each device gets its own container named wasmbed-renode-{device_id_prefix}.
        if use_docker {
            let firmware_path = self.get_firmware_path(&device.mcu_type)?;
            let firmware_filename = firmware_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("zephyr.elf"))
                .to_string_lossy()
                .to_string();
            let gateway_endpoint_str = device.gateway_endpoint.clone().unwrap_or_else(|| {
                let host = std::env::var("RENODE_GATEWAY_HOST").unwrap_or_else(|_| "192.168.1.1".to_string());
                let port = std::env::var("RENODE_GATEWAY_PORT").unwrap_or_else(|_| "30443".to_string());
                format!("{}:{}", host, port)
            });
            // In Docker mode the emulated device runs on the HOST network (--net=host), but
            // ClusterIP addresses are only reachable inside the k8s overlay. Translate the
            // ClusterIP endpoint to the host-visible NodePort endpoint so Zephyr firmware
            // can actually reach the gateway.
            let firmware_gateway_endpoint = translate_to_nodeport_endpoint(&gateway_endpoint_str);
            let gateway_http = device.gateway_endpoint.as_ref().map(|s| gateway_http_from_tls_endpoint(s.as_str()));
            let endpoint = device.endpoint.clone();
            let mcu_type_fmt = format!("{:?}", device.mcu_type);
            let has_eth = device.mcu_type.has_ethernet();
            let has_wifi = device.mcu_type.has_wifi();
            let _mcu_type_clone = device.mcu_type.clone();
            drop(devices);

            // Create shared volume if needed
            let _ = Command::new("docker")
                .args(&["volume", "create", WASMBED_FIRMWARE_VOLUME])
                .output();

            // Copy firmware into the shared volume
            copy_firmware_to_shared_volume(device_id, &firmware_path, &firmware_filename)?;

            // Build the .resc startup script
            let resc_content = {
                let devices = self.devices.lock().await;
                let device = devices.get(device_id).ok_or_else(|| anyhow::anyhow!("Device not found"))?;
                self.build_resc_script(
                    device,
                    device_id,
                    &firmware_gateway_endpoint,
                    &format!("/firmware/{}/{}", device_id, firmware_filename),
                ).map_err(|e| anyhow::anyhow!("build_resc_script: {}", e))?
            };

            // Write the .resc file to the zephyr-workspace host path.
            // The api-server pod mounts ZEPHYR_WORKSPACE as a hostPath volume (same path as on host),
            // so std::fs::write here lands on the HOST filesystem where Docker can also read it.
            let zephyr_ws = std::env::var("ZEPHYR_WORKSPACE")
                .unwrap_or_else(|_| "/home/ubuntu/retrospect/zephyr-workspace".to_string());
            let resc_dir = format!("{}/renode-scripts", zephyr_ws);
            std::fs::create_dir_all(&resc_dir)
                .map_err(|e| anyhow::anyhow!("Failed to create renode-scripts dir: {}", e))?;
            let resc_host_path = format!("{}/{}.resc", resc_dir, device_id);
            std::fs::write(&resc_host_path, &resc_content)
                .map_err(|e| anyhow::anyhow!("Failed to write .resc file: {}", e))?;

            eprintln!("Renode .resc written to {} for device {}:\n{}", resc_host_path, device_id, resc_content);

            // Remove any stale container for this device
            let container_name = renode_container_name(device_id);
            let _ = Command::new("docker")
                .args(&["rm", "-f", &container_name])
                .output();

            // Start per-device Renode container.
            // Mount the host-side zephyr-workspace (for the .resc script) and the firmware volume (for the ELF).
            // -t allocates a pseudo-TTY so Renode's stdin never gets EOF → simulation keeps running.
            // --cap-add=NET_ADMIN + --device=/dev/net/tun: required for TAP/TUN ethernet emulation.
            let resc_container_path = format!("/scripts/{}.resc", device_id);
            let status = Command::new("docker")
                .args(&[
                    "run", "-dt",
                    "--net=host",
                    "--cap-add=NET_ADMIN",
                    "--device=/dev/net/tun",
                    "--name", &container_name,
                    "-v", &format!("{}:/scripts:ro", resc_dir),
                    "-v", &format!("{}:/firmware:ro", WASMBED_FIRMWARE_VOLUME),
                    "antmicro/renode:nightly",
                    "renode", "--plain",
                    &resc_container_path,
                ])
                .status()?;

            if !status.success() {
                return Err(anyhow::anyhow!("Failed to start Renode container for device {}", device_id));
            }

            // Give Renode a moment to start the simulation
            std::thread::sleep(Duration::from_secs(4));

            if let Some(ref gw) = gateway_http {
                register_board_with_gateway(gw, device_id, &endpoint, &mcu_type_fmt, has_eth, has_wifi).await;
            }

            let mut devices = self.devices.lock().await;
            if let Some(device) = devices.get_mut(device_id) {
                device.status = QemuDeviceStatus::Running;
                device.process_id = None;
            }
            return Ok(());
        }

        // Per-device Renode process (native / no Docker)
        // Start Renode process
        let renode_args = self.build_renode_args(device, device_id)?;
        println!("Starting Renode with args: {:?}", renode_args);
        let mut cmd = Command::new(&renode_args[0]);
        cmd.args(&renode_args[1..]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                device.process_id = Some(child.id());
                device.status = QemuDeviceStatus::Running;

                // Register board with Gateway (so Gateway knows this device proxy is up)
                let device_id_clone = device_id.to_string();
                let gateway_http = device
                    .gateway_endpoint
                    .as_ref()
                    .map(|s| gateway_http_from_tls_endpoint(s.as_str()));
                let endpoint = device.endpoint.clone();
                let mcu_type_fmt = format!("{:?}", device.mcu_type);
                let has_eth = device.mcu_type.has_ethernet();
                let has_wifi = device.mcu_type.has_wifi();
                drop(devices);
                if let Some(gw) = gateway_http {
                    register_board_with_gateway(
                        &gw,
                        &device_id_clone,
                        &endpoint,
                        &mcu_type_fmt,
                        has_eth,
                        has_wifi,
                    )
                    .await;
                }
                let devices_clone = self.devices.clone();
                
                // Spawn a thread to monitor the process
                
                // Don't wait for the process to exit - Renode should stay running
                // Instead, just spawn a detached thread that monitors but doesn't block
                std::thread::spawn(move || {
                    // Give Renode time to start and accept connections
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    
                    // Check if process is still running
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            println!("Renode process for device {} exited early with status: {:?}", device_id_clone, status);
                            // Process ended, update status
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                let mut devices = devices_clone.lock().await;
                                if let Some(device) = devices.get_mut(&device_id_clone) {
                                    device.status = QemuDeviceStatus::Stopped;
                                    device.process_id = None;
                                    println!("Updated device {} status to Stopped", device_id_clone);
                                }
                            });
                        }
                        Ok(None) => {
                            // Process is still running, which is good
                            println!("Renode process for device {} is running", device_id_clone);
                            // Continue monitoring in background
                            let exit_status = child.wait();
                            println!("Renode process for device {} exited with status: {:?}", device_id_clone, exit_status);
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                let mut devices = devices_clone.lock().await;
                                if let Some(device) = devices.get_mut(&device_id_clone) {
                                    device.status = QemuDeviceStatus::Stopped;
                                    device.process_id = None;
                                    println!("Updated device {} status to Stopped", device_id_clone);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Error checking Renode process for device {}: {}", device_id_clone, e);
                        }
                    }
                });

                Ok(())
            }
            Err(e) => {
                device.status = QemuDeviceStatus::Error(e.to_string());
                Err(anyhow::anyhow!("Failed to start QEMU: {}", e))
            }
        }
    }

    pub async fn stop_device(&self, device_id: &str) -> Result<(), anyhow::Error> {
        let mut guard = Some(self.devices.lock().await);
        let device = guard.as_deref_mut().unwrap().get_mut(device_id)
            .ok_or_else(|| anyhow::anyhow!("Device {} not found", device_id))?;

        let gateway_http = device
            .gateway_endpoint
            .as_ref()
            .map(|s| gateway_http_from_tls_endpoint(s.as_str()));

        let use_docker =
            std::env::var("RENODE_USE_DOCKER").unwrap_or_else(|_| "true".to_string()) == "true";

        if use_docker {
            device.status = QemuDeviceStatus::Stopping;
            let device_id_str = device_id.to_string();
            drop(guard.take());

            // Stop the per-device Renode container
            let container_name = renode_container_name(&device_id_str);
            let _ = Command::new("docker")
                .args(&["rm", "-f", &container_name])
                .output();

            let mut devices = self.devices.lock().await;
            let device = devices.get_mut(&device_id_str).ok_or_else(|| anyhow::anyhow!("Device {} not found", device_id_str))?;
            device.process_id = None;
            device.status = QemuDeviceStatus::Stopped;
        } else if let Some(pid) = device.process_id {
            // Stop portable Renode process
            device.status = QemuDeviceStatus::Stopping;
            
            // Send SIGTERM to the process
            if let Err(e) = Command::new("kill")
                .args(&["-TERM", &pid.to_string()])
                .output()
            {
                device.status = QemuDeviceStatus::Error(e.to_string());
                return Err(anyhow::anyhow!("Failed to stop device: {}", e));
            }

            // Wait for process to terminate
            thread::sleep(Duration::from_secs(2));

            // Force kill if still running
            if let Err(e) = Command::new("kill")
                .args(&["-KILL", &pid.to_string()])
                .output()
            {
                device.status = QemuDeviceStatus::Error(e.to_string());
                return Err(anyhow::anyhow!("Failed to force stop device: {}", e));
            }

            device.process_id = None;
            device.status = QemuDeviceStatus::Stopped;
        }

        // Unregister board from Gateway (device proxy is going away)
        let device_id_clone = device_id.to_string();
        drop(guard);
        if let Some(gw) = gateway_http {
            unregister_board_from_gateway(&gw, &device_id_clone).await;
        }

        Ok(())
    }

    pub async fn deploy_wasm(&self, device_id: &str, wasm_bytes: Vec<u8>, config: WasmConfig) -> Result<(), anyhow::Error> {
        let mut devices = self.devices.lock().await;
        
        let device = devices.get_mut(device_id)
            .ok_or_else(|| anyhow::anyhow!("Device {} not found", device_id))?;

        if !matches!(device.status, QemuDeviceStatus::Running) {
            return Err(anyhow::anyhow!("Device is not running"));
        }

        device.wasm_runtime = Some(WasmRuntime {
            wasm_bytes,
            config,
            status: WasmRuntimeStatus::Loading,
        });

        // Simulate WASM loading process
        tokio::spawn({
            let device_id = device_id.to_string();
            let devices = self.devices.clone();
            
            async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                
                let mut devices = devices.lock().await;
                if let Some(device) = devices.get_mut(&device_id) {
                    if let Some(runtime) = &mut device.wasm_runtime {
                        runtime.status = WasmRuntimeStatus::Running;
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn get_device(&self, device_id: &str) -> Option<QemuDevice> {
        let devices = self.devices.lock().await;
        devices.get(device_id).cloned()
    }

    pub async fn list_devices(&self) -> Vec<QemuDevice> {
        let devices = self.devices.lock().await;
        devices.values().cloned().collect()
    }

    fn get_firmware_path(&self, mcu_type: &McuType) -> Result<std::path::PathBuf, std::io::Error> {
        // Use ZEPHYR_WORKSPACE env var if set (needed when running inside a k8s pod)
        let zephyr_workspace = if let Ok(ws) = std::env::var("ZEPHYR_WORKSPACE") {
            std::path::PathBuf::from(ws)
        } else {
            let current_dir = std::env::current_dir().unwrap_or_default();
            current_dir.join("zephyr-workspace")
        };
        let zephyr_firmware_nrf52840 = zephyr_workspace.join("build/nrf52840dk/nrf52840/zephyr/zephyr.elf");
        let zephyr_firmware_stm32f4 = zephyr_workspace.join("build/stm32f4/zephyr/zephyr.elf");
        let zephyr_firmware_arduino_nano = zephyr_workspace.join("build/nrf52840dk/nrf52840/zephyr/zephyr.elf");
        
        match mcu_type {
            // Ethernet boards
            McuType::Stm32F746gDisco => {
                let firmware_path = zephyr_workspace.join(mcu_type.get_firmware_path());
                if firmware_path.exists() {
                    Ok(firmware_path)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for STM32F746G Discovery. Expected: {}", firmware_path.display())
                    ))
                }
            },
            McuType::FrdmK64f => {
                let firmware_path = zephyr_workspace.join(mcu_type.get_firmware_path());
                if firmware_path.exists() {
                    Ok(firmware_path)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for FRDM-K64F. Expected: {}", firmware_path.display())
                    ))
                }
            },
            
            // WiFi boards
            McuType::Esp32DevkitC => {
                let firmware_path = zephyr_workspace.join(mcu_type.get_firmware_path());
                if firmware_path.exists() {
                    Ok(firmware_path)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for ESP32 DevKitC. Expected: {}", firmware_path.display())
                    ))
                }
            },
            
            // No network boards
            McuType::Nrf52840DK | McuType::Stm32F4Disco => {
                let firmware_path = zephyr_workspace.join(mcu_type.get_firmware_path());
                if firmware_path.exists() {
                    Ok(firmware_path)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for {}. Expected: {}", mcu_type.display_name(), firmware_path.display())
                    ))
                }
            },
            
            // Legacy boards
            McuType::RenodeArduinoNano33Ble => {
                if zephyr_firmware_arduino_nano.exists() {
                    Ok(zephyr_firmware_arduino_nano)
                } else if zephyr_firmware_nrf52840.exists() {
                    Ok(zephyr_firmware_nrf52840)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for nRF52840. Expected: {} or {}", 
                            zephyr_firmware_arduino_nano.display(), zephyr_firmware_nrf52840.display())
                    ))
                }
            },
            McuType::RenodeStm32F4Discovery => {
                if zephyr_firmware_stm32f4.exists() {
                    Ok(zephyr_firmware_stm32f4)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for STM32F4. Expected: {}", 
                            zephyr_firmware_stm32f4.display())
                    ))
                }
            },
            McuType::Mps2An385 => {
                if zephyr_firmware_nrf52840.exists() {
                    Ok(zephyr_firmware_nrf52840)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for MPS2-AN385. Expected: {}",
                            zephyr_firmware_nrf52840.display())
                    ))
                }
            },
            // Linux native devices do not use Renode emulation or Zephyr firmware.
            // They run wasmbed-edge-client directly and register via board/register API.
            McuType::LinuxArm64 | McuType::LinuxX86_64 | McuType::LinuxRiscV => {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Linux native devices do not have a Zephyr firmware path",
                ))
            },
            McuType::Nrf52840DK => {
                // Official Zephyr nRF52840 DK firmware
                if zephyr_firmware_nrf52840.exists() {
                    Ok(zephyr_firmware_nrf52840)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for nRF52840 DK. Expected: {}", 
                            zephyr_firmware_nrf52840.display())
                    ))
                }
            },
            McuType::Stm32F4Disco => {
                // Official Zephyr STM32F4 Discovery firmware
                if zephyr_firmware_stm32f4.exists() {
                    Ok(zephyr_firmware_stm32f4)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Zephyr firmware not found for STM32F4 Discovery. Expected: {}", 
                            zephyr_firmware_stm32f4.display())
                    ))
                }
            },
        }
    }

    /// Build the Renode monitor command string for one machine (for single-instance Renode).
    fn build_renode_commands_string(
        &self,
        device: &QemuDevice,
        device_id: &str,
        gateway_endpoint_str: &str,
        firmware_path_in_container: &str,
    ) -> Result<String, std::io::Error> {
        let mcu = &device.mcu_type;
        let platform = mcu.renode_platform();
        let uart = mcu.get_uart_name();
        let ethernet_config = if mcu.has_ethernet() {
            match mcu {
                McuType::Stm32F746gDisco => "\nemulation CreateSwitch \"ethernet_switch\"\nemulation CreateTap \"tap0\" \"ethernet_tap\"\nsysbus.ethernet MAC \"00:11:22:33:44:55\"\nconnector Connect sysbus.ethernet ethernet_switch\nconnector Connect host.ethernet_tap ethernet_switch\nhost.ethernet_tap Start",
                McuType::FrdmK64f => "\nemulation CreateSwitch \"ethernet_switch\"\nemulation CreateTap \"tap0\" \"ethernet_tap\"\nsysbus.ethernet MAC \"00:11:22:33:44:66\"\nconnector Connect sysbus.ethernet ethernet_switch\nconnector Connect host.ethernet_tap ethernet_switch\nhost.ethernet_tap Start",
                _ => "",
            }
        } else if mcu.has_wifi() {
            "\n# WiFi configuration for ESP32 (if supported)"
        } else {
            ""
        };
        let pc_sp = if *mcu == McuType::RenodeArduinoNano33Ble {
            "\nsysbus.cpu PC 0x866b\nsysbus.cpu SP 0x20020000"
        } else {
            ""
        };
        let loadelf_cmd = format!("sysbus LoadELF \"{}\"", firmware_path_in_container);
        let ethernet_block = if ethernet_config.is_empty() {
            "".to_string()
        } else {
            ethernet_config.trim().split(';').map(|c| c.trim()).filter(|c| !c.is_empty()).collect::<Vec<_>>().join("\n")
        };
        let renode_commands = format!(
            "mach add \"{id}\"\ninclude @platforms/boards/{platform}.repl\nshowAnalyzer sysbus.{uart}\n{loadelf}\nmach set \"{id}\"\n{ethernet}{pc_sp}\nlogLevel -1\nstart",
            id = device.id,
            platform = platform,
            uart = uart,
            loadelf = loadelf_cmd,
            ethernet = ethernet_block,
            pc_sp = pc_sp
        );
        let endpoint_bytes = gateway_endpoint_str.as_bytes();
        let mut endpoint_write = format!("\nsysbus WriteDoubleWord 0x20001000 0x{:08x}", endpoint_bytes.len());
        for (i, chunk) in endpoint_bytes.chunks(4).enumerate() {
            let mut word: u32 = 0;
            for (j, &byte) in chunk.iter().enumerate() {
                word |= (byte as u32) << (j * 8);
            }
            endpoint_write.push_str(&format!("\nsysbus WriteDoubleWord 0x{:08x} 0x{:08x}", 0x20001004 + (i as u32 * 4), word));
        }
        Ok(format!("{}{}", renode_commands, endpoint_write))
    }

    /// Build a self-contained .resc startup script for one device.
    /// Passes @-prefixed paths (Renode convention) and ends with `start`.
    /// No TCP monitor connection needed — Renode executes the script and runs.
    fn build_resc_script(
        &self,
        device: &QemuDevice,
        device_id: &str,
        gateway_endpoint_str: &str,
        firmware_path_in_container: &str,
    ) -> Result<String, std::io::Error> {
        let mcu = &device.mcu_type;
        let platform = mcu.renode_platform();
        let uart = mcu.get_uart_name();

        let ethernet_block = if mcu.has_ethernet() {
            match mcu {
                McuType::Stm32F746gDisco => {
                    "emulation CreateSwitch \"ethernet_switch\"\n\
                     emulation CreateTap \"tap0\" \"ethernet_tap\"\n\
                     sysbus.ethernet MAC \"00:11:22:33:44:55\"\n\
                     connector Connect sysbus.ethernet ethernet_switch\n\
                     connector Connect host.ethernet_tap ethernet_switch\n\
                     host.ethernet_tap Start"
                }
                McuType::FrdmK64f => {
                    "emulation CreateSwitch \"ethernet_switch\"\n\
                     emulation CreateTap \"tap0\" \"ethernet_tap\"\n\
                     sysbus.ethernet MAC \"00:11:22:33:44:66\"\n\
                     connector Connect sysbus.ethernet ethernet_switch\n\
                     connector Connect host.ethernet_tap ethernet_switch\n\
                     host.ethernet_tap Start"
                }
                _ => "",
            }
        } else {
            ""
        };

        let pc_sp = if *mcu == McuType::RenodeArduinoNano33Ble {
            "sysbus.cpu PC 0x866b\nsysbus.cpu SP 0x20020000\n"
        } else {
            ""
        };

        // Encode gateway endpoint address into memory at a known location
        let endpoint_bytes = gateway_endpoint_str.as_bytes();
        let mut endpoint_write = format!("sysbus WriteDoubleWord 0x20001000 0x{:08x}", endpoint_bytes.len());
        for (i, chunk) in endpoint_bytes.chunks(4).enumerate() {
            let mut word: u32 = 0;
            for (j, &byte) in chunk.iter().enumerate() {
                word |= (byte as u32) << (j * 8);
            }
            endpoint_write.push_str(&format!("\nsysbus WriteDoubleWord 0x{:08x} 0x{:08x}", 0x20001004 + (i as u32 * 4), word));
        }

        let mut script = format!(
            "using sysbus\n\
             mach create \"{id}\"\n\
             machine LoadPlatformDescription @platforms/boards/{platform}.repl\n\
             sysbus.{uart} CreateFileBackend @/tmp/uart-{id}.log true\n\
             sysbus LoadELF @{elf}\n",
            id = device_id,
            platform = platform,
            uart = uart,
            elf = firmware_path_in_container,
        );

        if !ethernet_block.is_empty() {
            script.push_str(ethernet_block);
            script.push('\n');
        }

        script.push_str(&endpoint_write);
        script.push('\n');
        script.push_str(pc_sp);
        script.push_str("logLevel 3\n");
        script.push_str("start\n");

        Ok(script)
    }

    fn build_renode_args(&self, device: &QemuDevice, device_id: &str) -> Result<Vec<String>, std::io::Error> {
        // CRITICAL: Write to file immediately to verify function is called
        let function_entry_log = std::env::temp_dir().join(format!("build_renode_args_entry_{}.log", device_id));
        let _ = std::fs::write(&function_entry_log, format!("build_renode_args called for device: {}\ndevice.id: {}\ndevice.mcu_type: {:?}\n", device_id, device.id, device.mcu_type));
        
        println!("DEBUG: build_renode_args called for device: {} (device.id: {}, device.mcu_type: {:?})", device_id, device.id, device.mcu_type);
        eprintln!("DEBUG: build_renode_args called for device: {} (device.id: {}, device.mcu_type: {:?})", device_id, device.id, device.mcu_type);
        
        // CRITICAL: ALWAYS read mcuType from CRD - this is the source of truth
        println!("🔍🔍🔍 CRITICAL: Reading mcuType from CRD for device {} 🔍🔍🔍", device_id);
        eprintln!("🔍🔍🔍 CRITICAL: Reading mcuType from CRD for device {} 🔍🔍🔍", device_id);
        
        let crd_mcu_type = std::process::Command::new("kubectl")
            .args(&["get", "devices.wasmbed.github.io", device_id, "-n", "wasmbed", "-o", "jsonpath={.spec.mcuType}"])
            .output();
        
        let final_mcu_type = match crd_mcu_type {
            Ok(output) if output.status.success() => {
                let mcu_type_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                println!("✅ CRITICAL: Read mcuType from CRD: '{}'", mcu_type_str);
                eprintln!("✅ CRITICAL: Read mcuType from CRD: '{}'", mcu_type_str);
                
                match mcu_type_str.as_str() {
                    "Stm32F746gDisco" => {
                        println!("✅✅✅ OVERRIDING to Stm32F746gDisco ✅✅✅");
                        eprintln!("✅✅✅ OVERRIDING to Stm32F746gDisco ✅✅✅");
                        McuType::Stm32F746gDisco
                    },
                    "FrdmK64f" => {
                        println!("✅ OVERRIDING to FrdmK64f");
                        eprintln!("✅ OVERRIDING to FrdmK64f");
                        McuType::FrdmK64f
                    },
                    "Esp32DevkitC" => {
                        println!("✅ OVERRIDING to Esp32DevkitC");
                        eprintln!("✅ OVERRIDING to Esp32DevkitC");
                        McuType::Esp32DevkitC
                    },
                    "Nrf52840DK" => {
                        println!("✅ OVERRIDING to Nrf52840DK");
                        eprintln!("✅ OVERRIDING to Nrf52840DK");
                        McuType::Nrf52840DK
                    },
                    "Stm32F4Disco" => {
                        println!("✅ OVERRIDING to Stm32F4Disco");
                        eprintln!("✅ OVERRIDING to Stm32F4Disco");
                        McuType::Stm32F4Disco
                    },
                    "LinuxArm64" | "linux_arm64" => McuType::LinuxArm64,
                    "LinuxX86_64" | "linux_x86_64" => McuType::LinuxX86_64,
                    "LinuxRiscV" | "linux_riscv" => McuType::LinuxRiscV,
                    _ => {
                        println!("⚠️ Unknown mcuType '{}', using device.mcu_type: {:?}", mcu_type_str, device.mcu_type);
                        eprintln!("⚠️ Unknown mcuType '{}', using device.mcu_type: {:?}", mcu_type_str, device.mcu_type);
                        device.mcu_type.clone()
                    }
                }
            },
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("⚠️ kubectl failed (status: {}): {}, using device.mcu_type: {:?}", output.status, stderr, device.mcu_type);
                eprintln!("⚠️ kubectl failed (status: {}): {}, using device.mcu_type: {:?}", output.status, stderr, device.mcu_type);
                device.mcu_type.clone()
            },
            Err(e) => {
                println!("⚠️ kubectl error: {:?}, using device.mcu_type: {:?}", e, device.mcu_type);
                eprintln!("⚠️ kubectl error: {:?}, using device.mcu_type: {:?}", e, device.mcu_type);
                device.mcu_type.clone()
            }
        };
        
        let final_platform = final_mcu_type.renode_platform();
        println!("✅✅✅ FINAL PLATFORM: {} (mcu_type: {:?}) ✅✅✅", final_platform, final_mcu_type);
        eprintln!("✅✅✅ FINAL PLATFORM: {} (mcu_type: {:?}) ✅✅✅", final_platform, final_mcu_type);
        
        // Check if we should use Docker (more reliable and isolate than portable)
        let use_docker = std::env::var("RENODE_USE_DOCKER").unwrap_or_else(|_| "true".to_string()) == "true";
        
        let renode_binary = if use_docker {
            // Use Docker - pull image if needed
            // Check if image exists
            let check_image = std::process::Command::new("docker")
                .args(&["images", "-q", "antmicro/renode:nightly"])
                .output();
            
            if let Ok(output) = check_image {
                if output.stdout.is_empty() {
                    println!("Pulling Renode Docker image (first time only)...");
                    let pull_result = std::process::Command::new("docker")
                        .args(&["pull", "antmicro/renode:nightly"])
                        .status();
                    if let Err(e) = pull_result {
                        panic!("Failed to pull Renode Docker image: {}", e);
                    }
                }
            }
            "docker".to_string()
        } else {
            // Fallback: Use RENODE_PATH environment variable if set
            // Otherwise, force Docker usage (recommended)
            if let Ok(renode_path) = std::env::var("RENODE_PATH") {
                renode_path
            } else {
                // Force Docker usage - portable Renode is no longer supported
                eprintln!("Warning: RENODE_PATH not set. Forcing Docker usage.");
                // Use Docker instead
                let check_image = std::process::Command::new("docker")
                    .args(&["images", "-q", "antmicro/renode:nightly"])
                    .output();
                
                if let Ok(output) = check_image {
                    if output.stdout.is_empty() {
                        println!("Pulling Renode Docker image (first time only)...");
                        let pull_result = std::process::Command::new("docker")
                            .args(&["pull", "antmicro/renode:nightly"])
                            .status();
                        if let Err(e) = pull_result {
                            panic!("Failed to pull Renode Docker image: {}", e);
                        }
                    }
                }
                "docker".to_string()
            }
        };
        
        // Get firmware path using helper method - use final_mcu_type from CRD
        let firmware_path = self.get_firmware_path(&final_mcu_type)?;
        let firmware_path_str = firmware_path.to_string_lossy().to_string();
        let firmware_filename = firmware_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("zephyr.elf"))
            .to_string_lossy()
            .to_string();
        
        // Post-load commands (empty for now, can be extended)
        let post_load_commands = String::new();
        
        // Build Renode commands to:
        // 1. Create machine
        // 2. Include platform description (use @ prefix for Renode-relative paths)
        // 3. Setup UART analyzer for logs (different UART per device)
        // 4. Load device runtime firmware (use absolute path)
        // 5. Set PC/SP if needed (for nRF52840)
        // 6. Start execution
        // Use final_mcu_type (from CRD) instead of device.mcu_type
        let uart_name = final_mcu_type.get_uart_name();
        
        // For Docker, use Docker volume mount (firmware-{device_id} volume)
        // The firmware is copied to the volume in start_device before calling build_renode_args
        // Note: Renode uses @ prefix for relative paths, but we need absolute paths in container
        let firmware_path_in_container = if use_docker {
            format!("/firmware/{}", firmware_filename)
        } else {
            firmware_path_str.clone()
        };
        
        // Renode command format: mach add "name" (not mach create "name")
        // For boards with Ethernet/WiFi, configure network interface
        // Use final_mcu_type (from CRD) instead of device.mcu_type
        let ethernet_config = if final_mcu_type.has_ethernet() {
            // Configure Ethernet for boards that support it
            match final_mcu_type {
                McuType::Stm32F746gDisco => {
                    // STM32F746G Discovery has Ethernet MAC
                    "\nemulation CreateSwitch \"ethernet_switch\"\nemulation CreateTap \"tap0\" \"ethernet_tap\"\nsysbus.ethernet MAC \"00:11:22:33:44:55\"\nconnector Connect sysbus.ethernet ethernet_switch\nconnector Connect host.ethernet_tap ethernet_switch\nhost.ethernet_tap Start"
                },
                McuType::FrdmK64f => {
                    // FRDM-K64F has Ethernet MAC
                    "\nemulation CreateSwitch \"ethernet_switch\"\nemulation CreateTap \"tap0\" \"ethernet_tap\"\nsysbus.ethernet MAC \"00:11:22:33:44:66\"\nconnector Connect sysbus.ethernet ethernet_switch\nconnector Connect host.ethernet_tap ethernet_switch\nhost.ethernet_tap Start"
                },
                _ => ""
            }
        } else if final_mcu_type.has_wifi() {
            // Configure WiFi for ESP32 (if supported by Renode)
            "\n# WiFi configuration for ESP32 (if supported)"
        } else {
            ""
        };
        
        let pc_sp_commands = if final_mcu_type == McuType::RenodeArduinoNano33Ble {
            // Only set PC/SP for Arduino Nano (legacy)
            // For official Zephyr boards, let ELF loader set PC/SP automatically
            "\nsysbus.cpu PC 0x866b\nsysbus.cpu SP 0x20020000"
        } else {
            ""
        };
        // For Docker, use absolute path without @ prefix (absolute paths don't need @)
        // The @ prefix is for Renode-relative paths, not absolute paths
        let loadelf_cmd = if use_docker {
            // Use absolute path without @ prefix
            format!("sysbus LoadELF \"{}\"", firmware_path_in_container)
        } else {
            format!("sysbus LoadELF @\"{}\"", firmware_path_in_container)
        };
        
        // Use final_mcu_type and final_platform that were already determined from CRD above
        let final_uart = final_mcu_type.get_uart_name();
        println!("✅ FINAL: Using platform = {}, uart = {}, mcu_type = {:?}", final_platform, final_uart, final_mcu_type);
        eprintln!("✅ FINAL: Using platform = {}, uart = {}, mcu_type = {:?}", final_platform, final_uart, final_mcu_type);
        
        let mut renode_commands = format!(
            "mach add \"{id}\"\ninclude @platforms/boards/{platform}.repl\nshowAnalyzer sysbus.{uart}\n{loadelf}\nmach set \"{id}\"\n{ethernet}{pc_sp}\n# Enable detailed logging\nlogLevel -1\n# Start machine\nstart\n# Keep machine running - Renode will stay active after start command",
            id = device.id,
            platform = final_platform,
            uart = final_uart,
            loadelf = loadelf_cmd,
            ethernet = if ethernet_config.is_empty() {
                "".to_string()
            } else {
                ethernet_config
                    .trim()
                    .trim_start_matches(';')
                    .split(';')
                    .map(|cmd| cmd.trim())
                    .filter(|cmd| !cmd.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            pc_sp = pc_sp_commands
        );
        if !post_load_commands.is_empty() {
            renode_commands.push('\n');
            renode_commands.push_str(
                &post_load_commands
                    .trim()
                    .trim_start_matches(';')
                    .split(';')
                    .map(|cmd| cmd.trim())
                    .filter(|cmd| !cmd.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        
        // Get bridge endpoint if TCP bridge is running for this device
        // Calculate bridge port using same hash as in start_device
        // Firmware can connect directly to gateway pod IP with TLS (no TCP bridge needed)
        // The gateway_endpoint should already contain the pod IP:port from start_device (format: "10.42.0.12:8081")
        println!("DEBUG: build_renode_args - device.gateway_endpoint = {:?}", device.gateway_endpoint);
        eprintln!("DEBUG: build_renode_args - device.gateway_endpoint = {:?}", device.gateway_endpoint);
        let _ = std::fs::write(
            std::env::temp_dir().join(format!("build_renode_args_gateway_endpoint_{}.txt", device_id)),
            format!("device.gateway_endpoint = {:?}", device.gateway_endpoint)
        );
        let gateway_endpoint_str = if let Some(ref gateway_ep) = device.gateway_endpoint {
            // Use the resolved gateway pod IP endpoint (format: "10.42.0.62:8443")
            println!("DEBUG: Using device.gateway_endpoint: {}", gateway_ep);
            eprintln!("DEBUG: Using device.gateway_endpoint: {}", gateway_ep);
            gateway_ep.clone()
        } else {
            // Fallback: try to resolve gateway pod IP now
            // Extract gateway name from device or use default
            let gateway_name = "gateway-1"; // Default, could be improved to get from device spec
            
            // Pods are labeled with app=wasmbed-gateway, not gateway={name}
            let pod_ip_output = Command::new("kubectl")
                .args(&["get", "pods", "-n", "wasmbed", "-l", "app=wasmbed-gateway", "-o", "jsonpath={.items[0].status.podIP}"])
                .output();
            
            if let Ok(output) = pod_ip_output {
                if output.status.success() {
                    let pod_ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !pod_ip.is_empty() {
                        format!("{}:8081", pod_ip)
                    } else {
                        "127.0.0.1:8081".to_string()
                    }
                } else {
                    "127.0.0.1:8081".to_string()
                }
            } else {
                "127.0.0.1:8081".to_string()
            }
        };
        
        println!("Writing gateway endpoint to memory for device {}: {}", device_id, gateway_endpoint_str);
        eprintln!("Writing gateway endpoint to memory for device {}: {}", device_id, gateway_endpoint_str);
        // Also write to a file for debugging
        let _ = std::fs::write(
            std::env::temp_dir().join(format!("gateway_endpoint_memory_{}.txt", device_id)),
            format!("Writing gateway endpoint to memory for device {}: {}", device_id, gateway_endpoint_str)
        );
        
        // Append gateway endpoint configuration to Renode commands
        // The endpoint will be written to memory address 0x20001000 (in RAM)
        // Format: write endpoint string bytes to memory
        let endpoint_bytes = gateway_endpoint_str.as_bytes();
        let mut endpoint_write_commands = String::new();
        
        // Write endpoint string to memory starting at 0x20001000
        // First write the length, then the string bytes
        endpoint_write_commands.push_str(&format!("\nsysbus WriteDoubleWord 0x20001000 0x{:08x}", endpoint_bytes.len()));
        
        // Write endpoint bytes (4 bytes at a time)
        for (i, chunk) in endpoint_bytes.chunks(4).enumerate() {
            let mut word: u32 = 0;
            for (j, &byte) in chunk.iter().enumerate() {
                word |= (byte as u32) << (j * 8);
            }
            endpoint_write_commands.push_str(&format!("\nsysbus WriteDoubleWord 0x{:08x} 0x{:08x}", 
                0x20001004 + (i as u32 * 4), word));
        }
        
        // Append endpoint configuration to Renode commands
        let renode_commands_with_endpoint = format!("{}{}", renode_commands, endpoint_write_commands);
        
        // Start Renode with a script file to keep it running
        // Create a temporary script file with all commands
        let port = device.endpoint.split(':').nth(1).unwrap_or("3000").to_string();
        
        // Create temporary script file
        // CRITICAL: Use device_id instead of device.id to ensure consistency
        let script_path = std::env::temp_dir().join(format!("renode_{}.resc", device_id));
        // Add commands to keep Renode running after start
        // The issue is that the machine may pause/dispose when firmware terminates
        // Solution: Use 'start' command and add a Python script to monitor and restart the machine
        // if it pauses. We'll use Renode's Python API to keep the machine running.
        // Build script content with Python monitoring code
        // IMPORTANT: The Python code must be added AFTER the 'start' command
        // Renode executes the script line by line, so Python must come after start
        // CRITICAL: Python code in Renode must use proper indentation and format
        // Note: Python monitoring is optional - machine should stay running after 'start'
        let python_code = r#"

# Python script to monitor and keep machine running (optional)
# Note: Machine should stay running after 'start' command
# This Python code is commented out to avoid syntax errors
# python:
# import time
# mach = machines.current
# while True:
#     try:
#         time.sleep(0.5)
#         if mach.cpu.IsPaused:
#             mach.cpu.Resume()
#     except:
#         pass"#;
        
        // Build the complete script: renode commands + start + Python monitoring
        // Ensure proper line breaks and formatting
        // CRITICAL: Make sure python_code is actually included
        // Note: renode_commands_with_endpoint already includes 'start', so we don't add it again
        let script_content = format!(
            "{}{}",
            renode_commands_with_endpoint,
            python_code
        );
        
        // CRITICAL: Log script_content immediately after format!
        let format_log = std::env::temp_dir().join(format!("script_content_after_format_{}.log", device_id));
        let format_log_msg = format!(
            "After format!:\nrenode_commands_with_endpoint length: {}\npython_code length: {}\nscript_content length: {}\nscript_content contains 'python:': {}\nscript_content last 500 chars:\n{}",
            renode_commands_with_endpoint.len(),
            python_code.len(),
            script_content.len(),
            script_content.contains("python:"),
            &script_content[script_content.len().saturating_sub(500)..]
        );
        let _ = std::fs::write(&format_log, &format_log_msg);
        eprintln!("DEBUG: script_content after format! - length: {}, contains python: {}", script_content.len(), script_content.contains("python:"));
        println!("DEBUG: script_content after format! - length: {}, contains python: {}", script_content.len(), script_content.contains("python:"));
        
        // CRITICAL DEBUG: Write script_content to a file IMMEDIATELY to verify it contains Python
        // Use unwrap to ensure the write happens and we see any errors
        let pre_write_debug_path = std::env::temp_dir().join(format!("renode_prewrite_{}.resc", device_id));
        if let Err(e) = std::fs::write(&pre_write_debug_path, &script_content) {
            eprintln!("ERROR: Failed to write pre-write debug file: {}", e);
            println!("ERROR: Failed to write pre-write debug file: {}", e);
        } else {
            eprintln!("DEBUG: Pre-write debug file written successfully to: {:?}", pre_write_debug_path);
            println!("DEBUG: Pre-write debug file written successfully to: {:?}", pre_write_debug_path);
        }
        
        // Debug output - use both eprintln and println to ensure visibility
        // Also write to a separate debug file to ensure we can see it
        let debug_log_path = std::env::temp_dir().join(format!("renode_debug_{}.log", device_id));
        let python_code_len = python_code.len();
        let script_contains_python = script_content.contains("python:");
        let debug_msg = format!(
            "DEBUG: Writing Renode script to: {:?}\nDEBUG: renode_commands_with_endpoint length: {} bytes\nDEBUG: python_code length: {} bytes\nDEBUG: Script content length: {} bytes\nDEBUG: Script contains 'python:': {}\nDEBUG: Last 300 chars of script: {:?}\n",
            script_path,
            renode_commands_with_endpoint.len(),
            python_code_len,
            script_content.len(),
            script_contains_python,
            &script_content[script_content.len().saturating_sub(300)..]
        );
        // Write debug BEFORE any potential errors - use unwrap to ensure it happens
        if let Err(e) = std::fs::write(&debug_log_path, &debug_msg) {
            eprintln!("ERROR: Failed to write debug log file: {}", e);
            println!("ERROR: Failed to write debug log file: {}", e);
        } else {
            eprintln!("DEBUG: Debug log file written successfully to: {:?}", debug_log_path);
            println!("DEBUG: Debug log file written successfully to: {:?}", debug_log_path);
        }
        eprintln!("{}", debug_msg);
        println!("{}", debug_msg);
        
        // CRITICAL: If script doesn't contain Python, log error immediately and PANIC to see the error
        if !script_contains_python {
            let error_msg = format!("ERROR: script_content does NOT contain 'python:' before writing!\nrenode_commands_with_endpoint length: {} bytes\nrenode_commands_with_endpoint ends with: {:?}\npython_code length: {} bytes\npython_code starts with: {:?}\nscript_content length: {} bytes\nscript_content last 200 chars: {:?}", 
                renode_commands_with_endpoint.len(),
                &renode_commands_with_endpoint[renode_commands_with_endpoint.len().saturating_sub(50)..],
                python_code.len(),
                &python_code[..50.min(python_code.len())],
                script_content.len(),
                &script_content[script_content.len().saturating_sub(200)..]);
            eprintln!("{}", error_msg);
            println!("{}", error_msg);
            let _ = std::fs::write(&debug_log_path, format!("{}\n{}", debug_msg, error_msg));
            // PANIC to see the error immediately
            panic!("{}", error_msg);
        }
        
        // Encode script content as base64 so we can recreate it inside the Renode container
        let script_b64 = general_purpose::STANDARD.encode(script_content.as_bytes());
        let script_filename = script_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("renode_{}.resc", device_id));
        
        // Write the script file - use unwrap_or_else to handle errors gracefully
        // This ensures we can still write debug files even if there's an error
        match std::fs::write(&script_path, &script_content) {
            Ok(_) => {
                // CRITICAL: Also write to a separate debug file to verify content
                let debug_script_path = std::env::temp_dir().join(format!("renode_debug_{}.resc", device_id));
                let _ = std::fs::write(&debug_script_path, &script_content);
                
                // Verify the file was written correctly
                match std::fs::read_to_string(&script_path) {
                    Ok(written_content) => {
                        // Write verification to both stderr and a log file
                        let verify_msg = format!(
                            "DEBUG: Written file length: {} bytes\nDEBUG: Written file contains 'python:': {}\nDEBUG: Expected length: {} bytes\nDEBUG: Expected contains 'python:': {}",
                            written_content.len(),
                            written_content.contains("python:"),
                            script_content.len(),
                            script_content.contains("python:")
                        );
                        eprintln!("{}", verify_msg);
                        println!("{}", verify_msg);
                        let _ = std::fs::write(&debug_log_path, format!("{}\n{}", debug_msg, verify_msg));
                        
                        if written_content != script_content {
                            eprintln!("ERROR: Written script content differs from expected!");
                            eprintln!("ERROR: Expected length: {}, Written length: {}", script_content.len(), written_content.len());
                            eprintln!("ERROR: Expected last 200 chars: {:?}", &script_content[script_content.len().saturating_sub(200)..]);
                            eprintln!("ERROR: Written last 200 chars: {:?}", &written_content[written_content.len().saturating_sub(200)..]);
                            // Write error to debug file
                            let error_msg = format!("ERROR: Content mismatch!\nExpected last 200: {:?}\nWritten last 200: {:?}", 
                                &script_content[script_content.len().saturating_sub(200)..],
                                &written_content[written_content.len().saturating_sub(200)..]);
                            let _ = std::fs::write(&debug_log_path, format!("{}\n{}", debug_msg, error_msg));
                        } else {
                            eprintln!("DEBUG: Script file written successfully with Python code");
                            println!("DEBUG: Script file written successfully with Python code");
                        }
                    },
                    Err(e) => {
                        let error_msg = format!("ERROR: Failed to read written script file: {}", e);
                        eprintln!("{}", error_msg);
                        println!("{}", error_msg);
                        let _ = std::fs::write(&debug_log_path, format!("{}\n{}", debug_msg, error_msg));
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, error_msg));
                    }
                }
            },
            Err(e) => {
                let error_msg = format!("ERROR: Failed to write script file: {}", e);
                eprintln!("{}", error_msg);
                println!("{}", error_msg);
                let _ = std::fs::write(&debug_log_path, format!("{}\n{}", debug_msg, error_msg));
                return Err(e);
            }
        }
        
        // Start Renode with script file - it will execute script and stay running
        let args = if use_docker {
            // Docker command format:
            // docker run --rm --net=host -v firmware:/firmware:ro -v script:/script:ro antmicro/renode:nightly --console --plain --port PORT /script/file.resc
            // Get parent directory of firmware for mounting (so /firmware_dir points to the firmware directory)
            // CRITICAL: Mount the ARM firmware directory (target/thumbv7em-none-eabihf/release), not x86-64 (target/release)
            println!("DEBUG: Firmware path: {}", firmware_path_str);
            
            let script_b64_sanitized = script_b64.replace('"', "\\\"");
            // Use sh -c to execute Renode and keep container running
            // Use --restart=unless-stopped to automatically restart if container exits
            // Remove --rm so container persists and can be restarted
            vec![
                renode_binary, // "docker"
                "run".to_string(),
                "--restart=unless-stopped".to_string(),
                "--net=host".to_string(),
                "--name".to_string(),
                format!("renode-{}", device.id), // Container name for easy cleanup
                "-v".to_string(),
                // Mount Docker volume containing the firmware (copied in start_device)
                format!("{}:/firmware:ro", format!("firmware-{}", device_id)),
                "antmicro/renode:nightly".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                format!(
                    "set -e; python3 - <<'PY'\nimport base64, os\nos.makedirs('/tmp/renode', exist_ok=True)\nwith open('/tmp/renode/{script_name}', 'wb') as f:\n    f.write(base64.b64decode(\"{script_b64}\"))\nPY\nexec renode --console --port {port} /tmp/renode/{script_name}",
                    script_name = script_filename,
                    script_b64 = script_b64_sanitized,
                    port = port.clone()
                ), // Execute Renode with script file as argument (script includes start)
            ]
        } else {
            vec![
                renode_binary,
                "--console".to_string(),
                "--plain".to_string(),
                "--port".to_string(),
                port.clone(),
                script_path.to_string_lossy().to_string(),
            ]
        };
        
        // Note: Renode will execute the script and stay running because machine is active after 'start'
        
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_device() {
        let manager = RenodeManager::new("qemu-system-arm".to_string(), 30000);
        let device = manager.create_device(
            "test-device".to_string(),
            "Test Device".to_string(),
            "ARM_CORTEX_M".to_string(),
            "MCU".to_string(),
            McuType::Mps2An385,
            None,
        ).await.unwrap();

        assert_eq!(device.id, "test-device");
        assert_eq!(device.architecture, "ARM_CORTEX_M");
        assert_eq!(device.mcu_type, McuType::Mps2An385);
        assert!(matches!(device.status, QemuDeviceStatus::Stopped));
    }

    #[tokio::test]
    async fn test_device_lifecycle() {
        let manager = RenodeManager::new("qemu-system-arm".to_string(), 30000);
        
        // Create device
        let device = manager.create_device(
            "test-device".to_string(),
            "Test Device".to_string(),
            "ARM_CORTEX_M".to_string(),
            "MCU".to_string(),
            McuType::Mps2An385,
            None,
        ).await.unwrap();

        // Start device (this will fail in test environment without QEMU)
        let result = manager.start_device(&device.id, None).await;
        // We expect this to fail in test environment
        assert!(result.is_err());

        // Stop device
        let result = manager.stop_device(&device.id).await;
        assert!(result.is_ok());
    }
}
