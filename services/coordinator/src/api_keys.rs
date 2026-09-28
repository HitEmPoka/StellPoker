//! API key authentication for MPC nodes.
//!
//! Provides secure authentication for coordinator-to-node communication:
//! - SHA-256 hashed keys stored in database
//! - Periodic key rotation support
//! - Immediate revocation for compromised keys
//! - Usage tracking and monitoring

use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use chrono::{DateTime, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Pool, Postgres};

const API_KEY_PREFIX: &str = "sk_";
const KEY_LENGTH: usize = 32; // 256 bits

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ApiKey {
    pub id: i64,
    pub key_id: String,
    pub node_id: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
}

/// Generate a new API key with secure random bytes
pub fn generate_api_key() -> String {
    let mut key_bytes = [0u8; KEY_LENGTH];
    rand::thread_rng().fill_bytes(&mut key_bytes);
    format!("{}{}", API_KEY_PREFIX, base64::engine::general_purpose::STANDARD.encode(&key_bytes))
}

/// Hash an API key for secure storage
fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Create a new API key for a node
pub async fn create_api_key(
    pool: &Pool<Postgres>,
    node_id: &str,
    description: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(String, ApiKey), sqlx::Error> {
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);
    let key_id = format!("key_{}", uuid::Uuid::new_v4().simple());

    let record = sqlx::query_as::<_, ApiKey>(
        r#"
        INSERT INTO api_keys (key_id, key_hash, node_id, description, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, key_id, node_id, description, is_active, created_at, expires_at, last_used_at, revoked_at, revoked_reason
        "#,
    )
    .bind(&key_id)
    .bind(&key_hash)
    .bind(node_id)
    .bind(description)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    let api_key_record = record;

    Ok((api_key, api_key_record))
}

/// Validate an API key and return the associated node_id
pub async fn validate_api_key(
    pool: &Pool<Postgres>,
    api_key: &str,
) -> Result<Option<String>, sqlx::Error> {
    let key_hash = hash_api_key(api_key);
    let now = Utc::now();

    let record = sqlx::query_as::<
        _,
        (String, bool, Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    >(
        r#"
        SELECT node_id, is_active, expires_at, revoked_at
        FROM api_keys
        WHERE key_hash = $1
        "#,
    )
    .bind(&key_hash)
    .fetch_optional(pool)
    .await?;

    if let Some((node_id, is_active, expires_at, revoked_at)) = record {
        // Check if key is active
        if !is_active {
            return Ok(None);
        }

        // Check if key is revoked
        if revoked_at.is_some() {
            return Ok(None);
        }

        // Check if key is expired
        if let Some(expires_at) = expires_at {
            if now > expires_at {
                return Ok(None);
            }
        }

        // Update last_used_at
        sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE key_hash = $2")
            .bind(now)
            .bind(&key_hash)
            .execute(pool)
            .await?;

        Ok(Some(node_id))
    } else {
        Ok(None)
    }
}

/// Revoke an API key
pub async fn revoke_api_key(
    pool: &Pool<Postgres>,
    key_id: &str,
    reason: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let rows_affected = sqlx::query(
        r#"
        UPDATE api_keys 
        SET is_active = false, revoked_at = $1, revoked_reason = $2
        WHERE key_id = $3 AND is_active = true
        "#,
    )
    .bind(Utc::now())
    .bind(reason)
    .bind(key_id)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(rows_affected > 0)
}

/// List API keys for a node
pub async fn list_api_keys(
    pool: &Pool<Postgres>,
    node_id: &str,
) -> Result<Vec<ApiKey>, sqlx::Error> {
    let records = sqlx::query_as::<_, ApiKey>(
        r#"
        SELECT id, key_id, node_id, description, is_active, created_at, expires_at, last_used_at, revoked_at, revoked_reason
        FROM api_keys
        WHERE node_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;

    let keys = records;

    Ok(keys)
}

/// Log API key usage
pub async fn log_api_key_usage(
    pool: &Pool<Postgres>,
    key_id: &str,
    node_id: &str,
    endpoint: &str,
    ip_address: Option<&str>,
    success: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO api_key_usage_log (key_id, node_id, endpoint, ip_address, success)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(key_id)
    .bind(node_id)
    .bind(endpoint)
    .bind(ip_address)
    .bind(success)
    .execute(pool)
    .await?;

    Ok(())
}

/// Middleware function to authenticate MPC node requests
pub async fn authenticate_mpc_node(
    pool: &Pool<Postgres>,
    headers: &HeaderMap,
    endpoint: &str,
    ip_address: Option<&str>,
) -> Result<String, StatusCode> {
    let api_key = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let node_id = validate_api_key(pool, api_key)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Extract key_id from the API key for logging
    let key_id = if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&api_key[API_KEY_PREFIX.len()..]) {
        format!("key_{}", hex::encode(&decoded[..8])) // Use first 8 bytes as identifier
    } else {
        "unknown".to_string()
    };

    // Log successful authentication
    if let Err(e) = log_api_key_usage(pool, &key_id, &node_id, endpoint, ip_address, true).await {
        tracing::warn!("Failed to log API key usage: {}", e);
    }

    Ok(node_id)
}

/// Clean up expired keys (should be run periodically)
pub async fn cleanup_expired_keys(pool: &Pool<Postgres>) -> Result<u64, sqlx::Error> {
    let now = Utc::now();
    let result = sqlx::query(
        r#"
        UPDATE api_keys 
        SET is_active = false, revoked_at = $1, revoked_reason = 'Expired'
        WHERE expires_at < $1 AND is_active = true
        "#,
    )
    .bind(now)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
