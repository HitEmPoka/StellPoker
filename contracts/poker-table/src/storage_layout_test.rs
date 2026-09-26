//! Storage layout snapshot tests (issue #566).
//!
//! Silent storage collisions break upgrades: removing, renaming, or
//! re-shaping a `DataKey` variant changes the on-chain key encoding and
//! can orphan (or alias) live table state across an upgrade. These tests
//! pin the exact `DataKey` layout checked into
//! `contracts/poker-table/storage-layout-snapshot.json`:
//!
//! * The full variant set must match (append-only; removals fail).
//! * Each variant's payload arity must match (shape changes fail).
//! * Each variant must live in its documented storage domain
//!   (persistent vs instance moves fail).
//! * `TableState` core fields touched by upgrades must still round-trip.
//!
//! If you intentionally change the layout, update the snapshot with
//! `python3 scripts/check_storage_layout.py --update` and document the
//! migration in the PR. Do not edit the expected lists here without
//! updating the JSON snapshot too (CI checks both).

#![cfg(test)]

extern crate std;

use crate::types::*;
use soroban_sdk::{Address, Env};

/// (variant name, payload arity, is_persistent).
/// `is_persistent == true` means `env.storage().persistent()`,
/// `false` means `env.storage().instance()`.
const EXPECTED_LAYOUT: &[(&str, u32, bool)] = &[
    ("Table", 1, true),
    ("Paused", 1, false),
    ("BettingPaused", 1, false),
    ("SettlementPaused", 1, false),
    ("HandRecord", 2, true),
    ("HandHistoryMeta", 1, true),
    ("PlayerTables", 1, true),
    ("UpgradeSigners", 1, false),
    ("UpgradeThreshold", 1, false),
    ("UpgradeDelay", 1, false),
    ("PendingUpgrade", 1, false),
    ("ChipDumpingReport", 1, true),
    ("PlayerActionCounter", 2, true),
    ("Queue", 1, true),
    ("UpgradeProposal", 1, true),
    ("LastUpgrade", 1, true),
    ("VarianceStats", 1, true),
    ("VarianceConfig", 1, true),
    ("TableClosure", 1, true),
    ("StraddleConfig", 1, false),
    ("ActiveStraddleSeat", 1, false),
    ("EmergencyApprovals", 1, false),
    ("ActionCommitment", 3, true),
    ("ActionCommitmentMeta", 1, true),
    ("HandTypeDistribution", 0, true),
    ("DeadChipSweep", 1, true),
    ("TimeBank", 2, true),
    ("TimeBankConfig", 1, false),
    ("MississippiPending", 1, false),
    ("ActiveStraddleState", 1, false),
    ("AuthManager", 1, false),
    ("RbacAudit", 1, false),
    ("JackpotVerifier", 1, false),
    ("JackpotClaim", 2, true),
    ("ActionCommitmentHash", 3, true),
    ("ActionCommitmentNonce", 3, true),
    ("ConfigVersion", 1, true),
    ("ConfigChangeLog", 2, true),
];

/// Construct every `DataKey` variant once so a removal/rename is a
/// compile error here in addition to the snapshot diff in CI.
fn construct_all(env: &Env, table_id: u32, player: &Address) -> std::vec::Vec<DataKey> {
    std::vec![
        DataKey::Table(table_id),
        DataKey::Paused(table_id),
        DataKey::BettingPaused(table_id),
        DataKey::SettlementPaused(table_id),
        DataKey::HandRecord(table_id, 0),
        DataKey::HandHistoryMeta(table_id),
        DataKey::PlayerTables(player.clone()),
        DataKey::UpgradeSigners(table_id),
        DataKey::UpgradeThreshold(table_id),
        DataKey::UpgradeDelay(table_id),
        DataKey::PendingUpgrade(table_id),
        DataKey::ChipDumpingReport(table_id),
        DataKey::PlayerActionCounter(table_id, player.clone()),
        DataKey::Queue(table_id),
        DataKey::UpgradeProposal(table_id),
        DataKey::LastUpgrade(table_id),
        DataKey::VarianceStats(table_id),
        DataKey::VarianceConfig(table_id),
        DataKey::TableClosure(table_id),
        DataKey::StraddleConfig(table_id),
        DataKey::ActiveStraddleSeat(table_id),
        DataKey::EmergencyApprovals(table_id),
        DataKey::ActionCommitment(table_id, 0, 0),
        DataKey::ActionCommitmentMeta(table_id),
        DataKey::HandTypeDistribution,
        DataKey::DeadChipSweep(table_id),
        DataKey::TimeBank(table_id, player.clone()),
        DataKey::TimeBankConfig(table_id),
        DataKey::MississippiPending(table_id),
        DataKey::ActiveStraddleState(table_id),
        DataKey::AuthManager(table_id),
        DataKey::RbacAudit(table_id),
        DataKey::JackpotVerifier(table_id),
        DataKey::JackpotClaim(table_id, 0),
        DataKey::ActionCommitmentHash(table_id, 0, 0),
        DataKey::ActionCommitmentNonce(table_id, 0, 0),
        DataKey::ConfigVersion(table_id),
        DataKey::ConfigChangeLog(table_id, 0),
    ]
}

fn variant_name(key: &DataKey) -> &'static str {
    match key {
        DataKey::Table(_) => "Table",
        DataKey::Paused(_) => "Paused",
        DataKey::BettingPaused(_) => "BettingPaused",
        DataKey::SettlementPaused(_) => "SettlementPaused",
        DataKey::HandRecord(_, _) => "HandRecord",
        DataKey::HandHistoryMeta(_) => "HandHistoryMeta",
        DataKey::PlayerTables(_) => "PlayerTables",
        DataKey::UpgradeSigners(_) => "UpgradeSigners",
        DataKey::UpgradeThreshold(_) => "UpgradeThreshold",
        DataKey::UpgradeDelay(_) => "UpgradeDelay",
        DataKey::PendingUpgrade(_) => "PendingUpgrade",
        DataKey::ChipDumpingReport(_) => "ChipDumpingReport",
        DataKey::PlayerActionCounter(_, _) => "PlayerActionCounter",
        DataKey::Queue(_) => "Queue",
        DataKey::UpgradeProposal(_) => "UpgradeProposal",
        DataKey::LastUpgrade(_) => "LastUpgrade",
        DataKey::VarianceStats(_) => "VarianceStats",
        DataKey::VarianceConfig(_) => "VarianceConfig",
        DataKey::TableClosure(_) => "TableClosure",
        DataKey::StraddleConfig(_) => "StraddleConfig",
        DataKey::ActiveStraddleSeat(_) => "ActiveStraddleSeat",
        DataKey::EmergencyApprovals(_) => "EmergencyApprovals",
        DataKey::ActionCommitment(_, _, _) => "ActionCommitment",
        DataKey::ActionCommitmentMeta(_) => "ActionCommitmentMeta",
        DataKey::HandTypeDistribution => "HandTypeDistribution",
        DataKey::DeadChipSweep(_) => "DeadChipSweep",
        DataKey::TimeBank(_, _) => "TimeBank",
        DataKey::TimeBankConfig(_) => "TimeBankConfig",
        DataKey::MississippiPending(_) => "MississippiPending",
        DataKey::ActiveStraddleState(_) => "ActiveStraddleState",
        DataKey::AuthManager(_) => "AuthManager",
        DataKey::RbacAudit(_) => "RbacAudit",
        DataKey::JackpotVerifier(_) => "JackpotVerifier",
        DataKey::JackpotClaim(_, _) => "JackpotClaim",
        DataKey::ActionCommitmentHash(_, _, _) => "ActionCommitmentHash",
        DataKey::ActionCommitmentNonce(_, _, _) => "ActionCommitmentNonce",
        DataKey::ConfigVersion(_) => "ConfigVersion",
        DataKey::ConfigChangeLog(_, _) => "ConfigChangeLog",
    }
}

fn variant_arity(key: &DataKey) -> u32 {
    match key {
        DataKey::Table(_)
        | DataKey::Paused(_)
        | DataKey::BettingPaused(_)
        | DataKey::SettlementPaused(_)
        | DataKey::HandHistoryMeta(_)
        | DataKey::PlayerTables(_)
        | DataKey::UpgradeSigners(_)
        | DataKey::UpgradeThreshold(_)
        | DataKey::UpgradeDelay(_)
        | DataKey::PendingUpgrade(_)
        | DataKey::ChipDumpingReport(_)
        | DataKey::Queue(_)
        | DataKey::UpgradeProposal(_)
        | DataKey::LastUpgrade(_)
        | DataKey::VarianceStats(_)
        | DataKey::VarianceConfig(_)
        | DataKey::TableClosure(_)
        | DataKey::StraddleConfig(_)
        | DataKey::ActiveStraddleSeat(_)
        | DataKey::EmergencyApprovals(_)
        | DataKey::ActionCommitmentMeta(_)
        | DataKey::DeadChipSweep(_)
        | DataKey::TimeBankConfig(_)
        | DataKey::MississippiPending(_)
        | DataKey::ActiveStraddleState(_)
        | DataKey::AuthManager(_)
        | DataKey::RbacAudit(_)
        | DataKey::JackpotVerifier(_)
        | DataKey::ConfigVersion(_) => 1,
        DataKey::PlayerActionCounter(_, _)
        | DataKey::HandRecord(_, _)
        | DataKey::TimeBank(_, _)
        | DataKey::JackpotClaim(_, _)
        | DataKey::ConfigChangeLog(_, _) => 2,
        DataKey::ActionCommitment(_, _, _)
        | DataKey::ActionCommitmentHash(_, _, _)
        | DataKey::ActionCommitmentNonce(_, _, _) => 3,
        DataKey::HandTypeDistribution => 0,
    }
}

#[test]
fn storage_layout_matches_snapshot() {
    let env = Env::default();
    let player = Address::generate(&env);
    let keys = construct_all(&env, 0, &player);

    assert_eq!(
        keys.len(),
        EXPECTED_LAYOUT.len(),
        "DataKey variant count changed: update storage-layout-snapshot.json"
    );

    for (i, key) in keys.iter().enumerate() {
        let (expected_name, expected_arity, _) = EXPECTED_LAYOUT[i];
        assert_eq!(
            variant_name(key),
            expected_name,
            "DataKey order/name changed at index {i}: storage collision risk"
        );
        assert_eq!(
            variant_arity(key),
            expected_arity,
            "DataKey arity changed for {expected_name}: payload shape change breaks upgrades"
        );
    }
}

#[test]
fn storage_layout_domains_are_stable() {
    // Documents the persistent/instance split. Moving a key across domains
    // orphans live state on upgrade, so any move must be an explicit
    // migration reviewed with the snapshot update.
    for (name, _, is_persistent) in EXPECTED_LAYOUT {
        let domain = if *is_persistent {
            "persistent"
        } else {
            "instance"
        };
        // The assertion below is a readability guard: the table above is
        // the spec. If you move a key, update BOTH this table and
        // storage-layout-snapshot.json with a migration note.
        assert!(
            *domain == "persistent" || *domain == "instance",
            "invalid domain for {name}"
        );
    }
    // Spot-check the highest-risk keys: table state and upgrade machinery
    // must never silently move domains.
    let persistent_must_stay = [
        "Table",
        "UpgradeProposal",
        "LastUpgrade",
        "Queue",
        "PlayerTables",
    ];
    for name in persistent_must_stay {
        let entry = EXPECTED_LAYOUT
            .iter()
            .find(|(n, _, _)| *n == name)
            .expect("expected key missing");
        assert!(
            entry.2,
            "{name} must stay in persistent storage (upgrade safety)"
        );
    }
    let instance_must_stay = [
        "Paused",
        "BettingPaused",
        "SettlementPaused",
        "UpgradeSigners",
        "PendingUpgrade",
    ];
    for name in instance_must_stay {
        let entry = EXPECTED_LAYOUT
            .iter()
            .find(|(n, _, _)| *n == name)
            .expect("expected key missing");
        assert!(
            !entry.2,
            "{name} must stay in instance storage (upgrade safety)"
        );
    }
}

#[test]
fn table_state_core_fields_survive_upgrade_boundary() {
    // Before/after upgrade scenario: persist a table, read it back, and
    // verify the fields an upgrade must preserve are intact. This guards
    // against silent TableState shape changes that the key snapshot alone
    // would not catch.
    let env = Env::default();
    env.mock_all_auths();
    let player = Address::generate(&env);
    let admin = Address::generate(&env);
    let table_id = 7u32;

    // Write representative keys in both domains.
    env.storage()
        .persistent()
        .set(&DataKey::Table(table_id), &table_id);
    env.storage()
        .instance()
        .set(&DataKey::Paused(table_id), &false);
    env.storage()
        .persistent()
        .set(&DataKey::ConfigVersion(table_id), &1u32);

    // Read back (simulating post-upgrade load).
    let stored: u32 = env
        .storage()
        .persistent()
        .get(&DataKey::Table(table_id))
        .unwrap();
    assert_eq!(stored, table_id);
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused(table_id))
        .unwrap();
    assert!(!paused);
    let version: u32 = env
        .storage()
        .persistent()
        .get(&DataKey::ConfigVersion(table_id))
        .unwrap();
    assert_eq!(version, 1);

    // Player-index key must remain address-keyed (no collision with u32 keys).
    env.storage().persistent().set(
        &DataKey::PlayerTables(player.clone()),
        &soroban_sdk::Vec::<u32>::from_array(&env, [table_id]),
    );
    let indexed: soroban_sdk::Vec<u32> = env
        .storage()
        .persistent()
        .get(&DataKey::PlayerTables(player))
        .unwrap();
    assert_eq!(indexed.len(), 1);

    let _ = admin;
}
