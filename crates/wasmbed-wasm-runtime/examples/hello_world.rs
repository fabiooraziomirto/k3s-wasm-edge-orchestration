// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use wasmbed_wasm_runtime::{WasmRuntime, RuntimeConfig, DeviceArchitecture};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 Wasmbed Hello World Example");
    println!("==============================");

    // Create runtime configuration for MPU (Microprocessor Unit)
    let config = RuntimeConfig::for_architecture(
        DeviceArchitecture::Mpu,
        "hello-world-device".to_string(),
    );

    // Create WASM runtime
    let mut runtime = WasmRuntime::new(config)?;
    println!("✅ WASM Runtime created successfully");

    // Simple hello world WASM module (in WAT format)
    let wat_code = r#"
(module
  (import "env" "print_message" (func $print_message (param i32 i32)))
  (import "env" "get_timestamp" (func $get_timestamp (param i32)))
  
  (memory 1)
  (export "memory" (memory 0))
  
  (func $main
    ;; Allocate memory for "Hello, WASM World!" message
    (i32.store (i32.const 0) (i32.const 0))  ;; ptr = 0
    (i32.store (i32.const 4) (i32.const 19)) ;; len = 19
    
    ;; Store the message in memory
    (i32.store8 (i32.const 8) (i32.const 72))   ;; 'H'
    (i32.store8 (i32.const 9) (i32.const 101))  ;; 'e'
    (i32.store8 (i32.const 10) (i32.const 108)) ;; 'l'
    (i32.store8 (i32.const 11) (i32.const 108)) ;; 'l'
    (i32.store8 (i32.const 12) (i32.const 111)) ;; 'o'
    (i32.store8 (i32.const 13) (i32.const 44))  ;; ','
    (i32.store8 (i32.const 14) (i32.const 32))  ;; ' '
    (i32.store8 (i32.const 15) (i32.const 87))  ;; 'W'
    (i32.store8 (i32.const 16) (i32.const 65))  ;; 'A'
    (i32.store8 (i32.const 17) (i32.const 83))  ;; 'S'
    (i32.store8 (i32.const 18) (i32.const 77))  ;; 'M'
    (i32.store8 (i32.const 19) (i32.const 32))  ;; ' '
    (i32.store8 (i32.const 20) (i32.const 87))  ;; 'W'
    (i32.store8 (i32.const 21) (i32.const 111)) ;; 'o'
    (i32.store8 (i32.const 22) (i32.const 114))  ;; 'r'
    (i32.store8 (i32.const 23) (i32.const 108)) ;; 'l'
    (i32.store8 (i32.const 24) (i32.const 100)) ;; 'd'
    (i32.store8 (i32.const 25) (i32.const 33))  ;; '!'
    
    ;; Call print_message with ptr=8, len=19
    (call $print_message (i32.const 8) (i32.const 19))
    
    ;; Get timestamp and print it
    (call $get_timestamp (i32.const 100))
    
    ;; Read timestamp from memory and print it
    (call $print_message (i32.const 100) (i32.const 10))
  )
  
  (export "main" (func $main))
)
"#;

    // Compile WAT to WASM
    let wasm_bytes = wat::parse_str(wat_code)?;
    println!("✅ WAT compiled to WASM successfully");

    // Load and instantiate the WASM module
    let module_id = "hello_world_module";
    let metadata = runtime.load_module(module_id, &wasm_bytes).await?;
    println!("✅ WASM module loaded and instantiated: {:?}", metadata);

    // Create an instance from the module
    let instance_id = runtime.create_instance(module_id, None).await?;
    println!("✅ WASM instance created: {}", instance_id);

    // Execute the main function
    println!("\n🎯 Executing WASM module...");
    runtime.call_function(&instance_id, "main", &[]).await?;
    
    println!("\n🎉 Hello World example completed successfully!");
    println!("The WASM module printed messages and retrieved a timestamp.");
    
    Ok(())
}
