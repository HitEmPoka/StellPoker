# ADR-006: Canary Release Process for Contract Upgrades

**Status**: Accepted

---

## Context

Issue #348 asks for a canary release process for contract upgrades: deploy
to a single test table first, monitor for errors, gradually roll out to
more tables, and roll back automatically if the error rate exceeds a
threshold.

`PokerTableContract` already had a timelocked upgrade mechanism
(`propose_upgrade` / `execute_upgrade` / `cancel_upgrade`,
`MIN_UPGRADE_DELAY_SECONDS` = 1 day) before this ADR. That mechanism is
**table-scoped in its API** (`propose_upgrade(table_id, ...)`) but the
underlying `env.deployer().update_current_contract_wasm(hash)` call it
makes upgrades the *entire deployed contract instance* — every table
managed by that instance shares one WASM binary. `create_table` returns a
`table_id` that's just a counter within one instance's storage; there is
no per-table contract deployment to canary independently at the WASM
level. This is a hard constraint of how Soroban contract upgrades work,
not a bug: **a subset of *existing* tables cannot run different code from
the rest while sharing one contract instance.**

That rules out the most literal reading of "deploy to a single test table
first" as an on-chain mechanism. The canary process below is designed
around this constraint rather than against it.

---

## Decision

A two-phase process: an **off-chain canary phase** using a genuinely
separate contract instance (where "single table first, gradually more"
*is* achievable, because each canary deployment is its own instance), followed by
the **existing on-chain timelock** for the real rollout, backed by a
**new fast-revert mechanism** for automatic rollback.

### Phase 1 — Canary instance (off-chain/deployment-tooling)

1. Deploy the candidate WASM as a **separate contract instance** (not an
   upgrade of any production instance) — a real, freshly-deployed
   `PokerTableContract`.
2. Create a small number of genuine test tables on it (`create_table`) —
   this is where "single test table first, gradually more" is actually
   true: each table created is a real table on real (canary) code, and
   the operator controls exactly how many exist, growing that count
   deliberately.
3. Route a controlled slice of traffic to it — internal testers first,
   then an opt-in cohort of real users if the game/product layer supports
   steering players toward specific tables — while production tables keep
   running unaffected on the current instance.
4. Monitor the canary instance's event stream (every state-changing
   entrypoint here already emits events —
   `upgrade_proposed`/`upgrade_executed`/`upgrade_reverted` and the
   gameplay events elsewhere in this contract) and transaction failure
   rate against the production instance's baseline for a defined bake
   period.
5. Any regression found here is cheap: tear down the canary instance,
   fix, redeploy a new canary. Nothing production-facing was ever at
   risk.

### Phase 2 — Production rollout (on-chain, this contract)

Once the canary instance has proven stable:

6. `propose_upgrade(table_id, new_wasm_hash, delay_seconds)` on the real
   production instance, starting the existing `MIN_UPGRADE_DELAY_SECONDS`
   (1 day) timelock. This delay is *not* the canary window — the canary
   phase above already generated the confidence to propose at all — it
   exists so seated players get advance notice, per the existing
   mechanism's original design.
7. `execute_upgrade(table_id)` once the delay elapses. This is the single
   point where *all* tables on the instance move to the new code at once
   — there is no gradual, in-place rollout across existing tables, per
   the Context section above.
8. **Automated rollback**: an off-chain monitor watches production error
   rate immediately after `execute_upgrade` (failed-transaction rate,
   `Failed`-status events, anomalous event patterns vs. the canary
   baseline). If it exceeds a configured threshold, it calls the new
   `revert_last_upgrade(table_id)` entrypoint automatically — no human in
   the loop required for the emergency-brake case, and no new timelock:
   a rollback needs to be fast, not deliberated over.

### `revert_last_upgrade` — new in this ADR

`execute_upgrade` now also records an `UpgradeRecord` (previous hash, new
hash, executed-at timestamp), chained from the previous tracked upgrade if
any. `revert_last_upgrade(table_id)`:

- Reverts to the *previous* hash with no timelock, admin-authorized only.
- Only available for `ROLLBACK_WINDOW_SECONDS` (6 hours) after the
  upgrade it would revert — a rollback that's still needed a week later
  isn't an emergency-brake case anymore; it needs the normal
  propose/execute process (and a real fix, not a revert to code that's
  now itself a week stale).
- Only reverts the single most recently executed upgrade, and consumes
  the record on use — no "redo," no reverting further back than one step.
  Going forward again after a revert requires a fresh
  `propose_upgrade`/`execute_upgrade` cycle with the normal timelock.
- Has nothing to revert to if the upgrade it would revert is the *first*
  one this mechanism has ever tracked for the table (the contract's
  genesis WASM hash was never recorded on-chain) — `NoUpgradeToRevert` in
  that case, same as when nothing was ever executed at all.

This directly satisfies "roll back automatically if error rate exceeds
threshold": before this ADR, undoing a bad `execute_upgrade` required a
brand-new `propose_upgrade` naming the old hash and waiting out a full
`MIN_UPGRADE_DELAY_SECONDS` — for an upgrade already causing production
errors, that's not a rollback, that's a day of degraded service. Now it's
one authorized call, immediately.

---

## Options Considered

### Option A: Per-table WASM versioning inside one instance

Store a `table_id -> wasm_hash` map and dispatch each table's logic to
the matching version at the entrypoint level, entirely inside one
contract instance.

Rejected: this isn't a Soroban contract upgrade at all — it would mean
maintaining N logic paths compiled into a *single* WASM binary and
switching between them by table_id, which defeats the purpose of a
canary (the "old" and "new" code aren't actually different deployments,
so a bug common to the dispatch logic itself, or in code shared between
versions, isn't caught any better than today) and makes every future
change progressively more expensive to build and audit as old versions
accumulate.

### Option B: A per-table factory (one contract instance per table)

Redesign so `create_table` deploys a fresh contract instance per table
(via `env.deployer().deploy_v2`) instead of a table_id key inside one
shared instance. This would make true per-table canarying possible
on-chain, since each table would be independently upgradable.

Rejected for this ADR: it's the architecturally "correct" answer for true
per-table canarying, but it's a breaking change to how tables are
identified and referenced throughout the rest of the contract, the
frontend, and the game-hub integration (`config.game_hub`) — far larger
in scope than a canary release process, and not something to fold into
this issue. Worth its own ADR if per-table independent upgradability
becomes a hard requirement later; the two-phase process above is the
practical answer given the current single-instance architecture.

### Option C: No automated rollback — propose a revert manually, like any other upgrade

Simpler: treat a bad upgrade exactly like a good one that needs reverting
— propose the old hash, wait out the timelock, execute.

Rejected: this doesn't satisfy "rollback automatically if error rate
exceeds threshold" at all — a day-long mandatory wait during a production
incident is not a rollback mechanism, it's an argument for adding a
faster one, which is what `revert_last_upgrade` is.
