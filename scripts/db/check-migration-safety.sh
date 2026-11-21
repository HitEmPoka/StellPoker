#!/usr/bin/env bash
# Static safety check for coordinator migrations.
#
# Thin wrapper over scripts/db/check_migration_safety.py so the migrate.sh
# toolchain stays shell-first while the pattern logic lives in Python (which
# gives us portable regex support — macOS grep lacks -P).
#
# Usage: check-migration-safety.sh <migration_name_or_glob>...

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
exec python3 "$here/check_migration_safety.py" "$@"
