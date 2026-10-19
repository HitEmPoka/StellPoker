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

/// Reveal policy for spectator/observer mode to prevent unfair real-time HUD / hole-card scraping (Issue #692).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "policy_type", content = "delay")]
pub enum SpectatorRevealPolicy {
    /// Immediately reveals showdown cards once hand is complete (casual / friendly games).
    Immediate,
    /// Only reveal cards after on-chain settlement has concluded.
    SettlementGated,
    /// Reveals showdown cards after a delay of N seconds past settlement.
    DelayedSeconds { min_delay_seconds: u64 },
    /// Reveals showdown cards after N Stellar ledgers have closed past settlement.
    DelayedLedgers { min_delay_ledgers: u32 },
    /// Hole cards are never revealed to observers (maximum privacy).
    Hidden,
}

impl Default for SpectatorRevealPolicy {
    fn default() -> Self {
        Self::DelayedSeconds {
            min_delay_seconds: 60,
        }
    }
}

/// Timing context for evaluating whether an observer is allowed to see cards.
#[derive(Debug, Clone, Copy, Default)]
pub struct ObserverTimingContext {
    pub current_time_unix: u64,
    pub current_ledger: u32,
    pub hand_settled_at_unix: Option<u64>,
    pub hand_settled_at_ledger: Option<u32>,
}

impl SpectatorRevealPolicy {
    /// Determines whether observer mode allows hole card reveals for this hand state.
    pub fn is_reveal_allowed(&self, phase: &str, timing: &ObserverTimingContext) -> bool {
        let normalized = phase.to_ascii_lowercase();
        // Never allow reveal during live betting rounds
        if !matches!(normalized.as_str(), "showdown" | "settlement" | "settled" | "complete") {
            return false;
        }

        match self {
            Self::Hidden => false,
            Self::Immediate => true,
            Self::SettlementGated => matches!(normalized.as_str(), "settlement" | "settled" | "complete"),
            Self::DelayedSeconds { min_delay_seconds } => {
                if let Some(settled_at) = timing.hand_settled_at_unix {
                    timing.current_time_unix >= settled_at.saturating_add(*min_delay_seconds)
                } else {
                    false
                }
            }
            Self::DelayedLedgers { min_delay_ledgers } => {
                if let Some(settled_ledger) = timing.hand_settled_at_ledger {
                    timing.current_ledger >= settled_ledger.saturating_add(*min_delay_ledgers)
                } else {
                    false
                }
            }
        }
    }

    /// Computes remaining delay in seconds, if applicable.
    pub fn delay_remaining_seconds(&self, timing: &ObserverTimingContext) -> Option<u64> {
        match self {
            Self::DelayedSeconds { min_delay_seconds } => {
                let settled_at = timing.hand_settled_at_unix?;
                let unlock_time = settled_at.saturating_add(*min_delay_seconds);
                Some(unlock_time.saturating_sub(timing.current_time_unix))
            }
            _ => None,
        }
    }
}

/// Sanitizes and filters a table state JSON snapshot for spectator/observer consumption.
/// Enforces anti-HUD delayed card reveal rules:
/// - Hole cards are stripped during preflop, flop, turn, and river betting rounds.
/// - Folded hands always have cards stripped (mucked).
/// - Unsettled or delayed hands have cards stripped until the reveal policy condition is met.
/// - Injects `anti_hud` status metadata indicating policy, delay status, and remaining time.
pub fn filter_spectator_state_json(
    raw_json: &str,
    policy: &SpectatorRevealPolicy,
    timing: &ObserverTimingContext,
) -> Result<String, serde_json::Error> {
    let mut val: serde_json::Value = serde_json::from_str(raw_json)?;
    if let Some(obj) = val.as_object_mut() {
        let phase = obj
            .get("phase")
            .and_then(|p| p.as_str())
            .unwrap_or("Waiting")
            .to_string();

        let reveal_allowed = policy.is_reveal_allowed(&phase, timing);
        let delay_remaining = policy.delay_remaining_seconds(timing);

        // Sanitize players
        if let Some(players) = obj.get_mut("players").and_then(|p| p.as_array_mut()) {
            for player in players {
                if let Some(p_obj) = player.as_object_mut() {
                    let folded = p_obj.get("folded").and_then(|f| f.as_bool()).unwrap_or(false);
                    if folded || !reveal_allowed {
                        p_obj.remove("hole_cards");
                        p_obj.remove("cards");
                    }
                }
            }
        }

        // Add anti-HUD metadata to the spectator frame
        let anti_hud_meta = serde_json::json!({
            "enabled": !matches!(policy, SpectatorRevealPolicy::Immediate),
            "policy": match policy {
                SpectatorRevealPolicy::Immediate => "Immediate",
                SpectatorRevealPolicy::SettlementGated => "SettlementGated",
                SpectatorRevealPolicy::DelayedSeconds { .. } => "DelayedSeconds",
                SpectatorRevealPolicy::DelayedLedgers { .. } => "DelayedLedgers",
                SpectatorRevealPolicy::Hidden => "Hidden",
            },
            "cards_delayed": !reveal_allowed,
            "delay_remaining_seconds": delay_remaining,
        });
        obj.insert("anti_hud".to_string(), anti_hud_meta);
    }
    serde_json::to_string(&val)
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

    #[test]
    fn anti_hud_policy_live_betting_never_reveals() {
        let policy = SpectatorRevealPolicy::Immediate;
        let timing = ObserverTimingContext::default();

        assert!(!policy.is_reveal_allowed("Preflop", &timing));
        assert!(!policy.is_reveal_allowed("Flop", &timing));
        assert!(!policy.is_reveal_allowed("Turn", &timing));
        assert!(!policy.is_reveal_allowed("River", &timing));
    }

    #[test]
    fn anti_hud_delayed_seconds_policy() {
        let policy = SpectatorRevealPolicy::DelayedSeconds {
            min_delay_seconds: 60,
        };
        let mut timing = ObserverTimingContext {
            current_time_unix: 1000,
            hand_settled_at_unix: Some(950), // 50s elapsed (< 60s)
            ..Default::default()
        };

        assert!(!policy.is_reveal_allowed("Settlement", &timing));
        assert_eq!(policy.delay_remaining_seconds(&timing), Some(10));

        // Advance past delay
        timing.current_time_unix = 1010; // 60s elapsed
        assert!(policy.is_reveal_allowed("Settlement", &timing));
        assert_eq!(policy.delay_remaining_seconds(&timing), Some(0));
    }

    #[test]
    fn anti_hud_delayed_ledgers_policy() {
        let policy = SpectatorRevealPolicy::DelayedLedgers {
            min_delay_ledgers: 12, // ~60 seconds on Stellar (5s per ledger)
        };
        let mut timing = ObserverTimingContext {
            current_ledger: 100,
            hand_settled_at_ledger: Some(90), // 10 ledgers elapsed (< 12)
            ..Default::default()
        };

        assert!(!policy.is_reveal_allowed("Settlement", &timing));

        timing.current_ledger = 102; // 12 ledgers elapsed
        assert!(policy.is_reveal_allowed("Settlement", &timing));
    }

    #[test]
    fn anti_hud_hidden_policy_never_reveals() {
        let policy = SpectatorRevealPolicy::Hidden;
        let timing = ObserverTimingContext {
            current_time_unix: 100000,
            hand_settled_at_unix: Some(100),
            ..Default::default()
        };
        assert!(!policy.is_reveal_allowed("Settlement", &timing));
    }

    #[test]
    fn filter_spectator_state_json_masks_cards_and_mucks_folds() {
        let raw = serde_json::json!({
            "phase": "River",
            "table_id": 1,
            "pot": 500,
            "players": [
                {
                    "address": "G_PLAYER_A",
                    "folded": false,
                    "hole_cards": ["As", "Kd"]
                },
                {
                    "address": "G_PLAYER_B",
                    "folded": true,
                    "hole_cards": ["7h", "2c"]
                }
            ]
        })
        .to_string();

        let policy = SpectatorRevealPolicy::DelayedSeconds { min_delay_seconds: 60 };
        let timing = ObserverTimingContext {
            current_time_unix: 1000,
            hand_settled_at_unix: Some(950),
            ..Default::default()
        };

        let filtered = filter_spectator_state_json(&raw, &policy, &timing).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&filtered).unwrap();

        // Cards must be stripped during river betting
        assert!(parsed["players"][0].get("hole_cards").is_none());
        assert!(parsed["players"][1].get("hole_cards").is_none());
        assert_eq!(parsed["anti_hud"]["enabled"], true);
        assert_eq!(parsed["anti_hud"]["cards_delayed"], true);
        assert_eq!(parsed["anti_hud"]["delay_remaining_seconds"], 10);

        // When showdown settles and delay expires
        let settled_raw = serde_json::json!({
            "phase": "Settlement",
            "table_id": 1,
            "pot": 500,
            "players": [
                {
                    "address": "G_PLAYER_A",
                    "folded": false,
                    "hole_cards": ["As", "Kd"]
                },
                {
                    "address": "G_PLAYER_B",
                    "folded": true,
                    "hole_cards": ["7h", "2c"]
                }
            ]
        })
        .to_string();

        let timing_expired = ObserverTimingContext {
            current_time_unix: 1020,
            hand_settled_at_unix: Some(950),
            ..Default::default()
        };

        let filtered_settled = filter_spectator_state_json(&settled_raw, &policy, &timing_expired).unwrap();
        let parsed_settled: serde_json::Value = serde_json::from_str(&filtered_settled).unwrap();

        // Player A should now have cards visible; Player B folded, so their cards remain mucked/stripped
        assert_eq!(parsed_settled["players"][0]["hole_cards"], serde_json::json!(["As", "Kd"]));
        assert!(parsed_settled["players"][1].get("hole_cards").is_none());
        assert_eq!(parsed_settled["anti_hud"]["cards_delayed"], false);
    }
}
