#!/usr/bin/env bash
# Circuit Provenance Generator
#
# Generates SLSA-style provenance files for circuit artifacts and verification keys.
# Records git SHA, nargo version, and artifact hash for traceability.
#
# Usage: ./scripts/generate-provenance.sh <artifact-path> <vk-path>

set -euo pipefail

ARTIFACT_PATH="${1:?Usage: $0 <artifact-path> <vk-path>}"
VK_PATH="${2:?Usage: $0 <artifact-path> <vk-path>}"
OUTPUT_DIR="${3:-./provenance}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}🔐 StellPoker Circuit Provenance Generator${NC}"
echo "============================================"
echo ""

# Ensure output directory exists
mkdir -p "$OUTPUT_DIR"

# Gather build information
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
GIT_SHA=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
GIT_SHA_SHORT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
GIT_DIRTY=$(git diff --quiet 2>/dev/null && echo "clean" || echo "dirty")
GIT_BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")

# Get nargo version
NARGO_VERSION=$(nargo --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown")

# Get circuit name from artifact path
CIRCUIT_NAME=$(basename "$ARTIFACT_PATH" .json 2>/dev/null || basename "$ARTIFACT_PATH")

# Compute artifact hashes
echo "📦 Computing artifact hashes..."
ARTIFACT_SHA256=$(sha256sum "$ARTIFACT_PATH" 2>/dev/null | awk '{print $1}' || shasum -a 256 "$ARTIFACT_PATH" | awk '{print $1}')
VK_SHA256=$(sha256sum "$VK_PATH" 2>/dev/null | awk '{print $1}' || shasum -a 256 "$VK_PATH" | awk '{print $1}')

# Get artifact sizes
ARTIFACT_SIZE=$(stat -f%z "$ARTIFACT_PATH" 2>/dev/null || stat --format=%s "$ARTIFACT_PATH" 2>/dev/null || echo "0")
VK_SIZE=$(stat -f%z "$VK_PATH" 2>/dev/null || stat --format=%s "$VK_PATH" 2>/dev/null || echo "0")

# Generate provenance file
PROVENANCE_FILE="${OUTPUT_DIR}/${CIRCUIT_NAME}-provenance.json"

cat > "$PROVENANCE_FILE" << EOF
{
  "_type": "https://slsa.dev/provenance/v1",
  "version": "1.0",
  "build": {
    "timestamp": "${TIMESTAMP}",
    "invocation": {
      "configSource": {
        "uri": "git+https://github.com/HitEmPoka/StellPoker.git@${GIT_SHA}",
        "digest": {
          "sha1": "${GIT_SHA}"
        },
        "entryPoint": "circuits/${CIRCUIT_NAME}"
      },
      "parameters": {
        "nargo_version": "${NARGO_VERSION}",
        "circuit_name": "${CIRCUIT_NAME}"
      }
    },
    "metadata": {
      "buildEnvironment": {
        "git_commit": "${GIT_SHA}",
        "git_branch": "${GIT_BRANCH}",
        "git_dirty": "${GIT_DIRTY}",
        "nargo_version": "${NARGO_VERSION}",
        "os": "$(uname -s)",
        "arch": "$(uname -m)"
      }
    }
  },
  "artifacts": {
    "circuit": {
      "path": "${ARTIFACT_PATH}",
      "sha256": "${ARTIFACT_SHA256}",
      "size_bytes": ${ARTIFACT_SIZE}
    },
    "verification_key": {
      "path": "${VK_PATH}",
      "sha256": "${VK_SHA256}",
      "size_bytes": ${VK_SIZE}
    }
  },
  "signer": {
    "key_id": "stellpoker-circuit-builder",
    "timestamp": "${TIMESTAMP}"
  }
}
EOF

echo ""
echo -e "${GREEN}✅ Provenance generated:${NC}"
echo "   ${PROVENANCE_FILE}"
echo ""
echo "📋 Build Information:"
echo "   Circuit:     ${CIRCUIT_NAME}"
echo "   Git SHA:     ${GIT_SHA_SHORT} (${GIT_DIRTY})"
echo "   Nargo:       ${NARGO_VERSION}"
echo "   Timestamp:   ${TIMESTAMP}"
echo ""
echo "📦 Artifact Hashes:"
echo "   Circuit:     ${ARTIFACT_SHA256:0:16}..."
echo "   VK:          ${VK_SHA256:0:16}..."
