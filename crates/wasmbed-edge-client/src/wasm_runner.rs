// SPDX-License-Identifier: AGPL-3.0
// Copyright © 2025 Wasmbed contributors

use anyhow::Result;
use std::collections::HashMap;
use wasmbed_protocol::ApplicationConfig;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime::component::ResourceTable;
use wasmtime_wasi::preview2::preview1::{self, WasiPreview1Adapter, WasiPreview1View};
use wasmtime_wasi::preview2::{WasiCtx, WasiCtxBuilder, WasiView};

/// Store data for WASM execution: holds WASI context + preview1 adapter.
struct WasiState {
    wasi: WasiCtx,
    table: ResourceTable,
    adapter: WasiPreview1Adapter,
}

impl WasiView for WasiState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl WasiPreview1View for WasiState {
    fn adapter(&self) -> &WasiPreview1Adapter {
        &self.adapter
    }
    fn adapter_mut(&mut self) -> &mut WasiPreview1Adapter {
        &mut self.adapter
    }
}

/// Manages deployed WASM applications running in background threads.
pub struct WasmRunner {
    /// app_id → abort handle for the spawned blocking task
    handles: HashMap<String, tokio::task::AbortHandle>,
}

impl WasmRunner {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
        }
    }

    /// Deploys a WASM application by loading and spawning it in a blocking task.
    /// Returns immediately after confirming the module has valid magic bytes.
    pub fn deploy(
        &mut self,
        app_id: String,
        wasm_bytes: Vec<u8>,
        config: Option<ApplicationConfig>,
    ) -> Result<()> {
        // Stop any previous instance of the same app
        self.stop(&app_id).ok();

        validate_wasm_magic(&wasm_bytes)?;

        let app_id_clone = app_id.clone();
        let handle = tokio::task::spawn_blocking(move || {
            tracing::info!("Running WASM app '{}'", app_id_clone);
            match run_wasm_module(&wasm_bytes, config.as_ref()) {
                Ok(()) => tracing::info!("WASM app '{}' finished", app_id_clone),
                Err(e) => tracing::error!("WASM app '{}' failed: {}", app_id_clone, e),
            }
        });

        self.handles.insert(app_id, handle.abort_handle());
        Ok(())
    }

    /// Aborts a running application.
    pub fn stop(&mut self, app_id: &str) -> Result<()> {
        match self.handles.remove(app_id) {
            Some(handle) => {
                handle.abort();
                tracing::info!("Aborted WASM app '{}'", app_id);
                Ok(())
            }
            None => anyhow::bail!("Application '{}' not found", app_id),
        }
    }

    /// Returns true if the app task entry still exists (the task may have finished).
    pub fn is_running(&self, app_id: &str) -> bool {
        self.handles.contains_key(app_id)
    }

    /// Returns IDs of all registered apps.
    pub fn running_app_ids(&self) -> Vec<String> {
        self.handles.keys().cloned().collect()
    }
}

/// Validates that bytes start with the WASM magic number `\0asm`.
fn validate_wasm_magic(bytes: &[u8]) -> Result<()> {
    let magic = bytes.get(0..4);
    if magic != Some(&[0x00, 0x61, 0x73, 0x6d]) {
        anyhow::bail!("Not a valid WASM module (missing magic bytes)");
    }
    Ok(())
}

/// Runs a WASM module synchronously using wasmtime with WASI preview1 compatibility.
/// Looks for `_start` (WASI command) or `main` as entry points.
fn run_wasm_module(wasm_bytes: &[u8], config: Option<&ApplicationConfig>) -> Result<()> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm_bytes)?;

    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();

    if let Some(cfg) = config {
        for (k, v) in &cfg.env_vars {
            builder.env(k, v);
        }
        let arg_refs: Vec<&str> = cfg.args.iter().map(String::as_str).collect();
        builder.args(&arg_refs);
    }

    let state = WasiState {
        wasi: builder.build(),
        table: ResourceTable::new(),
        adapter: WasiPreview1Adapter::new(),
    };

    let mut store = Store::new(&engine, state);
    let mut linker: Linker<WasiState> = Linker::new(&engine);
    preview1::add_to_linker_sync(&mut linker)?;

    let instance = linker.instantiate(&mut store, &module)?;

    // Try _start (WASI command pattern) first, then fall back to main
    if let Some(func) = instance.get_func(&mut store, "_start") {
        func.typed::<(), ()>(&store)?
            .call(&mut store, ())?;
        return Ok(());
    }

    if let Some(func) = instance.get_func(&mut store, "main") {
        func.typed::<(), ()>(&store)?
            .call(&mut store, ())?;
        return Ok(());
    }

    anyhow::bail!("WASM module exports neither '_start' nor 'main'")
}
