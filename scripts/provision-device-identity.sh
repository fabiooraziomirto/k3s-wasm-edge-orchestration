#!/usr/bin/env bash
# Provision the cryptographic identity of one or more devices.
#
# Each device gets an ECDSA P-256 key pair and a client certificate signed by
# the fleet CA. The private key and certificate are what RenodeManager injects
# into the emulated board; the SubjectPublicKeyInfo is what goes into
# Device.spec.publicKey, and is the value the gateway looks the device up by.
#
# Usage:
#   ./scripts/provision-device-identity.sh <device-id> [<device-id> ...]
#
# Environment:
#   CERT_DIR    fleet CA location            (default: config/certs)
#   DEVICE_DIR  where identities are written (default: config/devices)
#   NAMESPACE   Kubernetes namespace         (default: wasmbed)
#   APPLY       set to 0 to skip patching the Device CRDs (default: 1)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

CERT_DIR="${CERT_DIR:-$REPO_ROOT/config/certs}"
DEVICE_DIR="${DEVICE_DIR:-$REPO_ROOT/config/devices}"
NAMESPACE="${NAMESPACE:-wasmbed}"
APPLY="${APPLY:-1}"

if [ $# -eq 0 ]; then
  echo "usage: $0 <device-id> [<device-id> ...]" >&2
  exit 64
fi

if [ ! -f "$CERT_DIR/ca-key.pem" ] || [ ! -f "$CERT_DIR/ca-cert.pem" ]; then
  echo "No fleet CA in $CERT_DIR. Run ./scripts/generate-gateway-certs.sh first." >&2
  exit 66
fi

# Prefer whichever build already exists. On the lab host the workspace cannot be
# built with the system cargo (1.75 against a v4 lock file), so the binary comes
# from the same rust:1.88-slim image the Dockerfiles use:
#   docker run --rm -v "$PWD":/src -w /src rust:1.88-slim \
#     cargo build --release -p wasmbed-cert-tool
TOOL=""
for candidate in "$REPO_ROOT/target/release/wasmbed-cert-tool" "$REPO_ROOT/target/debug/wasmbed-cert-tool"; do
  [ -x "$candidate" ] && TOOL="$candidate" && break
done
if [ -z "$TOOL" ]; then
  echo "Building wasmbed-cert-tool..."
  cargo build -q -p wasmbed-cert-tool --manifest-path "$REPO_ROOT/Cargo.toml"
  TOOL="$REPO_ROOT/target/debug/wasmbed-cert-tool"
fi

mkdir -p "$DEVICE_DIR"

# The fleet CA in DER, beside the identities: RenodeManager injects it into
# every board so the device can verify the gateway it is talking to.
openssl x509 -in "$CERT_DIR/ca-cert.pem" -outform DER -out "$DEVICE_DIR/ca.der"
echo "Fleet CA written to $DEVICE_DIR/ca.der"

for device_id in "$@"; do
  spki_b64="$("$TOOL" issue-device \
    --ca-key "$CERT_DIR/ca-key.pem" \
    --ca-cert "$CERT_DIR/ca-cert.pem" \
    --device-id "$device_id" \
    --out-dir "$DEVICE_DIR")"

  echo "$device_id: key, cert and SPKI written to $DEVICE_DIR"
  echo "  spec.publicKey: $spki_b64"

  if [ "$APPLY" = "1" ] && command -v kubectl >/dev/null 2>&1; then
    if kubectl get devices.wasmbed.github.io "$device_id" -n "$NAMESPACE" >/dev/null 2>&1; then
      kubectl patch devices.wasmbed.github.io "$device_id" -n "$NAMESPACE" \
        --type merge -p "{\"spec\":{\"publicKey\":\"$spki_b64\"}}"
      echo "  Device CRD updated"
    else
      # Nothing to patch: the device enrolls in pairing mode and the gateway
      # creates the CRD with this key once it has proved it holds it.
      echo "  No Device CRD named $device_id in namespace $NAMESPACE; not patched"
    fi
  fi
done
