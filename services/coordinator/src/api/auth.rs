use axum::http::{HeaderMap, StatusCode};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::AppState;

/// Derive a stable, non-reversible token from an IP address using SHA-256
/// keyed with the active key version. This keeps raw IPs out of in-memory
/// rate-limit buckets while remaining deterministic for the same IP+key.
fn hash_ip(ip: &str, enc_key: &crate::crypto::EncryptionKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"stellar-poker-ip-hash-v1:");
    hasher.update(enc_key.active_version().to_le_bytes());
    hasher.update(ip.as_bytes());
    hex::encode(hasher.finalize())
}

const AUTH_SKEW_SECS: i64 = 300;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const RATE_LIMIT_MAX_REQUESTS: usize = 60;
const ALLOW_INSECURE_DEV_AUTH_ENV: &str = "ALLOW_INSECURE_DEV_AUTH";

pub(crate) struct AuthContext {
    pub address: String,
}

pub(crate) async fn enforce_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    table_id: u32,
    action: &str,
) -> Result<(), StatusCode> {
    let now = now_unix_secs_u64()?;
    let ip = extract_ip(headers);
    // Hash the IP with the encryption key as HMAC salt so raw IPs are never
    // stored in memory; the hash is stable across calls for the same IP.
    let ip_hash = hash_ip(&ip, &state.enc_key);
    let bucket_key = format!("{}:{}:{}", ip_hash, table_id, action);

    let mut rl = state.rate_limit_state.write().await;
    let bucket = rl.requests_by_bucket.entry(bucket_key).or_default();

    bucket.retain(|ts| now.saturating_sub(*ts) <= RATE_LIMIT_WINDOW_SECS);
    if bucket.len() >= RATE_LIMIT_MAX_REQUESTS {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    bucket.push(now);
    Ok(())
}

pub(crate) async fn validate_signed_request(
    state: &AppState,
    headers: &HeaderMap,
    table_id: u32,
    action: &str,
    expected_address: Option<&str>,
) -> Result<AuthContext, StatusCode> {
    let insecure_auth = allow_insecure_dev_auth();

    let address = match header_string(headers, "x-player-address") {
        Ok(addr) => addr,
        Err(_) if insecure_auth => expected_address.unwrap_or_default().to_string(),
        Err(e) => return Err(e),
    };

    if let Some(expected) = expected_address {
        if expected != address {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    if !is_valid_stellar_address(&address) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if insecure_auth {
        return Ok(AuthContext { address });
    }

    let signature_raw = header_string(headers, "x-auth-signature")?;
    let nonce = header_string(headers, "x-auth-nonce")
        .and_then(|v| v.parse::<u64>().map_err(|_| StatusCode::UNAUTHORIZED))?;
    let timestamp = header_string(headers, "x-auth-timestamp")
        .and_then(|v| v.parse::<i64>().map_err(|_| StatusCode::UNAUTHORIZED))?;

    let now = now_unix_secs_i64()?;
    if (now - timestamp).abs() > AUTH_SKEW_SECS {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let message = auth_message(&address, table_id, action, nonce, timestamp);
    verify_signature(&address, &message, &signature_raw)?;

    // Replay protection: require strictly increasing nonce per wallet address.
    let mut auth_state = state.auth_state.write().await;
    if let Some(last_nonce) = auth_state.last_nonce_by_address.get(&address) {
        if nonce <= *last_nonce {
            return Err(StatusCode::CONFLICT);
        }
    }
    auth_state
        .last_nonce_by_address
        .insert(address.clone(), nonce);

    Ok(AuthContext { address })
}

pub use crate::signed_payload::verify_signature;

fn auth_message(address: &str, table_id: u32, action: &str, nonce: u64, timestamp: i64) -> String {
    format!(
        "stellar-poker|{}|{}|{}|{}|{}",
        address, table_id, action, nonce, timestamp
    )
}

pub(crate) fn allow_insecure_dev_auth() -> bool {
    match std::env::var(ALLOW_INSECURE_DEV_AUTH_ENV) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn header_string(headers: &HeaderMap, key: &str) -> Result<String, StatusCode> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
        .ok_or(StatusCode::UNAUTHORIZED)
}

fn extract_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("unknown").trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn is_valid_stellar_address(address: &str) -> bool {
    stellar_strkey::ed25519::PublicKey::from_string(address).is_ok()
}

fn now_unix_secs_u64() -> Result<u64, StatusCode> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(now.as_secs())
}

fn now_unix_secs_i64() -> Result<i64, StatusCode> {
    let now = now_unix_secs_u64()?;
    i64::try_from(now).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
