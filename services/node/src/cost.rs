//! Per-session resource accounting for committee economics (#572).
//!
//! Nodes track CPU (wall-clock) seconds and bytes moved per MPC session.
//! Snapshots are exposed via `GET /session/:id/cost` and emitted as
//! structured log fields (`session_id`, `circuit`, `cpu_seconds`,
//! `bytes_in`, `bytes_out`) so the coordinator/fee model can aggregate
//! real cost data per table.

use serde::Serialize;
use std::time::Instant;

/// Serializable per-session cost attribution snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct SessionCost {
    pub session_id: String,
    pub circuit_name: String,
    pub status: String,
    /// Wall-clock seconds since the session was created (CPU-seconds proxy).
    pub cpu_seconds: f64,
    /// Total share-fragment bytes received for this session.
    pub bytes_in: u64,
    /// Proof bytes produced (0 until complete).
    pub bytes_out: u64,
    /// Number of share fragments received.
    pub fragments: usize,
}

impl SessionCost {
    pub fn snapshot(
        session_id: &str,
        circuit_name: &str,
        status: &str,
        created_at: &Instant,
        bytes_in: u64,
        bytes_out: u64,
        fragments: usize,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            circuit_name: circuit_name.to_string(),
            status: status.to_string(),
            cpu_seconds: created_at.elapsed().as_secs_f64(),
            bytes_in,
            bytes_out,
            fragments,
        }
    }
}
