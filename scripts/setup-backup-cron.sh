#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BACKUP_SCRIPT="$SCRIPT_DIR/backup-coordinator-db.sh"
TEST_SCRIPT="$SCRIPT_DIR/test-backup-restore.sh"

# Helper functions
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $*"
}

error() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $*" >&2
    exit 1
}

# Check if running on supported OS
check_os() {
    if [[ "$OSTYPE" != "linux-gnu"* ]] && [[ "$OSTYPE" != "darwin"* ]]; then
        error "This script is only supported on Linux and macOS"
    fi
}

# Install cron jobs
install_cron_jobs() {
    log "Setting up cron jobs for backup automation..."

    # Backup configuration
    local backup_schedule="0 2 * * *"  # Daily at 2 AM
    local test_schedule="0 3 1 * *"    # Monthly on 1st at 3 AM
    local env_vars="PATH=/usr/local/bin:/usr/bin:/bin
BACKUP_DIR=$PROJECT_ROOT/backups
DB_NAME=coordinator_db
DB_HOST=localhost
DB_PORT=5432
DB_USER=coordinator"

    local cron_entries="$env_vars

# Coordinator database backup (daily at 2 AM, auto-determines daily/weekly/monthly)
$backup_schedule $BACKUP_SCRIPT >> $PROJECT_ROOT/logs/backup.log 2>&1

# Monthly backup restore test (1st of month at 3 AM)
$test_schedule $TEST_SCRIPT >> $PROJECT_ROOT/logs/backup_test.log 2>&1
"

    # Create logs directory
    mkdir -p "$PROJECT_ROOT/logs"

    # Get current user
    local current_user=$(whoami)

    # Check for existing cron
    if crontab -l 2>/dev/null | grep -q "$BACKUP_SCRIPT"; then
        log "⚠ Cron jobs already configured for this user"
        log "Current cron entries:"
        crontab -l | grep -E "(backup|test-backup)" || true
        return 1
    fi

    # Create temporary cron file
    local temp_cron=$(mktemp)

    # Get existing cron entries and add new ones
    (crontab -l 2>/dev/null || echo ""; echo "$cron_entries") > "$temp_cron"

    # Install new cron
    if crontab "$temp_cron"; then
        log "✓ Cron jobs installed successfully"
        log ""
        log "Scheduled backups:"
        log "  Daily at 2 AM (UTC)"
        log "  Monthly restore test on 1st at 3 AM (UTC)"
        log ""
        log "Backup directory: $PROJECT_ROOT/backups"
        log "Log directory: $PROJECT_ROOT/logs"
    else
        error "Failed to install cron jobs"
    fi

    rm -f "$temp_cron"
}

# Setup environment variables
setup_environment() {
    log "Setting up environment variables..."

    local env_file="$PROJECT_ROOT/.env.backup"

    if [ -f "$env_file" ]; then
        log "Environment file already exists: $env_file"
        return 0
    fi

    cat > "$env_file" << 'EOF'
# Database backup configuration
BACKUP_DIR=${BACKUP_DIR:-./backups}
DB_NAME=${DB_NAME:-coordinator_db}
DB_HOST=${DB_HOST:-localhost}
DB_PORT=${DB_PORT:-5432}
DB_USER=${DB_USER:-coordinator}

# Optional: Database password (alternatively use .pgpass)
# DB_PASSWORD=

# Optional: GPG recipient for encrypted backups
# GPG_RECIPIENT=your-gpg-id@example.com

# Backup retention policies (days)
RETENTION_DAYS_DAILY=7
RETENTION_DAYS_WEEKLY=28
RETENTION_DAYS_MONTHLY=365
EOF

    log "✓ Environment file created: $env_file"
    log "  Edit this file to configure backup settings"
}

# Create PostgreSQL password file
setup_pgpass() {
    log "Setting up PostgreSQL password file (.pgpass)..."

    local pgpass_file="$HOME/.pgpass"
    local backup_host="${DB_HOST:-localhost}"
    local backup_port="${DB_PORT:-5432}"
    local backup_user="${DB_USER:-coordinator}"

    if [ -f "$pgpass_file" ]; then
        if grep -q "$backup_host:$backup_port:*:$backup_user" "$pgpass_file" 2>/dev/null; then
            log "⚠ .pgpass entry already exists"
            return 0
        fi
    fi

    cat >> "$pgpass_file" << EOF
$backup_host:$backup_port:*:$backup_user:password
EOF

    chmod 600 "$pgpass_file"
    log "✓ .pgpass configured"
    log "  Edit $pgpass_file to add your database password"
}

# Verify backup scripts
verify_scripts() {
    log "Verifying backup scripts..."

    for script in "$BACKUP_SCRIPT" "$TEST_SCRIPT"; do
        if [ ! -f "$script" ]; then
            error "Script not found: $script"
        fi
        if [ ! -x "$script" ]; then
            error "Script is not executable: $script"
        fi
    done

    success "✓ All backup scripts verified"
}

# Create backup directory structure
setup_directories() {
    log "Setting up backup directory structure..."

    local backup_dir="$PROJECT_ROOT/backups"
    mkdir -p "$backup_dir"/{daily,weekly,monthly}
    mkdir -p "$PROJECT_ROOT/logs"

    log "✓ Directories created:"
    log "  - $backup_dir/daily"
    log "  - $backup_dir/weekly"
    log "  - $backup_dir/monthly"
    log "  - $PROJECT_ROOT/logs"
}

# Test backup script
test_backup() {
    log "Testing backup script..."

    if "$BACKUP_SCRIPT" --help > /dev/null 2>&1; then
        log "✓ Backup script is functional"
    else
        log "⚠ Could not verify backup script functionality"
    fi
}

# Print summary
print_summary() {
    log ""
    log "========================================="
    log "Backup Automation Setup Complete"
    log "========================================="
    log ""
    log "Configuration files:"
    log "  - Backup scripts: $SCRIPT_DIR/"
    log "  - Environment: $PROJECT_ROOT/.env.backup"
    log "  - Cron schedule: See 'crontab -l'"
    log ""
    log "Next steps:"
    log "  1. Configure .env.backup with your database credentials"
    log "  2. Set up .pgpass or environment variables for auth"
    log "  3. Test backup manually: $BACKUP_SCRIPT"
    log "  4. Monitor backup logs: $PROJECT_ROOT/logs/backup.log"
    log ""
    log "To remove cron jobs later:"
    log "  crontab -e  # Remove backup-related entries"
    log "========================================="
}

# Main execution
main() {
    log "========================================="
    log "Backup Automation Setup"
    log "========================================="
    log ""

    check_os
    verify_scripts
    setup_directories
    setup_environment
    setup_pgpass
    install_cron_jobs
    test_backup
    print_summary
}

# Run if called directly
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    main "$@"
fi
