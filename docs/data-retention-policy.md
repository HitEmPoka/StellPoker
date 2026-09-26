# Coordinator Data Retention and Deletion Policy

## Overview

This document outlines the Stellar Poker Coordinator's PII (Personally Identifiable Information) retention windows and secure deletion policies, designed to comply with GDPR and other privacy regulations.

## Data Categories

### 1. Session Records
- **Description**: Session metadata including player IDs, session start/end times, table assignments
- **Retention Window**: 90 days
- **Rationale**: Sufficient for dispute resolution and gaming audits; shorter window minimizes PII exposure
- **Deletion Method**: Cryptographic erasure (overwrite with random data before deallocation)

### 2. Wallet Address Logs
- **Description**: Stellar wallet addresses associated with accounts, transaction records
- **Retention Window**: 365 days (1 year)
- **Rationale**: Required for GDPR compliance and regulatory audit trails
- **Deletion Method**: Secure deletion via `shred` equivalent; audit trail retained without address details

### 3. Temporary Session Metadata
- **Description**: MPC session commitments, hand commitments, board reveals
- **Retention Window**: 30 days
- **Rationale**: Temporary coordination data; minimal user-facing PII
- **Deletion Method**: Immediate deallocation after session completion; batch deletion on retention expiry

### 4. Payment Metadata
- **Description**: Transaction hashes, token transfers (non-sensitive)
- **Retention Window**: 365 days
- **Rationale**: Required for financial auditability
- **Deletion Method**: Pseudonymized before deletion (addresses removed, amounts retained)

## Deletion Process

### Automated Deletion Job

The Coordinator runs a scheduled deletion job (configurable frequency; default: daily at 00:00 UTC) that:

1. **Scans** all data records for expiration based on creation timestamp
2. **Identifies** records exceeding their retention windows
3. **Audits** the deletion operation (logs count, type, timestamp, reason)
4. **Securely deletes** using cryptographic erasure (overwrite with random bits)
5. **Verifies** deletion by attempting to read deleted keys; logs any failures

### Deletion Triggers

- **Automatic TTL Expiry**: Scheduled job (see above)
- **User Request**: Players may request deletion of their session data at any time (logged to audit trail)
- **Legal Hold**: Admin may mark records for indefinite retention (GDPR legal basis)
- **Regulatory Requirement**: Delete or pseudonymize on demand (coordinated with compliance team)

## Audit Trail

Every deletion operation is logged with:
- **Timestamp**: Unix timestamp (UTC) of deletion
- **Data Type**: Category of deleted records (session, wallet_log, etc.)
- **Record Count**: Number of records deleted
- **Reason**: Automated TTL expiry, user request, compliance hold, etc.
- **Operator**: Admin or system identity that triggered deletion

Audit logs themselves are retained indefinitely (no PII, only aggregate counts and reasons).

## Configuration

### Environment Variables

```bash
# Session retention in seconds (default: 7776000 = 90 days)
COORDINATOR_SESSION_RETENTION_SECONDS=7776000

# Wallet log retention in seconds (default: 31536000 = 365 days)
COORDINATOR_WALLET_LOG_RETENTION_SECONDS=31536000

# Temp metadata retention in seconds (default: 2592000 = 30 days)
COORDINATOR_TEMP_METADATA_RETENTION_SECONDS=2592000

# Deletion job frequency in seconds (default: 86400 = daily)
COORDINATOR_DELETION_JOB_INTERVAL_SECONDS=86400
```

### Rust Configuration

```rust
use coordinator::cache::RetentionConfig;

let config = RetentionConfig {
    session_retention_seconds: 90 * 24 * 60 * 60,      // 90 days
    wallet_log_retention_seconds: 365 * 24 * 60 * 60,  // 1 year
    temp_metadata_retention_seconds: 30 * 24 * 60 * 60, // 30 days
};
```

## User Privacy Rights

### Right to Access
Players may request a data export via the `/api/gdpr/export` endpoint, receiving:
- All session records associated with their wallet
- Timestamps and game summaries (hand results, payouts)
- PII retained about them (address, session metadata)

### Right to Deletion
Players may request deletion of their session data via `/api/gdpr/delete`:
- Sessions older than 30 days are immediately deleted
- Recent sessions are flagged for expedited deletion (within 7 days)
- Request is logged with player consent timestamp

### Right to Rectification
Players may request correction of personal data via `/api/gdpr/rectify`:
- Updates are only permitted for non-game-affecting fields (e.g., preferred name)
- Corrections are logged with before/after values for audit

## Compliance

### GDPR (EU/EEA)
- ✅ Lawful basis: Contractual necessity (gaming) + legitimate interest (fraud prevention)
- ✅ Data minimization: Retention windows sized to actual need
- ✅ User rights: Access, deletion, rectification endpoints provided
- ✅ Privacy by design: Deletion job integrated into Coordinator startup

### CCPA (California)
- ✅ Notice: Privacy policy published at `/privacy`
- ✅ Opt-out: Players may request non-sale of data (coordination with legal team)
- ✅ Deletion: Processed within 45 days via `/api/gdpr/delete`

## Testing

### Unit Tests
```bash
cargo test --package coordinator --lib cache::tests
```

Covers:
- Retention window calculations
- Deletion eligibility checks
- Audit log recording

### Integration Tests
```bash
cargo test --package coordinator --test integration -- --ignored deletion
```

Simulates:
- Full deletion job execution with sample data
- Verification of deletion completeness
- Audit trail correctness

## Operational Runbook

### Monitoring
- Alert if deletion job fails: `coordinator_deletion_job_failed`
- Alert if audit log grows unexpectedly: `coordinator_audit_log_size_high`
- Log retention metrics daily: `coordinator_pii_retention_metrics`

### Incident Response
If data exposure is suspected:
1. Enable "legal hold" for affected user records: `coordinator-cli hold --wallet <address>`
2. Notify legal/compliance team
3. Archive audit logs to cold storage
4. Investigate root cause and patch

## References
- [Rust coordinator cache module](../../coordinator/src/cache.rs)
- [GDPR Article 17: Right to erasure](https://gdpr-info.eu/articles/right-to-erasure/)
- [CCPA §1798.100: Consumer Privacy Rights](https://leginfo.legislature.ca.gov/faces/codes_displayText.xhtml?division=3.&part=4.&lawCode=CC&title=1.2)
