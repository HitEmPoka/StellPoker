#!/usr/bin/env bash
set -euo pipefail

# Stellar Poker - Nix flake / dev container parity check (Issue #671)
#
# flake.nix and .devcontainer/ describe the same toolchain in two formats.
# This fails when the pinned versions or the tool list drift apart, so a change
# to one has to be mirrored in the other.
#
#   bash scripts/check-devenv-parity.sh

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FLAKE="$ROOT/flake.nix"
DOCKERFILE="$ROOT/.devcontainer/Dockerfile"
DEVCONTAINER="$ROOT/.devcontainer/devcontainer.json"

fail=0
err() { echo "PARITY: $*" >&2; fail=1; }

# ── Noir version ─────────────────────────────────────────────────────────────
flake_noir="$(sed -nE 's/^[[:space:]]*noirVersion = "([^"]+)";.*/\1/p' "$FLAKE")"
docker_noir="$(sed -nE 's/^ARG NOIR_VERSION=(.+)$/\1/p' "$DOCKERFILE")"
json_noir="$(sed -nE 's/.*"NOIR_VERSION": "([^"]+)".*/\1/p' "$DEVCONTAINER")"

[ -n "$flake_noir" ] || err "could not read noirVersion from flake.nix"
[ "$flake_noir" = "$docker_noir" ] \
  || err "Noir: flake.nix=$flake_noir, .devcontainer/Dockerfile=$docker_noir"
[ "$flake_noir" = "$json_noir" ] \
  || err "Noir: flake.nix=$flake_noir, devcontainer.json=$json_noir"

# ── Node major version ───────────────────────────────────────────────────────
flake_node="$(grep -oE 'nodejs_[0-9]+' "$FLAKE" | head -n1 | sed 's/nodejs_//')"
json_node="$(grep -A2 'features/node:' "$DEVCONTAINER" | sed -nE 's/.*"version": "([0-9]+)".*/\1/p')"
[ "$flake_node" = "$json_node" ] \
  || err "Node: flake.nix=$flake_node, devcontainer.json=$json_node"

# ── Rust toolchain shape ─────────────────────────────────────────────────────
for item in rust-src rustfmt clippy wasm32-unknown-unknown; do
  grep -q "\"$item\"" "$FLAKE" || err "flake.nix no longer lists $item; update this check"
  sed -E 's/^[[:space:]]*#.*//' "$DOCKERFILE" | grep -q -- "$item" || err "$item is in flake.nix but not .devcontainer/Dockerfile"
done

# ── co-noir source ───────────────────────────────────────────────────────────
grep -q 'TaceoLabs/co-snarks --branch main co-noir' "$FLAKE" \
  || err "flake.nix co-noir source changed; update .devcontainer/Dockerfile and this check"
sed -E 's/^[[:space:]]*#.*//' "$DOCKERFILE" | grep -q 'TaceoLabs/co-snarks --branch main co-noir' \
  || err ".devcontainer/Dockerfile co-noir source differs from flake.nix"

# ── System packages ──────────────────────────────────────────────────────────
# flake.nix name -> name expected in the dev container (Dockerfile or features).
declare -A mapping=(
  [openssl]=openssl
  [curl]=curl
  [jq]=jq
  [git]=git
  [bash]=bash
  [pkg-config]=pkg-config
  [cmake]=cmake
  [clang]=clang
  [requests]=python3-requests
  [pynacl]=python3-nacl
  [stellar-cli]=stellar-cli
  [docker-compose]=docker-outside-of-docker
)

flake_pkgs="$(sed -n '/packages = with pkgs; \[/,/^[[:space:]]*\]/p' "$FLAKE" \
  | sed 's/#.*//' | grep -oE '[A-Za-z][A-Za-z0-9_-]*' \
  | grep -vE '^(packages|with|pkgs|rustToolchain|nodejs_[0-9]+|python3|withPackages|ps)$' || true)"
flake_pkgs="$flake_pkgs"$'\n'"$(grep -oE 'pkgs\.stellar-cli' "$FLAKE" | head -n1 | sed 's/pkgs\.//')"

# Match against comment-free content so the mapping table in the Dockerfile
# header cannot mask a removed package.
container_src="$(sed -E 's/^[[:space:]]*#.*//' "$DOCKERFILE"; sed -E 's#^[[:space:]]*//.*##' "$DEVCONTAINER")"

while read -r pkg; do
  [ -n "$pkg" ] || continue
  target="${mapping[$pkg]:-}"
  if [ -z "$target" ]; then
    err "flake.nix package '$pkg' has no dev container mapping; add it to .devcontainer and to this script"
    continue
  fi
  grep -qw -- "$target" <<< "$container_src" \
    || err "flake.nix package '$pkg' missing from dev container (expected '$target')"
done <<< "$flake_pkgs"

if [ "$fail" -ne 0 ]; then
  echo "flake.nix and .devcontainer/ have drifted; see messages above." >&2
  exit 1
fi
echo "flake.nix and .devcontainer/ are in parity (Noir $flake_noir, Node $flake_node)."
