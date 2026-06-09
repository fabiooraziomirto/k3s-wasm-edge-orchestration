// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::config::HostFunctionConfig;
use crate::context::WasmContext;
use crate::error::{WasmResult, WasmRuntimeError};
use std::sync::Arc;
use wasmtime::*;

pub mod hello_world;

use hello_world::HelloWorldHostFunctions;

/// Manager for host functions available to WASM modules
pub struct HostFunctionManager {
    /// WASM context
    context: Arc<WasmContext>,
    /// Hello world host functions
    hello_world_functions: Option<HelloWorldHostFunctions>,
}

impl HostFunctionManager {
    /// Create a new host function manager
    pub fn new(config: &HostFunctionConfig, context: Arc<WasmContext>) -> WasmResult<Self> {
        let mut manager = Self {
            context: context.clone(),
            hello_world_functions: None,
        };

        // Always enable hello world functions for basic functionality
        manager.hello_world_functions = Some(HelloWorldHostFunctions::new(context.clone())?);

        Ok(manager)
    }

    /// Get the hello world functions (for testing purposes)
    pub fn hello_world_functions(&self) -> &Option<HelloWorldHostFunctions> {
        &self.hello_world_functions
    }

    /// Get device functions (delegates to context)
    pub fn device_functions(&self) -> Option<&std::collections::HashMap<String, crate::context::DeviceFunction>> {
        self.context.host_functions.device_functions()
    }

    /// Get sensor functions (delegates to context)
    pub fn sensor_functions(&self) -> Option<&std::collections::HashMap<String, crate::context::SensorFunction>> {
        self.context.host_functions.sensor_functions()
    }

    /// Get security functions (delegates to context)
    pub fn security_functions(&self) -> Option<&std::collections::HashMap<String, crate::context::SecurityFunction>> {
        self.context.host_functions.security_functions()
    }

    /// Get GPIO functions (delegates to context)
    pub fn gpio_functions(&self) -> Option<&std::collections::HashMap<String, crate::context::GpioFunction>> {
        self.context.host_functions.gpio_functions()
    }

    /// Get I2C/SPI functions (delegates to context)
    pub fn i2c_spi_functions(&self) -> Option<&std::collections::HashMap<String, crate::context::I2cSpiFunction>> {
        self.context.host_functions.i2c_spi_functions()
    }

    /// Create WASM time imports for all enabled host functions
    pub fn create_imports(&self, store: &mut Store<WasmContext>) -> WasmResult<Vec<Extern>> {
        let mut imports = Vec::new();

        // Add hello world functions
        if let Some(ref hello_world_functions) = self.hello_world_functions {
            imports.extend(hello_world_functions.create_imports(store)?);
        }

        Ok(imports)
    }
}

/// Base trait for host function modules
pub trait HostFunctionModule {
    /// Create WASM time imports for this module
    fn create_imports(&self, store: &mut Store<WasmContext>) -> WasmResult<Vec<Extern>>;
}

/// Helper function to create a WASM time function that returns nothing
pub fn create_wasm_function_void<F>(
    store: &mut Store<WasmContext>,
    func: F,
) -> WasmResult<Func>
where
    F: Fn(&mut Caller<'_, WasmContext>, &[wasmtime::Val]) -> Result<(), WasmRuntimeError> + Send + Sync + 'static,
{
    let func_type = wasmtime::FuncType::new(
        std::iter::empty::<wasmtime::ValType>(),  // No parameters for now - this should be configurable
        std::iter::empty::<wasmtime::ValType>(),  // No results for now - this should be configurable
    );
    
    let func = Func::new(store, func_type, move |mut caller, args, results| {
        // Convert wasmtime::Val to our expected format
        let vals: Vec<wasmtime::Val> = args.iter().cloned().collect();
        
        if let Err(e) = func(&mut caller, &vals) {
            eprintln!("Host function error: {}", e);
        }
        Ok(())
    });
    Ok(func)
}

/// Helper function to extract string from WASM memory
pub fn extract_string_from_memory(
    caller: &mut Caller<'_, WasmContext>,
    ptr: i32,
    len: i32,
) -> WasmResult<String> {
    let memory = caller.get_export("memory")
        .ok_or_else(|| WasmRuntimeError::HostFunctionError("Memory not found".to_string()))?
        .into_memory()
        .ok_or_else(|| WasmRuntimeError::HostFunctionError("Export is not memory".to_string()))?;

    let mut data = vec![0u8; len as usize];
    memory.read(caller, ptr as usize, &mut data)
        .map_err(|e| WasmRuntimeError::HostFunctionError(format!("Failed to read memory: {}", e)))?;

    String::from_utf8(data)
        .map_err(|e| WasmRuntimeError::HostFunctionError(format!("Invalid UTF-8: {}", e)))
}

/// Helper function to write string to WASM memory
pub fn write_string_to_memory(
    caller: &mut Caller<'_, WasmContext>,
    ptr: i32,
    data: &[u8],
) -> WasmResult<()> {
    let memory = caller.get_export("memory")
        .ok_or_else(|| WasmRuntimeError::HostFunctionError("Memory not found".to_string()))?
        .into_memory()
        .ok_or_else(|| WasmRuntimeError::HostFunctionError("Export is not memory".to_string()))?;

    memory.write(caller, ptr as usize, data)
        .map_err(|e| WasmRuntimeError::HostFunctionError(format!("Failed to write memory: {}", e)))?;

    Ok(())
}

