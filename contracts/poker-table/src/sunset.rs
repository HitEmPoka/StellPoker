//! Table sunset (issue #564).
//!
//! A sunset table is frozen: `finalize_sunset` has paid out its residual
//! balances and written a [`SunsetRecord`], and the entrypoints that let chips
//! in (`join_table`, `buy_in_with_currency`, `rebuy`, `start_hand`) reject it
//! with `TableSunset`. Chips can only leave a frozen table, never enter it.
//! The runbook is `docs/contract-sunset-runbook.md`.

use soroban_sdk::Env;

use crate::ttl;
use crate::types::*;

/// The sunset record, present once the table is frozen.
pub fn load_record(env: &Env, table_id: u32) -> Option<SunsetRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::TableSunset(table_id))
}

/// Persist the sunset record. Its presence is what freezes the table.
pub fn save_record(env: &Env, table_id: u32, record: &SunsetRecord) {
    let key = DataKey::TableSunset(table_id);
    env.storage().persistent().set(&key, record);
    ttl::bump_persistent(env, &key, ttl::TABLE);
}

pub fn is_sunset(env: &Env, table_id: u32) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::TableSunset(table_id))
}

/// Reject entrypoints that would put chips into a sunset table.
pub fn require_not_sunset(env: &Env, table_id: u32) -> Result<(), PokerTableError> {
    if is_sunset(env, table_id) {
        return Err(PokerTableError::TableSunset);
    }
    Ok(())
}
