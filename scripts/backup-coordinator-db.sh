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
RETENTION_DAYS_DAILY=7
RETENTION_DAYS_WEEKLY=28
RETENTION_DAYS_MONTHLY=365

# Create backup directory
mkdir -p "$BACKUP_DIR"

# Backup categories
DAILY_DIR="$BACKUP_DIR/daily"
WEEKLY_DIR="$BACKUP_DIR/weekly"
MONTHLY_DIR="$BACKUP_DIR/monthly"

mkdir -p "$DAILY_DIR" "$WEEKLY_DIR" "$MONTHLY_DIR"

# Helper functions
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $*"
}

error() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $*" >&2
    exit 1
}

# Determine backup type based on date
determine_backup_type() {
    local day_of_month=$(date +%d)
    local day_of_week=$(date +%w)

    # Monthly backup on the 1st of each month
    if [ "$day_of_month" = "01" ]; then
        echo "monthly"
    # Weekly backup on Sunday
    elif [ "$day_of_week" = "0" ]; then
        echo "weekly"
    # Daily backup every day
    else
        echo "daily"
    fi
}

# Backup database
backup_database() {
    local backup_type=$1
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local backup_dir

    case $backup_type in
        monthly)
            backup_dir="$MONTHLY_DIR"
            ;;
        weekly)
            backup_dir="$WEEKLY_DIR"
            ;;
        daily)
            backup_dir="$DAILY_DIR"
            ;;
        *)
            error "Unknown backup type: $backup_type"
            ;;
    esac

    local backup_file="$backup_dir/${DB_NAME}_${backup_type}_${timestamp}.sql"
    local backup_file_encrypted="${backup_file}.gpg"

    log "Starting $backup_type backup of $DB_NAME..."

    # Check if database is available
    if ! pg_isready -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" > /dev/null 2>&1; then
        error "Database at $DB_HOST:$DB_PORT is not available"
    fi

    # Perform backup
    if PGPASSWORD="${DB_PASSWORD:-}" pg_dump \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$DB_NAME" \
        --verbose \
        --no-password \
        > "$backup_file" 2>&1; then

        log "Database dump created: $backup_file"
        log "Dump size: $(du -h "$backup_file" | cut -f1)"

        # Encrypt backup if GPG is configured
        if command -v gpg &> /dev/null && [ -n "$GPG_RECIPIENT" ]; then
            log "Encrypting backup with GPG..."
            if gpg --always-trust -r "$GPG_RECIPIENT" --encrypt "$backup_file"; then
                log "Backup encrypted: $backup_file_encrypted"
                rm -f "$backup_file"
                backup_file="$backup_file_encrypted"
                log "Encrypted backup size: $(du -h "$backup_file" | cut -f1)"
            else
                log "WARNING: GPG encryption failed, keeping unencrypted backup"
            fi
        else
            log "GPG encryption disabled or not available"
        fi

        # Create checksum
        sha256sum "$backup_file" > "${backup_file}.sha256"
        log "Created checksum: ${backup_file}.sha256"

        # Create metadata file
        cat > "${backup_file}.metadata" << EOF
{
  "backup_type": "$backup_type",
  "database_name": "$DB_NAME",
  "database_host": "$DB_HOST",
  "database_port": "$DB_PORT",
  "backup_timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "backup_size_bytes": "$(stat -f%z "$backup_file" 2>/dev/null || stat -c%s "$backup_file")",
  "pg_version": "$(psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -t -c "SELECT version();" 2>/dev/null || echo 'Unknown')",
  "compressed": false,
  "encrypted": $([ -f "$backup_file_encrypted" ] && echo true || echo false)
}
EOF
        log "Created metadata file: ${backup_file}.metadata"

        return 0
    else
        error "Failed to create database dump"
    fi
}

# Cleanup old backups based on retention policy
cleanup_old_backups() {
    log "Cleaning up old backups..."

    # Daily backups: keep for 7 days
    log "Removing daily backups older than $RETENTION_DAYS_DAILY days..."
    find "$DAILY_DIR" -name "${DB_NAME}_daily_*" -type f -mtime "+$RETENTION_DAYS_DAILY" -delete
    find "$DAILY_DIR" -name "${DB_NAME}_daily_*.metadata" -type f -mtime "+$RETENTION_DAYS_DAILY" -delete
    find "$DAILY_DIR" -name "${DB_NAME}_daily_*.sha256" -type f -mtime "+$RETENTION_DAYS_DAILY" -delete

    # Weekly backups: keep for 4 weeks (28 days)
    log "Removing weekly backups older than $RETENTION_DAYS_WEEKLY days..."
    find "$WEEKLY_DIR" -name "${DB_NAME}_weekly_*" -type f -mtime "+$RETENTION_DAYS_WEEKLY" -delete
    find "$WEEKLY_DIR" -name "${DB_NAME}_weekly_*.metadata" -type f -mtime "+$RETENTION_DAYS_WEEKLY" -delete
    find "$WEEKLY_DIR" -name "${DB_NAME}_weekly_*.sha256" -type f -mtime "+$RETENTION_DAYS_WEEKLY" -delete

    # Monthly backups: keep for 12 months (365 days)
    log "Removing monthly backups older than $RETENTION_DAYS_MONTHLY days..."
    find "$MONTHLY_DIR" -name "${DB_NAME}_monthly_*" -type f -mtime "+$RETENTION_DAYS_MONTHLY" -delete
    find "$MONTHLY_DIR" -name "${DB_NAME}_monthly_*.metadata" -type f -mtime "+$RETENTION_DAYS_MONTHLY" -delete
    find "$MONTHLY_DIR" -name "${DB_NAME}_monthly_*.sha256" -type f -mtime "+$RETENTION_DAYS_MONTHLY" -delete

    log "Backup cleanup completed"
}

# Report backup status
report_status() {
    log "Backup Status Report"
    log "===================="
    log "Daily backups (keep 7 days):"
    find "$DAILY_DIR" -name "${DB_NAME}_daily_*" -type f ! -name "*.metadata" ! -name "*.sha256" | sort | tail -7 | while read f; do
        log "  - $(basename $f) ($(du -h "$f" | cut -f1))"
    done
    log ""
    log "Weekly backups (keep 4 weeks):"
    find "$WEEKLY_DIR" -name "${DB_NAME}_weekly_*" -type f ! -name "*.metadata" ! -name "*.sha256" | sort | tail -4 | while read f; do
        log "  - $(basename $f) ($(du -h "$f" | cut -f1))"
    done
    log ""
    log "Monthly backups (keep 12 months):"
    find "$MONTHLY_DIR" -name "${DB_NAME}_monthly_*" -type f ! -name "*.metadata" ! -name "*.sha256" | sort | tail -12 | while read f; do
        log "  - $(basename $f) ($(du -h "$f" | cut -f1))"
    done
}

# Main execution
main() {
    log "========================================="
    log "Database Backup Script"
    log "========================================="
    log "Database: $DB_NAME"
    log "Host: $DB_HOST:$DB_PORT"
    log "Backup directory: $BACKUP_DIR"
    log ""

    BACKUP_TYPE=$(determine_backup_type)
    log "Backup type determined: $BACKUP_TYPE"

    backup_database "$BACKUP_TYPE"
    cleanup_old_backups
    report_status

    log "Backup completed successfully"
}

# Run main function
main "$@"
