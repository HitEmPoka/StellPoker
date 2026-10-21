#!/usr/bin/env bash
set -euo pipefail

# Deploy-time container image signature verification (Issue #678).
#
# Fail-closed verify policy: any image that is unsigned, signed by an
# unexpected identity, or unverifiable is REFUSED (exit 1) and the deploy
# must abort. There is intentionally no "skip" flag.

usage() {
    cat >&2 <<'EOF'
Usage: verify-image-signature.sh <image>[:tag|@digest]

Verifies the cosign signature of a GHCR image before it is deployed.

Example:
  ./scripts/verify-image-signature.sh ghcr.io/hitempoka/stellpoker/coordinator:latest

Environment:
  COSIGN_CERTIFICATE_REPO   GitHub repo whose workflow may sign images
                            (default: HitEmPoka/StellPoker)
EOF
    exit 2
}

IMAGE="${1:-}"
[ -n "$IMAGE" ] || usage

EXPECTED_REPO="${COSIGN_CERTIFICATE_REPO:-HitEmPoka/StellPoker}"
OIDC_ISSUER="https://token.actions.githubusercontent.com"
IDENTITY_REGEXP="https://github.com/${EXPECTED_REPO}/.github/workflows/docker-build-push.yml@"

fail() {
    echo "ERROR: $*" >&2
    echo "  Failure mode: deploy refuses unsigned images and aborts (fail-closed)." >&2
    echo "  Fix: push the image through .github/workflows/docker-build-push.yml so CI" >&2
    echo "       signs it, then retry the deploy." >&2
    exit 1
}

if ! command -v cosign >/dev/null 2>&1; then
    echo "ERROR: cosign is not installed; cannot verify image signature." >&2
    echo "  Fix: install cosign, e.g. 'brew install cosign' or see" >&2
    echo "       https://docs.sigstore.dev/cosign/system_config/installation/" >&2
    exit 1
fi

# GHCR references must be lowercase.
IMAGE_REF="$(printf '%s' "$IMAGE" | tr '[:upper:]' '[:lower:]')"

echo "Verifying signature for ${IMAGE_REF} ..."

if ! cosign verify \
    --certificate-identity-regexp "${IDENTITY_REGEXP}" \
    --certificate-oidc-issuer "${OIDC_ISSUER}" \
    "${IMAGE_REF}"; then
    fail "image is unsigned or was not signed by ${IDENTITY_REGEXP}* — refusing to deploy ${IMAGE_REF}"
fi

echo "OK: signature verified for ${IMAGE_REF}"
