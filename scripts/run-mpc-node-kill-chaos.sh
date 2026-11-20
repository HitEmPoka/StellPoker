#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
COORDINATOR_URL="${COORDINATOR_URL:?COORDINATOR_URL must point at the staging coordinator}"
TABLE_ID="${TABLE_ID:?TABLE_ID must identify an isolated staging table}"
COMPOSE_FILE="${COMPOSE_FILE:-${PROJECT_DIR}/docker-compose.staging.yml}"
MPC_NODE_SERVICE="${MPC_NODE_SERVICE:-mpc-node-1}"
WARMUP_SECONDS="${WARMUP_SECONDS:-5}"
RECOVERY_TIMEOUT_SECONDS="${RECOVERY_TIMEOUT_SECONDS:-90}"
DURATION="${DURATION:-2m}"
VUS="${VUS:-3}"
K6_IMAGE="${K6_IMAGE:-grafana/k6:latest}"
RESULTS_DIR="${RESULTS_DIR:-${PROJECT_DIR}/load-testing/results}"
AUTH_HEADERS_JSON="${AUTH_HEADERS_JSON:-{}}"
CONTAINER_NAME="stellpoker-mpc-kill-${GITHUB_RUN_ID:-local}-$$"

mkdir -p "$RESULTS_DIR"
BASELINE_STATE="${RESULTS_DIR}/mpc-node-kill-baseline-state.json"
BASELINE_HISTORY="${RESULTS_DIR}/mpc-node-kill-baseline-history.json"
FINAL_STATE="${RESULTS_DIR}/mpc-node-kill-final-state.json"
FINAL_HISTORY="${RESULTS_DIR}/mpc-node-kill-final-history.json"
K6_LOG="${RESULTS_DIR}/mpc-node-kill-chaos-k6.log"
SUMMARY_FILE="${RESULTS_DIR}/mpc-node-kill-chaos-summary.json"

cleanup() {
    docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
    docker compose -f "$COMPOSE_FILE" up -d --no-deps "$MPC_NODE_SERVICE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

curl_json() {
    local url="$1"
    local output="$2"
    curl --fail --silent --show-error "$url" > "$output"
}

wait_for_coordinator() {
    local deadline=$((SECONDS + RECOVERY_TIMEOUT_SECONDS))
    until curl --fail --silent --show-error "${COORDINATOR_URL%/}/api/health" >/dev/null; do
        if (( SECONDS >= deadline )); then
            return 1
        fi
        sleep 2
    done
}

healthy_node_count() {
    curl --fail --silent --show-error "${COORDINATOR_URL%/}/api/committee/status" | python3 -c '
import json
import sys
payload = json.load(sys.stdin)
healthy = payload.get("healthy", [])
print(sum(1 for value in healthy if value is True))
'
}

wait_for_committee_recovery() {
    local deadline=$((SECONDS + RECOVERY_TIMEOUT_SECONDS))
    local healthy
    while (( SECONDS < deadline )); do
        healthy="$(healthy_node_count || true)"
        if [[ "$healthy" =~ ^[0-9]+$ ]] && (( healthy >= 2 )); then
            return 0
        fi
        sleep 2
    done
    return 1
}

wait_for_coordinator

curl_json "${COORDINATOR_URL%/}/api/table/${TABLE_ID}/state" "$BASELINE_STATE"
curl_json "${COORDINATOR_URL%/}/api/table/${TABLE_ID}/hand-history/chunk?offset=0&limit=5" "$BASELINE_HISTORY"

docker run --detach --name "$CONTAINER_NAME" --network host \
    -e BASE_URL="$COORDINATOR_URL" -e TABLE_ID="$TABLE_ID" -e VUS="$VUS" \
    -e DURATION="$DURATION" -e AUTH_HEADERS_JSON="$AUTH_HEADERS_JSON" \
    -v "${PROJECT_DIR}/load-testing/k6:/scripts:ro" \
    -v "${RESULTS_DIR}:/results" \
    "$K6_IMAGE" run /scripts/mpc-node-kill-chaos.js > /dev/null

sleep "$WARMUP_SECONDS"

kill_started_at="$(date +%s%3N)"
docker compose -f "$COMPOSE_FILE" kill -s KILL "$MPC_NODE_SERVICE"

if wait_for_committee_recovery; then
    recovered_at="$(date +%s%3N)"
    recovery_time_ms=$((recovered_at - kill_started_at))
    recovery_status="recovered"
else
    recovered_at=""
    recovery_time_ms=""
    recovery_status="safe_abort"
fi

set +e
docker wait "$CONTAINER_NAME" >/dev/null
k6_exit=$?
docker logs "$CONTAINER_NAME" > "$K6_LOG" 2>&1
set -e

curl_json "${COORDINATOR_URL%/}/api/table/${TABLE_ID}/state" "$FINAL_STATE"
curl_json "${COORDINATOR_URL%/}/api/table/${TABLE_ID}/hand-history/chunk?offset=0&limit=5" "$FINAL_HISTORY"

python3 - "$SUMMARY_FILE" "$recovery_status" "$recovery_time_ms" "$k6_exit" \
    "$BASELINE_HISTORY" "$FINAL_HISTORY" "$BASELINE_STATE" "$FINAL_STATE" <<'PY'
import json
import sys
from pathlib import Path

summary_path, recovery_status, recovery_ms, k6_exit, baseline_history, final_history, baseline_state, final_state = sys.argv[1:]

def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))

before = read(baseline_history)
after = read(final_history)
before_state = read(baseline_state)
after_state = read(final_state)
phase = str(after_state.get("state", {}).get("phase", "")).lower()
settlement_occurred = (
    before.get("total") != after.get("total")
    or phase in {"settlement", "settled"}
)
payload = {
    "recovery_status": recovery_status,
    "recovery_time_ms": int(recovery_ms) if recovery_ms else None,
    "k6_exit_code": int(k6_exit),
    "settlement_occurred": settlement_occurred,
    "safe": int(k6_exit) == 0 and not settlement_occurred,
}
Path(summary_path).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(json.dumps(payload, indent=2))
PY

if ! python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1]))["safe"] else 1)' "$SUMMARY_FILE"; then
    echo "MPC node-kill chaos failed; inspect $SUMMARY_FILE and $K6_LOG." >&2
    exit 1
fi
