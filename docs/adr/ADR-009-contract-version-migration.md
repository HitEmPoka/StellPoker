# ADR: Contract Version Registry and Dual-Version Migration Window

Status: Accepted

## Context

`poker-table` upgrades replace the WASM bytecode in place with
`update_current_contract_wasm`, so every table sharing one contract instance
runs the same code (see `ADR-006-canary-contract-upgrades.md`). Production
migrations nevertheless need a window in which some tables are on the old
version and some are on the new version, and the coordinator must know which
instance to call for each table.

Contemporary Soroban guidance is to record an explicit storage/schema version,
keep storage keys append-only, run a dedicated migration step after the WASM
swap, and treat rollback as a forward upgrade to a known-good artifact. The
coordinator's existing blue/green database playbook applies the same
expand/migrate/contract shape.

## Decision

1. Each table has an on-chain contract version in persistent storage:
   - `DataKey::ContractVersion(table_id)` — active version (`1` for legacy
     tables with no marker).
   - `DataKey::ContractVersionRecord(table_id)` — active version, WASM hash,
     and activation timestamp.
   - `DataKey::PendingContractVersion(table_id)` — version committed by
     `propose_versioned_upgrade`.
   - `DataKey::ContractVersionBeforeUpgrade(table_id)` — rollback source.
   - `DataKey::ContractVersionRecordPrev(table_id)` — previous version record
     preserved for rollback.
   - `DataKey::ContractMigration(table_id)` — from/to version and window end.
2. `propose_upgrade` remains backward compatible and bumps the version by one
   on execution. `propose_versioned_upgrade` commits an explicit target version
   and is rejected when the target is not newer than the active version.
3. `execute_upgrade` records the version transition and opens a migration
   window (24 hours by default). `revert_last_upgrade` restores the previous
   version and closes the window. `close_contract_migration_window` lets an
   admin end the window early after verification.
4. The coordinator keeps a registry of versioned contract instances:
   - `POKER_TABLE_CONTRACT` is the fallback v1 instance.
   - `POKER_TABLE_CONTRACT_VERSIONS=2=C...,3=C...` lists the instances that
     coexist during a rollout.
   - `TABLE_CONTRACT_VERSIONS=42=2,43=2` pins individual tables to a version.
   Invocations resolve the contract id through `SorobanConfig::contract_for_table`;
   unpinned tables stay on v1. `GET /api/chain-config` exposes the active and
   configured versions so operators can verify the routing.
5. Rollout follows expand/migrate/contract:
   - Expand: deploy the new instance and add it to
     `POKER_TABLE_CONTRACT_VERSIONS` while all tables remain pinned to v1.
   - Migrate: move canary tables to the new version with
     `TABLE_CONTRACT_VERSIONS`; both versions stay configured and the on-chain
     window is active.
   - Contract: after the window expires and error rates are clean, remove the
     old instance from the registry and stop serving v1.

## Safety rules

- Never reorder, rename, or remove existing `DataKey` variants; the storage
  layout snapshot and `storage_layout_test.rs` are append-only.
- Never point two versions at the same contract id during a migration; that
  would make the routing table meaningless.
- Rollback is a forward upgrade to the previous known-good WASM hash. The
  fast revert path closes the migration window so stale routes cannot linger.
- A table with an active hand should only be moved at a hand boundary; contract
  code cannot change within the current invocation.

## References

- Stellar, "Migrate contract storage data when upgrading data structures"
- Stellar, "Upgrading Wasm bytecode for a deployed contract"
- OpenZeppelin, "Upgrades and Migrations" (Stellar Contracts)
- `docs/blue-green-migrations.md`
- `docs/adr/ADR-006-canary-contract-upgrades.md`
