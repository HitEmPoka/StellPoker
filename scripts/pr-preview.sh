#!/usr/bin/env bash
#
# pr-preview.sh — isolated per-PR preview environments (#683).
#
# Starts (or tears down) one docker-compose project per PR number, with host
# ports derived from the PR number so several previews can run side by side.
# See docs/preview-environments.md.
#
# Usage:
#   scripts/pr-preview.sh up   <pr-number>   build + start, print preview URLs
#   scripts/pr-preview.sh down <pr-number>   stop and remove the stack + volumes
#   scripts/pr-preview.sh url  <pr-number>   print the preview URLs only
set -euo pipefail
cd "$(dirname "$0")/.."

cmd="${1:-}"
pr="${2:-}"
if [[ -z "$cmd" || -z "$pr" || ! "$pr" =~ ^[0-9]+$ ]]; then
  echo "usage: $0 up|down|url <pr-number>" >&2
  exit 2
fi

project="stellpoker-pr-${pr}"
# Deterministic, collision-free ports per PR (mod 100 keeps us in 18080-18179).
PREVIEW_PORT=$((18080 + pr % 100))
PREVIEW_SOROBAN_PORT=$((19000 + pr % 100))
export PREVIEW_PORT PREVIEW_SOROBAN_PORT

compose=(docker compose
  -p "$project"
  -f docker-compose.yml
  -f docker-compose.preview.yml)

url="http://localhost:${PREVIEW_PORT}"

case "$cmd" in
  up)
    "${compose[@]}" up -d --build
    echo ""
    echo "Preview for PR #${pr} is coming up:"
    echo "  coordinator API: ${url}/api/health"
    echo "  soroban RPC:     http://localhost:${PREVIEW_SOROBAN_PORT}/soroban/rpc"
    echo ""
    echo "First boot compiles circuits/CRS — allow a few minutes before"
    echo "/api/health turns healthy."
    ;;
  down)
    "${compose[@]}" down -v --remove-orphans
    echo "Preview stack for PR #${pr} torn down."
    ;;
  url)
    echo "$url/api/health"
    ;;
  *)
    echo "unknown command: $cmd (expected up|down|url)" >&2
    exit 2
    ;;
esac
