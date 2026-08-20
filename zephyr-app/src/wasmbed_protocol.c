/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * Wasmbed Protocol Handler Implementation
 */

#include "wasmbed_protocol.h"
#include "wamr_integration.h"
#include "network_handler.h"
#include <zephyr/logging/log.h>
#include <string.h>
#include <zephyr/kernel.h>
#include <stdlib.h>
#include <mbedtls/sha256.h>
#include <mbedtls/pk.h>
#include <mbedtls/x509_crt.h>
#include <mbedtls/entropy.h>
#include <mbedtls/ctr_drbg.h>

LOG_MODULE_REGISTER(wasmbed_protocol, LOG_LEVEL_INF);

/* Memory address where Renode writes the gateway endpoint */
#define GATEWAY_ENDPOINT_ADDR 0x20001000
/* Memory address where Renode writes the device public key (4-byte LE length + key bytes) */
#define DEVICE_KEY_ADDR       0x20002000
/* This device's TLS client certificate (DER).
 *
 * 0x20006000 and not 0x20003000: on stm32f746g_disco the injected blobs live in
 * DTCM (0x20000000, 64KB), whose allocated part ends at __DTCM_start =
 * 0x20003100. A certificate placed at 0x20003000 had its first 12 bytes survive
 * and everything after overwritten by firmware data, which mbedTLS reported only
 * as MBEDTLS_ERR_X509_INVALID_SERIAL and Zephyr collapsed into EINVAL from
 * connect(). Anything injected here must sit above that boundary. */
#define DEVICE_CERT_ADDR      0x20006000
/* This device's PKCS#8 private key (DER) */
#define DEVICE_PRIVKEY_ADDR   0x20004000
/* The fleet CA certificate the gateway is verified against (DER) */
#define DEVICE_CA_ADDR        0x20005000

/* Domain separation tag for the proof-of-possession transcript; must match
 * POP_CONTEXT in crates/wasmbed-gateway/src/main.rs. */
#define POP_CONTEXT           "wasmbed-pop-v1"
#define POP_CONTEXT_LEN       (sizeof(POP_CONTEXT) - 1)

/* Heartbeat interval in milliseconds (keep below Gateway timeout, e.g. 90s) */
#define HEARTBEAT_INTERVAL_MS 25000U

static bool protocol_initialized = false;
static char gateway_endpoint[64] = {0};
/* Device public key, copied out of DEVICE_KEY_ADDR during init. It cannot be
 * read lazily at enrollment time: that RAM is reused by the firmware once it
 * starts running, so by then it holds garbage and enrollment silently fell back
 * to the shared static test key -- which made every emulated device present the
 * same identity to the gateway. */
/* A P-256 SubjectPublicKeyInfo is 91 bytes; 128 leaves room to spare. */
static uint8_t device_public_key[128] = {0};
static uint32_t device_public_key_len = 0U;
/* TLS client certificate, PKCS#8 private key and fleet CA, all DER, all read
 * out of injected memory during init for the same reason as the public key. */
/* Sized to what the fleet actually issues, because RAM here is scarce: a P-256
 * client certificate is ~550 bytes, its PKCS#8 key 138, and an RSA-2048 CA
 * certificate ~800. These buffers must outlive registration: Zephyr's
 * tls_credential_add stores pointers, it does not copy. */
static uint8_t device_client_cert[768] = {0};
static uint32_t device_client_cert_len = 0U;
static uint8_t device_private_key[192] = {0};
static uint32_t device_private_key_len = 0U;
static uint8_t fleet_ca_cert[1024] = {0};
static uint32_t fleet_ca_cert_len = 0U;

static void read_device_public_key(void);
static void read_device_credentials(void);
static int sign_challenge(const uint8_t *nonce, uint32_t nonce_len,
                          uint8_t *sig, size_t sig_size, size_t *sig_len);
static bool gateway_connected = false;
static uint32_t last_heartbeat_uptime_ms = 0U;

/* ClientMessage::Heartbeat = array(1), u32(0) => CBOR 0x81 0x00; wire = 4-byte len + CBOR */
static const uint8_t heartbeat_packet[] = {
    0x00, 0x00, 0x00, 0x02, 0x81, 0x00
};
#define HEARTBEAT_PACKET_LEN sizeof(heartbeat_packet)

/* ClientMessage::EnrollmentRequest = array(1), u32(1) => CBOR 0x81 0x01 */
static const uint8_t enrollment_request_pkt[] = {
    0x00, 0x00, 0x00, 0x02, 0x81, 0x01
};

/* ClientMessage::EnrollmentAcknowledgment = array(1), u32(3) => CBOR 0x81 0x03 */
static const uint8_t enrollment_ack_pkt[] = {
    0x00, 0x00, 0x00, 0x02, 0x81, 0x03
};

/*
 * Receive a complete length-prefixed frame from the network.
 * Accumulates data from multiple network_receive calls until the full
 * frame (4-byte BE header + payload) is available.
 * Returns 0 on success, -1 on error/timeout.
 */
static int recv_frame(uint8_t *buf, uint32_t buf_len, uint32_t *total_len,
                      int timeout_ms)
{
    uint32_t got = 0;
    int64_t deadline = k_uptime_get() + timeout_ms;

    /* Accumulate until we have at least the 4-byte header */
    while (got < 4) {
        uint32_t chunk = 0;
        int r = network_receive(buf + got, buf_len - got, &chunk);
        if (r < 0) {
            return -1;
        }
        got += chunk;
        if (got < 4) {
            if (k_uptime_get() >= deadline) {
                LOG_ERR("recv_frame: header timeout (got %u)", got);
                return -1;
            }
            k_sleep(K_MSEC(50));
        }
    }

    /* Parse payload length from the 4-byte BE header */
    uint32_t payload_len = ((uint32_t)buf[0] << 24) |
                           ((uint32_t)buf[1] << 16) |
                           ((uint32_t)buf[2] <<  8) |
                           ((uint32_t)buf[3]);
    uint32_t frame_len = 4 + payload_len;
    if (frame_len > buf_len) {
        LOG_ERR("recv_frame: frame too large (%u)", frame_len);
        return -1;
    }

    /* Accumulate remaining payload bytes */
    while (got < frame_len) {
        uint32_t chunk = 0;
        int r = network_receive(buf + got, buf_len - got, &chunk);
        if (r < 0) {
            return -1;
        }
        got += chunk;
        if (got < frame_len) {
            if (k_uptime_get() >= deadline) {
                LOG_ERR("recv_frame: payload timeout (got %u/%u)", got, frame_len);
                return -1;
            }
            k_sleep(K_MSEC(50));
        }
    }

    *total_len = frame_len;
    return 0;
}

/*
 * Perform enrollment handshake with the gateway.
 * Flow:
 *   C→S  EnrollmentRequest  (0x81 0x01)
 *   S→C  EnrollmentAccepted (0x81 0x01)
 *   C→S  PublicKey { key }  (0x82 0x02 0x58 <len> <key bytes>)
 *   S→C  Challenge { nonce } (0x82 0x09 0x58 <len> <nonce>)
 *   C→S  ChallengeResponse   (0x82 0x08 0x58 <len> <DER signature>)
 *   S→C  DeviceUuid         (0x82 0x03 0x50 <16 bytes>)
 *   C→S  EnrollmentAcknowledgment (0x81 0x03)
 *   S→C  EnrollmentCompleted (0x81 0x04)
 */
static int do_enrollment(void)
{
    uint8_t rx_buf[64];
    uint32_t rx_len;
    int ret;

    LOG_INF("Starting enrollment with gateway...");

    /* Step 1: Send EnrollmentRequest */
    ret = network_send(enrollment_request_pkt, sizeof(enrollment_request_pkt));
    if (ret < 0) {
        LOG_ERR("Failed to send EnrollmentRequest");
        return -1;
    }
    LOG_INF("Sent EnrollmentRequest");

    /* Step 2: Receive EnrollmentAccepted (wire: 00 00 00 02 81 01) */
    rx_len = 0;
    ret = recv_frame(rx_buf, sizeof(rx_buf), &rx_len, 5000);
    if (ret < 0 || rx_len < 6) {
        LOG_ERR("No enrollment response: ret=%d len=%u", ret, rx_len);
        return -1;
    }
    if (rx_buf[4] != 0x81 || rx_buf[5] != 0x01) {
        LOG_ERR("Enrollment rejected: 0x%02x 0x%02x", rx_buf[4], rx_buf[5]);
        return -1;
    }
    LOG_INF("Enrollment accepted by gateway");

    /* Step 3: Use the device public key cached during init. */
    if (device_public_key_len == 0U) {
        read_device_public_key();
    }
    const uint8_t *pub_key = device_public_key;
    uint32_t key_len = device_public_key_len;

    /* Step 4: Build and send PublicKey message.
     * CBOR: array(2) uint(2) bytes(n) = 82 02 58 <n> <key>, where the key is a
     * SubjectPublicKeyInfo (91 bytes for P-256), so the frame is 4 + 4 + n.
     */
    if (key_len == 0U || key_len > 255U) {
        LOG_ERR("No usable device public key (%u bytes); cannot enroll", key_len);
        return -1;
    }

    static uint8_t pub_key_pkt[8 + sizeof(device_public_key)];
    uint32_t cbor_len = 4 + key_len; /* 82 02 58 <klen> + key bytes */
    uint32_t pkt_len = 4 + cbor_len;
    pub_key_pkt[0] = 0x00;
    pub_key_pkt[1] = 0x00;
    pub_key_pkt[2] = (uint8_t)((cbor_len >> 8) & 0xFF);
    pub_key_pkt[3] = (uint8_t)(cbor_len & 0xFF);
    pub_key_pkt[4] = 0x82;            /* array(2) */
    pub_key_pkt[5] = 0x02;            /* uint(2) = ClientMessage::PublicKey tag */
    pub_key_pkt[6] = 0x58;            /* bytes with following 1-byte length */
    pub_key_pkt[7] = (uint8_t)key_len;
    memcpy(&pub_key_pkt[8], pub_key, key_len);

    ret = network_send(pub_key_pkt, pkt_len);
    if (ret < 0) {
        LOG_ERR("Failed to send PublicKey");
        return -1;
    }
    LOG_INF("Sent PublicKey (%u bytes)", key_len);

    /* Step 5: The gateway answers with a challenge we must sign before it will
     * accept the key we just announced. A gateway that predates the challenge
     * exchange replies with DeviceUuid straight away. */
    rx_len = 0;
    ret = recv_frame(rx_buf, sizeof(rx_buf), &rx_len, 5000);
    if (ret < 0 || rx_len < 6) {
        LOG_ERR("No response to PublicKey: ret=%d len=%u", ret, rx_len);
        return -1;
    }

    if (rx_buf[4] == 0x82 && rx_buf[5] == 0x09) {
        /* Challenge: 82 09 58 <nonce_len> <nonce> */
        if (rx_len < 8 || rx_buf[6] != 0x58) {
            LOG_ERR("Malformed Challenge (len=%u)", rx_len);
            return -1;
        }
        uint32_t nonce_len = rx_buf[7];
        if (nonce_len == 0U || rx_len < 8 + nonce_len) {
            LOG_ERR("Truncated Challenge: nonce_len=%u len=%u", nonce_len, rx_len);
            return -1;
        }
        LOG_INF("Received %u-byte challenge from gateway", nonce_len);

        uint8_t sig[80];  /* DER ECDSA P-256 signature is at most 72 bytes */
        size_t sig_len = 0;
        if (sign_challenge(&rx_buf[8], nonce_len, sig, sizeof(sig), &sig_len) != 0) {
            LOG_ERR("Could not prove possession of the device key");
            return -1;
        }
        if (sig_len > 255U) {
            LOG_ERR("Signature too long to encode: %u", (unsigned)sig_len);
            return -1;
        }

        uint8_t resp_pkt[8 + sizeof(sig)];
        uint32_t resp_cbor_len = 4 + (uint32_t)sig_len;
        resp_pkt[0] = 0x00;
        resp_pkt[1] = 0x00;
        resp_pkt[2] = (uint8_t)((resp_cbor_len >> 8) & 0xFF);
        resp_pkt[3] = (uint8_t)(resp_cbor_len & 0xFF);
        resp_pkt[4] = 0x82;  /* array(2) */
        resp_pkt[5] = 0x08;  /* uint(8) = ClientMessage::ChallengeResponse tag */
        resp_pkt[6] = 0x58;  /* bytes with following 1-byte length */
        resp_pkt[7] = (uint8_t)sig_len;
        memcpy(&resp_pkt[8], sig, sig_len);

        ret = network_send(resp_pkt, 4 + resp_cbor_len);
        if (ret < 0) {
            LOG_ERR("Failed to send ChallengeResponse");
            return -1;
        }
        LOG_INF("Sent ChallengeResponse (%u byte signature)", (unsigned)sig_len);

        rx_len = 0;
        ret = recv_frame(rx_buf, sizeof(rx_buf), &rx_len, 5000);
        if (ret < 0 || rx_len < 6) {
            LOG_ERR("No DeviceUuid after the challenge: ret=%d len=%u", ret, rx_len);
            return -1;
        }
    } else {
        LOG_WRN("Gateway issued no challenge; this device's identity is unproven");
    }

    /* Step 6: DeviceUuid (wire: 00 00 00 13  82 03 50 <16 bytes>) */
    if (rx_buf[4] != 0x82 || rx_buf[5] != 0x03) {
        LOG_ERR("Enrollment rejected: 0x%02x 0x%02x", rx_buf[4], rx_buf[5]);
        return -1;
    }
    LOG_INF("Received DeviceUuid from gateway");

    /* Step 7: Send EnrollmentAcknowledgment */
    ret = network_send(enrollment_ack_pkt, sizeof(enrollment_ack_pkt));
    if (ret < 0) {
        LOG_ERR("Failed to send EnrollmentAcknowledgment");
        return -1;
    }
    LOG_INF("Sent EnrollmentAcknowledgment");

    /* Step 8: Receive EnrollmentCompleted (wire: 00 00 00 02  81 04) — optional */
    rx_len = 0;
    recv_frame(rx_buf, sizeof(rx_buf), &rx_len, 3000);
    if (rx_len >= 6 && rx_buf[4] == 0x81 && rx_buf[5] == 0x04) {
        LOG_INF("Enrollment completed successfully!");
    } else {
        LOG_WRN("EnrollmentCompleted not received (len=%u) - continuing anyway", rx_len);
    }

    return 0;
}

/* Read gateway endpoint from memory (written by Renode) */
static int read_gateway_endpoint(void)
{
    /* Read length from first 4 bytes */
    uint32_t *length_ptr = (uint32_t *)GATEWAY_ENDPOINT_ADDR;
    uint32_t length = *length_ptr;
    
    if (length == 0 || length >= sizeof(gateway_endpoint)) {
        LOG_ERR("Invalid endpoint length: %u", length);
        return -1;
    }
    
    /* Read endpoint string from memory */
    char *endpoint_ptr = (char *)(GATEWAY_ENDPOINT_ADDR + 4);
    memcpy(gateway_endpoint, endpoint_ptr, length);
    gateway_endpoint[length] = '\0';
    
    LOG_INF("Read gateway endpoint from memory: %s (length: %u)", gateway_endpoint, length);
    return 0;
}

/* Copy a length-prefixed blob injected by Renode (4-byte LE length, then the
 * bytes) into a static buffer. Must run during init, while that memory still
 * holds what Renode wrote: the firmware reuses that RAM once it is running.
 * Returns 0 when a blob was read, -1 when there is none. */
static int read_injected_blob(uint32_t addr, uint8_t *out, uint32_t out_size,
                              uint32_t *out_len, const char *what)
{
    volatile uint32_t *len_ptr = (volatile uint32_t *)addr;
    uint32_t len = *len_ptr;

    if (len == 0 || len > out_size) {
        LOG_WRN("No %s injected at 0x%08x (len=%u)", what, addr, len);
        *out_len = 0U;
        return -1;
    }

    volatile uint8_t *data_ptr = (volatile uint8_t *)(addr + 4);
    for (uint32_t i = 0; i < len; i++) {
        out[i] = data_ptr[i];
    }
    *out_len = len;
    LOG_INF("Read %s from memory (%u bytes)", what, len);
    return 0;
}

/* Read the device public key written by Renode at DEVICE_KEY_ADDR.
 *
 * There is deliberately no fallback key. The previous static 0xAB key gave
 * every board in the fleet the same identity, and no private key backs it, so
 * a device using it could never answer the gateway's challenge anyway. */
static void read_device_public_key(void)
{
    if (read_injected_blob(DEVICE_KEY_ADDR, device_public_key,
                           sizeof(device_public_key), &device_public_key_len,
                           "device public key") != 0) {
        LOG_ERR("This device has no identity and will not be able to enroll");
    }
}

/* Read the TLS credentials injected for this device. */
static void read_device_credentials(void)
{
    read_injected_blob(DEVICE_CERT_ADDR, device_client_cert,
                       sizeof(device_client_cert), &device_client_cert_len,
                       "client certificate");
    read_injected_blob(DEVICE_PRIVKEY_ADDR, device_private_key,
                       sizeof(device_private_key), &device_private_key_len,
                       "private key");
    read_injected_blob(DEVICE_CA_ADDR, fleet_ca_cert,
                       sizeof(fleet_ca_cert), &fleet_ca_cert_len,
                       "fleet CA certificate");
}

/* Parse each injected credential the way Zephyr's TLS socket will, and report
 * which one fails and why.
 *
 * Without this a bad credential surfaces only as EINVAL from zsock_connect,
 * because sockets_tls.c collapses every mbedTLS parse error into that. */
static void check_credentials(void)
{
    static mbedtls_x509_crt crt;
    static mbedtls_pk_context pk;
    int ret;

    if (fleet_ca_cert_len > 0U) {
        mbedtls_x509_crt_init(&crt);
        ret = mbedtls_x509_crt_parse_der(&crt, fleet_ca_cert, fleet_ca_cert_len);
        LOG_INF("Credential check: CA certificate parse -> %d (-0x%04x)", ret, (unsigned)-ret);
        mbedtls_x509_crt_free(&crt);
    }

    if (device_client_cert_len > 0U) {
        LOG_HEXDUMP_INF(device_client_cert, 24, "client cert head as read");
        mbedtls_x509_crt_init(&crt);
        ret = mbedtls_x509_crt_parse_der(&crt, device_client_cert, device_client_cert_len);
        LOG_INF("Credential check: client certificate parse -> %d (-0x%04x)", ret, (unsigned)-ret);
        mbedtls_x509_crt_free(&crt);
    }

    if (device_private_key_len > 0U) {
        mbedtls_pk_init(&pk);
        ret = mbedtls_pk_parse_key(&pk, device_private_key, device_private_key_len,
                                   NULL, 0, NULL, NULL);
        LOG_INF("Credential check: private key parse -> %d (-0x%04x)", ret, (unsigned)-ret);
        mbedtls_pk_free(&pk);
    }
}

/* Compute the transcript the gateway expects to see signed:
 * SHA-256("wasmbed-pop-v1" || nonce || spki). */
static int pop_transcript(const uint8_t *nonce, uint32_t nonce_len, uint8_t out[32])
{
    mbedtls_sha256_context ctx;
    int ret;

    mbedtls_sha256_init(&ctx);
    ret = mbedtls_sha256_starts(&ctx, 0);
    if (ret == 0) {
        ret = mbedtls_sha256_update(&ctx, (const unsigned char *)POP_CONTEXT, POP_CONTEXT_LEN);
    }
    if (ret == 0) {
        ret = mbedtls_sha256_update(&ctx, nonce, nonce_len);
    }
    if (ret == 0) {
        ret = mbedtls_sha256_update(&ctx, device_public_key, device_public_key_len);
    }
    if (ret == 0) {
        ret = mbedtls_sha256_finish(&ctx, out);
    }
    mbedtls_sha256_free(&ctx);
    return ret;
}

/* Sign the gateway's challenge with the injected private key.
 * Writes an ASN.1 DER ECDSA signature into sig; returns 0 on success. */
static int sign_challenge(const uint8_t *nonce, uint32_t nonce_len,
                          uint8_t *sig, size_t sig_size, size_t *sig_len)
{
    /* Static rather than automatic: the entropy and DRBG contexts are over a
     * kilobyte together, and this runs on the main thread during init, whose
     * stack is already sized for WAMR instantiation. */
    static mbedtls_pk_context pk;
    static mbedtls_entropy_context entropy;
    static mbedtls_ctr_drbg_context ctr_drbg;
    uint8_t hash[32];
    int ret;

    if (device_private_key_len == 0U) {
        LOG_ERR("No private key injected: cannot answer the challenge");
        return -1;
    }

    mbedtls_pk_init(&pk);
    mbedtls_entropy_init(&entropy);
    mbedtls_ctr_drbg_init(&ctr_drbg);

    ret = mbedtls_ctr_drbg_seed(&ctr_drbg, mbedtls_entropy_func, &entropy,
                                (const unsigned char *)POP_CONTEXT, POP_CONTEXT_LEN);
    if (ret != 0) {
        LOG_ERR("ctr_drbg seed failed: -0x%04x", (unsigned)-ret);
        goto out;
    }

    ret = pop_transcript(nonce, nonce_len, hash);
    if (ret != 0) {
        LOG_ERR("Failed to compute the challenge transcript: -0x%04x", (unsigned)-ret);
        goto out;
    }

    ret = mbedtls_pk_parse_key(&pk, device_private_key, device_private_key_len,
                               NULL, 0, mbedtls_ctr_drbg_random, &ctr_drbg);
    if (ret != 0) {
        LOG_ERR("Failed to parse the private key: -0x%04x", (unsigned)-ret);
        goto out;
    }

    ret = mbedtls_pk_sign(&pk, MBEDTLS_MD_SHA256, hash, sizeof(hash),
                          sig, sig_size, sig_len,
                          mbedtls_ctr_drbg_random, &ctr_drbg);
    if (ret != 0) {
        LOG_ERR("Failed to sign the challenge: -0x%04x", (unsigned)-ret);
    }

out:
    mbedtls_pk_free(&pk);
    mbedtls_ctr_drbg_free(&ctr_drbg);
    mbedtls_entropy_free(&entropy);
    return ret == 0 ? 0 : -1;
}

/* Parse endpoint string (format: "host:port") */
static int parse_endpoint(const char *endpoint, char *host, size_t host_len, uint16_t *port)
{
    if (endpoint == NULL || host == NULL || port == NULL) {
        return -1;
    }
    
    /* Find colon separator */
    const char *colon = strchr(endpoint, ':');
    if (colon == NULL) {
        LOG_ERR("Invalid endpoint format (missing port): %s", endpoint);
        return -1;
    }
    
    /* Extract host */
    size_t host_len_actual = colon - endpoint;
    if (host_len_actual >= host_len) {
        LOG_ERR("Host name too long");
        return -1;
    }
    memcpy(host, endpoint, host_len_actual);
    host[host_len_actual] = '\0';
    
    /* Extract port */
    *port = (uint16_t)atoi(colon + 1);
    if (*port == 0) {
        LOG_ERR("Invalid port number");
        return -1;
    }
    
    return 0;
}

/* Initialize Wasmbed protocol handler */
int wasmbed_protocol_init(void)
{
    if (protocol_initialized) {
        LOG_WRN("Protocol already initialized");
        return 0;
    }

    LOG_INF("Initializing Wasmbed protocol handler...");
    
    /* Read gateway endpoint from memory (written by Renode) */
    if (read_gateway_endpoint() != 0) {
        LOG_ERR("Failed to read gateway endpoint from memory");
        /* Use default endpoint as fallback */
        strncpy(gateway_endpoint, "127.0.0.1:40029", sizeof(gateway_endpoint) - 1);
        LOG_WRN("Using default endpoint: %s", gateway_endpoint);
    }
    
    /* Read the device identity before anything else can overwrite that RAM */
    read_device_public_key();
    read_device_credentials();
    check_credentials();

    /* Register the credentials before connecting: the socket has to present the
     * client certificate during the handshake, and check the gateway's against
     * the fleet CA. */
    if (network_set_tls_credentials(fleet_ca_cert, fleet_ca_cert_len,
                                    device_client_cert, device_client_cert_len,
                                    device_private_key, device_private_key_len) != 0) {
        LOG_WRN("Continuing without TLS credentials; the gateway may refuse the connection");
    }

    /* Parse endpoint and connect to gateway with TLS */
    char host[64];
    uint16_t port;
    if (parse_endpoint(gateway_endpoint, host, sizeof(host), &port) == 0) {
        LOG_INF("Connecting to gateway with TLS: %s:%u", host, port);
        /* Add delay to ensure network is ready */
        k_sleep(K_SECONDS(1));
        if (network_connect_tls(host, port) == 0) {
            gateway_connected = true;
            LOG_INF("Connected to gateway via TLS");
            /* Perform enrollment */
            if (do_enrollment() != 0) {
                LOG_WRN("Enrollment failed - will continue with heartbeats only");
            }
        } else {
            LOG_ERR("Failed to connect to gateway with TLS - will retry later");
            /* Don't fail initialization - connection can be retried */
        }
    } else {
        LOG_ERR("Failed to parse gateway endpoint");
    }

    protocol_initialized = true;
    LOG_INF("Wasmbed protocol handler initialized");

    return 0;
}

/* Max WASM module size we accept (copy to static buffer per WAMR).
 * 16 KB: saves 2×32KB BSS vs 48KB; offset by WAMR_HEAP_SIZE increase to 128KB. */
#define MAX_WASM_SIZE (16 * 1024)
#define MAX_APP_ID_LEN 64

static uint8_t wasm_copy_buf[MAX_WASM_SIZE];
static char deploy_app_id_buf[MAX_APP_ID_LEN];

/* Running application state */
static uint32_t current_instance_id = 0;
static bool app_deployed = false;
#define APP_STATUS_INTERVAL_MS 30000U
static uint32_t last_app_status_ms = 0U;

/* Read CBOR text at *pp into buf (max buf_size), null-term; advance *pp. Return 0 or -1. */
static int cbor_read_text(const uint8_t **pp, const uint8_t *end, char *buf, size_t buf_size)
{
    const uint8_t *p = *pp;
    if (p >= end || buf_size == 0) return -1;
    uint32_t len;
    if (*p >= 0x60 && *p <= 0x77) {
        len = *p - 0x60;
        p += 1;
    } else if (*p == 0x78 && p + 2 <= end) {
        len = p[1];
        p += 2;
    } else if (*p == 0x79 && p + 3 <= end) {
        len = (uint32_t)p[1] << 8 | p[2];
        p += 3;
    } else {
        return -1;
    }
    if (p + len > end) return -1;
    if (len >= buf_size) len = (uint32_t)(buf_size - 1);
    memcpy(buf, p, len);
    buf[len] = '\0';
    *pp = p + len;
    return 0;
}

/* Advance *pp past a CBOR text (skip). */
static int cbor_skip_text(const uint8_t **pp, const uint8_t *end)
{
    const uint8_t *p = *pp;
    if (p >= end) return -1;
    uint32_t len;
    if (*p >= 0x60 && *p <= 0x77) {
        len = *p - 0x60;
        p += 1;
    } else if (*p == 0x78 && p + 2 <= end) {
        len = p[1];
        p += 2;
    } else if (*p == 0x79 && p + 3 <= end) {
        len = (uint32_t)p[1] << 8 | p[2];
        p += 3;
    } else {
        return -1;
    }
    if (p + len > end) return -1;
    *pp = p + len;
    return 0;
}

/* Read CBOR byte string at *pp; set *out_ptr, *out_len; advance *pp. Return 0 or -1. */
static int cbor_read_bytes(const uint8_t **pp, const uint8_t *end, const uint8_t **out_ptr, uint32_t *out_len)
{
    const uint8_t *p = *pp;
    if (p >= end) return -1;
    uint32_t len;
    const uint8_t *start;
    if (*p >= 0x40 && *p <= 0x57) {
        len = *p - 0x40;
        start = p + 1;
    } else if (*p == 0x58 && p + 2 <= end) {
        len = p[1];
        start = p + 2;
    } else if (*p == 0x59 && p + 3 <= end) {
        len = (uint32_t)p[1] << 8 | p[2];
        start = p + 3;
    } else {
        return -1;
    }
    if (start + len > end) return -1;
    *out_ptr = start;
    *out_len = len;
    *pp = start + len;
    return 0;
}

/* Skip one CBOR item. */
static const uint8_t *cbor_skip_one(const uint8_t *p, const uint8_t *end)
{
    if (p >= end) return end;
    if (*p >= 0x60 && *p <= 0x77) { return p + 1 + (*p - 0x60); }
    if (*p == 0x78 && p + 2 <= end) { return p + 2 + p[1]; }
    if (*p == 0x79 && p + 3 <= end) { return p + 3 + ((uint32_t)p[1]<<8|p[2]); }
    if (*p >= 0x40 && *p <= 0x57) { return p + 1 + (*p - 0x40); }
    if (*p == 0x58 && p + 2 <= end) { return p + 2 + p[1]; }
    if (*p == 0x59 && p + 3 <= end) { return p + 3 + ((uint32_t)p[1]<<8|p[2]); }
    if (*p == 0xf6 || *p == 0xf4 || *p == 0xf5) return p + 1;
    if (*p >= 0x00 && *p <= 0x17) return p + 1;
    if (*p == 0x18 && p + 2 <= end) return p + 2;
    if (*p == 0x19 && p + 3 <= end) return p + 3;
    if (*p == 0x1b && p + 9 <= end) return p + 9;
    if (*p >= 0x80 && *p <= 0x97) {
        unsigned n = *p - 0x80;
        p++;
        for (; n > 0 && p < end; n--) p = cbor_skip_one(p, end);
        return p;
    }
    if (*p >= 0xa0 && *p <= 0xb7) {
        unsigned n = *p - 0xa0;
        p++;
        for (unsigned i = 0; i < n * 2 && p < end; i++) p = cbor_skip_one(p, end);
        return p;
    }
    return end;
}

/*
 * Wire format from Gateway: 4 bytes length (big-endian u32) + CBOR(ServerMessage).
 * DeployApplication CBOR: array(6) = 0x86, u32(5), app_id (text), name (text),
 * module_hash (bytes32), wasm_bytes (bytes), config (null/object).
 */
static int handle_deploy_application(const uint8_t *cbor, uint32_t cbor_len,
                                     const uint8_t *module_hash,
                                     const uint8_t *wasm_ptr, uint32_t wasm_len)
{
    uint32_t module_id = 0, instance_id = 0;
    uint8_t actual_hash[32];
    int ret;

    ARG_UNUSED(cbor);
    ARG_UNUSED(cbor_len);

    if (wasm_len == 0 || wasm_len > MAX_WASM_SIZE) {
        LOG_ERR("WASM size invalid: %u", (unsigned)wasm_len);
        return -1;
    }

    /* Check the module against the digest the gateway announced before handing
     * anything to the runtime: a module altered in transit must not be
     * instantiated. */
    if (mbedtls_sha256(wasm_ptr, wasm_len, actual_hash, 0) != 0) {
        LOG_ERR("Failed to hash the received module");
        return -1;
    }
    if (memcmp(actual_hash, module_hash, sizeof(actual_hash)) != 0) {
        LOG_ERR("Module hash mismatch: refusing to instantiate %s", deploy_app_id_buf);
        return -1;
    }
    LOG_INF("Module hash verified (%u bytes)", (unsigned)wasm_len);

    /* Reset the WAMR runtime completely before a new deployment.
     * Unloading instances/modules alone is not sufficient when a previous
     * load attempt left the allocator state fragmented or exhausted. */
    wamr_cleanup();
    if (wamr_init() != 0) {
        LOG_ERR("Failed to reinitialize WAMR runtime");
        return -1;
    }
    app_deployed = false;
    current_instance_id = 0;

    memcpy(wasm_copy_buf, wasm_ptr, wasm_len);

    ret = wamr_load_module(wasm_copy_buf, wasm_len, &module_id);
    if (ret != 0) {
        LOG_ERR("wamr_load_module failed");
        return -1;
    }
    ret = wamr_instantiate(module_id, &instance_id);
    if (ret != 0) {
        LOG_ERR("wamr_instantiate failed");
        return -1;
    }
    LOG_INF("WASM deployed: app_id=%s module_id=%u instance_id=%u", deploy_app_id_buf, (unsigned)module_id, (unsigned)instance_id);
    /* Execute the WASM "run" entry point */
    if (wamr_call_wasi_start(instance_id) != 0) {
        if (wamr_last_call_hit_instruction_limit(instance_id)) {
            LOG_WRN("WASM run() returned error: instruction limit exceeded (computational proxy budget, see wamr_integration.c)");
        } else {
            LOG_WRN("WASM run() returned error");
        }
    }
    current_instance_id = instance_id;
    app_deployed = true;
    last_app_status_ms = k_uptime_get_32();
    return 0;
}

/* Encode and send ApplicationDeployAck: array(4), tag 5, app_id (str), success (bool), error (null/text). */
static void send_deploy_ack(const char *app_id, bool success, const char *error_msg)
{
    uint8_t buf[4 + 64 + 96];
    uint32_t app_id_len = (uint32_t)strlen(app_id);
    uint32_t error_len = error_msg ? (uint32_t)strlen(error_msg) : 0;
    if (app_id_len >= 64) app_id_len = 63;
    if (error_len >= 64) error_len = 63;
    uint32_t off = 4; /* leave space for length prefix */
    buf[off++] = 0x84;
    buf[off++] = 0x05;
    if (app_id_len <= 23) {
        buf[off++] = (uint8_t)(0x60 + app_id_len);
    } else {
        buf[off++] = 0x78;
        buf[off++] = (uint8_t)app_id_len;
    }
    memcpy(buf + off, app_id, app_id_len);
    off += app_id_len;
    buf[off++] = success ? 0xf5 : 0xf4;
    if (error_len == 0) {
        buf[off++] = 0xf6; /* null error */
    } else if (error_len <= 23) {
        buf[off++] = (uint8_t)(0x60 + error_len);
        memcpy(buf + off, error_msg, error_len);
        off += error_len;
    } else {
        buf[off++] = 0x78;
        buf[off++] = (uint8_t)error_len;
        memcpy(buf + off, error_msg, error_len);
        off += error_len;
    }
    uint32_t cbor_len = off - 4;
    buf[0] = (uint8_t)(cbor_len >> 24);
    buf[1] = (uint8_t)(cbor_len >> 16);
    buf[2] = (uint8_t)(cbor_len >> 8);
    buf[3] = (uint8_t)cbor_len;
    wasmbed_protocol_send_message(buf, off);
}

/* Encode and send ApplicationStatus: array(5), u32(4), str(app_id), u32(status), null, null.
 * status values: 0=Deploying, 1=Running, 2=Stopped, 3=Failed, 4=Unknown */
static void send_application_status(const char *app_id, uint8_t status)
{
    uint8_t buf[4 + 2 + 1 + MAX_APP_ID_LEN + 1 + 1 + 1];
    uint32_t app_id_len = (uint32_t)strlen(app_id);
    if (app_id_len >= MAX_APP_ID_LEN) app_id_len = MAX_APP_ID_LEN - 1;
    uint32_t off = 4; /* leave space for length prefix */
    buf[off++] = 0x85; /* array(5) */
    buf[off++] = 0x04; /* u32(4) = CLIENT_APPLICATION_STATUS */
    if (app_id_len <= 23) {
        buf[off++] = (uint8_t)(0x60 + app_id_len);
    } else {
        buf[off++] = 0x78;
        buf[off++] = (uint8_t)app_id_len;
    }
    memcpy(buf + off, app_id, app_id_len);
    off += app_id_len;
    buf[off++] = status; /* CBOR uint: 0x01 = Running */
    buf[off++] = 0xf6;   /* null error */
    buf[off++] = 0xf6;   /* null metrics */
    uint32_t cbor_len = off - 4;
    buf[0] = (uint8_t)(cbor_len >> 24);
    buf[1] = (uint8_t)(cbor_len >> 16);
    buf[2] = (uint8_t)(cbor_len >> 8);
    buf[3] = (uint8_t)cbor_len;
    wasmbed_protocol_send_message(buf, off);
}

/* Handle incoming message from gateway */
int wasmbed_protocol_handle_message(const uint8_t *data, uint32_t data_len)
{
    if (!protocol_initialized) {
        LOG_ERR("Protocol not initialized");
        return -1;
    }

    if (data == NULL || data_len == 0) {
        LOG_ERR("Invalid message data");
        return -1;
    }

    /* Wire format: 4 byte big-endian length + CBOR payload */
    if (data_len >= 4) {
        uint32_t payload_len = (uint32_t)data[0] << 24 | (uint32_t)data[1] << 16 |
                               (uint32_t)data[2] << 8 | (uint32_t)data[3];
        if (data_len >= 4 + payload_len && payload_len >= 2) {
            const uint8_t *cbor = data + 4;
            const uint8_t *cbor_end = cbor + payload_len;
            /* DeployApplication is array(6) = 0x86, tag u32(5) = 0x05, then
             * app_id, name, module_hash, wasm_bytes, config */
            if (cbor[0] == 0x86 && cbor[1] == 0x05) {
                const uint8_t *p = cbor + 2;
                if (cbor_read_text(&p, cbor_end, deploy_app_id_buf, MAX_APP_ID_LEN) != 0) {
                    LOG_ERR("DeployApplication: failed to read app_id");
                    send_deploy_ack("", false, "parse app_id");
                    return 0;
                }
                if (cbor_skip_text(&p, cbor_end) != 0) {
                    LOG_ERR("DeployApplication: failed to skip name");
                    send_deploy_ack(deploy_app_id_buf, false, "parse name");
                    return 0;
                }
                const uint8_t *hash_ptr = NULL;
                uint32_t hash_len = 0;
                if (cbor_read_bytes(&p, cbor_end, &hash_ptr, &hash_len) != 0 || hash_len != 32) {
                    LOG_ERR("DeployApplication: failed to read module_hash (len=%u)", hash_len);
                    send_deploy_ack(deploy_app_id_buf, false, "parse module_hash");
                    return 0;
                }
                const uint8_t *wasm_ptr = NULL;
                uint32_t wasm_len = 0;
                if (cbor_read_bytes(&p, cbor_end, &wasm_ptr, &wasm_len) != 0) {
                    LOG_ERR("DeployApplication: failed to read wasm_bytes");
                    send_deploy_ack(deploy_app_id_buf, false, "parse wasm_bytes");
                    return 0;
                }
                if (handle_deploy_application(cbor, payload_len, hash_ptr, wasm_ptr, wasm_len) == 0) {
                    send_deploy_ack(deploy_app_id_buf, true, NULL);
                } else {
                    send_deploy_ack(deploy_app_id_buf, false, "hash or load/instantiate failed");
                }
            }
        }
    }

    LOG_DBG("Handling message from gateway (size: %u bytes)", data_len);
    return 0;
}

/* Receive one complete framed message and dispatch via wasmbed_protocol_handle_message.
 * Uses zsock_poll to check availability first (honors timeout_ms),
 * then recv_frame() for correct accumulation when data arrives. */
int wasmbed_protocol_recv_and_handle(int timeout_ms)
{
    if (!protocol_initialized || !gateway_connected) {
        return -1;
    }

    /* Short poll to avoid blocking heartbeats when no message is coming */
    int ready = network_poll_readable(timeout_ms);
    if (ready < 0) {
        LOG_WRN("network_poll_readable error - connection may be lost");
        gateway_connected = false;
        return -1;
    }
    if (ready == 0) {
        return 1; /* timeout: no data available */
    }

    /* Data is available: receive the complete frame (data is there so recv_frame won't block long) */
    static uint8_t frame_buf[MAX_WASM_SIZE + 128];
    uint32_t total_len = 0;
    int ret = recv_frame(frame_buf, sizeof(frame_buf), &total_len, 5000);
    if (ret < 0 || total_len == 0) {
        LOG_WRN("recv_frame error - connection may be lost");
        gateway_connected = false;
        return -1;
    }
    return wasmbed_protocol_handle_message(frame_buf, total_len);
}

/* Send message to gateway */
int wasmbed_protocol_send_message(const uint8_t *data, uint32_t data_len)
{
    if (!protocol_initialized) {
        LOG_ERR("Protocol not initialized");
        return -1;
    }

    if (!gateway_connected) {
        LOG_ERR("Not connected to gateway");
        return -1;
    }

    if (data == NULL || data_len == 0) {
        LOG_ERR("Invalid message data");
        return -1;
    }

    LOG_DBG("Sending message to gateway (size: %u bytes)", data_len);

    if (network_send(data, data_len) != 0) {
        LOG_ERR("Failed to send message to gateway");
        return -1;
    }

    return 0;
}

int wasmbed_protocol_send_heartbeat(void)
{
    return wasmbed_protocol_send_message(heartbeat_packet, HEARTBEAT_PACKET_LEN);
}

/* Reconnect interval: try every 30 seconds when not connected */
#define RECONNECT_INTERVAL_MS 30000U

static uint32_t last_reconnect_attempt_ms = 0U;

/* Attempt to connect to gateway and perform enrollment */
static int try_connect_gateway(void)
{
    char host[64];
    uint16_t port;
    if (parse_endpoint(gateway_endpoint, host, sizeof(host), &port) != 0) {
        return -1;
    }
    LOG_INF("Retrying TLS connection to gateway: %s:%u", host, port);
    if (network_connect_tls(host, port) != 0) {
        return -1;
    }
    gateway_connected = true;
    LOG_INF("Connected to gateway via TLS");
    if (do_enrollment() != 0) {
        LOG_WRN("Enrollment failed - continuing with heartbeats only");
    }
    return 0;
}

void wasmbed_protocol_tick(void)
{
    uint32_t now = k_uptime_get_32();

    if (!gateway_connected) {
        /* Retry connection periodically */
        if (now - last_reconnect_attempt_ms >= RECONNECT_INTERVAL_MS) {
            last_reconnect_attempt_ms = now;
            if (try_connect_gateway() != 0) {
                LOG_WRN("Gateway reconnect failed - will retry in %u s",
                        RECONNECT_INTERVAL_MS / 1000U);
            }
        }
        return;
    }

    if (now - last_heartbeat_uptime_ms >= HEARTBEAT_INTERVAL_MS) {
        if (wasmbed_protocol_send_heartbeat() == 0) {
            last_heartbeat_uptime_ms = now;
            LOG_DBG("Heartbeat sent");
        } else {
            /* Heartbeat failed - connection likely dropped */
            LOG_WRN("Heartbeat failed - marking gateway as disconnected");
            gateway_connected = false;
            last_reconnect_attempt_ms = now - RECONNECT_INTERVAL_MS; /* retry immediately */
        }
    }

    /* Periodic ApplicationStatus reporting */
    if (app_deployed && (now - last_app_status_ms >= APP_STATUS_INTERVAL_MS)) {
        send_application_status(deploy_app_id_buf, 0x01); /* 0x01 = Running */
        last_app_status_ms = now;
        LOG_INF("ApplicationStatus sent for %s", deploy_app_id_buf);
    }
}

