use soroban_sdk::{contracttype, Env, Symbol};
use crate::types::{ContractMigrationWindow, DataKey, PokerTableError};

pub const LEGACY_CONTRACT_VERSION: u32 = 1;
pub const DEFAULT_MIGRATION_WINDOW_SECONDS: u64 = 86_400;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ContractVersionRecord {
    pub version: u32,
    pub wasm_hash: soroban_sdk::BytesN<32>,
    pub activated_at: u64,
}

pub fn get_contract_version(env: &Env, table_id: u32) -> u32 {
    env.storage()
        .persistent()
        .get::<DataKey, u32>(&DataKey::ContractVersion(table_id))
        .unwrap_or(LEGACY_CONTRACT_VERSION)
}

pub fn get_contract_version_record(env: &Env, table_id: u32) -> Option<ContractVersionRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::ContractVersionRecord(table_id))
}

pub fn get_pending_target_version(env: &Env, table_id: u32) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&DataKey::PendingContractVersion(table_id))
}

pub fn set_pending_target_version(
    env: &Env,
    table_id: u32,
    target_version: u32,
) -> Result<(), PokerTableError> {
    if target_version <= get_contract_version(env, table_id) {
        return Err(PokerTableError::InvalidContractVersion);
    }
    env.storage().persistent().set(
        &DataKey::PendingContractVersion(table_id),
        &target_version,
    );
    Ok(())
}

pub fn complete_upgrade(
    env: &Env,
    table_id: u32,
    wasm_hash: soroban_sdk::BytesN<32>,
    migration_window_seconds: u64,
) -> Result<ContractMigrationWindow, PokerTableError> {
    let from_version = get_contract_version(env, table_id);
    let to_version = env
        .storage()
        .persistent()
        .get::<DataKey, u32>(&DataKey::PendingContractVersion(table_id))
        .unwrap_or_else(|| from_version.saturating_add(1));

    if to_version <= from_version {
        return Err(PokerTableError::InvalidContractVersion);
    }

    let previous_record = get_contract_version_record(env, table_id);
    let now = env.ledger().timestamp();
    let record = ContractVersionRecord {
        version: to_version,
        wasm_hash: wasm_hash.clone(),
        activated_at: now,
    };
    let window = ContractMigrationWindow {
        from_version,
        to_version,
        started_at: now,
        ends_at: now.saturating_add(migration_window_seconds),
    };

    env.storage()
        .persistent()
        .set(&DataKey::ContractVersionBeforeUpgrade(table_id), &from_version);
    if let Some(previous_record) = previous_record {
        env.storage().persistent().set(
            &DataKey::ContractVersionRecordPrev(table_id),
            &previous_record,
        );
    }
    env.storage()
        .persistent()
        .set(&DataKey::ContractVersion(table_id), &to_version);
    env.storage()
        .persistent()
        .set(&DataKey::ContractVersionRecord(table_id), &record);
    env.storage()
        .persistent()
        .set(&DataKey::ContractMigration(table_id), &window);
    env.storage()
        .persistent()
        .remove(&DataKey::PendingContractVersion(table_id));

    env.events().publish(
        (Symbol::new(env, "contract_version_changed"), table_id),
        (from_version, to_version, wasm_hash),
    );
    Ok(window)
}

pub fn rollback(env: &Env, table_id: u32) -> Result<u32, PokerTableError> {
    let previous_version = env
        .storage()
        .persistent()
        .get::<DataKey, u32>(&DataKey::ContractVersionBeforeUpgrade(table_id))
        .ok_or(PokerTableError::NoUpgradeToRevert)?;
    let current_version = get_contract_version(env, table_id);
    let previous_record = env
        .storage()
        .persistent()
        .get::<DataKey, ContractVersionRecord>(&DataKey::ContractVersionRecordPrev(
            table_id,
        ));

    env.storage()
        .persistent()
        .set(&DataKey::ContractVersion(table_id), &previous_version);
    match previous_record {
        Some(record) => {
            env.storage().persistent().set(
                &DataKey::ContractVersionRecord(table_id),
                &record,
            );
        }
        None => {
            env.storage()
                .persistent()
                .remove(&DataKey::ContractVersionRecord(table_id));
        }
    }
    env.storage()
        .persistent()
        .remove(&DataKey::ContractVersionRecordPrev(table_id));
    env.storage()
        .persistent()
        .remove(&DataKey::ContractVersionBeforeUpgrade(table_id));
    env.storage()
        .persistent()
        .remove(&DataKey::ContractMigration(table_id));
    env.storage()
        .persistent()
        .remove(&DataKey::PendingContractVersion(table_id));

    env.events().publish(
        (Symbol::new(env, "version_rollback"), table_id),
        (current_version, previous_version),
    );
    Ok(previous_version)
}

pub fn get_migration_window(env: &Env, table_id: u32) -> Option<ContractMigrationWindow> {
    let window: ContractMigrationWindow = env
        .storage()
        .persistent()
        .get(&DataKey::ContractMigration(table_id))?;
    if env.ledger().timestamp() > window.ends_at {
        return None;
    }
    Some(window)
}

pub fn close_migration_window(env: &Env, table_id: u32) -> bool {
    let existed = env
        .storage()
        .persistent()
        .get::<DataKey, ContractMigrationWindow>(&DataKey::ContractMigration(table_id))
        .is_some();
    env.storage()
        .persistent()
        .remove(&DataKey::ContractMigration(table_id));
    existed
}
