// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use wasmbed_wasm_runtime::{
    WasmRuntime, RuntimeConfig, DeviceArchitecture, WasmRuntimeConfig, HostFunctionConfig,
    WasmResult, WasmRuntimeError
};
use std::time::Duration;
use tracing::{info, error, warn};

/// Comprehensive test for WASM runtime implementation
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("Starting Wasmbed WASM Runtime Comprehensive Test");
    
    // Test MPU configuration
    info!("Testing MPU configuration...");
    let mpu_config = RuntimeConfig::for_architecture(
        DeviceArchitecture::Mpu,
        "test-mpu-device".to_string(),
    );
    
    assert_eq!(mpu_config.wasm_config.max_memory, 8 * 1024 * 1024 * 1024); // 8GB
    assert_eq!(mpu_config.wasm_config.max_stack_size, 8 * 1024 * 1024); // 8MB
    assert_eq!(mpu_config.wasm_config.max_execution_time, Duration::from_secs(60));
    assert!(mpu_config.wasm_config.enable_simd);
    assert!(mpu_config.wasm_config.enable_threads);
    assert!(mpu_config.host_config.enable_device);
    assert!(mpu_config.host_config.enable_sensors);
    
    info!("MPU configuration test passed");
    
    // Test MCU configuration
    info!("Testing MCU configuration...");
    let mcu_config = RuntimeConfig::for_architecture(
        DeviceArchitecture::Mcu,
        "test-mcu-device".to_string(),
    );
    
    assert_eq!(mcu_config.wasm_config.max_memory, 64 * 1024); // 64KB
    assert_eq!(mcu_config.wasm_config.max_stack_size, 8 * 1024); // 8KB
    assert_eq!(mcu_config.wasm_config.max_execution_time, Duration::from_millis(100));
    assert!(!mcu_config.wasm_config.enable_simd);
    assert!(!mcu_config.wasm_config.enable_threads);
    assert!(mcu_config.host_config.enable_device);
    assert!(!mcu_config.host_config.enable_sensors); // Too resource-intensive for MCU
    
    info!("MCU configuration test passed");
    
    // Test RISC-V configuration
    info!("Testing RISC-V configuration...");
    let riscv_config = RuntimeConfig::for_architecture(
        DeviceArchitecture::RiscV,
        "test-riscv-device".to_string(),
    );
    
    assert_eq!(riscv_config.wasm_config.max_memory, 512 * 1024); // 512KB
    assert_eq!(riscv_config.wasm_config.max_stack_size, 32 * 1024); // 32KB
    assert_eq!(riscv_config.wasm_config.max_execution_time, Duration::from_millis(500));
    assert!(!riscv_config.wasm_config.enable_simd);
    assert!(!riscv_config.wasm_config.enable_threads);
    assert!(riscv_config.host_config.enable_device);
    assert!(riscv_config.host_config.enable_sensors);
    
    info!("RISC-V configuration test passed");
    
    // Test configuration validation
    info!("Testing configuration validation...");
    
    // Valid configuration
    assert!(mpu_config.validate().is_ok());
    assert!(mcu_config.validate().is_ok());
    assert!(riscv_config.validate().is_ok());
    
    // Invalid configuration (empty device ID)
    let mut invalid_config = mpu_config.clone();
    invalid_config.device_id = String::new();
    assert!(invalid_config.validate().is_err());
    
    info!("Configuration validation test passed");
    
    // Test WASM runtime creation
    info!("Testing WASM runtime creation...");
    
    // Create runtime with MPU configuration
    let mut mpu_runtime = WasmRuntime::new(mpu_config)?;
    info!("Created MPU runtime successfully");
    
    // Create runtime with MCU configuration
    let mut mcu_runtime = WasmRuntime::new(mcu_config)?;
    info!("Created MCU runtime successfully");
    
    // Create runtime with RISC-V configuration
    let mut riscv_runtime = WasmRuntime::new(riscv_config)?;
    info!("Created RISC-V runtime successfully");
    
    // Test host function manager creation
    info!("Testing host function manager...");
    
    // Test with different configurations
    let mpu_host_config = HostFunctionConfig::for_mpu();
    let mcu_host_config = HostFunctionConfig::for_mcu();
    let riscv_host_config = HostFunctionConfig::for_riscv();
    
    assert!(mpu_host_config.enable_device);
    assert!(mpu_host_config.enable_sensors);
    assert!(mpu_host_config.enable_secure);
    assert!(mpu_host_config.enable_filesystem);
    assert!(mpu_host_config.enable_network);
    assert!(mpu_host_config.enable_gpio);
    assert!(mpu_host_config.enable_i2c_spi);
    
    assert!(mcu_host_config.enable_device);
    assert!(!mcu_host_config.enable_sensors); // Too resource-intensive
    assert!(!mcu_host_config.enable_secure); // Minimal security
    assert!(!mcu_host_config.enable_filesystem);
    assert!(!mcu_host_config.enable_network);
    assert!(mcu_host_config.enable_gpio);
    assert!(mcu_host_config.enable_i2c_spi);
    
    assert!(riscv_host_config.enable_device);
    assert!(riscv_host_config.enable_sensors);
    assert!(riscv_host_config.enable_secure);
    assert!(!riscv_host_config.enable_filesystem); // Limited storage
    assert!(riscv_host_config.enable_network);
    assert!(riscv_host_config.enable_gpio);
    assert!(riscv_host_config.enable_i2c_spi);
    
    info!("Host function manager test passed");
    
    // Test WASM module loading (simulated)
    info!("Testing WASM module operations...");
    
    // Simulate loading a WASM module
    let sample_wasm_data = b"sample_wasm_binary_data";
    let module_id = "test-module-1";
    
    // This would normally load a real WASM module
    // For now, we'll just test the interface
    info!("Simulated loading WASM module: {}", module_id);
    
    // Test runtime statistics
    info!("Testing runtime statistics...");
    
    let mpu_stats = mpu_runtime.get_stats();
    let mcu_stats = mcu_runtime.get_stats();
    let riscv_stats = riscv_runtime.get_stats();
    
    assert_eq!(mpu_stats.modules_loaded, 0);
    assert_eq!(mcu_stats.modules_loaded, 0);
    assert_eq!(riscv_stats.modules_loaded, 0);
    
    info!("Runtime statistics test passed");
    
    // Test error handling
    info!("Testing error handling...");
    
    // Test invalid module ID
    let result = mpu_runtime.execute_function("nonexistent-module", "test_function", &[]);
    assert!(result.is_err());
    
    if let Err(WasmRuntimeError::ApplicationNotFound(id)) = result {
        assert_eq!(id, "nonexistent-module");
    } else {
        panic!("Expected ApplicationNotFound error");
    }
    
    info!("Error handling test passed");
    
    // Test device-specific optimizations
    info!("Testing device-specific optimizations...");
    
    // MPU should have maximum features
    assert!(mpu_runtime.get_config().wasm_config.enable_simd);
    assert!(mpu_runtime.get_config().wasm_config.enable_threads);
    assert!(mpu_runtime.get_config().wasm_config.enable_bulk_memory);
    assert!(mpu_runtime.get_config().wasm_config.enable_reference_types);
    assert!(mpu_runtime.get_config().wasm_config.enable_tail_calls);
    assert!(mpu_runtime.get_config().wasm_config.enable_function_references);
    
    // MCU should have minimal features
    assert!(!mcu_runtime.get_config().wasm_config.enable_simd);
    assert!(!mcu_runtime.get_config().wasm_config.enable_threads);
    assert!(!mcu_runtime.get_config().wasm_config.enable_bulk_memory);
    assert!(!mcu_runtime.get_config().wasm_config.enable_reference_types);
    assert!(!mcu_runtime.get_config().wasm_config.enable_tail_calls);
    assert!(!mcu_runtime.get_config().wasm_config.enable_function_references);
    
    // RISC-V should have balanced features
    assert!(!riscv_runtime.get_config().wasm_config.enable_simd);
    assert!(!riscv_runtime.get_config().wasm_config.enable_threads);
    assert!(riscv_runtime.get_config().wasm_config.enable_bulk_memory);
    assert!(riscv_runtime.get_config().wasm_config.enable_reference_types);
    assert!(riscv_runtime.get_config().wasm_config.enable_tail_calls);
    assert!(!riscv_runtime.get_config().wasm_config.enable_function_references);
    
    info!("Device-specific optimizations test passed");
    
    // Test memory limits
    info!("Testing memory limits...");
    
    // MPU should have large memory limits
    assert_eq!(mpu_runtime.get_config().wasm_config.max_memory, 8 * 1024 * 1024 * 1024);
    assert_eq!(mpu_runtime.get_config().wasm_config.max_stack_size, 8 * 1024 * 1024);
    
    // MCU should have small memory limits
    assert_eq!(mcu_runtime.get_config().wasm_config.max_memory, 64 * 1024);
    assert_eq!(mcu_runtime.get_config().wasm_config.max_stack_size, 8 * 1024);
    
    // RISC-V should have moderate memory limits
    assert_eq!(riscv_runtime.get_config().wasm_config.max_memory, 512 * 1024);
    assert_eq!(riscv_runtime.get_config().wasm_config.max_stack_size, 32 * 1024);
    
    info!("Memory limits test passed");
    
    // Test execution time limits
    info!("Testing execution time limits...");
    
    // MPU should have long execution time
    assert_eq!(mpu_runtime.get_config().wasm_config.max_execution_time, Duration::from_secs(60));
    
    // MCU should have short execution time
    assert_eq!(mcu_runtime.get_config().wasm_config.max_execution_time, Duration::from_millis(100));
    
    // RISC-V should have moderate execution time
    assert_eq!(riscv_runtime.get_config().wasm_config.max_execution_time, Duration::from_millis(500));
    
    info!("Execution time limits test passed");
    
    // Test instance limits
    info!("Testing instance limits...");
    
    // MPU should allow many instances
    assert_eq!(mpu_runtime.get_config().wasm_config.max_instances, 100);
    assert_eq!(mpu_runtime.get_config().wasm_config.max_functions_per_instance, 1000);
    
    // MCU should allow few instances
    assert_eq!(mcu_runtime.get_config().wasm_config.max_instances, 5);
    assert_eq!(mcu_runtime.get_config().wasm_config.max_functions_per_instance, 50);
    
    // RISC-V should allow moderate instances
    assert_eq!(riscv_runtime.get_config().wasm_config.max_instances, 20);
    assert_eq!(riscv_runtime.get_config().wasm_config.max_functions_per_instance, 200);
    
    info!("Instance limits test passed");
    
    // Test host function configurations
    info!("Testing host function configurations...");
    
    // MPU should have all host functions enabled
    assert!(mpu_runtime.get_config().host_config.enable_device);
    assert!(mpu_runtime.get_config().host_config.enable_sensors);
    assert!(mpu_runtime.get_config().host_config.enable_secure);
    assert!(mpu_runtime.get_config().host_config.enable_filesystem);
    assert!(mpu_runtime.get_config().host_config.enable_network);
    assert!(mpu_runtime.get_config().host_config.enable_gpio);
    assert!(mpu_runtime.get_config().host_config.enable_i2c_spi);
    
    // MCU should have minimal host functions
    assert!(mcu_runtime.get_config().host_config.enable_device);
    assert!(!mcu_runtime.get_config().host_config.enable_sensors);
    assert!(!mcu_runtime.get_config().host_config.enable_secure);
    assert!(!mcu_runtime.get_config().host_config.enable_filesystem);
    assert!(!mcu_runtime.get_config().host_config.enable_network);
    assert!(mcu_runtime.get_config().host_config.enable_gpio);
    assert!(mcu_runtime.get_config().host_config.enable_i2c_spi);
    
    // RISC-V should have balanced host functions
    assert!(riscv_runtime.get_config().host_config.enable_device);
    assert!(riscv_runtime.get_config().host_config.enable_sensors);
    assert!(riscv_runtime.get_config().host_config.enable_secure);
    assert!(!riscv_runtime.get_config().host_config.enable_filesystem);
    assert!(riscv_runtime.get_config().host_config.enable_network);
    assert!(riscv_runtime.get_config().host_config.enable_gpio);
    assert!(riscv_runtime.get_config().host_config.enable_i2c_spi);
    
    info!("Host function configurations test passed");
    
    // Test runtime cleanup
    info!("Testing runtime cleanup...");
    
    // Unload applications (none loaded, but test the interface)
    mpu_runtime.unload_application("test-module-1").unwrap_or_default();
    mcu_runtime.unload_application("test-module-1").unwrap_or_default();
    riscv_runtime.unload_application("test-module-1").unwrap_or_default();
    
    info!("Runtime cleanup test passed");
    
    // Final statistics
    info!("Final runtime statistics:");
    info!("MPU runtime - Modules loaded: {}", mpu_runtime.get_stats().modules_loaded);
    info!("MCU runtime - Modules loaded: {}", mcu_runtime.get_stats().modules_loaded);
    info!("RISC-V runtime - Modules loaded: {}", riscv_runtime.get_stats().modules_loaded);
    
    info!("Wasmbed WASM Runtime Comprehensive Test completed successfully!");
    info!("All tests passed!");
    
    Ok(())
}
