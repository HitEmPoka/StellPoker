//! Storage TTL policy for poker-table entries.
//!
//! Every persistent write goes through one of the policies below so the TTL
//! for an entry type is set in one place. The full table of entries and their
//! policies lives in `docs/soroban-storage-optimization.md`.

use soroban_sdk::{Env, IntoVal, Val};

/// Ledgers per day at ~5 seconds per ledger.
pub const LEDGERS_PER_DAY: u32 = 17_280;

/// When an entry's remaining TTL drops below `threshold`, it is extended to
/// `extend` ledgers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtlPolicy {
    pub threshold: u32,
    pub extend: u32,
}

/// Table state and everything that must live as long as the table: config,
/// queue, seat index, time banks, action counters, bans, currencies,
/// governance records and jackpot claim guards.
pub const TABLE: TtlPolicy = TtlPolicy {
    threshold: LEDGERS_PER_DAY,
    extend: 30 * LEDGERS_PER_DAY,
};

/// Archived hands in the circular history buffer. Matched to [`TABLE`] so
/// history stays readable for as long as the table itself does.
pub const HISTORY: TtlPolicy = TABLE;

/// Entries scoped to a single hand, such as action commitments. They are
/// useless once the hand settles, and the settled hand is kept only in the
/// history buffer, so they get a short TTL and expire on their own.
pub const HAND: TtlPolicy = TtlPolicy {
    threshold: LEDGERS_PER_DAY / 24,
    extend: LEDGERS_PER_DAY,
};

/// Extend a persistent entry according to `policy`.
pub fn bump_persistent<K>(env: &Env, key: &K, policy: TtlPolicy)
where
    K: IntoVal<Env, Val>,
{
    env.storage()
        .persistent()
        .extend_ttl(key, policy.threshold, policy.extend);
}

/// Extend the contract instance according to `policy`.
pub fn bump_instance(env: &Env, policy: TtlPolicy) {
    env.storage()
        .instance()
        .extend_ttl(policy.threshold, policy.extend);
}
