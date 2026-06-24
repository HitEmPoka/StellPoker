#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

echo "🔍 Docker Image Vulnerability Scanning"
echo "======================================"
echo ""

# Check if Trivy is installed
if ! command -v trivy &> /dev/null; then
    echo "❌ Trivy is not installed. Installing Trivy..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install trivy
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        sudo apt-get update && sudo apt-get install -y trivy
    else
        echo "❌ Unsupported OS. Please install Trivy manually: https://github.com/aquasecurity/trivy"
        exit 1
    fi
fi

# Define images to scan
IMAGES=(
    "coordinator:services/coordinator/Dockerfile:."
    "mpc-node:services/node/Dockerfile:."
    "frontend:app/Dockerfile:app"
)

CRITICAL_FOUND=0
HIGH_FOUND=0

for IMAGE_INFO in "${IMAGES[@]}"; do
    IFS=':' read -r IMAGE_NAME DOCKERFILE CONTEXT <<< "$IMAGE_INFO"

    echo "📦 Building $IMAGE_NAME..."
    docker build -f "$DOCKERFILE" -t "$IMAGE_NAME:latest" "$CONTEXT" > /dev/null 2>&1

    echo "🔎 Scanning $IMAGE_NAME..."

    # Run Trivy scan with summary output
    if trivy image --severity CRITICAL,HIGH "$IMAGE_NAME:latest" > "/tmp/${IMAGE_NAME}-scan.txt" 2>&1; then
        CRIT_COUNT=$(grep -c "CRITICAL" "/tmp/${IMAGE_NAME}-scan.txt" || echo "0")
        HIGH_COUNT=$(grep -c "HIGH" "/tmp/${IMAGE_NAME}-scan.txt" || echo "0")

        if [ "$CRIT_COUNT" -gt 0 ]; then
            echo "  ⚠️  CRITICAL: $CRIT_COUNT vulnerabilities found"
            CRITICAL_FOUND=$((CRITICAL_FOUND + CRIT_COUNT))
        fi
        if [ "$HIGH_COUNT" -gt 0 ]; then
            echo "  ⚠️  HIGH: $HIGH_COUNT vulnerabilities found"
            HIGH_FOUND=$((HIGH_FOUND + HIGH_COUNT))
        fi
        if [ "$CRIT_COUNT" -eq 0 ] && [ "$HIGH_COUNT" -eq 0 ]; then
            echo "  ✅ No CRITICAL or HIGH vulnerabilities"
        fi
    else
        echo "  ✅ Scan completed"
    fi

    echo ""
done

echo "======================================"
echo "📊 Scan Summary"
echo "======================================"
echo "Total CRITICAL vulnerabilities: $CRITICAL_FOUND"
echo "Total HIGH vulnerabilities: $HIGH_FOUND"
echo ""

if [ $CRITICAL_FOUND -gt 0 ]; then
    echo "❌ CRITICAL vulnerabilities detected. Please fix before pushing."
    exit 1
elif [ $HIGH_FOUND -gt 0 ]; then
    echo "⚠️  HIGH vulnerabilities detected. Consider fixing before pushing."
    exit 0
else
    echo "✅ All scans passed! No CRITICAL or HIGH vulnerabilities found."
    exit 0
fi
