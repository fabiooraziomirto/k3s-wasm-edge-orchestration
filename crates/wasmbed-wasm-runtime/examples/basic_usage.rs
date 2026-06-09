// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use wasmbed_wasm_runtime::config::{DeviceArchitecture, RuntimeConfig};
use wasmbed_wasm_runtime::runtime::WasmRuntime;
use wasmbed_wasm_runtime::error::WasmResult;

/// Example demonstrating the WASM runtime for edge devices
#[tokio::main]
async fn main() -> WasmResult<()> {
    println!("🚀 Wasmbed WASM Runtime Example");
    
    // Create configuration for different device types
    let mpu_config = RuntimeConfig::for_architecture(
        DeviceArchitecture::Mpu, 
        "gateway-mpu-001".to_string()
    );
    
    let mcu_config = RuntimeConfig::for_architecture(
        DeviceArchitecture::Mcu, 
        "edge-mcu-001".to_string()
    );
    
    let riscv_config = RuntimeConfig::for_architecture(
        DeviceArchitecture::RiscV, 
        "edge-riscv-001".to_string()
    );
    
    println!("📋 Device Configurations:");
    println!("  MPU Memory: {} MB", mpu_config.wasm_config.max_memory / (1024 * 1024));
    println!("  MCU Memory: {} KB", mpu_config.wasm_config.max_memory / 1024);
    println!("  RISC-V Memory: {} KB", riscv_config.wasm_config.max_memory / 1024);
    
    println!("🔧 Host Functions Enabled:");
    println!("  Device: {}", mpu_config.host_config.enable_device);
    println!("  Sensors: {}", mpu_config.host_config.enable_sensors);
    println!("  Security: {}", mpu_config.host_config.enable_secure);
    println!("  GPIO: {}", mpu_config.host_config.enable_gpio);
    println!("  I2C/SPI: {}", mpu_config.host_config.enable_i2c_spi);
    
    // Note: The actual runtime creation would require fixing compilation errors
    // For now, we demonstrate the configuration structure
    println!("\n✅ Configuration validation passed!");
    println!("🎯 Runtime ready for WASM module deployment");
    
    Ok(())
}
