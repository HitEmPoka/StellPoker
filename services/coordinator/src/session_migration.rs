//! Coordinator session migration between instances
//!
//! Issue #264: Implement coordinator session migration between coordinator instances
//! Support migrating an active session from one coordinator instance to another.
//! Transfer session state, MPC node connections, and pending actions.
//! Useful for blue/green deployments.

use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    pub table_id: u32,
    pub phase: String,
    pub deck_root: String,
    pub hand_commitments: Vec<String>,
    pub player_order: Vec<String>,
    pub dealt_indices: Vec<u32>,
    pub board_indices: Vec<u32>,
    pub reveal_tx_hashes: HashMap<String, String>,
    pub proof_nonce: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MpcConnections {
    pub node_endpoints: Vec<String>,
    pub active_share_sets: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingActions {
    pub actions: Vec<PendingAction>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingAction {
    pub action_type: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionMigration {
    pub id: i64,
    pub session_id: String,
    pub table_id: i32,
    pub from_instance_id: String,
    pub to_instance_id: String,
    pub migration_status: String,
    pub state_snapshot: Option<serde_json::Value>,
    pub mpc_connections: Option<serde_json::Value>,
    pub pending_actions: Option<serde_json::Value>,
    pub initiated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error_message: Option<String>,
}

/// Initiate a session migration
pub async fn initiate_migration(
    pool: &PgPool,
    session_id: &str,
    table_id: u32,
    from_instance: &str,
    to_instance: &str,
    state: &SessionState,
    connections: &MpcConnections,
    actions: &PendingActions,
) -> Result<SessionMigration, sqlx::Error> {
    let state_json = serde_json::to_value(state).map_err(|e| {
        sqlx::Error::Encode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        )))
    })?;

    let connections_json = serde_json::to_value(connections).map_err(|e| {
        sqlx::Error::Encode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        )))
    })?;

    let actions_json = serde_json::to_value(actions).map_err(|e| {
        sqlx::Error::Encode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        )))
    })?;

    sqlx::query_as::<_, SessionMigration>(
        "INSERT INTO session_migrations 
         (session_id, table_id, from_instance_id, to_instance_id, 
          migration_status, state_snapshot, mpc_connections, pending_actions) 
         VALUES ($1, $2, $3, $4, 'initiated', $5, $6, $7) 
         RETURNING id, session_id, table_id, from_instance_id, to_instance_id, 
                   migration_status, state_snapshot, mpc_connections, pending_actions, 
                   initiated_at, completed_at, error_message",
    )
    .bind(session_id)
    .bind(table_id as i32)
    .bind(from_instance)
    .bind(to_instance)
    .bind(state_json)
    .bind(connections_json)
    .bind(actions_json)
    .fetch_one(pool)
    .await
}

/// Update migration status
pub async fn update_migration_status(
    pool: &PgPool,
    migration_id: i64,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), sqlx::Error> {
    let completed_at = if status == "complete" || status == "failed" {
        Some(chrono::Utc::now())
    } else {
        None
    };

    sqlx::query(
        "UPDATE session_migrations 
         SET migration_status = $1, error_message = $2, completed_at = $3 
         WHERE id = $4",
    )
    .bind(status)
    .bind(error_message)
    .bind(completed_at)
    .bind(migration_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get migration by session ID
pub async fn get_migration_by_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<SessionMigration>, sqlx::Error> {
    sqlx::query_as::<_, SessionMigration>(
        "SELECT id, session_id, table_id, from_instance_id, to_instance_id, 
                migration_status, state_snapshot, mpc_connections, pending_actions, 
                initiated_at, completed_at, error_message 
         FROM session_migrations 
         WHERE session_id = $1 
         ORDER BY id DESC 
         LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
}

/// List pending migrations for an instance
pub async fn list_pending_migrations(
    pool: &PgPool,
    to_instance: &str,
) -> Result<Vec<SessionMigration>, sqlx::Error> {
    sqlx::query_as::<_, SessionMigration>(
        "SELECT id, session_id, table_id, from_instance_id, to_instance_id, 
                migration_status, state_snapshot, mpc_connections, pending_actions, 
                initiated_at, completed_at, error_message 
         FROM session_migrations 
         WHERE to_instance_id = $1 
         AND migration_status IN ('initiated', 'transferring') 
         ORDER BY initiated_at ASC",
    )
    .bind(to_instance)
    .fetch_all(pool)
    .await
}

/// Complete a migration and restore session state
pub async fn complete_migration(
    pool: &PgPool,
    migration_id: i64,
) -> Result<(SessionState, MpcConnections, PendingActions), Box<dyn std::error::Error>> {
    let migration = sqlx::query_as::<_, SessionMigration>(
        "SELECT id, session_id, table_id, from_instance_id, to_instance_id, 
                migration_status, state_snapshot, mpc_connections, pending_actions, 
                initiated_at, completed_at, error_message 
         FROM session_migrations 
         WHERE id = $1",
    )
    .bind(migration_id)
    .fetch_one(pool)
    .await?;

    let state: SessionState =
        serde_json::from_value(migration.state_snapshot.ok_or("Missing state snapshot")?)?;

    let connections: MpcConnections =
        serde_json::from_value(migration.mpc_connections.ok_or("Missing MPC connections")?)?;

    let actions: PendingActions =
        serde_json::from_value(migration.pending_actions.ok_or("Missing pending actions")?)?;

    update_migration_status(pool, migration_id, "complete", None).await?;

    Ok((state, connections, actions))
}

/// Cancel a migration
pub async fn cancel_migration(
    pool: &PgPool,
    migration_id: i64,
    reason: &str,
) -> Result<(), sqlx::Error> {
    update_migration_status(pool, migration_id, "failed", Some(reason)).await
}

/// Generate a unique instance ID for this coordinator instance
pub fn generate_instance_id() -> String {
    format!(
        "coordinator-{}-{}",
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string()),
        Uuid::new_v4()
    )
}

/// Check if there are any active migrations for a session
pub async fn has_active_migration(pool: &PgPool, session_id: &str) -> Result<bool, sqlx::Error> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM session_migrations 
         WHERE session_id = $1 
         AND migration_status IN ('initiated', 'transferring')",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;

    Ok(count.0 > 0)
}
