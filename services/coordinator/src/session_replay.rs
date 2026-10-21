//! Redacted coordinator session replay for support investigations.
//!
//! Replay is deliberately built from the append-only transcript metadata. It
//! never selects raw MPC shares, witnesses, or request bodies, and therefore
//! is safe to attach to a support ticket by default.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReplayEvent {
    pub phase: String,
    pub recorded_at: DateTime<Utc>,
    pub public_inputs_digest: String,
    pub proof_hash: Option<String>,
    pub previous_hash: Option<String>,
    pub record_hash: String,
}

/// Rebuild a session timeline without exposing secret material.
pub async fn replay_redacted(
    pool: &PgPool,
    session_id: &str,
) -> Result<Vec<ReplayEvent>, sqlx::Error> {
    sqlx::query_as::<_, ReplayEvent>(
        "SELECT phase, recorded_at, public_inputs_digest, proof_hash,
                previous_hash, record_hash
           FROM transcript_logs
          WHERE session_id = $1
          ORDER BY id ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
}
