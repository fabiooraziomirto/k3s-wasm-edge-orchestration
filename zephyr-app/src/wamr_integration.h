/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * WAMR Integration Layer
 * Interface for WebAssembly Micro Runtime
 */

#ifndef WAMR_INTEGRATION_H
#define WAMR_INTEGRATION_H

#include <stdint.h>
#include <stdbool.h>

/* Initialize WAMR runtime */
int wamr_init(void);

/* Load WASM module from bytes */
int wamr_load_module(const uint8_t *wasm_bytes, uint32_t wasm_size, uint32_t *module_id);

/* Instantiate WASM module */
int wamr_instantiate(uint32_t module_id, uint32_t *instance_id);

/* Execute WASM function by name */
int wamr_call_function(uint32_t instance_id, const char *function_name,
                       uint32_t *args, uint32_t args_count, uint32_t *results, uint32_t results_count);

/* Call WASI _start entry point (falls back to "run" for non-WASI modules) */
int wamr_call_wasi_start(uint32_t instance_id);

/* Process WAMR runtime (call periodically) */
void wamr_process(void);

/* Unload all instances and modules, keeping the runtime alive (for re-use) */
void wamr_unload_all(void);

/* Cleanup WAMR runtime */
void wamr_cleanup(void);

#endif /* WAMR_INTEGRATION_H */

