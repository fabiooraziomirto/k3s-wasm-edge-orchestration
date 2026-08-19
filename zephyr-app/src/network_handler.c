/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright © 2025 Wasmbed contributors
 *
 * Network Handler Implementation
 * Uses Zephyr network stack (TCP/IP, TLS)
 */

#include "network_handler.h"
#include <zephyr/logging/log.h>
#include <zephyr/net/net_if.h>
#include <zephyr/net/net_core.h>
#include <zephyr/net/net_context.h>
#include <zephyr/net/socket.h>
#include <zephyr/net/net_event.h>
#include <zephyr/net/dhcpv4.h>
#include <zephyr/net/ethernet.h>
#include <zephyr/net/ethernet_mgmt.h>
#include <zephyr/kernel.h>
#include <string.h>
#include <errno.h>

/* Use Zephyr socket API (zsock_*) instead of POSIX */
#include <zephyr/net/socket.h>
#include <zephyr/net/tls_credentials.h>

LOG_MODULE_REGISTER(network_handler, LOG_LEVEL_INF);

static bool network_initialized = false;
static int socket_fd = -1;
static struct net_if *net_iface = NULL;

/* Memory address where Renode writes this device's MAC address
 * (4-byte LE length, then the address bytes), mirroring the gateway endpoint
 * and public key injections in wasmbed_protocol.c. */
#define DEVICE_MAC_ADDR 0x20002100

/* Program the injected MAC before DHCP runs.
 * The Ethernet driver otherwise keeps its built-in address, which is identical
 * in every emulated instance: the whole fleet then asks for -- and shares -- one
 * DHCP lease, so return traffic follows whichever device last answered ARP. */
static void apply_injected_mac(struct net_if *iface)
{
    volatile uint32_t *len_ptr = (volatile uint32_t *)DEVICE_MAC_ADDR;
    uint32_t len = *len_ptr;
    uint8_t mac[6];

    if (len != sizeof(mac)) {
        LOG_WRN("No MAC injected (len=%u), keeping the driver default", len);
        return;
    }

    volatile uint8_t *mac_ptr = (volatile uint8_t *)(DEVICE_MAC_ADDR + 4);
    for (uint32_t i = 0; i < sizeof(mac); i++) {
        mac[i] = mac_ptr[i];
    }

    /* Go through the Ethernet management API, not net_if_set_link_addr(): the
     * latter only changes what the IP stack believes, while the driver keeps
     * programming the controller's address filter with its own built-in MAC.
     * Frames addressed to the new MAC would then be dropped by the NIC and DHCP
     * would never complete. The interface must be down for the change. */
    struct ethernet_req_params params = {0};
    memcpy(params.mac_address.addr, mac, sizeof(mac));

    net_if_down(iface);
    int ret = net_mgmt(NET_REQUEST_ETHERNET_SET_MAC_ADDRESS, iface,
                       &params, sizeof(params));
    if (ret != 0) {
        LOG_ERR("Cannot set injected MAC %02x:%02x:%02x:%02x:%02x:%02x (err %d)",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], ret);
        /* Fall back to the stack-level address: better than the shared default. */
        (void)net_if_set_link_addr(iface, mac, sizeof(mac), NET_LINK_ETHERNET);
        return;
    }

    LOG_INF("Using injected MAC %02x:%02x:%02x:%02x:%02x:%02x",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
}

static int configure_static_ipv4_fallback(void)
{
    struct in_addr addr;
    struct in_addr netmask;
    struct in_addr gateway;

    if (net_iface == NULL) {
        return -1;
    }

    if (net_addr_pton(AF_INET, "192.168.1.2", &addr) < 0 ||
        net_addr_pton(AF_INET, "255.255.255.0", &netmask) < 0 ||
        net_addr_pton(AF_INET, "192.168.1.1", &gateway) < 0) {
        LOG_ERR("Static IPv4 fallback parse failed");
        return -1;
    }

    if (!net_if_ipv4_addr_add(net_iface, &addr, NET_ADDR_MANUAL, 0)) {
        LOG_ERR("Cannot set static IPv4 fallback address");
        return -1;
    }

    net_if_ipv4_set_netmask(net_iface, &netmask);
    net_if_ipv4_set_gw(net_iface, &gateway);
    LOG_INF("Static IPv4 fallback configured: 192.168.1.2/24 gw 192.168.1.1");
    return 0;
}

/* Initialize network stack */
int network_init(void)
{
    if (network_initialized) {
        LOG_WRN("Network already initialized");
        return 0;
    }

    LOG_INF("Initializing network stack...");

    /* Get default network interface */
    /* Wait a bit for network interface to be available */
    int retries = 10;
    while (retries > 0) {
        net_iface = net_if_get_default();
        if (net_iface != NULL) {
            break;
        }
        LOG_WRN("Network interface not available yet, retrying... (%d)", retries);
        k_sleep(K_MSEC(500));
        retries--;
    }
    
    if (net_iface == NULL) {
        LOG_ERR("No network interface available after retries");
        /* Don't fail - allow firmware to continue without network */
        LOG_WRN("Continuing without network interface");
        return -1;
    }

    /* Give this instance its own link-layer identity before anything uses it */
    apply_injected_mac(net_iface);

    /* Bring interface up */
    if (!net_if_is_up(net_iface)) {
        net_if_up(net_iface);
        LOG_INF("Network interface brought up");
    }

    /* Start DHCP client if available */
#if defined(CONFIG_NET_DHCPV4)
    net_dhcpv4_start(net_iface);
    LOG_INF("DHCP client started");

    /* Wait for DHCP to assign an IP address (up to 120 seconds) */
    {
        int dhcp_wait = 120;
        bool got_ip = false;
        while (dhcp_wait > 0) {
            struct net_if_addr *unicast;
            struct net_if_ipv4 *ipv4 = net_iface->config.ip.ipv4;
            if (ipv4 != NULL) {
                for (int i = 0; i < NET_IF_MAX_IPV4_ADDR; i++) {
                    unicast = &ipv4->unicast[i];
                    if (unicast->is_used &&
                        unicast->addr_state == NET_ADDR_PREFERRED &&
                        unicast->addr_type == NET_ADDR_DHCP) {
                        got_ip = true;
                        break;
                    }
                }
            }
            if (got_ip) {
                break;
            }
            k_sleep(K_SECONDS(1));
            dhcp_wait--;
            if (dhcp_wait % 10 == 0) {
                LOG_INF("Waiting for DHCP... (%d s remaining)", dhcp_wait);
            }
        }
        if (got_ip) {
            LOG_INF("DHCP address acquired");
        } else {
            LOG_WRN("DHCP timeout - applying static IPv4 fallback");
            if (configure_static_ipv4_fallback() != 0) {
                LOG_WRN("Static IPv4 fallback failed - proceeding without IP");
            }
        }
    }
#else
    /* Static IP configuration would go here */
    LOG_INF("Using static IP configuration (DHCP disabled)");
    k_sleep(K_SECONDS(2));
#endif

    network_initialized = true;
    LOG_INF("Network stack initialized");

    return 0;
}

/* Process network events */
void network_process(void)
{
    if (!network_initialized) {
        return;
    }

    /* Network event processing is handled by Zephyr network stack automatically
     * This function can be used for custom event handling if needed */
    
    /* Check if interface is still up */
    if (net_iface != NULL && !net_if_is_up(net_iface)) {
        LOG_WRN("Network interface is down");
    }
}

/* Connect to gateway */
int network_connect(const char *host, uint16_t port)
{
    if (!network_initialized) {
        LOG_ERR("Network not initialized");
        return -1;
    }

    LOG_INF("Connecting to gateway: %s:%u", host, port);

    /* Close existing socket if any */
    if (socket_fd >= 0) {
        zsock_close(socket_fd);
        socket_fd = -1;
    }

    /* Create TCP socket */
    socket_fd = zsock_socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (socket_fd < 0) {
        LOG_ERR("Failed to create socket: %d", errno);
        return -1;
    }

    /* Setup address structure */
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);

    /* Resolve hostname to IP (simplified - assumes IP address string) */
    if (net_addr_pton(AF_INET, host, &addr.sin_addr) < 0) {
        LOG_ERR("Invalid IP address: %s", host);
        zsock_close(socket_fd);
        socket_fd = -1;
        return -1;
    }

    /* Connect to server */
    if (zsock_connect(socket_fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        LOG_ERR("Failed to connect: %d", errno);
        zsock_close(socket_fd);
        socket_fd = -1;
        return -1;
    }

    LOG_INF("Connected to gateway: %s:%u", host, port);

    return 0;
}

/* Connect to gateway with TLS */
int network_connect_tls(const char *host, uint16_t port)
{
    if (!network_initialized) {
        LOG_ERR("Network not initialized");
        return -1;
    }

    LOG_INF("Connecting to gateway with TLS: %s:%u", host, port);

    /* Close existing socket if any */
    if (socket_fd >= 0) {
        zsock_close(socket_fd);
        socket_fd = -1;
    }

    /* Create TLS socket */
    socket_fd = zsock_socket(AF_INET, SOCK_STREAM, IPPROTO_TLS_1_2);
    if (socket_fd < 0) {
        LOG_ERR("Failed to create TLS socket: %d", errno);
        return -1;
    }

    /* Skip server certificate verification (no CA cert loaded) */
    int verify = TLS_PEER_VERIFY_NONE;
    if (zsock_setsockopt(socket_fd, SOL_TLS, TLS_PEER_VERIFY, &verify, sizeof(verify)) < 0) {
        LOG_WRN("Failed to set TLS_PEER_VERIFY_NONE: %d", errno);
    }

    /* Set TLS hostname for SNI (required by some servers even with PEER_VERIFY_NONE) */
    if (zsock_setsockopt(socket_fd, SOL_TLS, TLS_HOSTNAME, host, strlen(host)) < 0) {
        LOG_WRN("Failed to set TLS_HOSTNAME (SNI): %d", errno);
    }

    /* Set send/connect timeout to 30 seconds to avoid blocking indefinitely */
    struct zsock_timeval tv = { .tv_sec = 30, .tv_usec = 0 };
    if (zsock_setsockopt(socket_fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv)) < 0) {
        LOG_WRN("Failed to set SO_SNDTIMEO: %d", errno);
    }
    if (zsock_setsockopt(socket_fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv)) < 0) {
        LOG_WRN("Failed to set SO_RCVTIMEO: %d", errno);
    }

    /* Setup address structure */
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);

    /* Resolve hostname to IP */
    if (net_addr_pton(AF_INET, host, &addr.sin_addr) < 0) {
        LOG_ERR("Invalid IP address: %s", host);
        zsock_close(socket_fd);
        socket_fd = -1;
        return -1;
    }

    /* Only configure TLS_HOSTNAME for real hostnames.
     * The gateway endpoint in Renode is an IP literal (192.168.1.1), and passing
     * it as SNI is unnecessary. When we do set TLS_HOSTNAME, Zephyr expects the
     * trailing NUL byte as in its own TLS clients. */
    if (strchr(host, '.') == NULL || strspn(host, "0123456789.") != strlen(host)) {
        if (zsock_setsockopt(socket_fd, SOL_TLS, TLS_HOSTNAME, host, strlen(host) + 1) < 0) {
            LOG_WRN("Failed to set TLS_HOSTNAME (SNI): %d", errno);
        }
    }

    /* Connect to server (TLS handshake happens during connect) */
    if (zsock_connect(socket_fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        LOG_ERR("Failed to connect: %d", errno);
        zsock_close(socket_fd);
        socket_fd = -1;
        return -1;
    }

    LOG_INF("Connected to gateway with TLS: %s:%u", host, port);

    return 0;
}

/* Send data via network */
int network_send(const uint8_t *data, uint32_t data_len)
{
    if (socket_fd < 0) {
        LOG_ERR("Socket not connected");
        return -1;
    }

    ssize_t sent = zsock_send(socket_fd, data, data_len, 0);
    if (sent < 0) {
        LOG_ERR("Failed to send data: %d", errno);
        return -1;
    }

    if (sent != (ssize_t)data_len) {
        LOG_WRN("Partial send: %zd/%u bytes", sent, data_len);
    }

    return 0;
}

/* Receive data from network */
int network_receive(uint8_t *buffer, uint32_t buffer_len, uint32_t *received_len)
{
    if (socket_fd < 0) {
        LOG_ERR("Socket not connected");
        return -1;
    }

    ssize_t received = zsock_recv(socket_fd, buffer, buffer_len, 0);
    if (received < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            /* No data available */
            *received_len = 0;
            return 0;
        }
        LOG_ERR("Failed to receive data: %d", errno);
        return -1;
    }

    *received_len = (uint32_t)received;
    return 0;
}

/* Poll socket for incoming data; returns >0 if readable, 0 on timeout, <0 on error */
int network_poll_readable(int timeout_ms)
{
    if (socket_fd < 0) {
        return -1;
    }
    struct zsock_pollfd pfd;
    pfd.fd     = socket_fd;
    pfd.events = ZSOCK_POLLIN;
    pfd.revents = 0;
    return zsock_poll(&pfd, 1, timeout_ms);
}

/* Receive a framed message: 4-byte BE length header + payload.
 * Uses individual recv calls for header and payload. */
int network_receive_framed(uint8_t *buffer, uint32_t buffer_len, uint32_t *received_len)
{
    if (socket_fd < 0) {
        return -1;
    }

    *received_len = 0;

    /* Read 4-byte length header — single blocking recv */
    ssize_t r = zsock_recv(socket_fd, buffer, 4, 0);
    if (r <= 0) {
        return -1;
    }
    /* Handle partial header read by accumulating */
    uint32_t hdr_got = (uint32_t)r;
    while (hdr_got < 4) {
        r = zsock_recv(socket_fd, buffer + hdr_got, 4 - hdr_got, 0);
        if (r <= 0) {
            return -1;
        }
        hdr_got += (uint32_t)r;
    }

    uint32_t payload_len = ((uint32_t)buffer[0] << 24) |
                           ((uint32_t)buffer[1] << 16) |
                           ((uint32_t)buffer[2] <<  8) |
                           ((uint32_t)buffer[3]);

    if (4 + payload_len > buffer_len) {
        LOG_ERR("Frame too large: %u bytes", payload_len);
        return -1;
    }

    /* Read payload — accumulate until complete */
    uint32_t got = 0;
    while (got < payload_len) {
        r = zsock_recv(socket_fd, buffer + 4 + got, payload_len - got, 0);
        if (r <= 0) {
            return -1;
        }
        got += (uint32_t)r;
    }

    *received_len = 4 + payload_len;
    return 0;
}

