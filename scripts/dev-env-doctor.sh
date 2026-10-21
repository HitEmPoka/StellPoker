#!/usr/bin/env bash
set -uo pipefail

# Stellar Poker - Dev environment doctor (Issue #671)
#
# Reports the toolchain the same way the `nix develop` shell hook does, so a
# dev container / Codespace and a Nix shell can be compared line by line.
# Exits non-zero when a required tool is missing or nargo does not match
# NOIR_VERSION.
#
#   bash scripts/dev-env-doctor.sh

EXPECTED_NOIR_VERSION="${NOIR_VERSION:-1.0.0-beta.17}"
missing=0

check() {
  local name="$1" cmd="$2" version_cmd="$3" hint="$4"
  if command -v "$cmd" >/dev/null 2>&1; then
    printf '  %-14s %s\n' "$name" "$(eval "$version_cmd" 2>/dev/null | head -n1)"
  else
    printf '  %-14s MISSING — %s\n' "$name" "$hint"
    missing=1
  fi
}

echo "Stellar Poker dev environment"
check rust         rustc          "rustc --version"            "install rustup (https://rustup.rs)"
check cargo        cargo          "cargo --version"            "install rustup (https://rustup.rs)"
check node         node           "node --version"             "install Node.js 20"
check python3      python3        "python3 --version"          "install python3"
check docker       docker         "docker --version"           "install Docker"
check stellar      stellar        "stellar --version"          "cargo install stellar-cli --features opt"
check nargo        nargo          "nargo --version"            "noirup -v ${EXPECTED_NOIR_VERSION}"
check co-noir      co-noir        "echo present"               "cargo install --git https://github.com/TaceoLabs/co-snarks --branch main co-noir"
check openssl      openssl        "openssl version"            "install openssl"
check jq           jq             "jq --version"               "install jq"
check pkg-config   pkg-config     "pkg-config --version"       "install pkg-config"
check cmake        cmake          "cmake --version"            "install cmake"
check clang        clang          "clang --version"            "install clang"

if command -v docker >/dev/null 2>&1 && ! docker compose version >/dev/null 2>&1 \
    && ! command -v docker-compose >/dev/null 2>&1; then
  printf '  %-14s %s\n' docker-compose "MISSING — install the compose plugin"
  missing=1
fi

if command -v rustup >/dev/null 2>&1 \
    && ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  printf '  %-14s %s\n' wasm32 "MISSING — rustup target add wasm32-unknown-unknown"
  missing=1
fi

if command -v python3 >/dev/null 2>&1 && ! python3 -c 'import requests, nacl' 2>/dev/null; then
  printf '  %-14s %s\n' "python deps" "MISSING — pip install requests pynacl"
  missing=1
fi

if command -v node >/dev/null 2>&1; then
  node_major="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
  if [ "$node_major" != "20" ]; then
    printf '  %-14s %s\n' node "WARNING — flake pins Node 20, found v${node_major}"
  fi
fi

if command -v nargo >/dev/null 2>&1; then
  if ! nargo --version | grep -q "${EXPECTED_NOIR_VERSION}"; then
    printf '  %-14s %s\n' nargo "WRONG VERSION — expected ${EXPECTED_NOIR_VERSION}; run noirup -v ${EXPECTED_NOIR_VERSION}"
    missing=1
  fi
fi

echo ""
if [ "$missing" -ne 0 ]; then
  echo "Some tools are missing or mismatched (see above)."
  exit 1
fi
echo "Next: ./scripts/setup.sh && ./scripts/download-crs.sh && docker-compose up"
echo "See docs/developer-onboarding.md for the full checklist."
