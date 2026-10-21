#!/usr/bin/env bash
set -uo pipefail

# Developer preflight checks (Issue #680): verifies env, tools, and service
# health before building, with actionable failure messages and fix hints.

REQUIRED_FAILS=0
WARNINGS=0
PASSES=0

ok() {
    PASSES=$((PASSES + 1))
    printf '  [ OK ] %s\n' "$1"
}

warn() {
    WARNINGS=$((WARNINGS + 1))
    printf '  [WARN] %s\n' "$1"
    printf '         Fix: %s\n' "$2"
}

fail() {
    REQUIRED_FAILS=$((REQUIRED_FAILS + 1))
    printf '  [FAIL] %s\n' "$1"
    printf '         Fix: %s\n' "$2"
}

check_required_cmd() {
    local name="$1" cmd="$2" fix="$3" version_args="${4:---version}"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        fail "$name not found ('${cmd}' is not on PATH)" "$fix"
        return
    fi
    local version rc
    version="$($cmd $version_args 2>&1)"
    rc=$?
    if [ "$rc" -ne 0 ]; then
        fail "$name present but '$cmd $version_args' failed: $(printf '%s' "$version" | head -n 1)" "$fix"
        return
    fi
    ok "$name — $(printf '%s' "$version" | head -n 1)"
}

check_optional_cmd() {
    local name="$1" cmd="$2" fix="$3" version_args="${4:---version}"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        warn "$name not found (optional for some workflows)" "$fix"
        return
    fi
    local version rc
    version="$($cmd $version_args 2>&1)"
    rc=$?
    if [ "$rc" -ne 0 ]; then
        warn "$name present but '$cmd $version_args' failed: $(printf '%s' "$version" | head -n 1)" "$fix"
        return
    fi
    ok "$name — $(printf '%s' "$version" | head -n 1)"
}

echo "=== StellPoker preflight ==="
echo ""
echo "Tools (required):"

check_required_cmd "git" git \
    "install git, e.g. 'sudo apt-get install -y git' or 'brew install git'"
check_required_cmd "rustc" rustc \
    "install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
check_required_cmd "cargo" cargo \
    "install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"

if command -v rustup >/dev/null 2>&1; then
    if rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
        ok "wasm32-unknown-unknown target installed"
    else
        fail "wasm32-unknown-unknown target missing (contract builds need it)" \
            "rustup target add wasm32-unknown-unknown"
    fi
fi

check_required_cmd "node" node \
    "install Node.js 18+: https://nodejs.org (or 'nvm install 20')"
check_required_cmd "npm" npm \
    "npm ships with Node.js — reinstall Node.js from https://nodejs.org"
check_required_cmd "python3" python3 \
    "install Python 3, e.g. 'sudo apt-get install -y python3' or 'brew install python'"
check_required_cmd "docker" docker \
    "install Docker: https://docs.docker.com/get-docker/"
check_required_cmd "docker compose" docker \
    "install the Compose plugin: https://docs.docker.com/compose/install/" "compose version"

if command -v docker >/dev/null 2>&1; then
    if docker info >/dev/null 2>&1; then
        ok "docker daemon reachable"
    else
        fail "docker daemon not running" \
            "start it: 'sudo systemctl start docker' (or launch Docker Desktop)"
    fi
fi

echo ""
echo "Tools (optional — needed for circuits / contracts / MPC):"

check_optional_cmd "nargo" nargo \
    "curl -L https://raw.githubusercontent.com/noir-lang/noirup/refs/heads/main/install | bash && noirup -v 1.0.0-beta.17"
check_optional_cmd "stellar CLI" stellar \
    "cargo install stellar-cli --features opt"
check_optional_cmd "co-noir" co-noir \
    "cargo install --git https://github.com/TaceoLabs/co-snarks --branch main co-noir"

echo ""
echo "Environment:"

if [ -f .env ]; then
    env_keys="$(grep -c '=' .env 2>/dev/null || true)"
    if [ "${env_keys}" -gt 0 ]; then
        ok ".env present (${env_keys} variables)"
    else
        fail ".env exists but has no KEY=value entries" \
            "recopy it: cp .env.example .env"
    fi
else
    fail ".env not found (required by the local stack)" \
        "cp .env.example .env"
fi

if [ -d crs ] && [ -n "$(ls -A crs 2>/dev/null)" ]; then
    ok "crs/ present (BN254 common reference string)"
else
    warn "crs/ missing or empty (needed for MPC proving, not for build/test)" \
        "./scripts/download-crs.sh"
fi

echo ""
echo "Services:"

stack_containers="$(docker compose ps -q 2>/dev/null || true)"
if [ -n "${stack_containers}" ]; then
    check_health() {
        local name="$1" url="$2"
        if curl -sfL --max-time 5 "$url" >/dev/null 2>&1; then
            ok "$name healthy ($url)"
        else
            warn "$name not responding at $url" \
                "check logs: 'docker compose logs $name' (stack: 'make start')"
        fi
    }
    check_health "coordinator" "http://localhost:8080/api/health"
    check_health "mpc-node-0" "http://localhost:8101/health"
    check_health "mpc-node-1" "http://localhost:8102/health"
    check_health "mpc-node-2" "http://localhost:8103/health"
else
    echo "  [SKIP] local stack not running (service health checks skipped)"
    echo "         Start it with: make start"
fi

echo ""
echo "Summary: ${PASSES} ok, ${REQUIRED_FAILS} failed, ${WARNINGS} warning(s)"

if [ "${REQUIRED_FAILS}" -gt 0 ]; then
    echo "Preflight FAILED — apply the fixes above, then re-run: make preflight"
    exit 1
fi

echo "Preflight passed."
