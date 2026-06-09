// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use thiserror::Error;

/// Errors that can occur in the WASM runtime
#[derive(Error, Debug)]
pub enum WasmRuntimeError {
    #[error("WASM compilation error: {0}")]
    CompilationError(String),

    #[error("WASM instantiation error: {0}")]
    InstantiationError(String),

    #[error("WASM execution error: {0}")]
    ExecutionError(String),

    #[error("Memory limit exceeded: {current} bytes (limit: {limit} bytes)")]
    MemoryLimitExceeded { current: usize, limit: usize },

    #[error("CPU time limit exceeded: {elapsed:?} (limit: {limit:?})")]
    CpuTimeLimitExceeded { elapsed: std::time::Duration, limit: std::time::Duration },

    #[error("Stack overflow: {current} bytes (limit: {limit} bytes)")]
    StackOverflow { current: usize, limit: usize },

    #[error("Instance limit exceeded: {current} instances (limit: {limit} instances)")]
    InstanceLimitExceeded { current: usize, limit: usize },

    #[error("Function limit exceeded: {current} functions (limit: {limit} functions)")]
    FunctionLimitExceeded { current: usize, limit: usize },

    #[error("Module validation failed: {0}")]
    ModuleValidationFailed(String),

    #[error("Host function error: {0}")]
    HostFunctionError(String),

    #[error("Device communication error: {0}")]
    DeviceError(String),

    #[error("Sensor access error: {0}")]
    SensorError(String),

    #[error("Security error: {0}")]
    SecurityError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("WASM time error: {0}")]
    WasmtimeError(wasmtime::Error),

    #[error("Anyhow error: {0}")]
    AnyhowError(#[from] anyhow::Error),

    #[error("WASM parser error: {0}")]
    WasmParserError(#[from] wasmparser::BinaryReaderError),

    #[error("Application not found: {0}")]
    ApplicationNotFound(String),
}

/// Result type for WASM runtime operations
pub type WasmResult<T> = Result<T, WasmRuntimeError>;
