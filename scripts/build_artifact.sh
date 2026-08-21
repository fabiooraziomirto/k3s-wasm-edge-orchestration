#!/usr/bin/env bash
# Assemble the code-and-data availability package.
#
# The Data Availability Statement promises that the records behind every number
# in the evaluation section are in the repository. This collects them in one
# place, next to a README that maps each published number to the file that
# backs it, so the claim can be checked rather than taken on trust.
#
# Usage: ./scripts/build_artifact.sh <campaign-dir> [output-dir]
#   campaign-dir  an experiments/<timestamp> directory
#   output-dir    default: artifact/

set -euo pipefail

CAMPAIGN="${1:?usage: build_artifact.sh <campaign-dir> [output-dir]}"
OUT="${2:-artifact}"

[ -d "$CAMPAIGN/summary" ] || { echo "no summary/ in $CAMPAIGN" >&2; exit 66; }

rm -rf "$OUT/campaign"
mkdir -p "$OUT/campaign" "$OUT/firmware" "$OUT/security"

cp -r "$CAMPAIGN/raw" "$CAMPAIGN/summary" "$CAMPAIGN/environment" "$OUT/campaign/"
[ -d "$CAMPAIGN/figures" ] && cp -r "$CAMPAIGN/figures" "$OUT/campaign/"

# The revision the package describes, recorded even when the tree is dirty so
# that a mismatch is visible rather than hidden.
{
  echo "commit: $(git rev-parse HEAD)"
  echo "describe: $(git describe --tags --always --dirty 2>/dev/null || echo none)"
  echo "assembled: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
  echo "--- uncommitted changes at assembly time ---"
  git status --short || true
} > "$OUT/PROVENANCE.txt"

echo "Wrote $OUT/ from $CAMPAIGN"
echo "Still to be placed by hand (they come from the lab host):"
echo "  $OUT/firmware/footprint-authenticated.txt"
echo "  $OUT/firmware/footprint-baseline.txt"
echo "  $OUT/security/proof-of-possession-probe.txt"
echo "  $OUT/security/tls-negative-tests.txt"
echo "  $OUT/enrollment-timings.txt"
