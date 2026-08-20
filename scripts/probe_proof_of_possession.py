#!/usr/bin/env python3
"""Exercise the gateway's proof-of-possession check against a live gateway.

Runs the southbound enrollment exchange twice with the same, correctly
provisioned client certificate:

  honest    the challenge is signed with the device's own key
  forged    the challenge is signed with a different key

The second is the fault injection: the device authenticates the transport and
announces a key registered in its Device resource, but cannot demonstrate that
it holds the matching private key. The gateway must refuse and must not record
an association.

Usage:
  ./scripts/probe_proof_of_possession.py --host 127.0.0.1 --port 30443 \
      --ca config/certs/ca-cert.pem --identity-dir config/devices --device probe-device-1
"""

import argparse
import socket
import ssl
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec

POP_CONTEXT = b"wasmbed-pop-v1"

# ClientMessage tags, mirroring crates/wasmbed-protocol/src/cbor.rs
ENROLLMENT_REQUEST = bytes([0x81, 0x01])
ENROLLMENT_ACK = bytes([0x81, 0x03])


def frame(payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + payload


def cbor_bytes(tag: int, blob: bytes) -> bytes:
    """array(2), uint(tag), byte string -- the encoding the firmware emits."""
    if len(blob) < 24:
        header = bytes([0x40 | len(blob)])
    elif len(blob) < 256:
        header = bytes([0x58, len(blob)])
    else:
        header = bytes([0x59]) + struct.pack(">H", len(blob))
    return bytes([0x82, tag]) + header + blob


def read_frame(sock: ssl.SSLSocket) -> bytes:
    header = b""
    while len(header) < 4:
        chunk = sock.recv(4 - len(header))
        if not chunk:
            raise ConnectionError("closed while reading the length prefix")
        header += chunk
    (length,) = struct.unpack(">I", header)
    payload = b""
    while len(payload) < length:
        chunk = sock.recv(length - len(payload))
        if not chunk:
            raise ConnectionError("closed while reading the payload")
        payload += chunk
    return payload


def describe(payload: bytes) -> str:
    if len(payload) >= 2 and payload[0] in (0x81, 0x82):
        names = {
            0x00: "HeartbeatAck", 0x01: "EnrollmentAccepted",
            0x02: "EnrollmentRejected", 0x03: "DeviceUuid",
            0x04: "EnrollmentCompleted", 0x09: "Challenge",
        }
        name = names.get(payload[1], f"tag {payload[1]}")
        if payload[1] == 0x02:
            return f"{name}: {payload[4:].decode('utf-8', 'replace')}"
        return name
    return payload.hex()


def enroll(host, port, ca, cert, key, spki, signing_key) -> str:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.load_verify_locations(ca)
    context.load_cert_chain(cert, key)
    context.check_hostname = False

    with socket.create_connection((host, port), timeout=20) as raw:
        with context.wrap_socket(raw) as sock:
            sock.send(frame(ENROLLMENT_REQUEST))
            accepted = describe(read_frame(sock))
            if accepted != "EnrollmentAccepted":
                return f"enrollment not accepted: {accepted}"

            sock.send(frame(cbor_bytes(0x02, spki)))
            challenge = read_frame(sock)
            if len(challenge) < 4 or challenge[1] != 0x09:
                return f"expected Challenge, got {describe(challenge)}"
            nonce = challenge[4:]

            # Sign the pre-image, not its digest: ec.ECDSA(SHA256) hashes what it
            # is given, so passing a digest would sign SHA-256 applied twice and
            # every honest attempt would be rejected.
            transcript = POP_CONTEXT + nonce + spki
            signature = signing_key.sign(transcript, ec.ECDSA(hashes.SHA256()))
            sock.send(frame(cbor_bytes(0x08, signature)))

            return describe(read_frame(sock))


def device_phase(device: str, namespace: str) -> str:
    result = subprocess.run(
        ["kubectl", "get", "device", device, "-n", namespace,
         "-o", "jsonpath={.status.phase}"],
        capture_output=True, text=True,
    )
    return result.stdout.strip() or "<none>"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=30443)
    parser.add_argument("--ca", required=True)
    parser.add_argument("--identity-dir", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--namespace", default="wasmbed")
    args = parser.parse_args()

    identity = Path(args.identity_dir)
    spki = (identity / f"{args.device}.spki").read_bytes()
    key_der = (identity / f"{args.device}.key").read_bytes()
    own_key = serialization.load_der_private_key(key_der, password=None)

    # ssl.load_cert_chain wants PEM files on disk.
    with tempfile.TemporaryDirectory() as tmp:
        cert_pem = Path(tmp) / "cert.pem"
        key_pem = Path(tmp) / "key.pem"
        subprocess.run(
            ["openssl", "x509", "-inform", "DER",
             "-in", str(identity / f"{args.device}.crt"), "-out", str(cert_pem)],
            check=True,
        )
        key_pem.write_bytes(own_key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        ))

        other_key = ec.generate_private_key(ec.SECP256R1())

        print(f"phase before: {device_phase(args.device, args.namespace)}")
        forged = enroll(args.host, args.port, args.ca, cert_pem, key_pem, spki, other_key)
        print(f"forged signature  -> {forged}")
        print(f"phase after forged: {device_phase(args.device, args.namespace)}")

        honest = enroll(args.host, args.port, args.ca, cert_pem, key_pem, spki, own_key)
        print(f"honest signature  -> {honest}")
        print(f"phase after honest: {device_phase(args.device, args.namespace)}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
