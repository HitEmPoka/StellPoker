use axum::http::{HeaderMap, StatusCode};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::{signed_payload, AppState};

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
    /// Empty allowlist used by tests and by a coordinator with no admin keys.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            key_by_address: HashMap::new(),
        }
    }

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

        let key_by_address = entries.iter().map(|e| (e.key.clone(), e.role)).collect();

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

/// State needed by the global `/api/admin/*` authorization middleware.
///
/// This deliberately carries only the admin allowlist and nonce store so the
/// middleware can be unit-tested without constructing a full `AppState`.
#[derive(Clone)]
pub struct AdminAuthGuardState {
    pub config: Arc<RwLock<AdminConfig>>,
    pub nonce_state: AdminState,
}

impl AdminAuthGuardState {
    pub fn new(config: AdminConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            nonce_state: AdminState::new(),
        }
    }

    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            config: state.admin_config.clone(),
            nonce_state: state.admin_state.clone(),
        }
    }
}

/// The canonical signed-payload challenge format for admin requests.
pub fn admin_auth_message(address: &str, action: &str, nonce: u64, timestamp: i64) -> String {
    format!(
        "stellar-poker-admin|{}|{}|{}|{}",
        address, action, nonce, timestamp
    )
}

/// Read a required header, returning 401 when it is absent or not UTF-8.
pub(crate) fn header_string(headers: &HeaderMap, key: &str) -> Result<String, StatusCode> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
        .ok_or(StatusCode::UNAUTHORIZED)
}

/// Parse the replay-protection nonce from `x-admin-nonce`.
pub fn nonce_from_headers(headers: &HeaderMap) -> Result<u64, StatusCode> {
    header_string(headers, "x-admin-nonce")?
        .parse::<u64>()
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

/// Verify an admin-signed request without consuming its nonce.
///
/// The global middleware calls this before any admin handler runs. Replay
/// protection is applied once by `consume_admin_nonce`; handlers call
/// `validate_admin_request` for their own action-scoped signature check.
///
/// Headers:
///   x-admin-address: Stellar G... public key
///   x-admin-signature: Ed25519 signature (hex or base64)
///   x-admin-action: action the caller signed (must match the handler action)
///   x-admin-nonce: strictly increasing u64 per-address for replay protection
///   x-admin-timestamp: unix epoch seconds, +/-300s skew allowed
pub async fn authenticate_admin_request(
    guard: &AdminAuthGuardState,
    headers: &HeaderMap,
    action: &str,
) -> Result<AdminAuth, StatusCode> {
    let address = header_string(headers, "x-admin-address")?;

    if !is_valid_stellar_address(&address) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Allowlist comes from ADMIN_KEYS configuration, never from client code.
    let role = guard
        .config
        .read()
        .await
        .key_by_address
        .get(&address)
        .copied()
        .ok_or(StatusCode::FORBIDDEN)?;

    let signature_raw = header_string(headers, "x-admin-signature")?;
    let nonce = nonce_from_headers(headers)?;
    let timestamp = header_string(headers, "x-admin-timestamp")?
        .parse::<i64>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let now = now_unix_secs_i64()?;
    if (now - timestamp).abs() > ADMIN_SKEW_SECS {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let message = admin_auth_message(&address, action, nonce, timestamp);
    signed_payload::verify_signature(&address, &message, &signature_raw)?;

    Ok(AdminAuth { address, role })
}

/// Enforce strictly increasing nonces. Called once per request by the global
/// admin middleware so replayed signed payloads cannot reach any handler.
pub async fn consume_admin_nonce(
    guard: &AdminAuthGuardState,
    address: &str,
    nonce: u64,
) -> Result<(), StatusCode> {
    let mut nonce_store = guard.nonce_state.last_nonce_by_address.write().await;
    if let Some(last_nonce) = nonce_store.get(address) {
        if nonce <= *last_nonce {
            return Err(StatusCode::CONFLICT);
        }
    }
    nonce_store.insert(address.to_string(), nonce);
    Ok(())
}

/// Validate an admin-signed request from a handler.
///
/// The global middleware has already authenticated the signature and consumed
/// the nonce; this re-checks the action-scoped signature so a caller cannot
/// reuse a signature minted for a different admin operation.
pub async fn validate_admin_request(
    state: &AppState,
    headers: &HeaderMap,
    action: &str,
    admin_state: &AdminState,
) -> Result<AdminAuth, StatusCode> {
    let guard = AdminAuthGuardState {
        config: state.admin_config.clone(),
        nonce_state: admin_state.clone(),
    };
    authenticate_admin_request(&guard, headers, action).await
}

/// Require a minimum admin role. Returns FORBIDDEN if the role is insufficient.
pub fn require_role(auth: &AdminAuth, min_role: AdminRole) -> Result<(), StatusCode> {
    if auth.role < min_role {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
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
