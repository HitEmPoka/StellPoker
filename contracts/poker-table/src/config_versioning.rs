//! Table configuration versioning for audit trail (Issue #553).
//!
//! Tracks configuration changes on a per-table basis so rule changes mid-lobby
//! are auditable. Each configuration change is logged with a version number,
//! timestamp, and change details.

use soroban_sdk::{contracttype, Env, Symbol, Vec};
use crate::types::{DataKey, PokerTableError};

/// A single configuration change event.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ConfigChangeEvent {
    /// Sequential version number (0 = genesis, 1 = first change, etc).
    pub version: u32,
    /// Ledger at which the change was recorded.
    pub changed_at_ledger: u32,
    /// Wall-clock timestamp (seconds) at the time of change.
    pub changed_at_timestamp: u64,
    /// Description of what changed (e.g., "rake_bps", "min_players").
    pub change_summary: Symbol,
}

/// Load the current configuration version for a table (0 = genesis/unversioned).
pub fn get_config_version(env: &Env, table_id: u32) -> u32 {
    env.storage()
        .persistent()
        .get::<DataKey, u32>(&DataKey::ConfigVersion(table_id))
        .unwrap_or(0)
}

/// Record a configuration change.
pub fn record_config_change(
    env: &Env,
    table_id: u32,
    change_summary: Symbol,
) -> Result<u32, PokerTableError> {
    let current_version = get_config_version(env, table_id);
    let new_version = current_version.saturating_add(1);

    let event = ConfigChangeEvent {
        version: new_version,
        changed_at_ledger: env.ledger().sequence(),
        changed_at_timestamp: env.ledger().timestamp(),
        change_summary: change_summary.clone(),
    };

    // Store the event in the changelog for this table
    let key = DataKey::ConfigChangeLog(table_id, new_version);
    env.storage().persistent().set(&key, &event);

    // Update current version
    env.storage()
        .persistent()
        .set(&DataKey::ConfigVersion(table_id), &new_version);

    // Emit change event for indexers
    env.events().publish(
        (Symbol::new(env, "config_changed"), table_id),
        (new_version, change_summary),
    );

    Ok(new_version)
}

/// Fetch a specific configuration change by version.
pub fn get_config_change(
    env: &Env,
    table_id: u32,
    version: u32,
) -> Option<ConfigChangeEvent> {
    env.storage()
        .persistent()
        .get(&DataKey::ConfigChangeLog(table_id, version))
}

/// Fetch all configuration changes for a table (newest first, limited to 50).
pub fn get_config_history(env: &Env, table_id: u32) -> Vec<ConfigChangeEvent> {
    let current_version = get_config_version(env, table_id);
    let mut out: Vec<ConfigChangeEvent> = Vec::new(env);

    let limit = core::cmp::min(50u32, current_version);
    for i in 0..limit {
        let v = current_version - i;
        if let Some(event) = get_config_change(env, table_id, v) {
            out.push_back(event);
        }
    }
    out
}
