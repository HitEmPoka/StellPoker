//! Operational control endpoints: maintenance toggle, blue-green drain
//! protocol, readiness probe and circuit-rollout administration.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

use super::admin::{self, AdminRole};
use crate::errors::ApiError;
use crate::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct MaintenanceBody {
    /// Omit to toggle the current value.
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DrainBody {
    pub reason: Option<String>,
    pub grace_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RolloutRollbackBody {
    /// Circuit kind, or `"*"` for every circuit. Defaults to `"*"`.
    #[serde(default)]
    pub circuit: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RolloutPromoteBody {
    pub circuit: String,
}

/// GET /api/readyz
///
/// Readiness probe used by the blue-green cutover gate. Unlike `/api/health`
/// (which always reports 200 so orchestrators do not kill a draining pod),
/// this returns 503 while the instance is in maintenance or draining mode, so
/// the load balancer stops routing **new** sessions here.
pub async fn readiness(State(state): State<AppState>) -> Response {
    let active = state.metrics.active_mpc_sessions.load(Ordering::SeqCst);
    let maintenance = state.maintenance_mode.load(Ordering::Relaxed);
    let draining = state.drain.is_draining();

    if maintenance {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "maintenance",
                "code": "maintenance",
                "retryable": true,
                "active_mpc_sessions": active,
            })),
        )
            .into_response();
    }
    if draining {
        let info = state.drain.info().await;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "draining",
                "code": "draining",
                "retryable": true,
                "active_mpc_sessions": active,
                "deadline": info.deadline,
            })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ready",
            "active_mpc_sessions": active,
        })),
    )
        .into_response()
}

/// POST /api/admin/maintenance
///
/// Toggle (or explicitly set) coordinator maintenance mode. While enabled,
/// every non-admin route returns `503 maintenance`. Requires super-admin.
pub async fn admin_toggle_maintenance(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<MaintenanceBody>>,
) -> Result<Json<Value>, ApiError> {
    let auth = admin::validate_admin_request(
        &state,
        &headers,
        "admin_toggle_maintenance",
        &state.admin_state,
    )
    .await?;
    admin::require_role(&auth, AdminRole::SuperAdmin)?;

    let requested = body.and_then(|Json(b)| b.enabled);
    let enabled = match requested {
        Some(v) => v,
        None => {
            let prev = state.maintenance_mode.load(Ordering::Relaxed);
            !prev
        }
    };
    state.maintenance_mode.store(enabled, Ordering::Relaxed);
    tracing::warn!(
        "Maintenance mode {} by {} ({})",
        if enabled { "ENABLED" } else { "disabled" },
        auth.address,
        auth.role.as_str()
    );

    Ok(Json(json!({
        "maintenance_mode": enabled,
        "action_by": auth.address,
        "role": auth.role.as_str(),
    })))
}

/// GET /api/admin/drain
///
/// Reports drain status and the number of in-flight MPC sessions the operator
/// is waiting on before cutover. Requires read-only or higher.
pub async fn admin_drain_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let auth = admin::validate_admin_request(
        &state,
        &headers,
        "admin_drain_status",
        &state.admin_state,
    )
    .await?;
    admin::require_role(&auth, AdminRole::ReadOnly)?;

    let info = state.drain.info().await;
    let active = state.metrics.active_mpc_sessions.load(Ordering::SeqCst);
    let grace_expired = state.drain.grace_expired().await;

    Ok(Json(json!({
        "draining": info.draining,
        "reason": info.reason,
        "started_at": info.started_at,
        "deadline": info.deadline,
        "grace_secs": info.grace_secs,
        "grace_expired": grace_expired,
        "active_mpc_sessions": active,
        "drain_complete": info.draining && active == 0,
    })))
}

/// POST /api/admin/drain
///
/// Enter drain mode. Refuses new game-mutating requests with `503 draining`
/// while letting in-flight hands finish. Requires operator or higher.
pub async fn admin_start_drain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<DrainBody>>,
) -> Result<Json<Value>, ApiError> {
    let auth =
        admin::validate_admin_request(&state, &headers, "admin_start_drain", &state.admin_state)
            .await?;
    admin::require_role(&auth, AdminRole::Operator)?;

    let (reason, grace_secs) = match body {
        Some(Json(b)) => (
            b.reason.unwrap_or_else(|| "operator request".to_string()),
            b.grace_secs
                .unwrap_or_else(crate::drain::DrainState::grace_secs_from_env),
        ),
        None => (
            "operator request".to_string(),
            crate::drain::DrainState::grace_secs_from_env(),
        ),
    };

    state.drain.begin(reason.clone(), grace_secs).await;
    let active = state.metrics.active_mpc_sessions.load(Ordering::SeqCst);
    tracing::warn!(
        "Drain started by {} ({}) — reason={:?}, grace={}s, active_sessions={}",
        auth.address,
        auth.role.as_str(),
        reason,
        grace_secs,
        active
    );

    Ok(Json(json!({
        "draining": true,
        "reason": reason,
        "grace_secs": grace_secs,
        "active_mpc_sessions": active,
        "action_by": auth.address,
        "role": auth.role.as_str(),
    })))
}

/// DELETE /api/admin/drain
///
/// Abort an in-progress drain (rollback switch). Requires operator or higher.
pub async fn admin_cancel_drain(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let auth =
        admin::validate_admin_request(&state, &headers, "admin_cancel_drain", &state.admin_state)
            .await?;
    admin::require_role(&auth, AdminRole::Operator)?;

    let was_draining = state.drain.cancel().await;
    Ok(Json(json!({
        "draining": false,
        "was_draining": was_draining,
        "action_by": auth.address,
        "role": auth.role.as_str(),
    })))
}

/// GET /api/admin/vk-rollout
///
/// Circuit (VK) rollout status: pinned/canary versions, canary percentage and
/// per-version proof metrics. Requires read-only or higher.
pub async fn admin_rollout_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let auth = admin::validate_admin_request(
        &state,
        &headers,
        "admin_rollout_status",
        &state.admin_state,
    )
    .await?;
    admin::require_role(&auth, AdminRole::ReadOnly)?;

    let status = state.vk_rollout.status().await;
    Ok(Json(serde_json::to_value(status).unwrap_or_else(|_| json!({}))))
}

/// POST /api/admin/vk-rollout/rollback
///
/// Force one circuit (or `"*"` for all) back to its pinned version. Requires
/// operator or higher.
pub async fn admin_rollout_rollback(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<RolloutRollbackBody>>,
) -> Result<Json<Value>, ApiError> {
    let auth = admin::validate_admin_request(
        &state,
        &headers,
        "admin_rollout_rollback",
        &state.admin_state,
    )
    .await?;
    admin::require_role(&auth, AdminRole::Operator)?;

    let circuit = body
        .and_then(|Json(b)| b.circuit)
        .unwrap_or_else(|| "*".to_string());
    let affected = state.vk_rollout.rollback(&circuit).await;

    Ok(Json(json!({
        "rolled_back": affected,
        "circuit": circuit,
        "action_by": auth.address,
        "role": auth.role.as_str(),
    })))
}

/// POST /api/admin/vk-rollout/promote
///
/// Manually promote a circuit's canary to the pinned version. Requires
/// operator or higher.
pub async fn admin_rollout_promote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RolloutPromoteBody>,
) -> Result<Json<Value>, ApiError> {
    let auth = admin::validate_admin_request(
        &state,
        &headers,
        "admin_rollout_promote",
        &state.admin_state,
    )
    .await?;
    admin::require_role(&auth, AdminRole::Operator)?;

    let promoted = state.vk_rollout.promote(&body.circuit).await;
    if !promoted {
        return Err(ApiError::with_message(
            "not_found",
            format!("no promotable canary for circuit '{}'", body.circuit),
        ));
    }

    Ok(Json(json!({
        "promoted": true,
        "circuit": body.circuit,
        "action_by": auth.address,
        "role": auth.role.as_str(),
    })))
}
