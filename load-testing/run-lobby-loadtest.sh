#!/usr/bin/env bash
# Run the 5000-table lobby load test against the deterministic fixture
# (issue #650). Set BASE_URL to target a coordinator instead.
set -euo pipefail
cd "$(dirname "$0")"

BASE_URL=${BASE_URL:-http://lobby-fixture:8090}
TABLE_COUNT=${TABLE_COUNT:-5000}
RATE=${RATE:-50}
DURATION=${DURATION:-1m}
VUS=${VUS:-50}
MAX_VUS=${MAX_VUS:-200}

LOBBY_TABLE_COUNT="$TABLE_COUNT" docker compose up -d --wait lobby-fixture

docker compose run --rm --entrypoint k6 \
  -e BASE_URL="$BASE_URL" \
  -e TABLE_COUNT="$TABLE_COUNT" \
  -e RATE="$RATE" \
  -e DURATION="$DURATION" \
  -e VUS="$VUS" \
  -e MAX_VUS="$MAX_VUS" \
  -e RESULTS_DIR=/scripts/results \
  k6 run /scripts/lobby-5000.js
