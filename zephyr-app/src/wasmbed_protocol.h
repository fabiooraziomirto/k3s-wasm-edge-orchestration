/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * Wasmbed Protocol Handler
 * Handles communication with Wasmbed gateway
 */

#ifndef WASMBED_PROTOCOL_H
#define WASMBED_PROTOCOL_H

#include <stdint.h>

/* Initialize Wasmbed protocol handler */
int wasmbed_protocol_init(void);

/* Handle incoming message from gateway */
int wasmbed_protocol_handle_message(const uint8_t *data, uint32_t data_len);

/* Send message to gateway */
int wasmbed_protocol_send_message(const uint8_t *data, uint32_t data_len);

/* Send periodic heartbeat (ClientMessage::Heartbeat = CBOR [0]) to keep TLS connection alive */
int wasmbed_protocol_send_heartbeat(void);

/* Call periodically from main loop; sends heartbeat every HEARTBEAT_INTERVAL_SEC */
void wasmbed_protocol_tick(void);

/* Receive one complete framed message from gateway and dispatch it.
 * Blocks at most timeout_ms. Returns 0 if a message was processed,
 * 1 if no data arrived (timeout/EAGAIN), -1 on error/disconnect. */
int wasmbed_protocol_recv_and_handle(int timeout_ms);

#endif /* WASMBED_PROTOCOL_H */

