//! Aggregate metrics for dashboards (issue #563).
//!
//! Every metric is a running counter updated by the entrypoint that changes
//! it, so a read never scans tables, seats or history:
//!
//! * `hands_played` and `total_rake` move when a hand settles
//!   (`history::archive_hand`, the single settlement chokepoint).
//! * `active_seats` moves when a player takes or leaves a seat
//!   (`join_table`, `seat_next_from_queue`, `leave_table`, `finalize_sunset`).
//!
//! `docs/contract-metrics.md` lists the storage cost of each update.

use soroban_sdk::Env;

use crate::ttl;
use crate::types::*;

/// Contract-wide counters. `tables_created` is not stored here: the getter
/// fills it from the table id allocator, which already counts every table.
pub fn load_contract(env: &Env) -> ContractMetrics {
    env.storage()
        .instance()
        .get(&DataKey::ContractMetrics)
        .unwrap_or(ContractMetrics {
            tables_created: 0,
            hands_played: 0,
            total_rake: 0,
            active_seats: 0,
        })
}

fn save_contract(env: &Env, counters: &ContractMetrics) {
    env.storage()
        .instance()
        .set(&DataKey::ContractMetrics, counters);
}

/// Per-table hand and rake totals.
pub fn load_table_totals(env: &Env, table_id: u32) -> TableTotals {
    env.storage()
        .persistent()
        .get(&DataKey::TableMetrics(table_id))
        .unwrap_or(TableTotals {
            hands_played: 0,
            total_rake: 0,
        })
}

/// Count one settled hand and the `rake` it took, contract-wide and for the
/// table. Costs one instance write and one persistent write.
pub fn record_hand_settled(env: &Env, table_id: u32, rake: i128) {
    let mut counters = load_contract(env);
    counters.hands_played = counters.hands_played.saturating_add(1);
    counters.total_rake = counters.total_rake.saturating_add(rake);
    save_contract(env, &counters);

    let key = DataKey::TableMetrics(table_id);
    let mut totals = load_table_totals(env, table_id);
    totals.hands_played = totals.hands_played.saturating_add(1);
    totals.total_rake = totals.total_rake.saturating_add(rake);
    env.storage().persistent().set(&key, &totals);
    ttl::bump_persistent(env, &key, ttl::TABLE);
}

/// Count `count` newly seated players. Costs one instance write.
pub fn record_seats_added(env: &Env, count: u32) {
    let mut counters = load_contract(env);
    counters.active_seats = counters.active_seats.saturating_add(count);
    save_contract(env, &counters);
}

/// Count `count` players who left their seats. Costs one instance write.
pub fn record_seats_removed(env: &Env, count: u32) {
    let mut counters = load_contract(env);
    counters.active_seats = counters.active_seats.saturating_sub(count);
    save_contract(env, &counters);
}
