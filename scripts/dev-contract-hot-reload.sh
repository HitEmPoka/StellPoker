#!/usr/bin/env bash
set -euo pipefail

: "${CONTRACT_GLOB:=contracts/*}"
: "${POLL_SECONDS:=2}"
: "${STATE_FILE:=.tmp/contract-hot-reload-state}"
: "${HOT_RELOAD_DEPLOY:=0}"
: "${NETWORK:=local}"
: "${DEPLOY_OUTPUT:=.tmp/contract-hot-reload-deployments.env}"

mkdir -p "$(dirname "$STATE_FILE")" "$(dirname "$DEPLOY_OUTPUT")"
touch "$STATE_FILE" "$DEPLOY_OUTPUT"

fingerprint_contract() {
  local path="$1"
  find "$path" -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -print0 \
    | sort -z \
    | xargs -0 shasum 2>/dev/null \
    | shasum \
    | awk '{print $1}'
}

build_contract() {
  local path="$1"
  local crate
  crate="$(basename "$path")"
  printf "\n[hot-reload] building %s\n" "$crate"
  cargo build -p "$crate" --target wasm32-unknown-unknown --release

  if [ "$HOT_RELOAD_DEPLOY" = "1" ]; then
    local wasm="target/wasm32-unknown-unknown/release/${crate//-/_}.wasm"
    if [ ! -f "$wasm" ]; then
      printf "[hot-reload] built %s but could not find %s; skipping deploy\n" "$crate" "$wasm"
      return 0
    fi
    printf "[hot-reload] deploying %s to %s\n" "$crate" "$NETWORK"
    local contract_id
    contract_id="$(stellar contract deploy --wasm "$wasm" --network "$NETWORK")"
    local env_name
    env_name="$(printf '%s_CONTRACT_ID' "$crate" | tr '[:lower:]-' '[:upper:]_')"
    grep -v "^${env_name}=" "$DEPLOY_OUTPUT" > "${DEPLOY_OUTPUT}.tmp" || true
    printf "%s=%s\n" "$env_name" "$contract_id" >> "${DEPLOY_OUTPUT}.tmp"
    mv "${DEPLOY_OUTPUT}.tmp" "$DEPLOY_OUTPUT"
    printf "[hot-reload] wrote %s to %s\n" "$env_name" "$DEPLOY_OUTPUT"
  fi
}

printf "StellPoker contract hot reload\n"
printf "Watching %s every %ss. Set HOT_RELOAD_DEPLOY=1 to deploy after build.\n" "$CONTRACT_GLOB" "$POLL_SECONDS"

while true; do
  for contract_path in $CONTRACT_GLOB; do
    [ -d "$contract_path/src" ] || continue
    name="$(basename "$contract_path")"
    next="$(fingerprint_contract "$contract_path")"
    previous="$(grep "^${name}=" "$STATE_FILE" | tail -n1 | cut -d= -f2- || true)"
    if [ "$next" != "$previous" ]; then
      build_contract "$contract_path"
      grep -v "^${name}=" "$STATE_FILE" > "${STATE_FILE}.tmp" || true
      printf "%s=%s\n" "$name" "$next" >> "${STATE_FILE}.tmp"
      mv "${STATE_FILE}.tmp" "$STATE_FILE"
    fi
  done
  sleep "$POLL_SECONDS"
done