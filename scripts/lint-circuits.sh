#!/usr/bin/env bash
# Format and lint gate for all Noir circuits in the workspace (Issue #887).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

echo "Checking Noir formatting (nargo fmt --check)..."
nargo fmt --check --program-dir circuits/lib
for manifest in circuits/*/Nargo.toml; do
    circuit="$(dirname "${manifest}")"
    [ "${circuit}" = "circuits/lib" ] && continue
    echo "  fmt check: ${circuit}"
    nargo fmt --check --program-dir "${circuit}"
done

echo "Checking Noir lints with warnings-as-errors (nargo check --deny-warnings)..."
nargo check --deny-warnings --program-dir circuits/lib
for manifest in circuits/*/Nargo.toml; do
    circuit="$(dirname "${manifest}")"
    [ "${circuit}" = "circuits/lib" ] && continue
    echo "  lint check: ${circuit}"
    nargo check --deny-warnings --program-dir "${circuit}"
done

echo "All Noir circuits passed formatting and lint checks."
