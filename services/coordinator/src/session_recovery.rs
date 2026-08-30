//! Partial MPC session recovery when a single committee node fails (Issue #235).
//!
//! The 3-node committee uses REP3 replicated secret sharing, which needs
//! every original share-holder to reconstruct a session (see the Issue #96
//! notes in `mpc.rs` / `api/mod.rs`). So by default a failed node can't just
//! be dropped — the session either continues with the surviving nodes (only
//! possible if `threshold` allows it, e.g. for a larger, partially
//! fault-tolerant committee) or the coordinator checkpoints the session's
//! intermediate state and looks for a healthy replacement node to swap into
//! the committee for the next retry. If no replacement is available yet, the
//! checkpointed state lets the session resume later instead of being lost.

use std::sync::OnceLock;

use crate::session_cache::{self, CachedSession, SessionCacheStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// Surviving nodes still meet the threshold; continue with them.
    ContinueWithRemaining { endpoints: Vec<String> },
    /// Threshold not met; a healthy replacement was found and should be
    /// substituted for the failed node.
    Reassigned {
        endpoints: Vec<String>,
        replacement: String,
    },
    /// Threshold not met and no healthy replacement is available. Session
    /// state has been checkpointed so it can be resumed once one is.
    AwaitingReplacement,
}

static RECOVERY_CACHE: OnceLock<SessionCacheStore> = OnceLock::new();

fn recovery_cache() -> &'static SessionCacheStore {
    RECOVERY_CACHE.get_or_init(|| {
        let path = std::env::var("SESSION_RECOVERY_CACHE_DB_PATH")
            .unwrap_or_else(|_| "./data/session_recovery_cache.db".to_string());
        session_cache::new_store(&path)
    })
}

/// Extract the 0-based committee index named in a `NODE_UNAVAILABLE:` error
/// produced by `mpc::trigger_and_collect_proof`, e.g.
/// "NODE_UNAVAILABLE: node 1 unreachable after 3 attempts...".
pub fn extract_failed_node_index(error: &str) -> Option<usize> {
    let after_marker = error.split("node ").nth(1)?;
    let digits: String = after_marker
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Decide how to recover a session after `failed_endpoint` drops out of
/// `committee`. `threshold` is the minimum number of surviving *original*
/// nodes needed to keep going without a replacement; `available_replacements`
/// is the pool of currently-healthy candidate endpoints (may overlap with
/// the committee — candidates already in it are skipped).
pub fn recover_from_node_failure(
    committee: &[String],
    failed_endpoint: &str,
    threshold: usize,
    available_replacements: &[String],
) -> RecoveryOutcome {
    let remaining: Vec<String> = committee
        .iter()
        .filter(|e| e.as_str() != failed_endpoint)
        .cloned()
        .collect();

    if remaining.len() >= threshold {
        return RecoveryOutcome::ContinueWithRemaining {
            endpoints: remaining,
        };
    }

    match available_replacements
        .iter()
        .find(|candidate| !committee.contains(candidate))
    {
        Some(replacement) => {
            let mut endpoints = remaining;
            endpoints.push(replacement.clone());
            RecoveryOutcome::Reassigned {
                endpoints,
                replacement: replacement.clone(),
            }
        }
        None => RecoveryOutcome::AwaitingReplacement,
    }
}

/// Checkpoint intermediate session state so it can be resumed later, whether
/// recovery finds a replacement immediately or not.
pub fn checkpoint_session_state(session: &CachedSession) -> Result<(), String> {
    let cache = recovery_cache();
    let guard = cache
        .lock()
        .map_err(|_| "session recovery cache lock poisoned".to_string())?;
    guard
        .save_session(session)
        .map_err(|e| format!("failed to checkpoint session for recovery: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached(session_id: &str) -> CachedSession {
        CachedSession {
            session_id: session_id.to_string(),
            table_id: 1,
            phase: "preflop".to_string(),
            circuit_name: "deal_valid".to_string(),
            deck_root: "0xroot".to_string(),
            hand_commitments: vec!["0xc1".to_string()],
            player_order: vec!["P1".to_string(), "P2".to_string()],
            dealt_indices: vec![0, 1, 2, 3],
            board_indices: vec![],
            reveal_tx_hashes: Default::default(),
            proof_nonce: 3,
            last_checkpoint: 0,
        }
    }

    #[test]
    fn extract_failed_node_index_parses_marker() {
        let err = "NODE_UNAVAILABLE: node 1 unreachable after 3 attempts triggering generate: connect error";
        assert_eq!(extract_failed_node_index(err), Some(1));
    }

    #[test]
    fn extract_failed_node_index_returns_none_without_marker() {
        assert_eq!(
            extract_failed_node_index("node 1 trigger failed: HTTP 500: internal error"),
            None
        );
        assert_eq!(extract_failed_node_index("no node info here"), None);
    }

    #[test]
    fn remaining_nodes_continue_when_threshold_met() {
        let committee = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let outcome = recover_from_node_failure(&committee, "b", 2, &[]);
        assert_eq!(
            outcome,
            RecoveryOutcome::ContinueWithRemaining {
                endpoints: vec!["a".to_string(), "c".to_string()]
            }
        );
    }

    #[test]
    fn reassigns_replacement_when_threshold_not_met() {
        // REP3-style committee: threshold == committee size, so losing any
        // node always requires a replacement.
        let committee = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let replacements = vec!["a".to_string(), "d".to_string()]; // "a" already in committee
        let outcome = recover_from_node_failure(&committee, "b", 3, &replacements);
        assert_eq!(
            outcome,
            RecoveryOutcome::Reassigned {
                endpoints: vec!["a".to_string(), "c".to_string(), "d".to_string()],
                replacement: "d".to_string(),
            }
        );
    }

    #[test]
    fn awaits_replacement_when_none_available() {
        let committee = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let outcome = recover_from_node_failure(&committee, "b", 3, &[]);
        assert_eq!(outcome, RecoveryOutcome::AwaitingReplacement);
    }

    #[test]
    fn checkpoint_round_trips_through_cache() {
        let session = cached("recovery-roundtrip-test");
        checkpoint_session_state(&session).expect("checkpoint should succeed");

        let cache = recovery_cache().lock().unwrap();
        let loaded = cache
            .load_session("recovery-roundtrip-test")
            .expect("load should succeed")
            .expect("session should have been checkpointed");
        assert_eq!(loaded.table_id, session.table_id);
        assert_eq!(loaded.phase, session.phase);
        assert_eq!(loaded.player_order, session.player_order);
    }
}
