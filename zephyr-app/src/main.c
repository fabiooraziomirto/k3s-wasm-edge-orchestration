/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * Wasmbed Zephyr Application
 * Main entry point for Zephyr RTOS with WAMR integration
 * Based on working dhcpv4_client sample structure
 */

#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>
#include <zephyr/net/net_if.h>
#include <zephyr/net/net_core.h>
#include <zephyr/net/net_mgmt.h>

#include "network_handler.h"
#include "wamr_integration.h"
#include "wasmbed_protocol.h"

LOG_MODULE_REGISTER(wasmbed_main, LOG_LEVEL_INF);

/* Main application thread */
void main(void)
{
    LOG_INF("=== Wasmbed Zephyr Application Starting ===");
    LOG_INF("Zephyr RTOS + WAMR Runtime");

    /* Initialize network stack */
    LOG_INF("Initializing network stack...");
    if (network_init() != 0) {
        LOG_ERR("Failed to initialize network stack - continuing without network");
        /* Continue execution - network might not be available in all configurations */
    } else {
        LOG_INF("Network stack initialized");
    }

    /* Initialize WAMR runtime */
    LOG_INF("Initializing WAMR runtime...");
    if (wamr_init() != 0) {
        LOG_ERR("Failed to initialize WAMR runtime");
        return;
    }
    LOG_INF("WAMR runtime initialized");

    /* Initialize Wasmbed protocol handler */
    LOG_INF("Initializing Wasmbed protocol...");
    if (wasmbed_protocol_init() != 0) {
        LOG_ERR("Failed to initialize Wasmbed protocol");
        return;
    }
    LOG_INF("Wasmbed protocol initialized");

    LOG_INF("=== Wasmbed Application Ready ===");
    LOG_INF("Waiting for WASM modules to deploy...");

    /* Main loop: handle WASM execution and network communication */
    while (1) {
        /* Process network events */
        network_process();

        /* Check for incoming messages from gateway using framed receive.
         * timeout_ms=100 matches k_sleep below; avoids partial-frame drops. */
        wasmbed_protocol_recv_and_handle(100);

        /* Send periodic heartbeat to keep TLS connection alive (Gateway expects it for last_heartbeat) */
        wasmbed_protocol_tick();

        /* Process WASM execution */
        wamr_process();

        /* Yield to other threads */
        k_sleep(K_MSEC(100));
    }
}
