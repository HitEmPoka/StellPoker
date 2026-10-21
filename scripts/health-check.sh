#!/usr/bin/env bash
set -euo pipefail

: "${SOROBAN_RPC:=http://localhost:8000}"
: "${COORDINATOR_URL:=http://localhost:8080}"
: "${MPC_NODE_URLS:=http://localhost:9000,http://localhost:9001,http://localhost:9002}"

red='\033[0;31m'
green='\033[0;32m'
yellow='\033[1;33m'
reset='\033[0m'
failed=0

check_http() {
  local label="$1"
  local url="$2"
  if curl -fsS --max-time 3 "$url" >/dev/null 2>&1; then
    printf "%bOK%b   %s (%s)\n" "$green" "$reset" "$label" "$url"
  else
    printf "%bFAIL%b %s (%s)\n" "$red" "$reset" "$label" "$url"
    failed=1
  fi
}

check_env_contract() {
  local name="$1"
  local value="${!name:-}"
  if [ -n "$value" ]; then
    printf "%bOK%b   %s=%s\n" "$green" "$reset" "$name" "$value"
  else
    printf "%bWARN%b %s is not set\n" "$yellow" "$reset" "$name"
  fi
}

printf "StellPoker local health check\n"
printf "=============================\n"
check_http "Soroban RPC" "$SOROBAN_RPC"
check_http "Coordinator" "$COORDINATOR_URL/health"

IFS=',' read -ra nodes <<< "$MPC_NODE_URLS"
for index in "${!nodes[@]}"; do
  check_http "MPC node $index" "${nodes[$index]}/health"
done

check_env_contract POKER_TABLE_CONTRACT_ID
check_env_contract ZK_VERIFIER_CONTRACT_ID
check_env_contract COMMITTEE_REGISTRY_CONTRACT_ID
check_env_contract GAME_HUB_CONTRACT_ID

if [ "$failed" -ne 0 ]; then
  printf "\n%bLocal environment has failing checks.%b\n" "$red" "$reset"
  exit 1
fi

printf "\n%bLocal environment looks healthy.%b\n" "$green" "$reset"