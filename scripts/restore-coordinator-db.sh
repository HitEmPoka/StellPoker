#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Configuration
BACKUP_DIR="${BACKUP_DIR:-${PROJECT_ROOT}/backups}"
DB_NAME="${DB_NAME:-coordinator_db}"
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5432}"
DB_USER="${DB_USER:-coordinator}"

# Helper functions
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $*"
}

error() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $*" >&2
    exit 1
}

usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Restore the coordinator database from a backup file.

OPTIONS:
    -f, --file FILE         Path to backup file (SQL or GPG-encrypted)
    -l, --list             List available backups
    -d, --dry-run          Test restore without applying changes
    -h, --help             Show this help message

EXAMPLES:
    # List available backups
    $0 --list

    # Restore from specific backup
    $0 --file backups/daily/coordinator_db_daily_20240101_000000.sql

    # Restore from encrypted backup (will prompt for passphrase)
    $0 --file backups/monthly/coordinator_db_monthly_20240101_000000.sql.gpg

    # Test restore without applying
    $0 --file backups/daily/coordinator_db_daily_20240101_000000.sql --dry-run

EOF
    exit 1
}

# List available backups
list_backups() {
    log "Available backups:"
    log "=================="

    for category in daily weekly monthly; do
        dir="$BACKUP_DIR/$category"
        if [ -d "$dir" ]; then
            log ""
            log "$category backups:"
            if ls -1 "$dir"/"$DB_NAME"_"$category"_* 2>/dev/null | head -10; then
                local count=$(find "$dir" -name "${DB_NAME}_${category}_*" -type f ! -name "*.metadata" ! -name "*.sha256" | wc -l)
                log "  Total: $count backup(s)"
            else
                log "  No backups found"
            fi
        fi
    done
}

# Verify backup integrity
verify_backup() {
    local backup_file=$1

    log "Verifying backup integrity..."

    # Check if backup exists
    if [ ! -f "$backup_file" ]; then
        error "Backup file not found: $backup_file"
    fi

    # Check checksum if available
    if [ -f "${backup_file}.sha256" ]; then
        log "Verifying SHA256 checksum..."
        if cd "$(dirname "$backup_file")" && sha256sum -c "$(basename ${backup_file}).sha256" > /dev/null 2>&1; then
            log "✓ Checksum verification passed"
        else
            error "Checksum verification failed - backup may be corrupted"
        fi
    else
        log "⚠ No checksum file found, skipping checksum verification"
    fi

    # Check metadata if available
    if [ -f "${backup_file}.metadata" ]; then
        log "Backup metadata:"
        cat "${backup_file}.metadata" | jq '.' || log "  (Could not parse metadata)"
    fi
}

# Prepare backup for restore (handle encryption/compression)
prepare_backup() {
    local backup_file=$1
    local temp_dir=$(mktemp -d)

    if [[ "$backup_file" == *.gpg ]]; then
        log "Backup is encrypted with GPG"
        log "Enter GPG passphrase to decrypt:"
        local decrypted_file="$temp_dir/backup.sql"
        if ! gpg --decrypt -o "$decrypted_file" "$backup_file"; then
            error "Failed to decrypt backup file"
        fi
        echo "$decrypted_file"
    else
        echo "$backup_file"
    fi
}

# Perform database restore
restore_database() {
    local backup_file=$1
    local dry_run=$2

    log "========================================="
    log "Database Restore Operation"
    log "========================================="
    log "Database: $DB_NAME"
    log "Host: $DB_HOST:$DB_PORT"
    log "Backup file: $backup_file"
    log ""

    # Verify backup before restoring
    verify_backup "$backup_file"
    log ""

    # Prepare backup (decrypt if needed)
    local prepared_backup=$(prepare_backup "$backup_file")
    log "Prepared backup file: $prepared_backup"
    log ""

    # Check database connection
    if ! pg_isready -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" > /dev/null 2>&1; then
        error "Database at $DB_HOST:$DB_PORT is not available"
    fi
    log "✓ Database connection verified"

    # List existing databases
    log "Existing databases:"
    PGPASSWORD="${DB_PASSWORD:-}" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -l | grep -E '^\s' | awk '{print $1}' | head -10 || true

    log ""
    log "⚠️  WARNING: This will overwrite the existing database!"
    log ""

    if [ "$dry_run" = true ]; then
        log "DRY RUN MODE: Validating backup syntax without applying changes..."
        if PGPASSWORD="${DB_PASSWORD:-}" pg_restore \
            --host "$DB_HOST" \
            --port "$DB_PORT" \
            --username "$DB_USER" \
            --dbname postgres \
            --list \
            "$prepared_backup" > /dev/null 2>&1; then
            log "✓ Backup validation passed (dry run)"
        else
            error "Backup validation failed"
        fi
        log ""
        log "To apply the restore, run without --dry-run flag"
        return 0
    fi

    # Confirm restore
    log "Press Ctrl+C to cancel, or wait 10 seconds to proceed with restore..."
    sleep 10

    log ""
    log "Stopping existing connections to database..."
    PGPASSWORD="${DB_PASSWORD:-}" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d postgres -c "
        SELECT pg_terminate_backend(pg_stat_activity.pid)
        FROM pg_stat_activity
        WHERE pg_stat_activity.datname = '$DB_NAME'
        AND pid <> pg_backend_pid();
    " 2>/dev/null || true

    log "Dropping existing database..."
    PGPASSWORD="${DB_PASSWORD:-}" dropdb \
        --host "$DB_HOST" \
        --port "$DB_PORT" \
        --username "$DB_USER" \
        --if-exists \
        "$DB_NAME" || true

    log "Creating database..."
    PGPASSWORD="${DB_PASSWORD:-}" createdb \
        --host "$DB_HOST" \
        --port "$DB_PORT" \
        --username "$DB_USER" \
        "$DB_NAME"

    log "Restoring database from backup..."
    if PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$DB_NAME" \
        --no-password \
        < "$prepared_backup"; then
        log "✓ Database restored successfully"
    else
        error "Failed to restore database"
    fi

    # Verify restored data
    log ""
    log "Verifying restored database..."
    local table_count=$(PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$DB_NAME" \
        -t -c "SELECT count(*) FROM information_schema.tables WHERE table_schema='public';")

    log "✓ Database restored with $table_count tables"

    log ""
    log "========================================="
    log "Restore completed successfully!"
    log "========================================="
}

# Main execution
main() {
    local backup_file=""
    local list_mode=false
    local dry_run=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -f|--file)
                backup_file="$2"
                shift 2
                ;;
            -l|--list)
                list_mode=true
                shift
                ;;
            -d|--dry-run)
                dry_run=true
                shift
                ;;
            -h|--help)
                usage
                ;;
            *)
                error "Unknown option: $1"
                ;;
        esac
    done

    if [ "$list_mode" = true ]; then
        list_backups
        return 0
    fi

    if [ -z "$backup_file" ]; then
        log "No backup file specified"
        list_backups
        log ""
        usage
    fi

    restore_database "$backup_file" "$dry_run"
}

# Run main function
main "$@"
