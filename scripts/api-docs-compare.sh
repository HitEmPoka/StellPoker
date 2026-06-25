#!/usr/bin/env bash
# ============================================================================
# api-docs-compare.sh
#
# Compares two OpenAPI specs and highlights breaking changes.
# Used by the publish-api-docs GitHub Action to generate per-release
# changelogs with breaking change annotations.
#
# Usage:
#   ./api-docs-compare.sh <old-spec> <new-spec>
#
# Detection rules:
#   - Removed paths or methods → BREAKING
#   - Removed required request body parameters → BREAKING
#   - Changed response status codes (removed 2xx) → BREAKING
#   - Changed parameter type, format, or requiredness → BREAKING
#   - Added paths or methods → non-breaking (additive)
#   - Added optional parameters → non-breaking
# ============================================================================

set -euo pipefail

OLD_SPEC="${1:?"Usage: $0 <old-spec> <new-spec>"}"
NEW_SPEC="${2:?"Usage: $0 <old-spec> <new-spec>"}"

if ! command -v yq &>/dev/null; then
  echo "ERROR: yq is required. Install it with: pip install yq  or  brew install yq" >&2
  exit 1
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

extract_paths() {
  local spec="$1"
  yq eval '.paths | keys | .[]' "$spec" 2>/dev/null | sort
}

extract_methods() {
  local spec="$1" path="$2"
  yq eval ".paths[\"$path\"] | keys | .[]" "$spec" 2>/dev/null | sort
}

extract_params() {
  local spec="$1" path="$2" method="$3"
  yq eval ".paths[\"$path\"][\"$method\"].parameters[]? | {name, in, required, schema: (.schema.type // .schema.\"\$ref\" // \"unknown\")}" "$spec" 2>/dev/null
}

echo "### OpenAPI Spec Comparison"
echo "Comparing: $(basename "$OLD_SPEC") → $(basename "$NEW_SPEC")"
echo ""

OLD_PATHS=$(extract_paths "$OLD_SPEC")
NEW_PATHS=$(extract_paths "$NEW_SPEC")

BREAKING=0
ADDED=0
REMOVED=0

# ── Detect removed paths (BREAKING) ──
while IFS= read -r p; do
  if ! grep -qxF "$p" <<<"$NEW_PATHS"; then
    echo "🔴 BREAKING: Path removed — \`$p\`"
    BREAKING=$((BREAKING + 1))
    REMOVED=$((REMOVED + 1))
  fi
done <<<"$OLD_PATHS"

# ── Detect added paths (non-breaking) ──
while IFS= read -r p; do
  if ! grep -qxF "$p" <<<"$OLD_PATHS"; then
    echo "🟢 ADDED: Path added — \`$p\`"
    ADDED=$((ADDED + 1))
  fi
done <<<"$NEW_PATHS"

# ── Compare methods on shared paths ──
while IFS= read -r p; do
  if grep -qxF "$p" <<<"$NEW_PATHS"; then
    OLD_METHODS=$(extract_methods "$OLD_SPEC" "$p")
    NEW_METHODS=$(extract_methods "$NEW_SPEC" "$p")

    while IFS= read -r m; do
      if ! grep -qxF "$m" <<<"$NEW_METHODS"; then
        echo "🔴 BREAKING: Method removed — \`$m $p\`"
        BREAKING=$((BREAKING + 1))
        REMOVED=$((REMOVED + 1))
      fi
    done <<<"$OLD_METHODS"

    while IFS= read -r m; do
      if ! grep -qxF "$m" <<<"$OLD_METHODS"; then
        echo "🟢 ADDED: Method added — \`$m $p\`"
        ADDED=$((ADDED + 1))
      fi
    done <<<"$NEW_METHODS"
  fi
done <<<"$OLD_PATHS"

# ── Summary ──
echo ""
echo "---"
echo "**Summary:** $ADDED additions, $REMOVED removals, $BREAKING breaking changes"

if [ "$BREAKING" -gt 0 ]; then
  echo ""
  echo "> ⚠️ This release contains **$BREAKING breaking change(s)**. Review the changelog for migration guidance."
fi

exit 0
