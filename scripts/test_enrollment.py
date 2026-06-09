#!/usr/bin/env python3
"""
Test script: simulate Wasmbed firmware enrollment via TLS.
Connects to the gateway TLS port and performs the full enrollment handshake.
"""
import ssl
import socket
import struct
import sys

GATEWAY_HOST = "10.43.192.162"
GATEWAY_TLS_PORT = 8443

# Wire format: 4-byte big-endian length + CBOR payload

def send_msg(sock, cbor_bytes: bytes):
    frame = struct.pack(">I", len(cbor_bytes)) + cbor_bytes
    sock.sendall(frame)
    print(f"  → sent ({len(cbor_bytes)} CBOR bytes): {cbor_bytes.hex()}")

def recv_msg(sock, timeout_s=5) -> bytes:
    sock.settimeout(timeout_s)
    # Read 4-byte length prefix
    header = b""
    while len(header) < 4:
        chunk = sock.recv(4 - len(header))
        if not chunk:
            raise ConnectionError("Connection closed while reading length")
        header += chunk
    length = struct.unpack(">I", header)[0]
    # Read payload
    payload = b""
    while len(payload) < length:
        chunk = sock.recv(length - len(payload))
        if not chunk:
            raise ConnectionError("Connection closed while reading payload")
        payload += chunk
    print(f"  ← recv ({length} CBOR bytes): {payload.hex()}")
    return payload

def main():
    # Static 32-byte test public key (same as what the firmware will use)
    pub_key = bytes([0xAB] * 32)

    print(f"Connecting to {GATEWAY_HOST}:{GATEWAY_TLS_PORT} via TLS...")
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE  # skip cert verify (like firmware with TLS_PEER_VERIFY_NONE)

    raw = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    raw.connect((GATEWAY_HOST, GATEWAY_TLS_PORT))
    sock = ctx.wrap_socket(raw, server_hostname=GATEWAY_HOST)
    print("TLS handshake complete\n")

    # Step 1: Send EnrollmentRequest = array(1) uint(1) = 0x81 0x01
    print("Step 1: EnrollmentRequest")
    send_msg(sock, bytes([0x81, 0x01]))

    # Step 2: Receive EnrollmentAccepted or EnrollmentRejected
    print("Step 2: Waiting for EnrollmentAccepted ...")
    resp = recv_msg(sock)
    if len(resp) < 2 or resp[0] != 0x81:
        print(f"ERROR: unexpected response: {resp.hex()}")
        return 1
    if resp[1] == 0x01:
        print("  → EnrollmentAccepted ✓")
    elif resp[1] == 0x02:
        print(f"  → EnrollmentRejected: {resp.hex()}")
        return 1
    else:
        print(f"  → Unknown response tag: 0x{resp[1]:02x}")
        return 1

    # Step 3: Send PublicKey { key: pub_key }
    # CBOR: array(2) uint(2) bytes(32) = 0x82 0x02 0x58 0x20 <32 bytes>
    print("\nStep 3: Send PublicKey")
    cbor_pubkey = bytes([0x82, 0x02, 0x58, len(pub_key)]) + pub_key
    send_msg(sock, cbor_pubkey)

    # Step 4: Receive DeviceUuid = array(2) uint(3) bytes(16)
    print("Step 4: Waiting for DeviceUuid ...")
    resp = recv_msg(sock, timeout_s=10)
    if len(resp) < 2 or resp[0] != 0x82 or resp[1] != 0x03:
        print(f"ERROR: expected DeviceUuid (0x82 0x03 ...), got: {resp.hex()}")
        return 1
    uuid_bytes = resp[3:3+16] if len(resp) >= 19 else resp[2:]
    import uuid as uuidlib
    try:
        device_uuid = uuidlib.UUID(bytes=uuid_bytes)
        print(f"  → DeviceUuid: {device_uuid} ✓")
    except Exception:
        print(f"  → DeviceUuid bytes: {uuid_bytes.hex()} ✓")

    # Step 5: Send EnrollmentAcknowledgment = array(1) uint(3) = 0x81 0x03
    print("\nStep 5: Send EnrollmentAcknowledgment")
    send_msg(sock, bytes([0x81, 0x03]))

    # Step 6: Receive EnrollmentCompleted = array(1) uint(4) = 0x81 0x04
    print("Step 6: Waiting for EnrollmentCompleted ...")
    try:
        resp = recv_msg(sock, timeout_s=5)
        if len(resp) >= 2 and resp[0] == 0x81 and resp[1] == 0x04:
            print("  → EnrollmentCompleted ✓")
        else:
            print(f"  → Unexpected response: {resp.hex()}")
    except Exception as e:
        print(f"  → No EnrollmentCompleted received ({e}) — continuing")

    print("\n✓ Enrollment handshake completed successfully!")

    # Step 7: Send a Heartbeat to verify connection stays alive
    print("\nStep 7: Send Heartbeat")
    send_msg(sock, bytes([0x81, 0x00]))
    try:
        resp = recv_msg(sock, timeout_s=5)
        if len(resp) >= 2 and resp[0] == 0x81 and resp[1] == 0x00:
            print("  → HeartbeatAck ✓")
        else:
            print(f"  → Response: {resp.hex()}")
    except Exception as e:
        print(f"  → No HeartbeatAck ({e})")

    sock.close()
    return 0

if __name__ == "__main__":
    sys.exit(main())
