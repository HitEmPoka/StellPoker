//! Handlers for the feature-flag admin API.
//!
//! `GET  /api/flags`       — returns a JSON snapshot of all flag values.
//! `POST /api/flags/:key`  — sets a specific flag key (body: `{"enabled": bool}`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::collections::HashMap;
use utoipa::ToSchema;

use crate::AppState;

/// GET /api/flags
///
/// Returns every flag key currently held in the store, including any
/// per-table / per-player scoped overrides that were loaded from env vars
/// or set via the admin endpoint at runtime.
#[utoipa::path(
    get,
    path = "/api/flags",
    tag = "Flags",
    responses(
        (status = 200, description = "Feature flag snapshot", body = HashMap<String, bool>)
    )
)]
pub async fn list_flags(State(state): State<AppState>) -> Json<HashMap<String, bool>> {
    let snap = state.feature_flags.snapshot().await;
    Json(snap)
}

/// POST /api/flags/:key
///
/// Set or override a flag value at runtime.
///
/// # Path parameters
/// - `key` — the full flag key, e.g. `solo_mode`, `chat_enabled.table.3`
///
/// # Request body
/// ```json
/// { "enabled": true }
/// ```
///
/// Returns `200 OK` on success, `400 Bad Request` if the key is empty.
#[utoipa::path(
    post,
    path = "/api/flags/{key}",
    tag = "Flags",
    params(
        ("key" = String, Path, description = "Full flag key, e.g. `solo_mode`, `chat_enabled.table.3`")
    ),
    request_body = SetFlagBody,
    responses(
        (status = 200, description = "Flag updated"),
        (status = 400, description = "Empty key")
    )
)]
pub async fn set_flag(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<super::types::SetFlagBody>,
) -> StatusCode {
    let key = key.trim().to_string();
    // Reject empty/oversized/control-character keys before they ever reach
    // the flag store or the audit log (this endpoint takes untrusted input
    // from an unauthenticated route — see the PR note on that).
    if key.is_empty() || key.len() > 128 || key.chars().any(|c| c.is_control()) {
        return StatusCode::BAD_REQUEST;
    }
    state.feature_flags.set_flag(&key, body.enabled).await;

    // Issue #603: runtime flag changes had no audit trail — reuse the
    // existing compliance audit log rather than inventing a new store.
    // `action` records the resulting value since log_audit_entry has no
    // free-form details column to put it in.
    if let Some(pool) = &state.db_pool {
        let _ = crate::audit_log::log_audit_entry(
            pool,
            uuid::Uuid::new_v4(),
            None,
            &format!("feature_flag_changed:enabled={}", body.enabled),
            &format!("/api/flags/{key}"),
            &axum::http::Method::POST,
            None,
            Some(200),
            None,
            None,
            None,
        )
        .await;
    }

    StatusCode::OK
}

/// Request body for `POST /api/flags/:key`.
#[derive(ToSchema)]
pub struct SetFlagBody {
    pub enabled: bool,
}
