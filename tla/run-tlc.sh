#!/usr/bin/env bash
# Run TLC model checker on DealReveal.tla
#
# Prerequisites:
#   - Java 11+ on PATH
#   - TLC jar: https://github.com/tlaplus/tlaplus/releases
#     Default expected location: ~/tla/tla2tools.jar
#     Override: TLC_JAR=/path/to/tla2tools.jar ./run-tlc.sh
#
# Usage:
#   ./tla/run-tlc.sh
#
# TLC checks the three invariants:
#   - TypeInvariant
#   - SettlementRequiresProof
#   - MonotonicPhase
#
# Configuration: 6 players, 3 MPC nodes (matches acceptance criteria in #892).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TLC_JAR="${TLC_JAR:-$HOME/tla/tla2tools.jar}"

if [ ! -f "$TLC_JAR" ]; then
    echo "ERROR: TLC jar not found at $TLC_JAR"
    echo "Download it from https://github.com/tlaplus/tlaplus/releases"
    echo "and set TLC_JAR=/path/to/tla2tools.jar"
    exit 1
fi

echo "=== Running TLC on DealReveal.tla ==="
java -jar "$TLC_JAR" \
    -config "$SCRIPT_DIR/DealReveal.cfg" \
    -workers auto \
    "$SCRIPT_DIR/DealReveal.tla"
