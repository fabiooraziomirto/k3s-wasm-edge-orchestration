#!/usr/bin/env bash
# Generate TLS certificates for wasmbed-gateway (test/local use).
# Usage: ./scripts/generate-gateway-certs.sh [output_dir]
# Default output_dir: config/certs (relative to repo root).

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="${1:-$REPO_ROOT/config/certs}"
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
echo -e "[v3_server]\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1" > v3.ext
openssl x509 -req -in server.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial \
  -out server-cert.pem -days 365 -sha256 -extfile v3.ext -extensions v3_server
rm -f server.csr v3.ext

# client_ca for Gateway = same CA (devices use certs signed by this CA)
cp ca-cert.pem client-ca.pem

echo "Done. Use:"
echo "  --private-key $OUT_DIR/server-key.pem"
echo "  --certificate $OUT_DIR/server-cert.pem"
echo "  --client-ca    $OUT_DIR/client-ca.pem"
