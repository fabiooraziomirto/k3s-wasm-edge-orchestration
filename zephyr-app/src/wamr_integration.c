/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * WAMR Integration Layer Implementation
 */

#include "wamr_integration.h"
#include <zephyr/logging/log.h>
#include "wasm_export.h"
#include <string.h>

LOG_MODULE_REGISTER(wamr_integration, LOG_LEVEL_INF);

/* WAMR runtime state */
static bool wamr_initialized = false;

/* WAMR heap buffer: pool passato a wasm_runtime_full_init (Alloc_With_Pool).
 * Con WAMR_BUILD_LIBC_WASI=1, WASI aggiunge ~10-15 KB di overhead interno
 * (fd table, environ, args) rispetto al baseline senza WASI.
 * 128 KB: linear memory (1 page=64KB) + exec stack (16KB) + instance overhead (~10KB).
 * Sul target STM32F746 non c'e' headroom sufficiente per aumentarlo ulteriormente. */
#define WAMR_HEAP_SIZE (128 * 1024)
static uint8_t wamr_heap_buffer[WAMR_HEAP_SIZE] __aligned(8);

/* Module registry */
#define MAX_MODULES 16
#define MAX_INSTANCES 16

typedef struct {
    uint32_t id;
    wasm_module_t module;
    bool in_use;
} module_entry_t;

typedef struct {
    uint32_t id;
    wasm_module_inst_t instance;
    wasm_exec_env_t exec_env;
    bool in_use;
} instance_entry_t;

static module_entry_t modules[MAX_MODULES];
static instance_entry_t instances[MAX_INSTANCES];
static uint32_t next_module_id = 1;
static uint32_t next_instance_id = 1;

/* Initialize WAMR runtime */
int wamr_init(void)
{
    if (wamr_initialized) {
        LOG_WRN("WAMR already initialized");
        return 0;
    }

    LOG_INF("Initializing WAMR runtime...");
    
    RuntimeInitArgs init_args;
    memset(&init_args, 0, sizeof(RuntimeInitArgs));
    
    /* Use pool memory allocator with static buffer */
    init_args.mem_alloc_type = Alloc_With_Pool;
    init_args.mem_alloc_option.pool.heap_buf = wamr_heap_buffer;
    init_args.mem_alloc_option.pool.heap_size = WAMR_HEAP_SIZE;
    
    /* No native symbols for now */
    init_args.native_symbols = NULL;
    init_args.n_native_symbols = 0;
    
    if (!wasm_runtime_full_init(&init_args)) {
        LOG_ERR("Failed to initialize WAMR runtime");
        return -1;
    }

    /* Initialize module and instance registries */
    memset(modules, 0, sizeof(modules));
    memset(instances, 0, sizeof(instances));

    wamr_initialized = true;
    LOG_INF("WAMR runtime initialized");
    
    return 0;
}

/* Load WASM module from bytes */
int wamr_load_module(const uint8_t *wasm_bytes, uint32_t wasm_size, uint32_t *module_id)
{
    if (!wamr_initialized) {
        LOG_ERR("WAMR not initialized");
        return -1;
    }

    if (wasm_bytes == NULL || wasm_size == 0 || module_id == NULL) {
        LOG_ERR("Invalid parameters");
        return -1;
    }

    LOG_INF("Loading WASM module (size: %u bytes)...", wasm_size);

    /* Find free module slot */
    int slot = -1;
    for (int i = 0; i < MAX_MODULES; i++) {
        if (!modules[i].in_use) {
            slot = i;
            break;
        }
    }
    
    if (slot < 0) {
        LOG_ERR("No free module slots");
        return -1;
    }

    /* Error buffer for WAMR */
    static char error_buf[128];
    error_buf[0] = '\0';

    /* Load WASM module */
    wasm_module_t module = wasm_runtime_load((uint8_t *)wasm_bytes, wasm_size, 
                                            error_buf, sizeof(error_buf));
    
    if (module == NULL) {
        LOG_ERR("Failed to load WASM module: %s", error_buf);
        return -1;
    }

    /* Store module in registry */
    modules[slot].id = next_module_id++;
    modules[slot].module = module;
    modules[slot].in_use = true;

    *module_id = modules[slot].id;
    LOG_INF("WASM module loaded (module_id: %u)", *module_id);

    return 0;
}

/* Instantiate WASM module */
int wamr_instantiate(uint32_t module_id, uint32_t *instance_id)
{
    if (!wamr_initialized) {
        LOG_ERR("WAMR not initialized");
        return -1;
    }

    LOG_INF("Instantiating WASM module (module_id: %u)...", module_id);

    /* Find module */
    wasm_module_t module = NULL;
    int module_slot = -1;
    for (int i = 0; i < MAX_MODULES; i++) {
        if (modules[i].in_use && modules[i].id == module_id) {
            module = modules[i].module;
            module_slot = i;
            break;
        }
    }
    
    if (module == NULL) {
        LOG_ERR("Module not found: %u", module_id);
        return -1;
    }

    /* Find free instance slot */
    int slot = -1;
    for (int i = 0; i < MAX_INSTANCES; i++) {
        if (!instances[i].in_use) {
            slot = i;
            break;
        }
    }
    
    if (slot < 0) {
        LOG_ERR("No free instance slots");
        return -1;
    }

    uint32_t stack_size = 16 * 1024;
    uint32_t heap_size = 0;

    /* Error buffer for WAMR */
    static char error_buf[128];
    error_buf[0] = '\0';

#if WASM_ENABLE_LIBC_WASI != 0
    /* Set minimal WASI args: no dirs, no env, argv = {"app"}.
     * Required so WAMR can resolve proc_exit / fd_write / environ_sizes_get.
     * Without this call the module may trap on first WASI import call. */
    /* No dir preopens: Zephyr's FS layer rejects "/" (no real mount),
     * and WASI modules that only use proc_exit/fd_write don't need them. */
    static const char *wasi_env_list[]  = {NULL};
    static const char *wasi_argv_list[] = {"app"};
    wasm_runtime_set_wasi_args(module,
                               NULL, 0,
                               NULL, 0,
                               wasi_env_list, 0,
                               (char **)wasi_argv_list, 1);
    LOG_DBG("WASI args set on module %u", module_id);
#endif

    /* Instantiate WASM module */
    wasm_module_inst_t instance = wasm_runtime_instantiate(module, stack_size,
                                                           heap_size, error_buf,
                                                           sizeof(error_buf));
    
    if (instance == NULL) {
        LOG_ERR("Failed to instantiate WASM module: %s", error_buf);
        return -1;
    }

    /* Create execution environment */
    wasm_exec_env_t exec_env = wasm_runtime_create_exec_env(instance, stack_size);
    if (exec_env == NULL) {
        LOG_ERR("Failed to create execution environment");
        wasm_runtime_deinstantiate(instance);
        return -1;
    }

    /* Store instance in registry */
    instances[slot].id = next_instance_id++;
    instances[slot].instance = instance;
    instances[slot].exec_env = exec_env;
    instances[slot].in_use = true;

    *instance_id = instances[slot].id;
    LOG_INF("WASM module instantiated (instance_id: %u)", *instance_id);

    return 0;
}

/* Execute WASM function */
int wamr_call_function(uint32_t instance_id, const char *function_name,
                       uint32_t *args, uint32_t args_count, uint32_t *results, uint32_t results_count)
{
    if (!wamr_initialized) {
        LOG_ERR("WAMR not initialized");
        return -1;
    }

    LOG_INF("Calling WASM function: %s (instance_id: %u)", function_name, instance_id);

    /* Find instance */
    wasm_module_inst_t instance = NULL;
    wasm_exec_env_t exec_env = NULL;
    for (int i = 0; i < MAX_INSTANCES; i++) {
        if (instances[i].in_use && instances[i].id == instance_id) {
            instance = instances[i].instance;
            exec_env = instances[i].exec_env;
            break;
        }
    }
    
    if (instance == NULL || exec_env == NULL) {
        LOG_ERR("Instance not found: %u", instance_id);
        return -1;
    }

    /* Find function */
    wasm_function_inst_t function = wasm_runtime_lookup_function(instance, function_name);
    if (function == NULL) {
        LOG_ERR("Function not found: %s", function_name);
        return -1;
    }

    /* Call WASM function - wasm_runtime_call_wasm takes argc and argv array */
    /* Note: Results are stored in the WASM stack and can be read using 
     * wasm_runtime_get_function_ret_value or similar APIs if needed */
    if (!wasm_runtime_call_wasm(exec_env, function, args_count, args)) {
        const char *exception = wasm_runtime_get_exception(instance);
        if (exception) {
            LOG_ERR("WASM exception: %s", exception);
        } else {
            LOG_ERR("Failed to call WASM function: %s", function_name);
        }
        return -1;
    }

    /* TODO: Read return values from WASM stack if results_count > 0
     * For now, results parameter is ignored as WAMR stores results differently
     * This can be enhanced later if needed for specific use cases */

    LOG_INF("WASM function executed: %s", function_name);

    return 0;
}

/*
 * Call the WASI _start entry point of a module instance.
 * WASI command modules export "_start" (not "main" or "run").
 * Falls back to looking for "run" for non-WASI modules.
 * Returns 0 on success (including proc_exit(0)), -1 on error.
 */
int wamr_call_wasi_start(uint32_t instance_id)
{
    if (!wamr_initialized) {
        LOG_ERR("WAMR not initialized");
        return -1;
    }

    wasm_module_inst_t instance = NULL;
    wasm_exec_env_t exec_env = NULL;
    for (int i = 0; i < MAX_INSTANCES; i++) {
        if (instances[i].in_use && instances[i].id == instance_id) {
            instance = instances[i].instance;
            exec_env = instances[i].exec_env;
            break;
        }
    }

    if (instance == NULL || exec_env == NULL) {
        LOG_ERR("Instance not found: %u", instance_id);
        return -1;
    }

#if WASM_ENABLE_LIBC_WASI != 0
    /* Prefer WASI-aware start: handles proc_exit correctly */
    wasm_function_inst_t start = wasm_runtime_lookup_wasi_start_function(instance);
    if (start != NULL) {
        LOG_INF("Calling WASI _start (instance %u)", instance_id);
        if (!wasm_runtime_call_wasm(exec_env, start, 0, NULL)) {
            /* proc_exit() always surfaces as "wasi proc exit" exception in WAMR.
             * Distinguish success (exit_code == 0) from failure via the WASI API. */
            const char *ex = wasm_runtime_get_exception(instance);
            if (ex != NULL && strstr(ex, "wasi proc exit") != NULL) {
                uint32_t exit_code = wasm_runtime_get_wasi_exit_code(instance);
                wasm_runtime_clear_exception(instance);
                if (exit_code == 0) {
                    LOG_INF("WASI module exited cleanly (proc_exit 0)");
                    return 0;
                }
                LOG_ERR("WASI module exited with error (proc_exit %u)", exit_code);
                return -1;
            }
            LOG_ERR("WASI _start failed: %s", ex ? ex : "(unknown)");
            return -1;
        }
        return 0;
    }
#endif

    /* Fallback: non-WASI module exporting "run" */
    wasm_function_inst_t run_fn = wasm_runtime_lookup_function(instance, "run");
    if (run_fn != NULL) {
        LOG_INF("Calling 'run' (instance %u)", instance_id);
        if (!wasm_runtime_call_wasm(exec_env, run_fn, 0, NULL)) {
            LOG_ERR("'run' failed: %s", wasm_runtime_get_exception(instance));
            return -1;
        }
        return 0;
    }

    LOG_ERR("No entry point found (_start / run) in instance %u", instance_id);
    return -1;
}

/* Process WAMR runtime (call periodically) */
void wamr_process(void)
{
    if (!wamr_initialized) {
        return;
    }

    /* TODO: Process WAMR runtime events if needed */
}

/* Unload all running instances and loaded modules WITHOUT destroying the runtime.
 * Call this before deploying a new application so WAMR slots are recycled. */
void wamr_unload_all(void)
{
    if (!wamr_initialized) {
        return;
    }

    for (int i = 0; i < MAX_INSTANCES; i++) {
        if (instances[i].in_use) {
            if (instances[i].exec_env) {
                wasm_runtime_destroy_exec_env(instances[i].exec_env);
                instances[i].exec_env = NULL;
            }
            if (instances[i].instance) {
                wasm_runtime_deinstantiate(instances[i].instance);
                instances[i].instance = NULL;
            }
            instances[i].in_use = false;
        }
    }

    for (int i = 0; i < MAX_MODULES; i++) {
        if (modules[i].in_use) {
            if (modules[i].module) {
                wasm_runtime_unload(modules[i].module);
                modules[i].module = NULL;
            }
            modules[i].in_use = false;
        }
    }

    LOG_INF("WAMR: all instances and modules unloaded");
}

/* Cleanup WAMR runtime */
void wamr_cleanup(void)
{
    if (!wamr_initialized) {
        return;
    }

    LOG_INF("Cleaning up WAMR runtime...");

    /* Cleanup all instances */
    for (int i = 0; i < MAX_INSTANCES; i++) {
        if (instances[i].in_use) {
            if (instances[i].exec_env) {
                wasm_runtime_destroy_exec_env(instances[i].exec_env);
            }
            if (instances[i].instance) {
                wasm_runtime_deinstantiate(instances[i].instance);
            }
            instances[i].in_use = false;
        }
    }

    /* Cleanup all modules */
    for (int i = 0; i < MAX_MODULES; i++) {
        if (modules[i].in_use) {
            if (modules[i].module) {
                wasm_runtime_unload(modules[i].module);
            }
            modules[i].in_use = false;
        }
    }

    /* Destroy WAMR runtime */
    wasm_runtime_destroy();

    wamr_initialized = false;
    LOG_INF("WAMR runtime cleaned up");
}

