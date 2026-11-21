# Blue-Green Database Migrations — Expand-Contract Playbook

StellPoker's coordinator runs behind a hot standby and cannot tolerate
schema-change downtime during a trading session. This playbook defines the
**expand-contract** pattern we use for every production migration, the exact
commands to run at each phase, and the rollback procedure.

It extends `scripts/db/migrate.sh` with a safety wrapper
(`scripts/db/blue-green-migrate.sh`) and a dry-run checker
(`scripts/db/check-migration-safety.sh`).

> **Severities:** a migration that acquires a table-rewriting lock on a hot
> table in business hours is P1 by default — see
> [`incident-response.md`](incident-response.md).

---

## Why Expand-Contract

A traditional "edit the column, deploy the app" migration requires both the
old and new application code to be *simultaneously* incompatible with the
database for the duration of the deploy. On a two-instance blue/green topology
that is impossible to achieve without downtime.

Expand-contract breaks each schema change into three independent phases that
are each individually backwards- and forwards-compatible:

1. **Expand.** Add the new shape alongside the old. Both old and new app
   versions read and write successfully.
2. **Migrate.** Backfill data, dual-write from the application, switch reads
   to the new shape. Still both app versions working.
3. **Contract.** Remove the old shape *after* every running instance has been
   on the new code for at least one full deploy cycle.

Each phase is its own PR, its own migration pair, and its own deploy.

---

## Phase 1 — Expand

**Goal:** make the new shape available. **Never remove or rename.**

Allowed in an expand migration:
- `ADD COLUMN ... NULL` (never `NOT NULL` without a default; see below).
- `ADD COLUMN ... NOT NULL DEFAULT <literal>` on PostgreSQL 11+ (metadata-only).
- `CREATE INDEX CONCURRENTLY`.
- `CREATE TABLE`.
- `ADD CONSTRAINT ... NOT VALID` (deferred validation).
- Adding a new nullable foreign key.

Forbidden in an expand migration:
- `DROP COLUMN`, `DROP TABLE`, `ALTER COLUMN TYPE` on a non-empty table.
- `ADD COLUMN ... NOT NULL` without a default on a non-empty table.
- `ALTER TABLE ... RENAME`. If you must rename, add the new column, dual-write,
  switch reads, and drop the old one in a later contract migration.
- `CREATE INDEX` without `CONCURRENTLY`.

```bash
# Create the expand migration pair.
./scripts/db/migrate.sh add 20260615_expand_add_player_region
# Apply locally and verify.
./scripts/db/check-migration-safety.sh 20260615_expand_add_player_region
./scripts/db/migrate.sh up
```

Deploy the expand migration **before** the application change that depends on
it. Then deploy the app.

---

## Phase 2 — Migrate

Backfill in **small batched transactions**, not a single `UPDATE`. The helper
below is the canonical pattern.

```bash
./scripts/db/blue-green-migrate.sh backfill \
    --table players \
    --set "region = infer_region_from_ip(last_ip)" \
    --where "region IS NULL" \
    --batch-size 5000 \
    --sleep-ms 100
```

The app should dual-write for the duration of this phase: write both the old
and new columns on every mutation. Dual-writes land before the backfill
starts, so new rows are already correct and the backfill only has to clean up
historical data.

Switch reads to the new column once:
- The backfill has reached 100% (`region IS NULL` count is zero).
- Dual-writes have been in production for at least one deploy cycle.

---

## Phase 3 — Contract

Only after **every instance** on **every environment** has been running the
"reads from the new column" code for at least 24 hours.

```bash
# Create the contract migration pair.
./scripts/db/migrate.sh add 20260622_contract_drop_player_legacy_region
./scripts/db/migrate.sh up
```

Contract migrations are the only place `DROP COLUMN` / `DROP TABLE` /
`ALTER TYPE` on non-empty tables appear. The down migration for a contract
step is `RAISE EXCEPTION 'irreversible'` — rollback at this stage means
restoring from a snapshot, not running the down migration.

---

## Rollback Decision Tree

| Phase failed       | Rollback action                                                   |
|--------------------|-------------------------------------------------------------------|
| Expand             | Run `migrate.sh down`. New shape disappears; old code still works.|
| Migrate (backfill) | Stop the backfill script. No schema change to undo.               |
| Migrate (read-switch) | Deploy the previous app version. Old column is still populated.|
| Contract           | Restore from the pre-contract snapshot. Do **not** run `down`.    |

Always take a **named** snapshot before Phase 3:

```bash
./scripts/backup-coordinator-db.sh --tag pre-contract-20260622
```

The tag format `pre-contract-<YYYYMMDD>` is what `restore-coordinator-db.sh`
auto-completes against.

---

## Pre-flight Safety Checks

`scripts/db/check-migration-safety.sh <migration_name>` statically inspects a
migration file and refuses to pass if it sees any of the forbidden operations
above. It is wired into the `coordinator-schema-check` job in `ci.yml` and
runs on every PR touching `services/coordinator/migrations/`.

Run it locally before pushing:

```bash
./scripts/db/check-migration-safety.sh 20260615_expand_add_player_region
```

Exit code `0` = safe to deploy. Non-zero = read the flagged lines and either
split into expand+contract or add an inline `-- blue-green: acknowledged`
comment on the line with a written justification. Overrides are rare and
reviewed by a second engineer at PR time.

---

## Staging Snapshot Validation

Every expand-contract sequence is first exercised against a **staging snapshot
of production** before it is merged to `main`:

```bash
# Load last night's production snapshot into staging.
./scripts/restore-coordinator-db.sh --target staging --snapshot latest-prod

# Run all three phases against staging in sequence.
DATABASE_URL=$STAGING_DATABASE_URL ./scripts/db/blue-green-migrate.sh \
    rehearse --migration-tag 20260615_expand_add_player_region
```

Record the rehearsal run ID in the PR description. The CI `migration-rehearsal`
required check greps for `Rehearsal: <uuid>` and will not pass without it.

---

## Checklist Before Merging a Migration PR

- [ ] Migration file is named `<timestamp>_<expand|contract>_<verb>_<table>.{up,down}.sql`.
- [ ] `check-migration-safety.sh` passes.
- [ ] Rehearsed on a staging snapshot of production; rehearsal ID in PR body.
- [ ] If Phase 2 or 3: dual-write / read-switch deploys have shipped and been
      stable for at least one deploy cycle.
- [ ] If Phase 3: named snapshot taken (`pre-contract-<YYYYMMDD>`).
- [ ] PR description links the companion app PR, if any.
