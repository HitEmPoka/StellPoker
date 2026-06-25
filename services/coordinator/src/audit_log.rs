//! Tamper-evident audit logging for compliance
//!
//! Issue #265: Add coordinator request audit logging for compliance
//! Log all coordinator requests and responses in a tamper-evident audit log.
//! Include: timestamp, requester identity, action, IP, and response status.
//! Store in append-only format with hash chaining.

use axum::http::{HeaderMap, Method};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditLog {
    pub id: i64,
    pub request_id: Uuid,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub requester_address: Option<String>,
    pub action: String,
    pub endpoint: String,
    pub method: String,
    pub ip_address: Option<String>,
    pub response_status: Option<i32>,
    pub error_message: Option<String>,
    pub table_id: Option<i32>,
    pub session_id: Option<String>,
    pub previous_hash: Option<String>,
    pub record_hash: String,
}

/// Extract IP address from request headers
pub fn extract_ip_address(headers: &HeaderMap) -> Option<String> {
    // Check common proxy headers first
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
}

/// Extract requester address from headers
pub fn extract_requester_address(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-wallet-address")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("x-admin-address")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
}

/// Extract table ID from path
pub fn extract_table_id(path: &str) -> Option<i32> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(idx) = segments.iter().position(|&s| s == "table") {
        if let Some(id_str) = segments.get(idx + 1) {
            return id_str.parse::<i32>().ok();
        }
    }
    None
}

/// Extract session ID from path
pub fn extract_session_id(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(idx) = segments.iter().position(|&s| s == "session") {
        if let Some(id) = segments.get(idx + 1) {
            return Some(id.to_string());
        }
    }
    None
}

/// Compute SHA256 hash of audit record for tamper detection
fn compute_record_hash(
    request_id: &Uuid,
    timestamp: &chrono::DateTime<chrono::Utc>,
    requester_address: Option<&str>,
    action: &str,
    endpoint: &str,
    method: &str,
    ip_address: Option<&str>,
    response_status: Option<i32>,
    previous_hash: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request_id.to_string().as_bytes());
    hasher.update(timestamp.to_rfc3339().as_bytes());
    hasher.update(requester_address.unwrap_or("").as_bytes());
    hasher.update(action.as_bytes());
    hasher.update(endpoint.as_bytes());
    hasher.update(method.as_bytes());
    hasher.update(ip_address.unwrap_or("").as_bytes());
    hasher.update(response_status.unwrap_or(0).to_string().as_bytes());
    hasher.update(previous_hash.unwrap_or("").as_bytes());

    format!("{:x}", hasher.finalize())
}

/// Get the hash of the most recent audit log entry
async fn get_latest_hash(pool: &PgPool) -> Result<Option<String>, sqlx::Error> {
    let result: Option<(String,)> =
        sqlx::query_as("SELECT record_hash FROM audit_logs ORDER BY id DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;

    Ok(result.map(|r| r.0))
}

/// Log an audit entry (append-only with hash chain)
pub async fn log_audit_entry(
    pool: &PgPool,
    request_id: Uuid,
    requester_address: Option<&str>,
    action: &str,
    endpoint: &str,
    method: &Method,
    ip_address: Option<&str>,
    response_status: Option<i32>,
    error_message: Option<&str>,
    table_id: Option<i32>,
    session_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    let timestamp = chrono::Utc::now();
    let method_str = method.as_str();

    // Get the previous hash for chaining
    let previous_hash = get_latest_hash(pool).await?;

    // Compute hash for this record
    let record_hash = compute_record_hash(
        &request_id,
        &timestamp,
        requester_address,
        action,
        endpoint,
        method_str,
        ip_address,
        response_status,
        previous_hash.as_deref(),
    );

    sqlx::query(
        "INSERT INTO audit_logs 
         (request_id, timestamp, requester_address, action, endpoint, method, 
          ip_address, response_status, error_message, table_id, session_id, 
          previous_hash, record_hash) 
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(request_id)
    .bind(timestamp)
    .bind(requester_address)
    .bind(action)
    .bind(endpoint)
    .bind(method_str)
    .bind(ip_address)
    .bind(response_status)
    .bind(error_message)
    .bind(table_id)
    .bind(session_id)
    .bind(previous_hash)
    .bind(record_hash)
    .execute(pool)
    .await?;

    Ok(())
}

/// Query audit logs with filters
pub async fn query_audit_logs(
    pool: &PgPool,
    requester_address: Option<&str>,
    action: Option<&str>,
    table_id: Option<i32>,
    from_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    to_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditLog>, sqlx::Error> {
    let mut query = String::from(
        "SELECT id, request_id, timestamp, requester_address, action, endpoint, method, 
         ip_address, response_status, error_message, table_id, session_id, 
         previous_hash, record_hash 
         FROM audit_logs WHERE 1=1",
    );

    if requester_address.is_some() {
        query.push_str(" AND requester_address = $1");
    }
    if action.is_some() {
        query.push_str(" AND action = $2");
    }
    if table_id.is_some() {
        query.push_str(" AND table_id = $3");
    }
    if from_timestamp.is_some() {
        query.push_str(" AND timestamp >= $4");
    }
    if to_timestamp.is_some() {
        query.push_str(" AND timestamp <= $5");
    }

    query.push_str(" ORDER BY id DESC LIMIT $6 OFFSET $7");

    let mut q = sqlx::query_as::<_, AuditLog>(&query);

    if let Some(addr) = requester_address {
        q = q.bind(addr);
    }
    if let Some(act) = action {
        q = q.bind(act);
    }
    if let Some(tid) = table_id {
        q = q.bind(tid);
    }
    if let Some(from) = from_timestamp {
        q = q.bind(from);
    }
    if let Some(to) = to_timestamp {
        q = q.bind(to);
    }

    q.bind(limit).bind(offset).fetch_all(pool).await
}

/// Verify the integrity of the audit log chain
pub async fn verify_audit_chain(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let logs = sqlx::query_as::<_, AuditLog>(
        "SELECT id, request_id, timestamp, requester_address, action, endpoint, method, 
         ip_address, response_status, error_message, table_id, session_id, 
         previous_hash, record_hash 
         FROM audit_logs 
         ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut previous_hash: Option<String> = None;

    for log in logs {
        // Recompute hash
        let computed_hash = compute_record_hash(
            &log.request_id,
            &log.timestamp,
            log.requester_address.as_deref(),
            &log.action,
            &log.endpoint,
            &log.method,
            log.ip_address.as_deref(),
            log.response_status,
            previous_hash.as_deref(),
        );

        // Verify it matches stored hash
        if computed_hash != log.record_hash {
            tracing::error!(
                "Audit chain integrity violation: log id={}, expected hash={}, actual={}",
                log.id,
                computed_hash,
                log.record_hash
            );
            return Ok(false);
        }

        // Verify chain link
        if log.previous_hash != previous_hash {
            tracing::error!(
                "Audit chain link broken at log id={}: expected previous={:?}, actual={:?}",
                log.id,
                previous_hash,
                log.previous_hash
            );
            return Ok(false);
        }

        previous_hash = Some(log.record_hash);
    }

    Ok(true)
}
