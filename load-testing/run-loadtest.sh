#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

BASE_URL=${BASE_URL:-http://host.docker.internal:8080}
TABLE_COUNT=${TABLE_COUNT:-1000}
VUS=${VUS:-100}
DURATION=${DURATION:-3m}

docker compose up -d

docker compose run --rm \
  -e BASE_URL="$BASE_URL" \
  -e TABLE_COUNT="$TABLE_COUNT" \
  -e VUS="$VUS" \
  -e DURATION="$DURATION" \
  k6 run --out influxdb=http://influxdb:8086/k6 /scripts/benchmark.js
