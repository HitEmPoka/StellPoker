#!/usr/bin/env bash
set -euo pipefail

# Stellar Poker - Contract Upgrade Script
#
# Upgrades a deployed Soroban contract following the path declared for it in
# scripts/migrations.json. Currently only `poker-table` supports an on-chain
# upgrade (a timelocked propose/execute flow with a fast rollback window) —
# see contracts/poker-table/src/lib.rs's propose_upgrade/execute_upgrade/
# revert_last_upgrade. Other contracts require a redeploy; this script exits
# with guidance rather than pretending to upgrade them.
#
# Usage:
#   ./scripts/upgrade.sh propose <contract-key> <table-id> <new-wasm-path> [delay-seconds]
#   ./scripts/upgrade.sh execute <contract-key> <table-id>
#   ./scripts/upgrade.sh rollback <contract-key> <table-id>
#
# Environment variables:
#   NETWORK          Target network: "testnet" (default), "staging", or "mainnet" — same as deploy.sh
#   CONTRACT_ID      The deployed contract's address (required — read it from your deploy.sh
#                    OUTPUT_ENV_FILE, e.g. POKER_TABLE_CONTRACT)
#   SOURCE_IDENTITY  `stellar` CLI identity to sign with (default: deployer)
#
# Examples:
#   NETWORK=testnet CONTRACT_ID=C... \
#     ./scripts/upgrade.sh propose poker-table 1 target/wasm32-unknown-unknown/release/poker_table.wasm 90000
#
#   # ... wait for the delay to elapse ...
#   NETWORK=testnet CONTRACT_ID=C... ./scripts/upgrade.sh execute poker-table 1
#
#   # If the new version misbehaves, within the rollback window:
#   NETWORK=testnet CONTRACT_ID=C... ./scripts/upgrade.sh rollback poker-table 1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST="${SCRIPT_DIR}/migrations.json"

NETWORK="${NETWORK:-testnet}"
SOURCE_IDENTITY="${SOURCE_IDENTITY:-deployer}"

if [[ "$NETWORK" == "mainnet" ]]; then
  STELLAR_NETWORK="mainnet"
elif [[ "$NETWORK" == "staging" ]]; then
  STELLAR_NETWORK="testnet"
else
  STELLAR_NETWORK="testnet"
fi

usage() {
  echo "Usage:"
  echo "  $0 propose <contract-key> <table-id> <new-wasm-path> [delay-seconds]"
  echo "  $0 execute <contract-key> <table-id>"
  echo "  $0 rollback <contract-key> <table-id>"
  exit 1
}

command -v jq >/dev/null 2>&1 || { echo "jq is required (brew install jq / apt install jq)"; exit 1; }
command -v stellar >/dev/null 2>&1 || { echo "stellar CLI not found. Install: cargo install stellar-cli"; exit 1; }

ACTION="${1:-}"
CONTRACT_KEY="${2:-}"
[[ -n "$ACTION" && -n "$CONTRACT_KEY" ]] || usage

ENTRY="$(jq -c --arg key "$CONTRACT_KEY" '.contracts[] | select(.key == $key)' "$MANIFEST")"
if [[ -z "$ENTRY" ]]; then
  echo "ERROR: no entry for contract key '$CONTRACT_KEY' in $MANIFEST"
  echo "Known keys:"
  jq -r '.contracts[].key' "$MANIFEST" | sed 's/^/  - /'
  exit 1
fi

SUPPORTED="$(echo "$ENTRY" | jq -r '.upgrade.supported')"
if [[ "$SUPPORTED" != "true" ]]; then
  NOTE="$(echo "$ENTRY" | jq -r '.upgrade.note // "No upgrade path defined."')"
  echo "ERROR: '$CONTRACT_KEY' does not support an on-chain upgrade."
  echo "       $NOTE"
  exit 1
fi

: "${CONTRACT_ID:?CONTRACT_ID is required (the deployed address of $CONTRACT_KEY)}"

case "$ACTION" in
  propose)
    TABLE_ID="${3:-}"
    WASM_PATH="${4:-}"
    DELAY_SECONDS="${5:-$(echo "$ENTRY" | jq -r '.upgrade.minDelaySeconds')}"
    [[ -n "$TABLE_ID" && -n "$WASM_PATH" ]] || usage
    [[ -f "$WASM_PATH" ]] || { echo "ERROR: wasm file not found: $WASM_PATH"; exit 1; }

    MIN_DELAY="$(echo "$ENTRY" | jq -r '.upgrade.minDelaySeconds')"
    if (( DELAY_SECONDS < MIN_DELAY )); then
      echo "ERROR: delay-seconds ($DELAY_SECONDS) is below the contract's minimum ($MIN_DELAY)."
      exit 1
    fi

    echo "=== Installing new WASM for $CONTRACT_KEY ==="
    NEW_WASM_HASH=$(stellar contract install \
      --wasm "$WASM_PATH" \
      --source "$SOURCE_IDENTITY" \
      --network "$STELLAR_NETWORK" 2>/dev/null)
    echo "New WASM hash: $NEW_WASM_HASH"

    PROPOSE_FN="$(echo "$ENTRY" | jq -r '.upgrade.proposeFunction')"
    echo ""
    echo "=== Proposing upgrade (table $TABLE_ID, delay ${DELAY_SECONDS}s) ==="
    stellar contract invoke \
      --id "$CONTRACT_ID" \
      --source "$SOURCE_IDENTITY" \
      --network "$STELLAR_NETWORK" \
      -- "$PROPOSE_FN" \
      --table_id "$TABLE_ID" \
      --new_wasm_hash "$NEW_WASM_HASH" \
      --delay_seconds "$DELAY_SECONDS"

    echo ""
    echo "Proposed. Execute after ~${DELAY_SECONDS}s with:"
    echo "  NETWORK=$NETWORK CONTRACT_ID=$CONTRACT_ID $0 execute $CONTRACT_KEY $TABLE_ID"
    ;;

  execute)
    TABLE_ID="${3:-}"
    [[ -n "$TABLE_ID" ]] || usage
    EXECUTE_FN="$(echo "$ENTRY" | jq -r '.upgrade.executeFunction')"

    echo "=== Executing upgrade (table $TABLE_ID) ==="
    stellar contract invoke \
      --id "$CONTRACT_ID" \
      --source "$SOURCE_IDENTITY" \
      --network "$STELLAR_NETWORK" \
      -- "$EXECUTE_FN" \
      --table_id "$TABLE_ID"

    ROLLBACK_WINDOW="$(echo "$ENTRY" | jq -r '.upgrade.rollback.windowSeconds')"
    echo ""
    echo "Executed. If this needs to be undone, you have ${ROLLBACK_WINDOW}s to run:"
    echo "  NETWORK=$NETWORK CONTRACT_ID=$CONTRACT_ID $0 rollback $CONTRACT_KEY $TABLE_ID"
    ;;

  rollback)
    TABLE_ID="${3:-}"
    [[ -n "$TABLE_ID" ]] || usage
    ROLLBACK_FN="$(echo "$ENTRY" | jq -r '.upgrade.rollback.function')"

    echo "=== Rolling back to the previous WASM (table $TABLE_ID) ==="
    echo "This only reverts the single most recently executed upgrade, and only"
    echo "within the rollback window — see scripts/migrations.json."
    stellar contract invoke \
      --id "$CONTRACT_ID" \
      --source "$SOURCE_IDENTITY" \
      --network "$STELLAR_NETWORK" \
      -- "$ROLLBACK_FN" \
      --table_id "$TABLE_ID"

    echo ""
    echo "Rolled back."
    ;;

  *)
    usage
    ;;
esac
