/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * Complete WAMR integration example
 * Shows how to load and execute WASM modules
 */

#include "wamr_integration.h"
#include "network_handler.h"
#include <zephyr/logging/log.h>
#include <zephyr/kernel.h>
#include <string.h>

LOG_MODULE_REGISTER(wamr_example, LOG_LEVEL_INF);

/* Example: Load and execute a simple WASM module */
int example_load_and_run_wasm(const uint8_t *wasm_bytes, uint32_t wasm_size)
{
    int ret = 0;
    int module_id = -1;
    int instance_id = -1;

    LOG_INF("Loading WASM module (size: %u bytes)", wasm_size);

    /* Load WASM module */
    module_id = wamr_load_module(wasm_bytes, wasm_size);
    if (module_id < 0) {
        LOG_ERR("Failed to load WASM module");
        return -1;
    }

    LOG_INF("WASM module loaded (id: %d)", module_id);

    /* Instantiate module */
    instance_id = wamr_instantiate(module_id, 8192, 8192);
    if (instance_id < 0) {
        LOG_ERR("Failed to instantiate WASM module");
        wamr_cleanup();
        return -1;
    }

    LOG_INF("WASM module instantiated (id: %d)", instance_id);

    /* Call add function: add(5, 3) */
    uint32_t args_add[2] = {5, 3};
    ret = wamr_call_function(instance_id, "add", 2, args_add);
    if (ret == 0) {
        LOG_INF("add(5, 3) = %u", args_add[0]);
    } else {
        LOG_WRN("Failed to call add function");
    }

    /* Call multiply function: multiply(4, 7) */
    uint32_t args_mul[2] = {4, 7};
    ret = wamr_call_function(instance_id, "multiply", 2, args_mul);
    if (ret == 0) {
        LOG_INF("multiply(4, 7) = %u", args_mul[0]);
    } else {
        LOG_WRN("Failed to call multiply function");
    }

    /* Call fibonacci function: fibonacci(10) */
    uint32_t args_fib[1] = {10};
    ret = wamr_call_function(instance_id, "fibonacci", 1, args_fib);
    if (ret == 0) {
        LOG_INF("fibonacci(10) = %u", args_fib[0]);
    } else {
        LOG_WRN("Failed to call fibonacci function");
    }

    LOG_INF("WASM module execution completed");

    return 0;
}

/* Example: Network communication */
int example_network_communication(void)
{
    int ret;

    LOG_INF("Initializing network...");

    /* Initialize network stack */
    ret = network_init();
    if (ret != 0) {
        LOG_ERR("Failed to initialize network");
        return -1;
    }

    LOG_INF("Network initialized");

    /* Wait for network to be ready (DHCP) */
    k_sleep(K_SECONDS(5));

    /* Connect to gateway (example) */
    ret = network_connect("192.168.1.100", 8080);
    if (ret != 0) {
        LOG_WRN("Failed to connect (this is expected if server is not running)");
        return 0; /* Not a fatal error for example */
    }

    LOG_INF("Connected to gateway");

    /* Send data */
    const char *message = "Hello from Wasmbed!\n";
    ret = network_send((const uint8_t *)message, strlen(message));
    if (ret == 0) {
        LOG_INF("Data sent successfully");
    }

    return 0;
}

/* Main example function */
int wamr_example_main(void)
{
    LOG_INF("=== WAMR Example Started ===");

    /* Initialize WAMR */
    if (wamr_init() != 0) {
        LOG_ERR("Failed to initialize WAMR");
        return -1;
    }

    /* Example: Load WASM module from memory */
    /* In a real application, this would come from network or storage */
    /* For now, we'll just show the structure */
    LOG_INF("WASM module loading example (requires actual WASM bytes)");

    /* Example: Network communication */
    example_network_communication();

    LOG_INF("=== WAMR Example Completed ===");

    return 0;
}

