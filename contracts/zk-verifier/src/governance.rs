//! Upgrade governance for the verifier contract (Issue #504): N-of-M signers
//! plus a configurable timelock.
//!
//! The verifier is a single global contract (no per-table queue), so all keys
//! are instance-level and shared across every proof request. Once
//! `configure_upgrade_governance` stores a non-empty signer set, upgrades must
//! be proposed by a signer, approved by at least `threshold` signers, and only
//! executed after `delay_ledgers` have elapsed since the proposal opened.

use crate::{PendingUpgrade, StorageKey, VerifierError};
use soroban_sdk::{Address, BytesN, Env, Vec};

pub fn governance_configured(env: &Env) -> bool {
    env.storage()
        .instance()
        .get::<StorageKey, Vec<Address>>(&StorageKey::UpgradeSigners)
        .map(|signers| !signers.is_empty())
        .unwrap_or(false)
}

pub fn load_signers(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get::<StorageKey, Vec<Address>>(&StorageKey::UpgradeSigners)
        .unwrap_or_else(|| Vec::new(env))
}

fn load_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get::<StorageKey, u32>(&StorageKey::UpgradeThreshold)
        .unwrap_or(0)
}

fn load_delay(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get::<StorageKey, u32>(&StorageKey::UpgradeDelay)
        .unwrap_or(0)
}

pub fn load_pending(env: &Env) -> Result<PendingUpgrade, VerifierError> {
    env.storage()
        .instance()
        .get::<StorageKey, PendingUpgrade>(&StorageKey::PendingUpgrade)
        .ok_or(VerifierError::NoPendingUpgrade)
}

fn is_signer(env: &Env, signers: &Vec<Address>, candidate: &Address) -> bool {
    for i in 0..signers.len() {
        if let Some(s) = signers.get(i) {
            if crate::ct_address_eq(env, &s, candidate) {
                return true;
            }
        }
    }
    false
}

fn already_approved(env: &Env, pending: &PendingUpgrade, candidate: &Address) -> bool {
    for i in 0..pending.approvals.len() {
        if let Some(a) = pending.approvals.get(i) {
            if crate::ct_address_eq(env, &a, candidate) {
                return true;
            }
        }
    }
    false
}

fn wasm_hash_eq(left: &BytesN<32>, right: &BytesN<32>) -> bool {
    crate::ct_bytes32_eq(left, right)
}

/// Validate a `signers` set passed to `configure_upgrade_governance`.
pub fn validate_governance_config(
    env: &Env,
    signers: &Vec<Address>,
    threshold: u32,
    delay_ledgers: u32,
) -> Result<(), VerifierError> {
    if signers.is_empty() || threshold == 0 || threshold > signers.len() as u32 {
        return Err(VerifierError::InvalidGovernanceConfig);
    }
    if delay_ledgers == 0 {
        return Err(VerifierError::InvalidGovernanceConfig);
    }
    for i in 0..signers.len() {
        let a = signers.get(i).ok_or(VerifierError::InvalidGovernanceConfig)?;
        for j in (i + 1)..signers.len() {
            let b = signers.get(j).ok_or(VerifierError::InvalidGovernanceConfig)?;
            if crate::ct_address_eq(env, &a, &b) {
                return Err(VerifierError::InvalidGovernanceConfig);
            }
        }
    }
    Ok(())
}

pub fn store_governance_config(env: &Env, signers: Vec<Address>, threshold: u32, delay_ledgers: u32) {
    env.storage()
        .instance()
        .set(&StorageKey::UpgradeSigners, &signers);
    env.storage()
        .instance()
        .set(&StorageKey::UpgradeThreshold, &threshold);
    env.storage()
        .instance()
        .set(&StorageKey::UpgradeDelay, &delay_ledgers);
}

/// Record (or extend) an upgrade proposal in the signer's name.
///
/// Returns the new approval count after `signer` has been appended.
pub fn propose_upgrade(
    env: &Env,
    signer: &Address,
    wasm_hash: BytesN<32>,
) -> Result<u32, VerifierError> {
    let signers = load_signers(env);
    if !is_signer(env, &signers, signer) {
        return Err(VerifierError::NotAnUpgradeSigner);
    }

    let existing = env
        .storage()
        .instance()
        .get::<StorageKey, PendingUpgrade>(&StorageKey::PendingUpgrade);

    let mut pending = match existing {
        Some(p) if wasm_hash_eq(&p.wasm_hash, &wasm_hash) => p,
        Some(_) | None => PendingUpgrade {
            wasm_hash,
            approvals: Vec::new(env),
            started_ledger: env.ledger().sequence(),
        },
    };

    if already_approved(env, &pending, signer) {
        return Err(VerifierError::UpgradeAlreadyApproved);
    }

    pending.approvals.push_back(signer.clone());
    let approvals = pending.approvals.len();
    env.storage()
        .instance()
        .set(&StorageKey::PendingUpgrade, &pending);

    Ok(approvals as u32)
}

/// Verify an upgrade proposal is ready to execute: threshold reached and, if a
/// delay was configured, the timelock window has elapsed.
pub fn can_execute(env: &Env, target: &BytesN<32>) -> Result<PendingUpgrade, VerifierError> {
    let pending = load_pending(env)?;
    if !wasm_hash_eq(&pending.wasm_hash, target) {
        return Err(VerifierError::NoPendingUpgrade);
    }
    let threshold = load_threshold(env);
    if (pending.approvals.len() as u32) < threshold {
        return Err(VerifierError::NotEnoughUpgradeApprovals);
    }
    let delay = load_delay(env);
    if env.ledger().sequence() < pending.started_ledger + delay {
        return Err(VerifierError::UpgradeTimelockPending);
    }
    Ok(pending)
}

/// Clear the pending proposal after a successful (or cancelled) upgrade.
pub fn clear_pending(env: &Env) {
    env.storage().instance().remove(&StorageKey::PendingUpgrade);
}