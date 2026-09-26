//! Queryable history of a table's rake configuration (issue #559).
//!
//! Rake is charged on every settled pot, so a change to `rake_bps` changes what
//! players pay. Each table therefore keeps an append-only log of its rake
//! parameters with the time each one took effect, readable on-chain through
//! `get_rake_history` / `get_rake_bps_at`, and announced with a
//! `rake_config_changed` event for indexers.
//!
//! Layout: one persistent entry per change, keyed `(table_id, index)`, plus a
//! length counter. Entry `0` is the rake the table was created with, so the log
//! always answers "what was the rake at time T" for any T since creation.
//! Appending is O(1) and never rewrites old entries, so the write cost of a
//! change does not grow with the length of the log; reads are paginated.
//!
//! A rake change takes effect immediately: `effective_at_timestamp` is the
//! ledger time of the `set_rake_bps` call, and a hand that settles afterwards is
//! raked at the new rate, including one already in progress.

use soroban_sdk::{contracttype, Address, Env, Symbol};

use crate::ttl;
use crate::types::DataKey;

/// Most entries a single `get_rake_history` call returns.
pub const MAX_PAGE_SIZE: u32 = 50;

/// One rake configuration, effective from `effective_at_timestamp` until the
/// next entry's.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RakeChange {
    /// Position in the table's log (0 = configuration at creation).
    pub index: u32,
    /// Rake in basis points that applies from this entry on.
    pub rake_bps: u32,
    /// The rake in force immediately before this entry (equal to `rake_bps` for
    /// entry 0, which has no predecessor).
    pub previous_bps: u32,
    /// Ledger timestamp (seconds) at which this rake took effect. `0` marks a
    /// table created before rake history existed: the entry stands for "the
    /// rake up to the first recorded change".
    pub effective_at_timestamp: u64,
    /// Ledger sequence at which this rake took effect.
    pub effective_at_ledger: u32,
    /// The table admin that made the change.
    pub changed_by: Address,
}

/// Number of entries in a table's rake history.
pub fn len(env: &Env, table_id: u32) -> u32 {
    env.storage()
        .persistent()
        .get::<DataKey, u32>(&DataKey::RakeHistoryLen(table_id))
        .unwrap_or(0)
}

/// The entry at `index`, if any.
pub fn get(env: &Env, table_id: u32, index: u32) -> Option<RakeChange> {
    env.storage()
        .persistent()
        .get(&DataKey::RakeHistory(table_id, index))
}

fn append(env: &Env, table_id: u32, entry: &RakeChange) {
    let key = DataKey::RakeHistory(table_id, entry.index);
    env.storage().persistent().set(&key, entry);
    ttl::bump_persistent(env, &key, ttl::TABLE);

    let len_key = DataKey::RakeHistoryLen(table_id);
    env.storage().persistent().set(&len_key, &(entry.index + 1));
    ttl::bump_persistent(env, &len_key, ttl::TABLE);
}

/// Record the rake a table starts with. Called once, from `create_table`.
pub fn record_initial(env: &Env, table_id: u32, rake_bps: u32, admin: &Address) {
    append(
        env,
        table_id,
        &RakeChange {
            index: 0,
            rake_bps,
            previous_bps: rake_bps,
            effective_at_timestamp: env.ledger().timestamp(),
            effective_at_ledger: env.ledger().sequence(),
            changed_by: admin.clone(),
        },
    );
}

/// Record a change from `previous_bps` to `rake_bps` taking effect now, and
/// announce it. Returns the new entry's index. A no-op change is not recorded.
pub fn record_change(
    env: &Env,
    table_id: u32,
    previous_bps: u32,
    rake_bps: u32,
    admin: &Address,
) -> Option<u32> {
    if previous_bps == rake_bps {
        return None;
    }
    let mut index = len(env, table_id);
    if index == 0 {
        // Table predates rake history: keep what it charged until now as entry 0.
        append(
            env,
            table_id,
            &RakeChange {
                index: 0,
                rake_bps: previous_bps,
                previous_bps,
                effective_at_timestamp: 0,
                effective_at_ledger: 0,
                changed_by: admin.clone(),
            },
        );
        index = 1;
    }

    let timestamp = env.ledger().timestamp();
    append(
        env,
        table_id,
        &RakeChange {
            index,
            rake_bps,
            previous_bps,
            effective_at_timestamp: timestamp,
            effective_at_ledger: env.ledger().sequence(),
            changed_by: admin.clone(),
        },
    );

    env.events().publish(
        (Symbol::new(env, "rake_config_changed"), table_id),
        (index, previous_bps, rake_bps, timestamp),
    );
    Some(index)
}

/// Up to `limit` entries (capped at [`MAX_PAGE_SIZE`]) starting at `start`,
/// oldest first. Past the end of the log this is empty.
pub fn page(env: &Env, table_id: u32, start: u32, limit: u32) -> soroban_sdk::Vec<RakeChange> {
    let total = len(env, table_id);
    let end = start.saturating_add(limit.min(MAX_PAGE_SIZE)).min(total);
    let mut out = soroban_sdk::Vec::new(env);
    for index in start..end {
        if let Some(entry) = get(env, table_id, index) {
            out.push_back(entry);
        }
    }
    out
}

/// The rake in force at `timestamp`: the latest entry that took effect at or
/// before it. `None` if the table has no history or `timestamp` precedes its
/// creation. Timestamps in the log never decrease, so this is a binary search.
pub fn rake_bps_at(env: &Env, table_id: u32, timestamp: u64) -> Option<u32> {
    let total = len(env, table_id);
    // Find the number of entries that took effect at or before `timestamp`.
    let (mut lo, mut hi) = (0u32, total);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let effective = get(env, table_id, mid)?.effective_at_timestamp;
        if effective <= timestamp {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return None;
    }
    get(env, table_id, lo - 1).map(|entry| entry.rake_bps)
}
