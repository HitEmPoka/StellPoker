//! Upgrade governance (Issue #504): N-of-M signers + a configurable timelock.
//!
//! A single admin address can still upgrade a table that was created before
//! governance was available (legacy `upgrade` path). Once
//! `configure_upgrade_governance` stores a non-empty signer set for a table,
//! every upgrade must be:
//!
//! 1. **Proposed** by one of the configured signers (`propose_upgrade`).
//! 2. **Approved** by at least `threshold` distinct signers.
//! 3. **Executed** only after `delay_ledgers` ledger sequences have elapsed
//!    since the proposal opened (the per-network timelock).
//!
//! All storage is instance-level and keyed by `table_id`, so concurrent tables
//! keep fully independent upgrade queues.

use crate::types::{DataKey, PendingUpgrade, PokerTableError};
use soroban_sdk::{Address, BytesN, Env, Vec};

/// True when a table has a configured upgrade-governance signer set.
pub fn governance_configured(env: &Env, table_id: u32) -> bool {
    env.storage()
        .instance()
        .get::<DataKey, Vec<Address>>(&DataKey::UpgradeSigners(table_id))
        .map(|signers| !signers.is_empty())
        .unwrap_or(false)
}

/// Load the configured signer set (empty when governance is not configured).
pub fn load_signers(env: &Env, table_id: u32) -> Vec<Address> {
    env.storage()
        .instance()
        .get::<DataKey, Vec<Address>>(&DataKey::UpgradeSigners(table_id))
        .unwrap_or_else(|| Vec::new(env))
}

fn load_threshold(env: &Env, table_id: u32) -> u32 {
    env.storage()
        .instance()
        .get::<DataKey, u32>(&DataKey::UpgradeThreshold(table_id))
        .unwrap_or(0)
}

fn load_delay(env: &Env, table_id: u32) -> u32 {
    env.storage()
        .instance()
        .get::<DataKey, u32>(&DataKey::UpgradeDelay(table_id))
        .unwrap_or(0)
}

pub fn load_pending(env: &Env, table_id: u32) -> Result<PendingUpgrade, PokerTableError> {
    env.storage()
        .instance()
        .get::<DataKey, PendingUpgrade>(&DataKey::PendingUpgrade(table_id))
        .ok_or(PokerTableError::NoPendingUpgrade)
}

fn is_signer(env: &Env, signers: &Vec<Address>, candidate: &Address) -> bool {
    for i in 0..signers.len() {
        if let Some(s) = signers.get(i) {
            if crate::constant_time::address_eq(env, &s, candidate) {
                return true;
            }
        }
    }
    false
}

fn already_approved(env: &Env, pending: &PendingUpgrade, candidate: &Address) -> bool {
    for i in 0..pending.approvals.len() {
        if let Some(a) = pending.approvals.get(i) {
            if crate::constant_time::address_eq(env, &a, candidate) {
                return true;
            }
        }
    }
    false
}

fn wasm_hash_eq(left: &BytesN<32>, right: &BytesN<32>) -> bool {
    left.to_array() == right.to_array()
}

/// Validate a `signers` set passed to `configure_upgrade_governance`.
pub fn validate_governance_config(
    env: &Env,
    signers: &Vec<Address>,
    threshold: u32,
    delay_ledgers: u32,
) -> Result<(), PokerTableError> {
    if signers.is_empty() || threshold == 0 || threshold > signers.len() as u32 {
        return Err(PokerTableError::InvalidGovernanceConfig);
    }
    if delay_ledgers == 0 {
        return Err(PokerTableError::InvalidGovernanceConfig);
    }
    // Reject duplicate signer addresses.
    for i in 0..signers.len() {
        let a = signers.get(i).ok_or(PokerTableError::InvalidGovernanceConfig)?;
        for j in (i + 1)..signers.len() {
            let b = signers.get(j).ok_or(PokerTableError::InvalidGovernanceConfig)?;
            if crate::constant_time::address_eq(env, &a, &b) {
                return Err(PokerTableError::InvalidGovernanceConfig);
            }
        }
    }
    Ok(())
}

/// Persist the governance signer set, threshold, and timelock delay.
pub fn store_governance_config(
    env: &Env,
    table_id: u32,
    signers: Vec<Address>,
    threshold: u32,
    delay_ledgers: u32,
) {
    env.storage()
        .instance()
        .set(&DataKey::UpgradeSigners(table_id), &signers);
    env.storage()
        .instance()
        .set(&DataKey::UpgradeThreshold(table_id), &threshold);
    env.storage()
        .instance()
        .set(&DataKey::UpgradeDelay(table_id), &delay_ledgers);
}

/// Record (or extend) an upgrade proposal in the signer's name.
///
/// Returns the new approval count after `signer` has been appended.
pub fn propose_upgrade(
    env: &Env,
    table_id: u32,
    signer: &Address,
    wasm_hash: BytesN<32>,
) -> Result<u32, PokerTableError> {
    let signers = load_signers(env, table_id);
    if !is_signer(env, &signers, signer) {
        return Err(PokerTableError::NotAnUpgradeSigner);
    }

    let existing = env
        .storage()
        .instance()
        .get::<DataKey, PendingUpgrade>(&DataKey::PendingUpgrade(table_id));

    let mut pending = match existing {
        Some(p) if wasm_hash_eq(&p.wasm_hash, &wasm_hash) => p,
        // A new target (or a stale proposal for a different target) restarts
        // the proposal: only the caller's signature carries over implicitly by
        // being appended fresh below.
        Some(_) | None => PendingUpgrade {
            wasm_hash,
            approvals: Vec::new(env),
            started_ledger: env.ledger().sequence(),
        },
    };

    if already_approved(env, &pending, signer) {
        return Err(PokerTableError::UpgradeAlreadyApproved);
    }

    pending.approvals.push_back(signer.clone());
    let approvals = pending.approvals.len();
    env.storage()
        .instance()
        .set(&DataKey::PendingUpgrade(table_id), &pending);

    Ok(approvals as u32)
}

/// Verify an upgrade proposal is ready to execute: threshold reached and, if a
/// delay was configured, the timelock window has elapsed.
pub fn can_execute(
    env: &Env,
    table_id: u32,
    target: &BytesN<32>,
) -> Result<PendingUpgrade, PokerTableError> {
    let pending = load_pending(env, table_id)?;
    if !wasm_hash_eq(&pending.wasm_hash, target) {
        // The pending proposal (if any) targets a different WASM hash.
        return Err(PokerTableError::NoPendingUpgrade);
    }
    let threshold = load_threshold(env, table_id);
    if (pending.approvals.len() as u32) < threshold {
        return Err(PokerTableError::NotEnoughUpgradeApprovals);
    }
    let delay = load_delay(env, table_id);
    if env.ledger().sequence() < pending.started_ledger + delay {
        return Err(PokerTableError::UpgradeTimelockPending);
    }
    Ok(pending)
}

/// Clear the pending proposal after a successful (or cancelled) upgrade.
pub fn clear_pending(env: &Env, table_id: u32) {
    env.storage()
        .instance()
        .remove(&DataKey::PendingUpgrade(table_id));
}