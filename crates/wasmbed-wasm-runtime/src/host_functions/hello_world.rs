// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::context::WasmContext;
use crate::error::{WasmResult, WasmRuntimeError};
use crate::host_functions::{HostFunctionModule, create_wasm_function_void, write_string_to_memory};
use std::sync::Arc;
use wasmtime::*;

/// Simple hello world host functions for basic WASM execution
pub struct HelloWorldHostFunctions {
    context: Arc<WasmContext>,
}

impl HelloWorldHostFunctions {
    /// Create new hello world host functions
    pub fn new(context: Arc<WasmContext>) -> WasmResult<Self> {
        Ok(Self { context })
    }

    /// Print a message to the console
    pub fn print_message(
        &self,
        caller: &mut Caller<'_, WasmContext>,
        args: &[wasmtime::Val],
    ) -> Result<(), WasmRuntimeError> {
        if args.len() < 2 {
            return Err(WasmRuntimeError::ExecutionError(
                "print_message requires 2 parameters (ptr, len)".to_string()
            ));
        }

        let ptr = match args[0] {
            wasmtime::Val::I32(p) => p as usize,
            _ => return Err(WasmRuntimeError::ExecutionError("ptr must be i32".to_string())),
        };

        let len = match args[1] {
            wasmtime::Val::I32(l) => l as usize,
            _ => return Err(WasmRuntimeError::ExecutionError("len must be i32".to_string())),
        };

        // Read string from WASM memory
        let memory = caller.get_export("memory")
            .ok_or_else(|| WasmRuntimeError::HostFunctionError("Memory not found".to_string()))?
            .into_memory()
            .ok_or_else(|| WasmRuntimeError::HostFunctionError("Export is not memory".to_string()))?;

        let mut data = vec![0u8; len];
        memory.read(caller, ptr, &mut data)
            .map_err(|e| WasmRuntimeError::HostFunctionError(format!("Failed to read memory: {}", e)))?;

        let message = String::from_utf8(data)
            .map_err(|e| WasmRuntimeError::HostFunctionError(format!("Invalid UTF-8: {}", e)))?;

        println!("Hello from WASM: {}", message);
        Ok(())
    }

    /// Get current timestamp
    pub fn get_timestamp(
        &self,
        caller: &mut Caller<'_, WasmContext>,
        args: &[wasmtime::Val],
    ) -> Result<(), WasmRuntimeError> {
        if args.len() < 1 {
            return Err(WasmRuntimeError::ExecutionError(
                "get_timestamp requires 1 parameter (memory address)".to_string()
            ));
        }

        let memory_address = match args[0] {
            wasmtime::Val::I32(addr) => addr as usize,
            _ => return Err(WasmRuntimeError::ExecutionError("memory address must be i32".to_string())),
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let timestamp_str = timestamp.to_string();
        write_string_to_memory(caller, memory_address as i32, timestamp_str.as_bytes())?;

        Ok(())
    }
}

impl HostFunctionModule for HelloWorldHostFunctions {
    fn create_imports(&self, store: &mut Store<WasmContext>) -> WasmResult<Vec<Extern>> {
        let mut imports = Vec::new();

        // Create print_message function with correct signature (i32, i32) -> ()
        let print_message_func_type = wasmtime::FuncType::new(
            vec![wasmtime::ValType::I32, wasmtime::ValType::I32], // ptr, len
            vec![], // no return value
        );
        
        let print_message = Func::new(&mut *store, print_message_func_type, {
            let context = self.context.clone();
            move |mut caller: Caller<'_, WasmContext>, args: &[wasmtime::Val], _results: &mut [wasmtime::Val]| {
                if args.len() != 2 {
                    eprintln!("print_message expects 2 arguments");
                    return Ok(());
                }
                
                let ptr = match args[0] {
                    wasmtime::Val::I32(p) => p as usize,
                    _ => {
                        eprintln!("print_message: first argument must be i32");
                        return Ok(());
                    }
                };
                
                let len = match args[1] {
                    wasmtime::Val::I32(l) => l as usize,
                    _ => {
                        eprintln!("print_message: second argument must be i32");
                        return Ok(());
                    }
                };
                
                // Read string from WASM memory
                let memory = match caller.get_export("memory") {
                    Some(mem) => mem.into_memory().unwrap(),
                    None => {
                        eprintln!("print_message: memory not found");
                        return Ok(());
                    }
                };

                let mut data = vec![0u8; len];
                if let Err(e) = memory.read(&mut caller, ptr, &mut data) {
                    eprintln!("print_message: failed to read memory: {}", e);
                    return Ok(());
                }

                let message = match String::from_utf8(data) {
                    Ok(msg) => msg,
                    Err(e) => {
                        eprintln!("print_message: invalid UTF-8: {}", e);
                        return Ok(());
                    }
                };

                println!("Hello from WASM: {}", message);
                Ok(())
            }
        });

        // Create get_timestamp function with correct signature (i32) -> ()
        let get_timestamp_func_type = wasmtime::FuncType::new(
            vec![wasmtime::ValType::I32], // memory address
            vec![], // no return value
        );
        
        let get_timestamp = Func::new(&mut *store, get_timestamp_func_type, {
            let context = self.context.clone();
            move |mut caller: Caller<'_, WasmContext>, args: &[wasmtime::Val], _results: &mut [wasmtime::Val]| {
                if args.len() != 1 {
                    eprintln!("get_timestamp expects 1 argument");
                    return Ok(());
                }
                
                let memory_address = match args[0] {
                    wasmtime::Val::I32(addr) => addr as usize,
                    _ => {
                        eprintln!("get_timestamp: argument must be i32");
                        return Ok(());
                    }
                };

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let timestamp_str = timestamp.to_string();
                
                // Write timestamp to WASM memory
                let memory = match caller.get_export("memory") {
                    Some(mem) => mem.into_memory().unwrap(),
                    None => {
                        eprintln!("get_timestamp: memory not found");
                        return Ok(());
                    }
                };

                if let Err(e) = memory.write(&mut caller, memory_address, timestamp_str.as_bytes()) {
                    eprintln!("get_timestamp: failed to write memory: {}", e);
                    return Ok(());
                }

                Ok(())
            }
        });

        // Add functions to imports
        imports.push(Extern::Func(print_message));
        imports.push(Extern::Func(get_timestamp));

        Ok(imports)
    }
}
