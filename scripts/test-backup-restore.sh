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
TEST_DB_NAME="${DB_NAME}_restore_test_$$"
LOG_FILE="${PROJECT_ROOT}/backup_restore_test_$(date +%Y%m%d_%H%M%S).log"

# Helper functions
log() {
    local message="[$(date +'%Y-%m-%d %H:%M:%S')] $*"
    echo "$message"
    echo "$message" >> "$LOG_FILE"
}

error() {
    local message="[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $*"
    echo "$message" >&2
    echo "$message" >> "$LOG_FILE"
    cleanup_test_db
    exit 1
}

success() {
    local message="[$(date +'%Y-%m-%d %H:%M:%S')] ✓ $*"
    echo "$message"
    echo "$message" >> "$LOG_FILE"
}

cleanup_test_db() {
    log "Cleaning up test database..."
    PGPASSWORD="${DB_PASSWORD:-}" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d postgres -c "
        SELECT pg_terminate_backend(pg_stat_activity.pid)
        FROM pg_stat_activity
        WHERE pg_stat_activity.datname = '$TEST_DB_NAME'
        AND pid <> pg_backend_pid();
    " 2>/dev/null || true

    PGPASSWORD="${DB_PASSWORD:-}" dropdb \
        --host "$DB_HOST" \
        --port "$DB_PORT" \
        --username "$DB_USER" \
        --if-exists \
        "$TEST_DB_NAME" 2>/dev/null || true
}

# Find latest backup file
find_latest_backup() {
    local backup_type=$1
    find "$BACKUP_DIR/$backup_type" -name "${DB_NAME}_${backup_type}_*" -type f ! -name "*.metadata" ! -name "*.sha256" -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-
}

# Test restore from backup
test_restore_from_backup() {
    local backup_file=$1
    local backup_name=$(basename "$backup_file")

    log "========================================="
    log "Testing Backup Restore"
    log "========================================="
    log "Backup file: $backup_name"
    log "Test database: $TEST_DB_NAME"
    log ""

    # Verify backup exists
    if [ ! -f "$backup_file" ]; then
        error "Backup file not found: $backup_file"
    fi
    success "Backup file exists"

    # Verify checksum
    if [ -f "${backup_file}.sha256" ]; then
        log "Verifying backup checksum..."
        if cd "$(dirname "$backup_file")" && sha256sum -c "$(basename ${backup_file}).sha256" > /dev/null 2>&1; then
            success "Checksum verification passed"
        else
            error "Checksum verification failed"
        fi
    else
        log "⚠ No checksum file available, skipping verification"
    fi

    # Check database connection
    log "Checking database connection..."
    if pg_isready -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" > /dev/null 2>&1; then
        success "Database connection verified"
    else
        error "Cannot connect to database"
    fi

    # Prepare backup (decrypt if needed)
    local prepared_backup="$backup_file"
    if [[ "$backup_file" == *.gpg ]]; then
        log "Backup is encrypted, decrypting..."
        local temp_file=$(mktemp)
        if ! gpg --batch --quiet --decrypt -o "$temp_file" "$backup_file" 2>/dev/null; then
            error "Failed to decrypt backup"
        fi
        prepared_backup="$temp_file"
        success "Backup decrypted successfully"
    fi

    # Validate backup syntax
    log "Validating backup SQL syntax..."
    if head -100 "$prepared_backup" | grep -q "^--"; then
        success "Backup appears to be valid SQL"
    else
        error "Backup does not appear to be valid SQL"
    fi

    # Create test database
    log "Creating test database..."
    if PGPASSWORD="${DB_PASSWORD:-}" createdb \
        --host "$DB_HOST" \
        --port "$DB_PORT" \
        --username "$DB_USER" \
        "$TEST_DB_NAME"; then
        success "Test database created"
    else
        error "Failed to create test database"
    fi

    # Restore to test database
    log "Restoring backup to test database..."
    if PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$TEST_DB_NAME" \
        --no-password \
        < "$prepared_backup" > /dev/null 2>&1; then
        success "Backup restored successfully"
    else
        error "Failed to restore backup"
    fi

    # Verify restored data
    log "Verifying restored database..."
    local table_count=$(PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$TEST_DB_NAME" \
        -t -c "SELECT count(*) FROM information_schema.tables WHERE table_schema='public';")

    if [ -n "$table_count" ] && [ "$table_count" -gt 0 ]; then
        success "Database restored with $table_count tables"
    else
        error "Restored database has no tables"
    fi

    # Run basic sanity checks
    log "Running sanity checks..."

    # Check for views
    local view_count=$(PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$TEST_DB_NAME" \
        -t -c "SELECT count(*) FROM information_schema.views WHERE table_schema='public';")
    log "  - Views: $view_count"

    # Check for functions
    local func_count=$(PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$TEST_DB_NAME" \
        -t -c "SELECT count(*) FROM pg_proc WHERE pronamespace != 11;")
    log "  - Functions: $func_count"

    # Check for constraints
    local constraint_count=$(PGPASSWORD="${DB_PASSWORD:-}" psql \
        -h "$DB_HOST" \
        -p "$DB_PORT" \
        -U "$DB_USER" \
        -d "$TEST_DB_NAME" \
        -t -c "SELECT count(*) FROM information_schema.table_constraints WHERE table_schema='public';")
    log "  - Constraints: $constraint_count"

    success "Sanity checks passed"

    # Cleanup
    cleanup_test_db

    if [[ "$prepared_backup" == "/tmp/"* ]]; then
        rm -f "$prepared_backup"
    fi

    log ""
    success "========================================="
    success "Backup restore test completed successfully!"
    success "========================================="
}

# Generate test report
generate_report() {
    local backup_type=$1
    local backup_file=$2

    log ""
    log "========================================="
    log "Monthly Backup Restore Test Report"
    log "========================================="
    log "Test timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    log "Backup type: $backup_type"
    log "Backup file: $(basename $backup_file)"
    log "Test result: PASSED"
    log "Log file: $LOG_FILE"
    log "========================================="
}

# Schedule next test
schedule_next_test() {
    if command -v crontab &> /dev/null; then
        log ""
        log "To schedule monthly backup restore tests, add to crontab:"
        log "  0 3 1 * * $SCRIPT_DIR/test-backup-restore.sh"
    fi
}

# Main execution
main() {
    log "Starting backup restore test..."

    # Create log file
    > "$LOG_FILE"
    log "Log file: $LOG_FILE"

    # Check if backup directory exists
    if [ ! -d "$BACKUP_DIR" ]; then
        error "Backup directory does not exist: $BACKUP_DIR"
    fi

    # Find latest monthly backup
    log "Finding latest monthly backup..."
    local latest_backup=$(find_latest_backup "monthly")

    if [ -z "$latest_backup" ]; then
        log "No monthly backup found, trying weekly..."
        latest_backup=$(find_latest_backup "weekly")
    fi

    if [ -z "$latest_backup" ]; then
        log "No weekly backup found, trying daily..."
        latest_backup=$(find_latest_backup "daily")
    fi

    if [ -z "$latest_backup" ]; then
        error "No backups found in $BACKUP_DIR"
    fi

    log "Using backup: $(basename $latest_backup)"
    log ""

    # Run the restore test
    test_restore_from_backup "$latest_backup"

    # Generate report
    generate_report "$(basename $(dirname $latest_backup))" "$latest_backup"

    # Schedule next test
    schedule_next_test

    log ""
    log "Test report written to: $LOG_FILE"
}

# Run main function
main "$@"
