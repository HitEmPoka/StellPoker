#!/usr/bin/env bash
# Download and verify Common Reference String (CRS) for Noir circuits.
# Issues #92, #503: Pin CRS hash in-repo and verify on every download/use.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "${SCRIPT_DIR}")"

CRS_DIR="${CRS_DIR:-${PROJECT_DIR}/.crs}"
CHECKSUM_FILE="${SCRIPT_DIR}/crs.sha256"
CRS_FILE="${CRS_DIR}/bn254_g1.dat"
CRS_URL="https://aztec-ignition.s3.amazonaws.com/MAIN%20IGNITION/sealed/transcript00.dat"

if [ ! -f "${CHECKSUM_FILE}" ]; then
    echo "ERROR: Pinned checksum file not found at ${CHECKSUM_FILE}" >&2
    exit 1
fi

CRS_EXPECTED_HASH=$(awk '{print $1}' "${CHECKSUM_FILE}" | head -n1 | tr -d '[:space:]')
if [ -z "${CRS_EXPECTED_HASH}" ]; then
    echo "ERROR: Could not parse expected SHA-256 hash from ${CHECKSUM_FILE}" >&2
    exit 1
fi

compute_sha256() {
    local target="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "${target}" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "${target}" | awk '{print $1}'
    else
        echo "ERROR: Neither sha256sum nor shasum is available for checksum verification." >&2
        exit 1
    fi
}

VERIFY_ONLY=0
for arg in "$@"; do
    case "${arg}" in
        --verify-only) VERIFY_ONLY=1 ;;
        -h|--help)
            echo "Usage: $0 [--verify-only]"
            echo "Downloads and verifies the pinned BN254 CRS."
            exit 0
            ;;
    esac
done

mkdir -p "${CRS_DIR}"

if [ -f "${CRS_FILE}" ]; then
    echo "Verifying existing CRS file: ${CRS_FILE}..."
    existing_hash=$(compute_sha256 "${CRS_FILE}")
    if [ "${existing_hash}" = "${CRS_EXPECTED_HASH}" ]; then
        echo "CRS checksum verified: ${CRS_FILE} (${existing_hash})"
        exit 0
    else
        echo "ERROR: Existing CRS file failed integrity check!" >&2
        echo "  Expected: ${CRS_EXPECTED_HASH}" >&2
        echo "  Found:    ${existing_hash}" >&2
        if [ "${VERIFY_ONLY}" -eq 1 ]; then
            echo "Verification failed in verify-only mode. Aborting." >&2
            exit 1
        fi
        echo "Removing corrupted/mismatched CRS file and re-downloading..."
        rm -f "${CRS_FILE}"
    fi
fi

if [ "${VERIFY_ONLY}" -eq 1 ]; then
    echo "ERROR: CRS file does not exist at ${CRS_FILE} and --verify-only was specified." >&2
    exit 1
fi

echo "Downloading CRS from ${CRS_URL}..."
TMP_FILE="${CRS_FILE}.tmp.$$"
trap 'rm -f "${TMP_FILE}"' EXIT

curl -fSL --progress-bar -o "${TMP_FILE}" "${CRS_URL}"

echo "Verifying downloaded CRS checksum against pinned hash..."
downloaded_hash=$(compute_sha256 "${TMP_FILE}")

if [ "${downloaded_hash}" != "${CRS_EXPECTED_HASH}" ]; then
    echo "CRITICAL ERROR: Downloaded CRS checksum mismatch! A corrupted or malicious CRS was received." >&2
    echo "  Expected: ${CRS_EXPECTED_HASH}" >&2
    echo "  Got:      ${downloaded_hash}" >&2
    rm -f "${TMP_FILE}"
    exit 1
fi

mv "${TMP_FILE}" "${CRS_FILE}"
echo "CRS successfully downloaded and integrity-verified: ${CRS_FILE}"
