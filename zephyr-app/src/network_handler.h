/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * Network Handler
 * TCP/IP and TLS communication
 */

#ifndef NETWORK_HANDLER_H
#define NETWORK_HANDLER_H

#include <stdint.h>

/* Initialize network stack */
int network_init(void);

/* Process network events */
void network_process(void);

/* Connect to gateway */
int network_connect(const char *host, uint16_t port);

/* Connect to gateway with TLS */
int network_connect_tls(const char *host, uint16_t port);

/* Send data via network */
int network_send(const uint8_t *data, uint32_t data_len);

/* Receive data from network */
int network_receive(uint8_t *buffer, uint32_t buffer_len, uint32_t *received_len);

/* Poll the socket for readability; returns >0 if data available within timeout_ms, 0 on timeout, <0 on error. */
int network_poll_readable(int timeout_ms);

/* Receive a length-prefixed framed message (4-byte BE header + CBOR payload).
 * buffer receives the full frame (header + payload), buffer_len is the max size.
 * received_len is set to total bytes read (header + payload). */
int network_receive_framed(uint8_t *buffer, uint32_t buffer_len, uint32_t *received_len);

#endif /* NETWORK_HANDLER_H */

