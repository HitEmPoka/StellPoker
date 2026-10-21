//! Anonymous spectator tracking (Issue #171).
//!
//! Anyone can watch a table over `/api/table/:table_id/spectate/ws` without
//! connecting a wallet. Spectators only ever receive the public game-state
//! snapshot (phase, community cards, on-chain betting state) — hole cards are
//! served exclusively by the authenticated `/cards` endpoint, so there is
//! nothing private to filter here.
//!
//! This module keeps a per-table count of live spectator connections so
//! tables can show a "N watching" indicator. Each connection holds a
//! [`SpectatorGuard`]; dropping it (on disconnect) decrements the count.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct SpectatorRegistry {
    counts: Arc<Mutex<HashMap<u32, usize>>>,
}

impl SpectatorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new spectator on `table_id`. The returned guard keeps the
    /// spectator counted until it is dropped.
    pub fn join(&self, table_id: u32) -> SpectatorGuard {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        *counts.entry(table_id).or_insert(0) += 1;
        SpectatorGuard {
            registry: self.clone(),
            table_id,
        }
    }

    /// Current number of live spectators on `table_id`.
    pub fn count(&self, table_id: u32) -> usize {
        let counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        counts.get(&table_id).copied().unwrap_or(0)
    }

    fn leave(&self, table_id: u32) {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = counts.get_mut(&table_id) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                counts.remove(&table_id);
            }
        }
    }
}

/// RAII handle for one spectator connection.
pub struct SpectatorGuard {
    registry: SpectatorRegistry,
    table_id: u32,
}

impl Drop for SpectatorGuard {
    fn drop(&mut self) {
        self.registry.leave(self.table_id);
    }
}

/// Push frame broadcast on a table's game-state channel whenever its
/// spectator count changes, so players and spectators alike can update the
/// indicator without polling.
pub fn spectator_count_message(table_id: u32, count: usize) -> String {
    serde_json::json!({
        "type": "spectators",
        "table_id": table_id,
        "spectator_count": count,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_and_drop_track_count_per_table() {
        let registry = SpectatorRegistry::new();
        assert_eq!(registry.count(1), 0);

        let a = registry.join(1);
        let b = registry.join(1);
        let c = registry.join(2);
        assert_eq!(registry.count(1), 2);
        assert_eq!(registry.count(2), 1);

        drop(a);
        assert_eq!(registry.count(1), 1);
        drop(b);
        drop(c);
        assert_eq!(registry.count(1), 0);
        assert_eq!(registry.count(2), 0);
        assert!(registry.counts.lock().unwrap().is_empty());
    }

    #[test]
    fn count_message_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&spectator_count_message(7, 3)).unwrap();
        assert_eq!(v["type"], "spectators");
        assert_eq!(v["table_id"], 7);
        assert_eq!(v["spectator_count"], 3);
    }
}
