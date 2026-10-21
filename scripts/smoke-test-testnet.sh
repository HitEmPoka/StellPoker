#!/usr/bin/env bash
set -euo pipefail

TESTNET_RPC="${TESTNET_RPC:-https://soroban-testnet.stellar.org}"
POKER_TABLE_ID="${POKER_TABLE_ID:-}"
GAME_HUB_ID="${GAME_HUB_ID:-}"

if [ -z "$POKER_TABLE_ID" ] || [ -z "$GAME_HUB_ID" ]; then
    echo "Error: POKER_TABLE_ID and GAME_HUB_ID must be set"
    exit 1
fi

echo "Running post-deploy smoke tests against testnet..."
echo "RPC: $TESTNET_RPC"
echo "Poker Table: $POKER_TABLE_ID"
echo "Game Hub: $GAME_HUB_ID"

test_read_state() {
    echo "Test 1: Reading contract state..."
    soroban contract invoke \
        --id "$POKER_TABLE_ID" \
        --network testnet \
        --source-account deployer \
        -- get_table_info
}

test_simulate_buyin() {
    echo "Test 2: Simulating buy-in path..."
    local player="GBXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
    local amount=1000
    
    soroban contract invoke \
        --id "$POKER_TABLE_ID" \
        --network testnet \
        --source-account deployer \
        -- simulate_buy_in \
        --player "$player" \
        --amount "$amount" || {
        echo "Warning: Buy-in simulation returned non-zero"
        return 1
    }
}

test_verify_events() {
    echo "Test 3: Verifying event emission..."
    
    local events=$(soroban events \
        --start-ledger -100 \
        --contract-ids "$POKER_TABLE_ID" \
        --network testnet)
    
    if [ -z "$events" ]; then
        echo "Warning: No events found for contract"
        return 1
    fi
    
    echo "Events found and verified"
}

main() {
    local failed=0
    
    test_read_state || ((failed++))
    test_simulate_buyin || ((failed++))
    test_verify_events || ((failed++))
    
    if [ $failed -eq 0 ]; then
        echo "All smoke tests passed"
        exit 0
    else
        echo "Failed $failed/3 smoke tests"
        exit 1
    fi
}

main
