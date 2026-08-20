#!/usr/bin/env bash
# Generate TLS certificates for wasmbed-gateway (test/local use).
# Usage: ./scripts/generate-gateway-certs.sh [output_dir] [extra_san ...]
# Default output_dir: config/certs (relative to repo root).
#
# Devices reach the gateway by IP literal and verify its certificate, so any
# address they dial has to appear in the SAN list. Pass extra addresses as
# further arguments, e.g.:
#   ./scripts/generate-gateway-certs.sh config/certs IP:192.168.1.1 DNS:gateway

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="${1:-$REPO_ROOT/config/certs}"
shift || true
# Every address is listed twice, as IP and as DNS. The devices run mbedTLS,
# whose name matching only looks at dNSName entries and ignores iPAddress ones,
# and Zephyr sets the verified name to the empty string when none is configured
# precisely so that verification cannot silently skip it. A device dialling an
# IP literal therefore only accepts the certificate if that literal appears as a
# dNSName. Browsers and OpenSSL use the IP entries.
SAN="DNS:localhost,IP:127.0.0.1,DNS:127.0.0.1"
# The gateway endpoint Renode writes into device memory (see
# DEVICE_ENDPOINT_ADDR in wasmbed_protocol.c) defaults to this address.
SAN="$SAN,IP:192.168.1.1,DNS:192.168.1.1"
for extra in "$@"; do
  SAN="$SAN,$extra"
  case "$extra" in
    IP:*) SAN="$SAN,DNS:${extra#IP:}" ;;
  esac
done
mkdir -p "$OUT_DIR"
cd "$OUT_DIR"

echo "Generating certificates in $OUT_DIR"

# CA (used as client CA: devices must present certs signed by this CA)
openssl req -x509 -newkey rsa:2048 -keyout ca-key.pem -out ca-cert.pem \
  -days 365 -nodes -subj "/CN=Wasmbed-Test-CA"

# Server key and cert (Gateway TLS server) - X.509 v3 required by rustls
openssl genpkey -algorithm RSA -out server-key.pem -pkeyopt rsa_keygen_bits:2048
openssl req -new -key server-key.pem -out server.csr -subj "/CN=localhost/O=Wasmbed-Gateway"
# v3 extensions for TLS server (rustls requires v3)
echo -e "[v3_server]\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=$SAN" > v3.ext
openssl x509 -req -in server.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial \
  -out server-cert.pem -days 365 -sha256 -extfile v3.ext -extensions v3_server
rm -f server.csr v3.ext

# client_ca for Gateway = same CA (devices use certs signed by this CA)
cp ca-cert.pem client-ca.pem

echo "Server certificate SANs: $SAN"
echo "Done. Use:"
echo "  --private-key $OUT_DIR/server-key.pem"
echo "  --certificate $OUT_DIR/server-cert.pem"
echo "  --client-ca    $OUT_DIR/client-ca.pem"
