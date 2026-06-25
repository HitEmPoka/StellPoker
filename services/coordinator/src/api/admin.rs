use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::AppState;

const ADMIN_SKEW_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdminRole {
    ReadOnly = 1,
    Operator = 2,
    SuperAdmin = 3,
}

impl AdminRole {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "super-admin" | "superadmin" => Some(AdminRole::SuperAdmin),
            "operator" => Some(AdminRole::Operator),
            "read-only" | "readonly" | "read_only" => Some(AdminRole::ReadOnly),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AdminRole::SuperAdmin => "super-admin",
            AdminRole::Operator => "operator",
            AdminRole::ReadOnly => "read-only",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdminEntry {
    pub key: String,
    pub role: AdminRole,
}

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub entries: Vec<AdminEntry>,
    pub key_by_address: HashMap<String, AdminRole>,
}

impl AdminConfig {
    pub fn from_env() -> Self {
        let raw = std::env::var("ADMIN_KEYS").unwrap_or_default();
        let entries = if raw.is_empty() {
            Vec::new()
        } else {
            serde_json::from_str::<Vec<serde_json::Value>>(&raw)
                .ok()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            let key = v.get("key")?.as_str()?.to_string();
                            let role = AdminRole::from_str(v.get("role")?.as_str()?)?;
                            Some(AdminEntry { key, role })
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let key_by_address = entries
            .iter()
            .map(|e| (e.key.clone(), e.role))
            .collect();

        if !entries.is_empty() {
            tracing::info!(
                "Admin RBAC configured: {} admin key(s) loaded",
                entries.len()
            );
            for entry in &entries {
                tracing::debug!(
                    "  Admin key: address={}, role={}",
                    entry.key,
                    entry.role.as_str()
                );
            }
        } else {
            tracing::warn!("Admin RBAC not configured (ADMIN_KEYS not set or empty)");
        }

        Self {
            entries,
            key_by_address,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdminAuth {
    pub address: String,
    pub role: AdminRole,
}

#[derive(Clone)]
pub struct AdminState {
    pub last_nonce_by_address: Arc<RwLock<HashMap<String, u64>>>,
}

impl AdminState {
    pub fn new() -> Self {
        Self {
            last_nonce_by_address: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// Validate an admin-signed request. Returns the authenticated admin context
/// with the assigned role from the allowlist.
///
/// Headers:
///   x-admin-address: Stellar G... public key
///   x-admin-signature: Ed25519 signature (hex or base64)
///   x-admin-nonce: strictly increasing u64 per-address for replay protection
///   x-admin-timestamp: unix epoch seconds, ±300s skew allowed
///
/// The signed message format is:
///   stellar-poker-admin|{address}|{action}|{nonce}|{timestamp}
pub async fn validate_admin_request(
    state: &AppState,
    headers: &HeaderMap,
    action: &str,
    admin_state: &AdminState,
) -> Result<AdminAuth, StatusCode> {
    let address = header_string(headers, "x-admin-address")?;

    if !is_valid_stellar_address(&address) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Check the address is in the admin allowlist and get its role.
    let admin_config = state.admin_config.read().await;
    let role = admin_config
        .key_by_address
        .get(&address)
        .copied()
        .ok_or(StatusCode::FORBIDDEN)?;
    drop(admin_config);

    let insecure_auth = allow_insecure_dev_auth();
    if insecure_auth {
        return Ok(AdminAuth { address, role });
    }

    let signature_raw = header_string(headers, "x-admin-signature")?;
    let nonce = header_string(headers, "x-admin-nonce")
        .and_then(|v| v.parse::<u64>().map_err(|_| StatusCode::UNAUTHORIZED))?;
    let timestamp = header_string(headers, "x-admin-timestamp")
        .and_then(|v| v.parse::<i64>().map_err(|_| StatusCode::UNAUTHORIZED))?;

    let now = now_unix_secs_i64()?;
    if (now - timestamp).abs() > ADMIN_SKEW_SECS {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let message = admin_auth_message(&address, action, nonce, timestamp);
    verify_signature(&address, &message, &signature_raw)?;

    // Replay protection: strictly increasing nonces per admin address.
    let mut nonce_store = admin_state.last_nonce_by_address.write().await;
    if let Some(last_nonce) = nonce_store.get(&address) {
        if nonce <= *last_nonce {
            return Err(StatusCode::CONFLICT);
        }
    }
    nonce_store.insert(address.clone(), nonce);

    Ok(AdminAuth { address, role })
}

/// Require a minimum admin role. Returns FORBIDDEN if the role is insufficient.
pub fn require_role(auth: &AdminAuth, min_role: AdminRole) -> Result<(), StatusCode> {
    if auth.role < min_role {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

fn verify_signature(address: &str, message: &str, signature_raw: &str) -> Result<(), StatusCode> {
    let stellar_pk = stellar_strkey::ed25519::PublicKey::from_string(address)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let verifying_key =
        VerifyingKey::from_bytes(&stellar_pk.0).map_err(|_| StatusCode::UNAUTHORIZED)?;

    let signature = decode_signature(signature_raw)?;

    if verifying_key.verify(message.as_bytes(), &signature).is_ok() {
        return Ok(());
    }

    // SEP-53: SHA256("Stellar Signed Message:\n" + message)
    let mut hasher = Sha256::new();
    hasher.update(b"Stellar Signed Message:\n");
    hasher.update(message.as_bytes());
    let message_hash: [u8; 32] = hasher.finalize().into();

    verifying_key
        .verify(&message_hash, &signature)
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

fn decode_signature(signature_raw: &str) -> Result<Signature, StatusCode> {
    let s = signature_raw.trim();

    let decoded = if let Some(hex) = s.strip_prefix("0x") {
        hex::decode(hex).map_err(|_| StatusCode::UNAUTHORIZED)?
    } else if s.len() == 128 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        hex::decode(s).map_err(|_| StatusCode::UNAUTHORIZED)?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    };

    let normalized: [u8; 64] = if decoded.len() == 64 {
        decoded
            .as_slice()
            .try_into()
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    } else if decoded.len() == 68 {
        decoded[4..68]
            .try_into()
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    } else if decoded.len() == 72 && decoded[4..8] == [0, 0, 0, 64] {
        decoded[8..72]
            .try_into()
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    Ok(Signature::from_bytes(&normalized))
}

fn admin_auth_message(address: &str, action: &str, nonce: u64, timestamp: i64) -> String {
    format!(
        "stellar-poker-admin|{}|{}|{}|{}",
        address, action, nonce, timestamp
    )
}

fn allow_insecure_dev_auth() -> bool {
    match std::env::var("ALLOW_INSECURE_DEV_AUTH") {
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

fn is_valid_stellar_address(address: &str) -> bool {
    stellar_strkey::ed25519::PublicKey::from_string(address).is_ok()
}

fn now_unix_secs_i64() -> Result<i64, StatusCode> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    i64::try_from(now.as_secs()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
