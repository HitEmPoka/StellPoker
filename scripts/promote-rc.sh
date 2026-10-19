#!/usr/bin/env bash
# Promote the frontend RC image to latest without rebuilding.
#
# The RC channel (ghcr.io/<owner>/stellpoker/frontend:rc) is tagged from
# the same digest as the git-SHA image built on main, so promoting is a
# pure re-tag — the promoted image is byte-identical to what passed smoke.
#
# Usage: OWNER=<github-owner> ./scripts/promote-rc.sh
set -euo pipefail

OWNER="${OWNER:-${GHCR_OWNER:-}}"
if [ -z "$OWNER" ]; then
    echo "ERROR: set OWNER=<github-owner> (or GHCR_OWNER) to the GHCR namespace"
    exit 1
fi

# GHCR requires lowercase repository names.
OWNER_LOWER="$(printf '%s' "$OWNER" | tr '[:upper:]' '[:lower:]')"
IMAGE="ghcr.io/${OWNER_LOWER}/stellpoker/frontend"

echo "=== Promoting ${IMAGE}:rc -> ${IMAGE}:latest ==="
docker buildx imagetools create \
    -t "${IMAGE}:latest" \
    "${IMAGE}:rc"

echo ""
echo "Promoted. Verify the digest matches RC:"
echo "  docker buildx imagetools inspect ${IMAGE}:latest"
echo "  docker buildx imagetools inspect ${IMAGE}:rc"
