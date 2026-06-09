// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::config::WasmRuntimeConfig;
use crate::error::{WasmResult, WasmRuntimeError};
use wasmparser::{Parser, Payload, Validator, WasmFeatures};

/// WASM module validator with device-specific constraints
pub struct WasmValidator {
    /// Runtime configuration for validation constraints
    config: WasmRuntimeConfig,
}

impl WasmValidator {
    /// Create a new WASM validator
    pub fn new(config: &WasmRuntimeConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    /// Validate a WASM module against device constraints
    pub fn validate_module(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        // Check basic WASM format
        self.validate_wasm_format(wasm_bytes)?;

        // Check size constraints
        self.validate_size_constraints(wasm_bytes)?;

        // Check WASM features compatibility
        self.validate_features(wasm_bytes)?;

        // Check memory constraints
        self.validate_memory_constraints(wasm_bytes)?;

        // Check function constraints
        self.validate_function_constraints(wasm_bytes)?;

        // Check import constraints
        self.validate_import_constraints(wasm_bytes)?;

        Ok(())
    }

    /// Validate basic WASM format
    fn validate_wasm_format(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        if wasm_bytes.len() < 8 {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                "WASM module too small".to_string()
            ));
        }

        // Check WASM magic number
        if &wasm_bytes[0..4] != b"\0asm" {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                "Invalid WASM magic number".to_string()
            ));
        }

        // Check WASM version
        let version = u32::from_le_bytes([wasm_bytes[4], wasm_bytes[5], wasm_bytes[6], wasm_bytes[7]]);
        if version != 1 {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                format!("Unsupported WASM version: {}", version)
            ));
        }

        Ok(())
    }

    /// Validate size constraints
    fn validate_size_constraints(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        if wasm_bytes.len() > self.config.max_memory / 4 {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                format!(
                    "Module size {} bytes exceeds limit {} bytes",
                    wasm_bytes.len(),
                    self.config.max_memory / 4
                )
            ));
        }

        Ok(())
    }

    /// Validate WASM features compatibility
    fn validate_features(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        // Simplified validation - just check basic WASM format
        // In a production system, you would use the full wasmparser validation
        let mut parser = Parser::new(0);
        
        for payload in parser.parse_all(wasm_bytes) {
            match payload? {
                Payload::Version { .. } => continue,
                Payload::TypeSection(_) => continue,
                Payload::ImportSection(_) => continue,
                Payload::FunctionSection(_) => continue,
                Payload::TableSection(_) => continue,
                Payload::MemorySection(_) => continue,
                Payload::GlobalSection(_) => continue,
                Payload::ExportSection(_) => continue,
                Payload::StartSection { .. } => continue,
                Payload::ElementSection(_) => continue,
                Payload::DataSection(_) => continue,
                Payload::CodeSectionStart { .. } => continue,
                Payload::CodeSectionEntry(_) => continue,
                Payload::DataCountSection { .. } => continue,
                Payload::CustomSection(_) => continue,
                Payload::UnknownSection { .. } => continue,
                Payload::End(_) => break,
                _ => continue, // Handle other payload types as needed
            }
        }

        Ok(())
    }

    /// Validate memory constraints
    fn validate_memory_constraints(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        let mut parser = Parser::new(0);
        let mut max_memory_pages = 0;

        for payload in parser.parse_all(wasm_bytes) {
            match payload? {
                Payload::MemorySection(reader) => {
                    for memory in reader {
                        let memory = memory?;
                        if let Some(max) = memory.maximum {
                            max_memory_pages = max_memory_pages.max(max);
                        }
                    }
                }
                Payload::End(_) => break,
                _ => continue,
            }
        }

        // Convert pages to bytes (1 page = 64KB)
        let max_memory_bytes = max_memory_pages as usize * 64 * 1024;
        
        if max_memory_bytes > self.config.max_memory {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                format!(
                    "Module memory requirement {} bytes exceeds device limit {} bytes",
                    max_memory_bytes,
                    self.config.max_memory
                )
            ));
        }

        Ok(())
    }

    /// Validate function constraints
    fn validate_function_constraints(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        let mut parser = Parser::new(0);
        let mut function_count = 0;

        for payload in parser.parse_all(wasm_bytes) {
            match payload? {
                Payload::FunctionSection(reader) => {
                    function_count = reader.count();
                }
                Payload::End(_) => break,
                _ => continue,
            }
        }

        if function_count > self.config.max_functions_per_instance as u32 {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                format!(
                    "Module has {} functions, exceeds limit of {}",
                    function_count,
                    self.config.max_functions_per_instance
                )
            ));
        }

        Ok(())
    }

    /// Validate import constraints
    fn validate_import_constraints(&self, wasm_bytes: &[u8]) -> WasmResult<()> {
        let mut parser = Parser::new(0);
        let mut import_count = 0;

        for payload in parser.parse_all(wasm_bytes) {
            match payload? {
                Payload::ImportSection(reader) => {
                    for import in reader {
                        let import = import?;
                        import_count += 1;
                        
                        // Validate import module name
                        let module_name = import.module;
                        if !self.is_allowed_import_module(module_name) {
                            return Err(WasmRuntimeError::ModuleValidationFailed(
                                format!("Disallowed import module: {}", module_name)
                            ));
                        }
                    }
                }
                Payload::End(_) => break,
                _ => continue,
            }
        }

        // Limit number of imports for resource-constrained devices
        let max_imports = match self.config.max_memory {
            mem if mem < 128 * 1024 => 10,  // MCU: 10 imports max
            mem if mem < 1024 * 1024 => 50, // RISC-V: 50 imports max
            _ => 100, // MPU: 100 imports max
        };

        if import_count > max_imports {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                format!(
                    "Module has {} imports, exceeds limit of {}",
                    import_count,
                    max_imports
                )
            ));
        }

        Ok(())
    }

    /// Check if an import module is allowed
    fn is_allowed_import_module(&self, module_name: &str) -> bool {
        match module_name {
            // Standard WASI modules
            "wasi_snapshot_preview1" => true,
            "wasi_unstable" => true,
            
            // Wasmbed host function modules
            "wasmbed_device" => true,
            "wasmbed_sensors" => true,
            "wasmbed_security" => true,
            "wasmbed_gpio" => true,
            "wasmbed_i2c_spi" => true,
            
            // Standard WebAssembly modules
            "env" => true,
            
            // Disallow everything else for security
            _ => false,
        }
    }
}
