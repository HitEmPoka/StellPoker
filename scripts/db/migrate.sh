#!/usr/bin/env bash
# Database migration helper for the StellPoker coordinator.
# Requires: sqlx-cli  (cargo install sqlx-cli --no-default-features --features native-tls,postgres)
#
# Usage:
#   ./scripts/db/migrate.sh up           -- apply all pending migrations
#   ./scripts/db/migrate.sh down         -- revert the most recent migration
#   ./scripts/db/migrate.sh status       -- show migration history
#   ./scripts/db/migrate.sh add <name>   -- create a new timestamped migration pair
#   ./scripts/db/migrate.sh seed         -- load dev seed data (psql required)

set -euo pipefail

MIGRATIONS_DIR="$(cd "$(dirname "$0")/../../services/coordinator/migrations" && pwd)"
SEEDS_DIR="$(cd "$(dirname "$0")/../../services/coordinator/seeds" && pwd)"

require_database_url() {
    if [[ -z "${DATABASE_URL:-}" ]]; then
        echo "ERROR: DATABASE_URL is not set." >&2
        echo "  export DATABASE_URL=postgres://coordinator:password@localhost:5432/coordinator" >&2
        exit 1
    fi
}

cmd="${1:-help}"

case "$cmd" in
    up)
        require_database_url
        echo "Applying pending migrations..."
        sqlx migrate run --source "$MIGRATIONS_DIR"
        echo "Done."
        ;;
    down)
        require_database_url
        echo "Reverting latest migration..."
        sqlx migrate revert --source "$MIGRATIONS_DIR"
        echo "Done."
        ;;
    status)
        require_database_url
        sqlx migrate info --source "$MIGRATIONS_DIR"
        ;;
    add)
        name="${2:-}"
        if [[ -z "$name" ]]; then
            echo "Usage: $0 add <migration_name>" >&2
            exit 1
        fi
        timestamp=$(date -u '+%Y%m%d%H%M%S')
        up_file="$MIGRATIONS_DIR/${timestamp}_${name}.up.sql"
        down_file="$MIGRATIONS_DIR/${timestamp}_${name}.down.sql"
        echo "-- Migration: $name" > "$up_file"
        echo "-- Rollback: $name" > "$down_file"
        echo "Created:"
        echo "  $up_file"
        echo "  $down_file"
        ;;
    seed)
        require_database_url
        echo "Loading seed data..."
        for seed in "$SEEDS_DIR"/*.sql; do
            echo "  -> $seed"
            psql "$DATABASE_URL" -f "$seed"
        done
        echo "Done."
        ;;
    help|*)
        echo "Usage: $0 {up|down|status|add <name>|seed}"
        ;;
esac
