// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Device architecture types supported by the WASM runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceArchitecture {
    /// Microprocessor Unit (Linux-based, full-featured)
    Mpu,
    /// Microcontroller Unit (ARM Cortex-M, resource-constrained)
    Mcu,
    /// RISC-V processor (SiFive HiFive1, balanced features)
    RiscV,
}

/// WASM runtime configuration optimized for specific device architectures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmRuntimeConfig {
    /// Maximum memory allocation in bytes
    pub max_memory: usize,
    /// Maximum stack size in bytes
    pub max_stack_size: usize,
    /// Maximum execution time per function call
    pub max_execution_time: Duration,
    /// Enable SIMD instructions
    pub enable_simd: bool,
    /// Enable multi-threading
    pub enable_threads: bool,
    /// Enable bulk memory operations
    pub enable_bulk_memory: bool,
    /// Enable reference types
    pub enable_reference_types: bool,
    /// Enable tail call optimization
    pub enable_tail_calls: bool,
    /// Enable function references
    pub enable_function_references: bool,
    /// Maximum number of WASM instances per device
    pub max_instances: usize,
    /// Maximum number of functions per instance
    pub max_functions_per_instance: usize,
    /// Enable debug information
    pub enable_debug: bool,
    /// Enable profiling
    pub enable_profiling: bool,
}

impl WasmRuntimeConfig {
    /// Create a configuration optimized for MPU (Microprocessor Unit)
    /// 
    /// MPUs are Linux-based systems with abundant resources, supporting
    /// full-featured WASM execution with all optimizations enabled.
    pub fn for_mpu() -> Self {
        Self {
            max_memory: 8 * 1024 * 1024 * 1024, // 8GB
            max_stack_size: 8 * 1024 * 1024,    // 8MB
            max_execution_time: Duration::from_secs(60), // 60 seconds
            enable_simd: true,
            enable_threads: true,
            enable_bulk_memory: true,
            enable_reference_types: true,
            enable_tail_calls: true,
            enable_function_references: true,
            max_instances: 100,
            max_functions_per_instance: 1000,
            enable_debug: true,
            enable_profiling: true,
        }
    }

    /// Create a configuration optimized for MCU (Microcontroller Unit)
    /// 
    /// MCUs are ARM Cortex-M systems with very limited resources, requiring
    /// minimal WASM runtime with strict memory and time constraints.
    pub fn for_mcu() -> Self {
        Self {
            max_memory: 64 * 1024,               // 64KB
            max_stack_size: 8 * 1024,            // 8KB
            max_execution_time: Duration::from_millis(100), // 100ms
            enable_simd: false,
            enable_threads: false,
            enable_bulk_memory: false,
            enable_reference_types: false,
            enable_tail_calls: false,
            enable_function_references: false,
            max_instances: 5,
            max_functions_per_instance: 50,
            enable_debug: false,
            enable_profiling: false,
        }
    }

    /// Create a configuration optimized for RISC-V
    /// 
    /// RISC-V systems like SiFive HiFive1 have moderate resources, supporting
    /// balanced WASM execution with some optimizations enabled.
    pub fn for_riscv() -> Self {
        Self {
            max_memory: 512 * 1024,              // 512KB
            max_stack_size: 32 * 1024,           // 32KB
            max_execution_time: Duration::from_millis(500), // 500ms
            enable_simd: false,
            enable_threads: false,
            enable_bulk_memory: true,
            enable_reference_types: true,
            enable_tail_calls: true,
            enable_function_references: false,
            max_instances: 20,
            max_functions_per_instance: 200,
            enable_debug: true,
            enable_profiling: false,
        }
    }

    /// Get configuration for a specific device architecture
    pub fn for_architecture(arch: DeviceArchitecture) -> Self {
        match arch {
            DeviceArchitecture::Mpu => Self::for_mpu(),
            DeviceArchitecture::Mcu => Self::for_mcu(),
            DeviceArchitecture::RiscV => Self::for_riscv(),
        }
    }

    /// Validate the configuration for consistency
    pub fn validate(&self) -> Result<(), String> {
        if self.max_memory == 0 {
            return Err("max_memory must be greater than 0".to_string());
        }
        
        if self.max_stack_size == 0 {
            return Err("max_stack_size must be greater than 0".to_string());
        }
        
        if self.max_stack_size > self.max_memory {
            return Err("max_stack_size cannot exceed max_memory".to_string());
        }
        
        if self.max_execution_time.as_nanos() == 0 {
            return Err("max_execution_time must be greater than 0".to_string());
        }
        
        if self.max_instances == 0 {
            return Err("max_instances must be greater than 0".to_string());
        }
        
        if self.max_functions_per_instance == 0 {
            return Err("max_functions_per_instance must be greater than 0".to_string());
        }
        
        Ok(())
    }
}

impl Default for WasmRuntimeConfig {
    fn default() -> Self {
        Self::for_mpu()
    }
}

/// Host function configuration for different integrations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionConfig {
    /// Enable device communication functions
    pub enable_device: bool,
    /// Enable sensor access functions
    pub enable_sensors: bool,
    /// Enable security functions
    pub enable_secure: bool,
    /// Enable filesystem functions
    pub enable_filesystem: bool,
    /// Enable network functions
    pub enable_network: bool,
    /// Enable GPIO functions
    pub enable_gpio: bool,
    /// Enable I2C/SPI functions
    pub enable_i2c_spi: bool,
}

impl HostFunctionConfig {
    /// Create configuration for MPU with all features enabled
    pub fn for_mpu() -> Self {
        Self {
            enable_device: true,
            enable_sensors: true,
            enable_secure: true,
            enable_filesystem: true,
            enable_network: true,
            enable_gpio: true,
            enable_i2c_spi: true,
        }
    }

    /// Create configuration for MCU with minimal features
    pub fn for_mcu() -> Self {
        Self {
            enable_device: true,
            enable_sensors: false, // Too resource-intensive
            enable_secure: false, // Minimal security
            enable_filesystem: false,
            enable_network: false,
            enable_gpio: true,
            enable_i2c_spi: true,
        }
    }

    /// Create configuration for RISC-V with balanced features
    pub fn for_riscv() -> Self {
        Self {
            enable_device: true,
            enable_sensors: true,
            enable_secure: true,
            enable_filesystem: false, // Limited storage
            enable_network: true,
            enable_gpio: true,
            enable_i2c_spi: true,
        }
    }

    /// Get configuration for a specific device architecture
    pub fn for_architecture(arch: DeviceArchitecture) -> Self {
        match arch {
            DeviceArchitecture::Mpu => Self::for_mpu(),
            DeviceArchitecture::Mcu => Self::for_mcu(),
            DeviceArchitecture::RiscV => Self::for_riscv(),
        }
    }
}

impl Default for HostFunctionConfig {
    fn default() -> Self {
        Self::for_mpu()
    }
}

/// Complete runtime configuration combining WASM and host function settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Device architecture
    pub architecture: DeviceArchitecture,
    /// WASM runtime configuration
    pub wasm_config: WasmRuntimeConfig,
    /// Host function configuration
    pub host_config: HostFunctionConfig,
    /// Device-specific identifier
    pub device_id: String,
    /// Gateway endpoint for communication
    pub gateway_endpoint: Option<String>,
}

impl RuntimeConfig {
    /// Create a complete configuration for a specific device architecture
    pub fn for_architecture(arch: DeviceArchitecture, device_id: String) -> Self {
        Self {
            architecture: arch,
            wasm_config: WasmRuntimeConfig::for_architecture(arch),
            host_config: HostFunctionConfig::for_architecture(arch),
            device_id,
            gateway_endpoint: None,
        }
    }

    /// Validate the complete configuration
    pub fn validate(&self) -> Result<(), String> {
        self.wasm_config.validate()?;
        
        if self.device_id.is_empty() {
            return Err("device_id cannot be empty".to_string());
        }
        
        Ok(())
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self::for_architecture(DeviceArchitecture::Mpu, "default-device".to_string())
    }
}
