#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
COORDINATOR_URL="${COORDINATOR_URL:?COORDINATOR_URL must point at the staging coordinator}"
KUBE_NAMESPACE="${KUBE_NAMESPACE:-stellpoker-staging}"
COORDINATOR_DEPLOYMENT="${COORDINATOR_DEPLOYMENT:-coordinator}"
WARMUP_SECONDS="${WARMUP_SECONDS:-20}"
RECOVERY_TIMEOUT_SECONDS="${RECOVERY_TIMEOUT_SECONDS:-90}"
VUS="${VUS:-25}"
DURATION="${DURATION:-2m}"
TABLE_COUNT="${TABLE_COUNT:-1000}"
K6_IMAGE="${K6_IMAGE:-grafana/k6:latest}"
RESULTS_DIR="${RESULTS_DIR:-${PROJECT_DIR}/load-testing/results}"
CONTAINER_NAME="stellpoker-chaos-${GITHUB_RUN_ID:-local}-$$"

mkdir -p "$RESULTS_DIR"
SUMMARY_FILE="${RESULTS_DIR}/coordinator-restart-chaos-summary.json"
K6_LOG="${RESULTS_DIR}/coordinator-restart-chaos-k6.log"

cleanup() {
    docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

wait_for_health() {
    local deadline=$((SECONDS + RECOVERY_TIMEOUT_SECONDS))
    until curl --fail --silent --show-error "${COORDINATOR_URL%/}/api/health" >/dev/null; do
        if (( SECONDS >= deadline )); then
            return 1
        fi
        sleep 2
    done
}

echo "Checking staging coordinator before load..."
wait_for_health

echo "Starting active load for ${WARMUP_SECONDS}s before the restart."
docker run --detach --name "$CONTAINER_NAME" --network host \
    -e BASE_URL="$COORDINATOR_URL" -e VUS="$VUS" -e DURATION="$DURATION" \
    -e TABLE_COUNT="$TABLE_COUNT" \
    -v "${PROJECT_DIR}/load-testing/k6:/scripts:ro" \
    "$K6_IMAGE" run /scripts/coordinator-restart-chaos.js > /dev/null
sleep "$WARMUP_SECONDS"

restart_started_at="$(date +%s%3N)"
echo "Restarting deployment ${KUBE_NAMESPACE}/${COORDINATOR_DEPLOYMENT}."
kubectl -n "$KUBE_NAMESPACE" rollout restart "deployment/${COORDINATOR_DEPLOYMENT}"
kubectl -n "$KUBE_NAMESPACE" rollout status \
    "deployment/${COORDINATOR_DEPLOYMENT}" \
    --timeout="${RECOVERY_TIMEOUT_SECONDS}s"

if wait_for_health; then
    recovered_at="$(date +%s%3N)"
    recovery_time_ms=$((recovered_at - restart_started_at))
    recovery_status="recovered"
else
    recovered_at=""
    recovery_time_ms=""
    recovery_status="timed_out"
fi

set +e
docker wait "$CONTAINER_NAME" >/dev/null
k6_exit=$?
docker logs "$CONTAINER_NAME" | tee "$K6_LOG"
set -e

python3 - "$SUMMARY_FILE" "$recovery_status" "$recovery_time_ms" "$k6_exit" <<'PY'
import json
import sys
from pathlib import Path

path, status, recovery_ms, k6_exit = sys.argv[1:]
payload = {
    "recovery_status": status,
    "recovery_time_ms": int(recovery_ms) if recovery_ms else None,
    "k6_exit_code": int(k6_exit),
}
Path(path).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(json.dumps(payload, indent=2))
PY

if [[ "$recovery_status" != "recovered" || "$k6_exit" -ne 0 ]]; then
    echo "Coordinator restart chaos test failed; inspect $SUMMARY_FILE and $K6_LOG." >&2
    exit 1
fi
