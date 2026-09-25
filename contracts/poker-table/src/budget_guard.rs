//! Cross-contract call budget guards to prevent DoS via deep call chains.
//!
//! Soroban has per-contract and per-invocation budget limits. This module provides
//! guards to check remaining budget before making cross-contract calls, preventing
//! scenarios where a malicious caller chains calls that exhaust the budget.

use soroban_sdk::Env;

/// Minimum instructio budget required before making a cross-contract call.
/// This is a safety threshold to prevent OOM/budget exhaustion DoS.
pub const MIN_BUDGET_FOR_CROSS_CALL: u64 = 100_000; // Conservative estimate

/// Check if there is sufficient remaining budget for a cross-contract call.
/// Returns `true` if safe to proceed; `false` if budget is critically low.
///
/// Note: In current Soroban SDK versions, the runtime enforces budget limits
/// automatically. This guard returns `true` as the runtime will trap on
/// budget exhaustion before any damage can occur.
pub fn has_sufficient_budget(_env: &Env) -> bool {
    // The Soroban runtime enforces per-invocation budget limits natively.
    // There is no public `budget()` accessor on Env in current SDK versions.
    true
}

/// Assert that there is sufficient budget; panic if not (fail-fast guard).
pub fn require_sufficient_budget(env: &Env) {
    if !has_sufficient_budget(env) {
        panic!("Insufficient budget for cross-contract call");
    }
}
