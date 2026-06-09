// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::config::DeviceArchitecture;
use crate::error::{WasmResult, WasmRuntimeError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Device-specific information and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device architecture
    pub architecture: DeviceArchitecture,
    /// Device identifier
    pub device_id: String,
    /// Device name
    pub name: String,
    /// Device manufacturer
    pub manufacturer: String,
    /// Device model
    pub model: String,
    /// Device version
    pub version: String,
    /// CPU architecture
    pub cpu_arch: String,
    /// Memory size in bytes
    pub memory_size: u64,
    /// Flash size in bytes
    pub flash_size: u64,
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// CPU frequency in Hz
    pub cpu_frequency: u64,
    /// Available features
    pub features: Vec<String>,
    /// Device capabilities
    pub capabilities: DeviceCapabilities,
}

/// Device capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    /// Supports SIMD instructions
    pub simd: bool,
    /// Supports multi-threading
    pub threading: bool,
    /// Supports floating point
    pub floating_point: bool,
    /// Supports hardware crypto
    pub crypto: bool,
    /// Supports DMA
    pub dma: bool,
    /// Supports interrupts
    pub interrupts: bool,
    /// Supports timers
    pub timers: bool,
    /// Supports watchdog
    pub watchdog: bool,
    /// Supports power management
    pub power_management: bool,
    /// Supports sleep modes
    pub sleep_modes: bool,
    /// Number of GPIO pins
    pub gpio_pins: u32,
    /// Number of I2C buses
    pub i2c_buses: u32,
    /// Number of SPI buses
    pub spi_buses: u32,
    /// Number of UART ports
    pub uart_ports: u32,
    /// Number of ADC channels
    pub adc_channels: u32,
    /// Number of PWM channels
    pub pwm_channels: u32,
}

/// Device manager for handling device-specific operations
pub struct DeviceManager {
    /// Device information
    device_info: DeviceInfo,
    /// Device state
    device_state: DeviceState,
}

/// Device state
#[derive(Debug, Clone)]
pub struct DeviceState {
    /// Power state
    pub power_state: PowerState,
    /// Temperature in Celsius
    pub temperature: f32,
    /// Voltage in Volts
    pub voltage: f32,
    /// Current in Amperes
    pub current: f32,
    /// Uptime in seconds
    pub uptime: u64,
    /// Boot count
    pub boot_count: u32,
    /// Error count
    pub error_count: u32,
    /// Last error
    pub last_error: Option<String>,
}

/// Power states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerState {
    /// Device is running normally
    Active,
    /// Device is in low power mode
    Sleep,
    /// Device is in deep sleep mode
    DeepSleep,
    /// Device is hibernating
    Hibernate,
    /// Device is powered off
    Off,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new(architecture: DeviceArchitecture, device_id: String) -> WasmResult<Self> {
        let device_info = Self::create_device_info(architecture, device_id)?;
        let device_state = DeviceState::new();

        Ok(Self {
            device_info,
            device_state,
        })
    }

    /// Create device information based on architecture
    fn create_device_info(architecture: DeviceArchitecture, device_id: String) -> WasmResult<DeviceInfo> {
        let (name, manufacturer, model, version, cpu_arch, memory_size, flash_size, cpu_cores, cpu_frequency, capabilities) = match architecture {
            DeviceArchitecture::Mpu => (
                "Gateway MPU".to_string(),
                "Generic".to_string(),
                "x86_64".to_string(),
                "1.0".to_string(),
                "x86_64".to_string(),
                8 * 1024 * 1024 * 1024, // 8GB
                256 * 1024 * 1024,      // 256MB
                4,                      // 4 cores
                2_400_000_000,          // 2.4GHz
                DeviceCapabilities {
                    simd: true,
                    threading: true,
                    floating_point: true,
                    crypto: true,
                    dma: true,
                    interrupts: true,
                    timers: true,
                    watchdog: true,
                    power_management: true,
                    sleep_modes: true,
                    gpio_pins: 40,
                    i2c_buses: 2,
                    spi_buses: 2,
                    uart_ports: 4,
                    adc_channels: 8,
                    pwm_channels: 8,
                }
            ),
            DeviceArchitecture::Mcu => (
                "Edge MCU".to_string(),
                "STMicroelectronics".to_string(),
                "STM32F4".to_string(),
                "1.0".to_string(),
                "ARM Cortex-M4".to_string(),
                256 * 1024,             // 256KB
                1 * 1024 * 1024,        // 1MB
                1,                      // 1 core
                168_000_000,            // 168MHz
                DeviceCapabilities {
                    simd: false,
                    threading: false,
                    floating_point: true,
                    crypto: false,
                    dma: true,
                    interrupts: true,
                    timers: true,
                    watchdog: true,
                    power_management: true,
                    sleep_modes: true,
                    gpio_pins: 82,
                    i2c_buses: 3,
                    spi_buses: 3,
                    uart_ports: 6,
                    adc_channels: 16,
                    pwm_channels: 12,
                }
            ),
            DeviceArchitecture::RiscV => (
                "Edge RISC-V".to_string(),
                "SiFive".to_string(),
                "HiFive1".to_string(),
                "1.0".to_string(),
                "RISC-V".to_string(),
                16 * 1024 * 1024,       // 16MB
                32 * 1024 * 1024,       // 32MB
                1,                      // 1 core
                320_000_000,            // 320MHz
                DeviceCapabilities {
                    simd: false,
                    threading: false,
                    floating_point: true,
                    crypto: false,
                    dma: true,
                    interrupts: true,
                    timers: true,
                    watchdog: true,
                    power_management: true,
                    sleep_modes: true,
                    gpio_pins: 19,
                    i2c_buses: 1,
                    spi_buses: 1,
                    uart_ports: 1,
                    adc_channels: 0,
                    pwm_channels: 0,
                }
            ),
        };

        Ok(DeviceInfo {
            architecture,
            device_id,
            name,
            manufacturer,
            model,
            version,
            cpu_arch,
            memory_size,
            flash_size,
            cpu_cores,
            cpu_frequency,
            features: vec![
                "wasm".to_string(),
                "tls".to_string(),
                "dds".to_string(),
                "mavlink".to_string(),
            ],
            capabilities,
        })
    }

    /// Get device information
    pub fn get_device_info(&self) -> &DeviceInfo {
        &self.device_info
    }

    /// Get device state
    pub fn get_device_state(&self) -> &DeviceState {
        &self.device_state
    }

    /// Update device state
    pub fn update_device_state(&mut self, temperature: f32, voltage: f32, current: f32) {
        self.device_state.temperature = temperature;
        self.device_state.voltage = voltage;
        self.device_state.current = current;
        self.device_state.uptime += 1; // Increment uptime
    }

    /// Set power state
    pub fn set_power_state(&mut self, state: PowerState) {
        self.device_state.power_state = state;
    }

    /// Increment boot count
    pub fn increment_boot_count(&mut self) {
        self.device_state.boot_count += 1;
    }

    /// Record error
    pub fn record_error(&mut self, error: String) {
        self.device_state.error_count += 1;
        self.device_state.last_error = Some(error);
    }

    /// Check if device supports a feature
    pub fn supports_feature(&self, feature: &str) -> bool {
        self.device_info.features.contains(&feature.to_string())
    }

    /// Check if device has a capability
    pub fn has_capability(&self, capability: &str) -> bool {
        match capability {
            "simd" => self.device_info.capabilities.simd,
            "threading" => self.device_info.capabilities.threading,
            "floating_point" => self.device_info.capabilities.floating_point,
            "crypto" => self.device_info.capabilities.crypto,
            "dma" => self.device_info.capabilities.dma,
            "interrupts" => self.device_info.capabilities.interrupts,
            "timers" => self.device_info.capabilities.timers,
            "watchdog" => self.device_info.capabilities.watchdog,
            "power_management" => self.device_info.capabilities.power_management,
            "sleep_modes" => self.device_info.capabilities.sleep_modes,
            _ => false,
        }
    }

    /// Get device resource usage
    pub fn get_resource_usage(&self) -> ResourceUsage {
        ResourceUsage {
            memory_used: self.device_info.memory_size * 3 / 10, // 30% usage
            memory_total: self.device_info.memory_size,
            flash_used: self.device_info.flash_size / 2, // 50% usage
            flash_total: self.device_info.flash_size,
            cpu_usage: 0.25, // 25% usage
            temperature: self.device_state.temperature,
            voltage: self.device_state.voltage,
            current: self.device_state.current,
        }
    }
}

impl DeviceState {
    /// Create a new device state
    pub fn new() -> Self {
        Self {
            power_state: PowerState::Active,
            temperature: 25.0,
            voltage: 3.3,
            current: 0.1,
            uptime: 0,
            boot_count: 1,
            error_count: 0,
            last_error: None,
        }
    }
}

/// Resource usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub memory_used: u64,
    pub memory_total: u64,
    pub flash_used: u64,
    pub flash_total: u64,
    pub cpu_usage: f32,
    pub temperature: f32,
    pub voltage: f32,
    pub current: f32,
}
