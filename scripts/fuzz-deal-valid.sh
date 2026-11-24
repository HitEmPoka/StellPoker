#!/usr/bin/env bash
# Run the deterministic deal-valid witness fuzzer (Issue #888).
#
# Usage: ./scripts/fuzz-deal-valid.sh [--runs N] [--seed N]
#
# Requires nargo on PATH. The circuit's public return value is 0 when all
# cases matched expectations; otherwise it is failing_seed + 1. The fuzz
# library also prints the exact failing seed to stdout before returning.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
CIRCUIT_DIR="${PROJECT_DIR}/circuits/deal_valid_fuzz"

if ! command -v nargo >/dev/null 2>&1; then
    echo "ERROR: nargo is required to run the deal-valid fuzzer" >&2
    exit 127
fi

echo "Running deterministic deal_valid witness fuzz (10,000 cases)..."
OUTPUT="$(cd "${CIRCUIT_DIR}" && nargo execute 2>&1)"
echo "${OUTPUT}"

# `nargo execute` prints witness values including the return value. A mismatch
# is also signalled by the circuit itself; check both so the job fails loudly
# even if the output format changes between nargo versions.
if echo "${OUTPUT}" | grep -q "fuzz mismatch"; then
    echo "ERROR: deal_valid fuzz found an invalid witness acceptance (see seed above)" >&2
    exit 1
fi

if echo "${OUTPUT}" | grep -Eq '"return_value"[[:space:]]*:[[:space:]]*"[1-9][0-9]*"'; then
    echo "ERROR: deal_valid fuzz returned a non-zero failing seed" >&2
    exit 1
fi

echo "deal_valid fuzz: all generated witnesses matched expected validity."
