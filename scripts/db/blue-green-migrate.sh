#!/usr/bin/env bash
# Blue-green migration helpers.
#
# Subcommands:
#   backfill   — batched UPDATE loop with sleep between batches
#   rehearse   — run expand → migrate → contract against a staging snapshot
#
# See docs/blue-green-migrations.md for when to use each.

set -euo pipefail

usage() {
    cat <<'EOF'
Usage:
  blue-green-migrate.sh backfill  --table T --set "COL = EXPR" \
                                  [--where "COND"] [--batch-size N] [--sleep-ms M]
  blue-green-migrate.sh rehearse  --migration-tag TAG

Environment:
  DATABASE_URL   PostgreSQL connection string (required)
EOF
}

require_database_url() {
    if [[ -z "${DATABASE_URL:-}" ]]; then
        echo "ERROR: DATABASE_URL is not set." >&2
        exit 1
    fi
}

cmd="${1:-}"
shift || true

case "$cmd" in
    backfill)
        table=""
        set_expr=""
        where_expr="TRUE"
        batch_size=5000
        sleep_ms=100
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --table)       table="$2"; shift 2 ;;
                --set)         set_expr="$2"; shift 2 ;;
                --where)       where_expr="$2"; shift 2 ;;
                --batch-size)  batch_size="$2"; shift 2 ;;
                --sleep-ms)    sleep_ms="$2"; shift 2 ;;
                *) echo "unknown flag: $1" >&2; usage; exit 2 ;;
            esac
        done
        if [[ -z "$table" || -z "$set_expr" ]]; then
            usage; exit 2
        fi
        require_database_url

        # UUID or bigint PK assumed to be named "id". Override with PK_COL env.
        pk="${PK_COL:-id}"
        rehearsal_id=$(uuidgen 2>/dev/null || python3 -c 'import uuid; print(uuid.uuid4())')
        echo "Rehearsal: $rehearsal_id"
        echo "Backfilling $table in batches of $batch_size (sleep ${sleep_ms}ms between)…"

        total=0
        while true; do
            # Batched by PK via a CTE so each UPDATE is a short transaction.
            affected=$(psql "$DATABASE_URL" -At <<SQL
WITH batch AS (
    SELECT $pk FROM $table
    WHERE $where_expr
    ORDER BY $pk
    LIMIT $batch_size
    FOR UPDATE SKIP LOCKED
)
UPDATE $table t
   SET $set_expr
  FROM batch
 WHERE t.$pk = batch.$pk
RETURNING 1;
SQL
            )
            n=$(echo "$affected" | grep -c '^1$' || true)
            total=$((total + n))
            if [[ $n -eq 0 ]]; then break; fi
            printf '  batch: %d rows, cumulative: %d\n' "$n" "$total"
            # BSD and GNU sleep both accept fractional seconds.
            sleep "$(awk "BEGIN { print $sleep_ms / 1000 }")"
        done

        echo "Backfill complete. Rows updated: $total. Rehearsal: $rehearsal_id"
        ;;

    rehearse)
        tag=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --migration-tag) tag="$2"; shift 2 ;;
                *) echo "unknown flag: $1" >&2; usage; exit 2 ;;
            esac
        done
        if [[ -z "$tag" ]]; then usage; exit 2; fi
        require_database_url

        rehearsal_id=$(uuidgen 2>/dev/null || python3 -c 'import uuid; print(uuid.uuid4())')
        echo "Rehearsal: $rehearsal_id"
        echo "Target database: $(psql "$DATABASE_URL" -Atc 'SELECT current_database()')"
        echo "Running migrations up to and including $tag…"

        here="$(cd "$(dirname "$0")" && pwd)"
        "$here/migrate.sh" status | tee "/tmp/stellpoker-rehearse-$rehearsal_id.log"
        "$here/migrate.sh" up
        echo "Rehearsal $rehearsal_id complete. Record this ID in your PR description."
        ;;

    ""|-h|--help|help)
        usage
        ;;
    *)
        echo "unknown subcommand: $cmd" >&2
        usage
        exit 2
        ;;
esac
