// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::config::{DeviceArchitecture, HostFunctionConfig, WasmRuntimeConfig};
use crate::error::{WasmResult, WasmRuntimeError};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

/// Context for WASM execution containing device-specific state
#[derive(Debug, Clone)]
pub struct WasmContext {
    /// Device architecture
    pub architecture: DeviceArchitecture,
    /// Device identifier
    pub device_id: String,
    /// Current memory usage in bytes
    pub memory_usage: usize,
    /// Maximum memory limit
    pub memory_limit: usize,
    /// Current stack usage in bytes
    pub stack_usage: usize,
    /// Maximum stack limit
    pub stack_limit: usize,
    /// CPU time used in this execution session
    pub cpu_time_used: Duration,
    /// Maximum CPU time limit
    pub cpu_time_limit: Duration,
    /// Start time of current execution session
    pub session_start: Instant,
    /// Number of active instances
    pub active_instances: usize,
    /// Maximum number of instances allowed
    pub max_instances: usize,
    /// Host function registry
    pub host_functions: Arc<HostFunctionRegistry>,
    /// Device-specific state
    pub device_state: Arc<DeviceState>,
    /// Security context
    pub security_context: Arc<SecurityContext>,
}

/// Registry for host functions available to WASM modules
pub struct HostFunctionRegistry {
    /// Device communication functions
    pub device_functions: HashMap<String, DeviceFunction>,
    /// Sensor access functions
    pub sensor_functions: HashMap<String, SensorFunction>,
    /// Security functions
    pub security_functions: HashMap<String, SecurityFunction>,
    /// GPIO functions
    pub gpio_functions: HashMap<String, GpioFunction>,
    /// I2C/SPI functions
    pub i2c_spi_functions: HashMap<String, I2cSpiFunction>,
}

impl std::fmt::Debug for HostFunctionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostFunctionRegistry")
            .field("device_functions", &self.device_functions.len())
            .field("sensor_functions", &self.sensor_functions.len())
            .field("security_functions", &self.security_functions.len())
            .field("gpio_functions", &self.gpio_functions.len())
            .field("i2c_spi_functions", &self.i2c_spi_functions.len())
            .finish()
    }
}

/// Device-specific state and resources
#[derive(Debug, Clone)]
pub struct DeviceState {
    /// GPIO pin states
    pub gpio_pins: DashMap<u32, GpioPinState>,
    /// I2C devices
    pub i2c_devices: DashMap<u8, I2cDevice>,
    /// SPI devices
    pub spi_devices: DashMap<u8, SpiDevice>,
    /// Sensor readings
    pub sensor_readings: DashMap<String, SensorReading>,
    /// Network connections
    pub network_connections: DashMap<String, NetworkConnection>,
    /// File system state
    pub filesystem_state: Arc<FilesystemState>,
}

/// Security context for encrypted communication
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Encryption keys
    pub encryption_keys: DashMap<String, Vec<u8>>,
    /// MAC keys for integrity
    pub mac_keys: DashMap<String, Vec<u8>>,
    /// Session keys
    pub session_keys: DashMap<String, SessionKey>,
    /// Device certificates
    pub certificates: DashMap<String, Vec<u8>>,
    /// Nonce counters for encryption
    pub nonce_counters: DashMap<String, u64>,
}

/// GPIO pin state
#[derive(Debug, Clone)]
pub struct GpioPinState {
    pub pin: u32,
    pub mode: GpioMode,
    pub value: bool,
    pub pull: GpioPull,
}

/// GPIO modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioMode {
    Input,
    Output,
    Alternate,
    Analog,
}

/// GPIO pull configurations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioPull {
    None,
    Up,
    Down,
}

/// I2C device information
#[derive(Debug, Clone)]
pub struct I2cDevice {
    pub address: u8,
    pub bus: u8,
    pub speed: u32, // Hz
    pub connected: bool,
}

/// SPI device information
#[derive(Debug, Clone)]
pub struct SpiDevice {
    pub cs_pin: u32,
    pub bus: u8,
    pub speed: u32, // Hz
    pub mode: SpiMode,
    pub connected: bool,
}

/// SPI communication modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiMode {
    Mode0, // CPOL=0, CPHA=0
    Mode1, // CPOL=0, CPHA=1
    Mode2, // CPOL=1, CPHA=0
    Mode3, // CPOL=1, CPHA=1
}

/// Sensor reading data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    pub sensor_id: String,
    pub sensor_type: SensorType,
    pub value: f64,
    pub unit: String,
    pub timestamp: SystemTime,
    pub quality: SensorQuality,
}

/// Types of sensors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorType {
    Accelerometer,
    Gyroscope,
    Magnetometer,
    Barometer,
    Temperature,
    Humidity,
    Pressure,
    Light,
    Proximity,
    Camera,
    Gps,
    Lidar,
    Ultrasonic,
    Infrared,
}

/// Sensor data quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorQuality {
    Excellent,
    Good,
    Fair,
    Poor,
    Unknown,
}

/// Network connection state
#[derive(Debug, Clone)]
pub struct NetworkConnection {
    pub connection_id: String,
    pub protocol: NetworkProtocol,
    pub remote_address: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub connected: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Network protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProtocol {
    Tcp,
    Udp,
    WebSocket,
    Mqtt,
    Coap,
}

/// File system state
#[derive(Debug, Clone)]
pub struct FilesystemState {
    pub files: DashMap<String, FileEntry>,
    pub directories: DashMap<String, DirectoryEntry>,
    pub total_space: u64,
    pub used_space: u64,
}

/// File entry in the virtual file system
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub permissions: u32,
    pub content: Vec<u8>,
}

/// Directory entry in the virtual file system
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub path: String,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub permissions: u32,
    pub entries: Vec<String>,
}

/// Session key for secure communication
#[derive(Debug, Clone)]
pub struct SessionKey {
    pub key_id: String,
    pub key: Vec<u8>,
    pub created: SystemTime,
    pub expires: Option<SystemTime>,
    pub algorithm: EncryptionAlgorithm,
}

/// Encryption algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionAlgorithm {
    Aes128Gcm,
    Aes256Gcm,
    ChaCha20Poly1305,
}

/// Function signatures for different host function types
pub type DeviceFunction = Box<dyn Fn(&[u8]) -> WasmResult<Vec<u8>> + Send + Sync>;
pub type SensorFunction = Box<dyn Fn(&[u8]) -> WasmResult<Vec<u8>> + Send + Sync>;
pub type SecurityFunction = Box<dyn Fn(&[u8]) -> WasmResult<Vec<u8>> + Send + Sync>;
pub type GpioFunction = Box<dyn Fn(&[u8]) -> WasmResult<Vec<u8>> + Send + Sync>;
pub type I2cSpiFunction = Box<dyn Fn(&[u8]) -> WasmResult<Vec<u8>> + Send + Sync>;

impl WasmContext {
    /// Create a new WASM context for a specific device architecture
    pub fn new(
        architecture: DeviceArchitecture,
        device_id: String,
        wasm_config: &WasmRuntimeConfig,
        host_config: &HostFunctionConfig,
    ) -> Self {
        let host_functions = Arc::new(HostFunctionRegistry::new(host_config));
        let device_state = Arc::new(DeviceState::new());
        let security_context = Arc::new(SecurityContext::new());

        Self {
            architecture,
            device_id,
            memory_usage: 0,
            memory_limit: wasm_config.max_memory,
            stack_usage: 0,
            stack_limit: wasm_config.max_stack_size,
            cpu_time_used: Duration::ZERO,
            cpu_time_limit: wasm_config.max_execution_time,
            session_start: Instant::now(),
            active_instances: 0,
            max_instances: wasm_config.max_instances,
            host_functions,
            device_state,
            security_context,
        }
    }

    /// Check if memory limit would be exceeded
    pub fn check_memory_limit(&self, additional_bytes: usize) -> WasmResult<()> {
        let new_usage = self.memory_usage + additional_bytes;
        if new_usage > self.memory_limit {
            return Err(WasmRuntimeError::MemoryLimitExceeded {
                current: new_usage,
                limit: self.memory_limit,
            });
        }
        Ok(())
    }

    /// Check if stack limit would be exceeded
    pub fn check_stack_limit(&self, additional_bytes: usize) -> WasmResult<()> {
        let new_usage = self.stack_usage + additional_bytes;
        if new_usage > self.stack_limit {
            return Err(WasmRuntimeError::StackOverflow {
                current: new_usage,
                limit: self.stack_limit,
            });
        }
        Ok(())
    }

    /// Check if CPU time limit would be exceeded
    pub fn check_cpu_time_limit(&self, additional_time: Duration) -> WasmResult<()> {
        let new_time = self.cpu_time_used + additional_time;
        if new_time > self.cpu_time_limit {
            return Err(WasmRuntimeError::CpuTimeLimitExceeded {
                elapsed: new_time,
                limit: self.cpu_time_limit,
            });
        }
        Ok(())
    }

    /// Check if instance limit would be exceeded
    pub fn check_instance_limit(&self) -> WasmResult<()> {
        if self.active_instances >= self.max_instances {
            return Err(WasmRuntimeError::InstanceLimitExceeded {
                current: self.active_instances,
                limit: self.max_instances,
            });
        }
        Ok(())
    }

    /// Update memory usage
    pub fn update_memory_usage(&mut self, bytes: usize) -> WasmResult<()> {
        self.check_memory_limit(bytes)?;
        self.memory_usage += bytes;
        Ok(())
    }

    /// Update stack usage
    pub fn update_stack_usage(&mut self, bytes: usize) -> WasmResult<()> {
        self.check_stack_limit(bytes)?;
        self.stack_usage += bytes;
        Ok(())
    }

    /// Update CPU time usage
    pub fn update_cpu_time(&mut self, time: Duration) -> WasmResult<()> {
        self.check_cpu_time_limit(time)?;
        self.cpu_time_used += time;
        Ok(())
    }

    /// Reset execution session
    pub fn reset_session(&mut self) {
        self.cpu_time_used = Duration::ZERO;
        self.session_start = Instant::now();
    }

    /// Get current session duration
    pub fn session_duration(&self) -> Duration {
        self.session_start.elapsed()
    }
}

impl HostFunctionRegistry {
    /// Create a new host function registry
    pub fn new(_config: &HostFunctionConfig) -> Self {
        Self {
            device_functions: HashMap::new(),
            sensor_functions: HashMap::new(),
            security_functions: HashMap::new(),
            gpio_functions: HashMap::new(),
            i2c_spi_functions: HashMap::new(),
        }
    }
    
    /// Get device functions
    pub fn device_functions(&self) -> Option<&HashMap<String, DeviceFunction>> {
        Some(&self.device_functions)
    }
    
    /// Get sensor functions
    pub fn sensor_functions(&self) -> Option<&HashMap<String, SensorFunction>> {
        Some(&self.sensor_functions)
    }
    
    /// Get security functions
    pub fn security_functions(&self) -> Option<&HashMap<String, SecurityFunction>> {
        Some(&self.security_functions)
    }
    
    /// Get GPIO functions
    pub fn gpio_functions(&self) -> Option<&HashMap<String, GpioFunction>> {
        Some(&self.gpio_functions)
    }
    
    /// Get I2C/SPI functions
    pub fn i2c_spi_functions(&self) -> Option<&HashMap<String, I2cSpiFunction>> {
        Some(&self.i2c_spi_functions)
    }
}

impl DeviceState {
    /// Create a new device state
    pub fn new() -> Self {
        Self {
            gpio_pins: DashMap::new(),
            i2c_devices: DashMap::new(),
            spi_devices: DashMap::new(),
            sensor_readings: DashMap::new(),
            network_connections: DashMap::new(),
            filesystem_state: Arc::new(FilesystemState::new()),
        }
    }
}

impl SecurityContext {
    /// Create a new security context
    pub fn new() -> Self {
        Self {
            encryption_keys: DashMap::new(),
            mac_keys: DashMap::new(),
            session_keys: DashMap::new(),
            certificates: DashMap::new(),
            nonce_counters: DashMap::new(),
        }
    }
}

impl FilesystemState {
    /// Create a new file system state
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            directories: DashMap::new(),
            total_space: 1024 * 1024 * 1024, // 1GB default
            used_space: 0,
        }
    }
}
