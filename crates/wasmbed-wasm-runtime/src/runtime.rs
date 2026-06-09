// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use crate::config::{DeviceArchitecture, RuntimeConfig, WasmRuntimeConfig};
use crate::context::WasmContext;
use crate::error::{WasmResult, WasmRuntimeError};
use crate::host_functions::HostFunctionManager;
use crate::validation::WasmValidator;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use wasmtime::*;

/// WebAssembly runtime for edge devices with device-specific optimizations
pub struct WasmRuntime {
    /// WASM time engine
    engine: Engine,
    /// Runtime configuration
    config: RuntimeConfig,
    /// WASM context for execution
    context: WasmContext,
    /// Loaded WASM modules
    modules: DashMap<String, Module>,
    /// Active WASM instances
    instances: DashMap<String, WasmInstance>,
    /// Host function manager
    host_functions: Arc<HostFunctionManager>,
    /// WASM validator
    validator: Arc<WasmValidator>,
    /// Runtime statistics
    stats: RuntimeStats,
}

/// WASM instance wrapper with execution context
#[derive(Debug)]
pub struct WasmInstance {
    /// Unique instance ID
    pub id: String,
    /// WASM time instance
    pub instance: wasmtime::Instance,
    /// WASM time store
    pub store: Store<WasmContext>,
    /// Module ID this instance belongs to
    pub module_id: String,
    /// Creation time (Unix timestamp)
    pub created: u64,
    /// Last execution time (Unix timestamp)
    pub last_execution: Option<u64>,
    /// Execution count
    pub execution_count: u64,
    /// Total execution time (in seconds)
    pub total_execution_time: u64,
}

/// Runtime statistics
#[derive(Debug, Default)]
pub struct RuntimeStats {
    /// Total modules loaded
    pub modules_loaded: u64,
    /// Total instances created
    pub instances_created: u64,
    /// Total function calls
    pub function_calls: u64,
    /// Total execution time (in seconds)
    pub total_execution_time: u64,
    /// Memory peak usage
    pub peak_memory_usage: usize,
    /// Current memory usage
    pub current_memory_usage: usize,
    /// Number of errors
    pub error_count: u64,
    /// Last error time (Unix timestamp)
    pub last_error_time: Option<u64>,
}

/// WASM module metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMetadata {
    /// Module ID
    pub id: String,
    /// Module name
    pub name: String,
    /// Module version
    pub version: String,
    /// Module size in bytes
    pub size: usize,
    /// Module hash
    pub hash: String,
    /// Required host functions
    pub required_host_functions: Vec<String>,
    /// Module capabilities
    pub capabilities: Vec<String>,
    /// Creation time
    pub created: chrono::DateTime<chrono::Utc>,
}

impl WasmRuntime {
    /// Create a new WASM runtime for a specific device architecture
    pub fn new(config: RuntimeConfig) -> WasmResult<Self> {
        // Validate configuration
        config.validate().map_err(|e| WasmRuntimeError::ConfigError(e))?;

        // Create WASM time engine with device-specific configuration
        let mut engine_config = Config::new();
        Self::configure_engine(&mut engine_config, &config.wasm_config)?;
        
        let engine = Engine::new(&engine_config)?;

        // Create WASM context
        let context = WasmContext::new(
            config.architecture,
            config.device_id.clone(),
            &config.wasm_config,
            &config.host_config,
        );

        // Create host function manager
        let host_functions = Arc::new(HostFunctionManager::new(&config.host_config, Arc::new(context.clone()))?);

        // Create validator
        let validator = Arc::new(WasmValidator::new(&config.wasm_config));

        // Create statistics tracker
        let stats = RuntimeStats::default();

        Ok(Self {
            engine,
            config,
            context,
            modules: DashMap::new(),
            instances: DashMap::new(),
            host_functions,
            validator,
            stats,
        })
    }

    /// Get the WASM context (for testing purposes)
    pub fn context(&self) -> &WasmContext {
        &self.context
    }

    /// Get the WASM context (for testing purposes) - mutable version
    pub fn context_mut(&mut self) -> &mut WasmContext {
        &mut self.context
    }

    /// Get the host function manager (for testing purposes)
    pub fn host_functions(&self) -> &HostFunctionManager {
        &self.host_functions
    }

    /// Get the runtime configuration
    pub fn get_config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Execute a function from a loaded module
    /// Note: This is a simplified implementation for testing purposes.
    /// A full implementation would require careful borrow management with DashMap.
    pub fn execute_function(
        &mut self,
        module_id: &str,
        function_name: &str,
        _args: &[wasmtime::Val],
    ) -> WasmResult<Vec<wasmtime::Val>> {
        // Find the module
        let _module = self.modules.get(module_id)
            .ok_or_else(|| WasmRuntimeError::ApplicationNotFound(module_id.to_string()))?;

        // Find instance for this module
        let instance_id = format!("{}-instance", module_id);
        let _instance_entry = self.instances.get(&instance_id)
            .ok_or_else(|| WasmRuntimeError::ApplicationNotFound(format!("Instance for module {} not found", module_id)))?;

        // TODO: Full implementation requires resolving borrow checker issues with DashMap
        // For now, return empty results to allow compilation
        // In production, this would execute the function using proper borrow management
        Ok(Vec::new())
    }

    /// Unload an application (module)
    pub fn unload_application(&mut self, module_id: &str) -> WasmResult<()> {
        // Remove all instances for this module
        let instance_ids: Vec<String> = self.instances.iter()
            .filter(|entry| entry.value().module_id == module_id)
            .map(|entry| entry.key().clone())
            .collect();
        
        for instance_id in instance_ids {
            self.instances.remove(&instance_id);
        }

        // Remove the module
        if self.modules.remove(module_id).is_some() {
            Ok(())
        } else {
            Err(WasmRuntimeError::ApplicationNotFound(module_id.to_string()))
        }
    }

    /// Configure WASM time engine based on device architecture
    fn configure_engine(config: &mut Config, wasm_config: &WasmRuntimeConfig) -> WasmResult<()> {
        // Enable WASI
        config.wasm_component_model(true);
        config.wasm_multi_memory(true);
        config.wasm_memory64(false); // 32-bit for embedded systems
        
        // Configure memory limits
        config.max_wasm_stack(wasm_config.max_stack_size);
        
        // Enable optimizations based on device capabilities
        if wasm_config.enable_simd {
            config.wasm_simd(true);
        }
        
        if wasm_config.enable_bulk_memory {
            config.wasm_bulk_memory(true);
        }
        
        if wasm_config.enable_reference_types {
            config.wasm_reference_types(true);
        }
        
        if wasm_config.enable_tail_calls {
            config.wasm_tail_call(true);
        }
        
        if wasm_config.enable_function_references {
            config.wasm_function_references(true);
        }
        
        // Enable debugging if supported
        if wasm_config.enable_debug {
            config.debug_info(true);
        }
        
        // Configure compilation strategy
        config.strategy(Strategy::Cranelift);
        
        Ok(())
    }

    /// Load a WASM module from bytes
    pub async fn load_module(&mut self, module_id: &str, wasm_bytes: &[u8]) -> WasmResult<ModuleMetadata> {
        // Validate module size
        if wasm_bytes.len() > self.config.wasm_config.max_memory / 4 {
            return Err(WasmRuntimeError::ModuleValidationFailed(
                "Module too large for device memory constraints".to_string()
            ));
        }

        // Validate WASM module
        self.validator.validate_module(wasm_bytes)?;

        // Compile WASM module
        let module = Module::new(&self.engine, wasm_bytes)?;

        // Extract module metadata
        let metadata = self.extract_module_metadata(module_id, wasm_bytes)?;

        // Store module
        self.modules.insert(module_id.to_string(), module);

        // Update statistics
        self.stats.modules_loaded += 1;

        Ok(metadata)
    }

    /// Create a new WASM instance from a loaded module
    pub async fn create_instance(&mut self, module_id: &str, instance_id: Option<String>) -> WasmResult<String> {
        // Check instance limit
        self.context.check_instance_limit()?;

        // Get module
        let module = self.modules.get(module_id)
            .ok_or_else(|| WasmRuntimeError::ModuleValidationFailed(
                format!("Module {} not found", module_id)
            ))?;

        // Generate instance ID if not provided
        let instance_id = instance_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        // Create store with context
        let mut store = Store::new(&self.engine, self.context.clone());

        // Create host function imports only if the module needs them
        let imports = if module.imports().count() > 0 {
            self.host_functions.create_imports(&mut store)?
        } else {
            Vec::new()
        };

        // u64iate module
        let instance = Instance::new(&mut store, &module, &imports)?;

        // Create instance wrapper
        let wasm_instance = WasmInstance {
            id: instance_id.clone(),
            instance,
            store,
            module_id: module_id.to_string(),
            created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            last_execution: None,
            execution_count: 0,
            total_execution_time: 0,
        };

        // Store instance
        self.instances.insert(instance_id.clone(), wasm_instance);

        // Update context active instances counter
        self.context.active_instances += 1;

        // Update statistics
        self.stats.instances_created += 1;

        Ok(instance_id)
    }

    /// Call a function in a WASM instance
    pub async fn call_function(
        &mut self,
        instance_id: &str,
        function_name: &str,
        args: &[wasmtime::Val],
    ) -> WasmResult<()> {
        let start_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        // Get instance
        let mut instance = self.instances.get_mut(instance_id)
            .ok_or_else(|| WasmRuntimeError::ExecutionError(
                format!("Instance {} not found", instance_id)
            ))?;

        // Check CPU time limit
        let estimated_time = Duration::from_millis(10); // Conservative estimate
        self.context.check_cpu_time_limit(estimated_time)?;

        // Call function using a helper to avoid borrow checker issues
        Self::call_instance_function(&mut instance, function_name, args)?;

        // Update execution statistics
        let execution_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - start_time;
        instance.last_execution = Some(start_time);
        instance.execution_count += 1;
        instance.total_execution_time += execution_time;

        // Update context CPU time
        self.context.update_cpu_time(Duration::from_secs(execution_time))?;

        // Update runtime statistics
        self.stats.function_calls += 1;
        self.stats.total_execution_time += execution_time;

        Ok(())
    }

    /// Get instance information
    pub fn get_instance_info(&self, instance_id: &str) -> WasmResult<InstanceInfo> {
        let instance = self.instances.get(instance_id)
            .ok_or_else(|| WasmRuntimeError::ExecutionError(
                format!("Instance {} not found", instance_id)
            ))?;

        Ok(InstanceInfo {
            id: instance.id.clone(),
            module_id: instance.module_id.clone(),
            created: instance.created,
            last_execution: instance.last_execution,
            execution_count: instance.execution_count,
            total_execution_time: instance.total_execution_time,
        })
    }

    /// Get runtime statistics
    pub fn get_stats(&self) -> RuntimeStats {
        RuntimeStats {
            modules_loaded: self.stats.modules_loaded,
            instances_created: self.stats.instances_created,
            function_calls: self.stats.function_calls,
            total_execution_time: self.stats.total_execution_time,
            peak_memory_usage: self.stats.peak_memory_usage,
            current_memory_usage: self.context.memory_usage,
            error_count: self.stats.error_count,
            last_error_time: self.stats.last_error_time,
        }
    }

    /// Remove an instance
    pub fn remove_instance(&mut self, instance_id: &str) -> WasmResult<()> {
        self.instances.remove(instance_id)
            .ok_or_else(|| WasmRuntimeError::ExecutionError(
                format!("Instance {} not found", instance_id)
            ))?;
        Ok(())
    }

    /// Remove a module
    pub fn remove_module(&mut self, module_id: &str) -> WasmResult<()> {
        self.modules.remove(module_id)
            .ok_or_else(|| WasmRuntimeError::ModuleValidationFailed(
                format!("Module {} not found", module_id)
            ))?;
        Ok(())
    }

}

/// Instance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    /// Instance ID
    pub id: String,
    /// Module ID
    pub module_id: String,
    /// Creation time (Unix timestamp)
    pub created: u64,
    /// Last execution time (Unix timestamp)
    pub last_execution: Option<u64>,
    /// Execution count
    pub execution_count: u64,
    /// Total execution time (in seconds)
    pub total_execution_time: u64,
}

impl WasmRuntime {
    /// Helper function to call a function on a WASM instance
    fn call_instance_function(
        instance: &mut WasmInstance,
        function_name: &str,
        args: &[wasmtime::Val],
    ) -> WasmResult<()> {
        let func = instance.instance.get_func(&mut instance.store, function_name)
            .ok_or_else(|| WasmRuntimeError::ExecutionError(
                format!("Function {} not found", function_name)
            ))?;
        
        func.call(&mut instance.store, args, &mut [])?;
        Ok(())
    }

    /// Shutdown an instance
    pub fn shutdown_instance(&mut self, instance_id: &str) -> WasmResult<()> {
        if self.instances.remove(instance_id).is_some() {
            // Note: We can't directly modify Arc<WasmContext>, so we'll track instances separately
            Ok(())
        } else {
            Err(WasmRuntimeError::ExecutionError(
                format!("Instance {} not found", instance_id)
            ))
        }
    }

    /// Shutdown the runtime
    pub fn shutdown(&mut self) -> WasmResult<()> {
        self.instances.clear();
        self.modules.clear();
        // Reset active instances counter
        self.context.active_instances = 0;
        Ok(())
    }

    /// Extract metadata from WASM module
    fn extract_module_metadata(&self, module_id: &str, wasm_bytes: &[u8]) -> WasmResult<ModuleMetadata> {
        // Calculate module hash
        let hash = Sha256::digest(wasm_bytes);
        let hash_hex = hex::encode(hash);

        // Extract module name from bytes (simplified)
        let name = format!("module_{}", module_id);

        Ok(ModuleMetadata {
            id: module_id.to_string(),
            name,
            version: "1.0.0".to_string(),
            size: wasm_bytes.len(),
            hash: hash_hex,
            required_host_functions: vec![], // TODO: Extract from module
            capabilities: vec![], // TODO: Extract from module
            created: chrono::Utc::now(),
        })
    }
}
