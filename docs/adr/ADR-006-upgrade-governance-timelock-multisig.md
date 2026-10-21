# ADR-006: Timelock + Multi-Sig Gated Contract Upgrades

**Status**: Accepted

---

## Context

`PokerTable` and the `ZkVerifier` contracts are upgradeable: an admin can
replace the contract WASM in place. In production, a single admin key holding
unilateral upgrade power is an unacceptable single point of failure:

- Compromise of the admin key lets an attacker replace the contract with a
  malicious implementation and steal table funds / rake balances.
- There is no way for stakeholders to observe and veto an upgrade before it
  lands.
- Deployment networks separate naturally into environments where different
  timelock lengths are appropriate (testnets prefer short delays, mainnet
  prefers long ones).

Issue #504 requested a per-network-configurable timelock plus an N-of-M
multi-sig required to invoke contract upgrades, applied to both contracts that
can be upgraded in place: `poker-table` and `zk-verifier`.

---

## Decision

Introduce an upgrade-governance layer on both contracts with three knobs and a
three-step lifecycle:

1. **`configure_upgrade_governance(admin, signers, threshold, delay_ledgers)`**
   — admin-only, stores the M-signer set, the N threshold, and the timelock
   delay in ledgers. Validation rejects empty signer sets, `threshold` outside
   `[1, len(signers)]`, duplicate signer addresses, and a zero delay.

2. **`propose_upgrade(signer, wasm_hash)`** — a configured signer opens (or
   extends) a pending proposal. A proposal targeting a different WASM hash
   discards the previous approvals and restarts the timelock clock.

3. **`execute_upgrade()`** — succeeds only when the pending proposal has
   collected at least `threshold` distinct signer approvals **and**
   `current_ledger >= started_ledger + delay_ledgers`. Execution clears the
   pending proposal and emits `upgrade_executed`.

Behavioural rules:

- A proposal may only be opened by a member of the configured signer set
  (`NotAnUpgradeSigner` otherwise).
- A signer may not approve a proposal twice (`UpgradeAlreadyApproved`).
- The legacy single-admin `upgrade(wasm_hash)` path is preserved **only for
  tables that predate governance**: once a signer set is configured for a
  table, `upgrade` routes through the governance checks and a fresh single
  signer is never sufficient on its own.
- The timelock window is deliberately applied even for `threshold = 1`
  configurations: a sole signer still cannot upgrade instantly.

The `PokerTable` variant is keyed per `table_id` (each table has its own signer
set, threshold, delay, and pending proposal). The `ZkVerifier` variant is
single-instance and shares one governance configuration. Storage keys and error
variants were added to both contracts (`UpgradeSigners`, `UpgradeThreshold`,
`UpgradeDelay`, `PendingUpgrade`, plus governance error codes 51–56 for
poker-table and 13–18 for zk-verifier).

---

## Options Considered

### Option A: Single upgraded admin key (status quo, rejected)

**Pros:** Minimal code, no ceremony, existing behaviour unchanged.

**Cons:** Single point of failure; no community veto window; no recovery if the
key rotates into hostile hands. Fails the stated acceptance criteria outright.

### Option B: Pure multi-sig, no timelock (considered)

**Pros:** N-of-M quorum removes reliance on a single key.

**Cons:** Window-binding is absent: an N-of-M that is itself compromised can
upgrade instantly with no opportunity for external interception. The issue
explicitly requires "cannot upgrade before delay", so `UpgradeTimelockPending`
is a first-class failure mode.

### Option C: Full DAO / governance token vote (considered and rejected)

**Pros:** The most decentralized upgrade authority.

**Cons:** Unjustified operational weight for contract upgrades; whip latency and
token manipulation risks; per-table voting state violates the "simple config,
observable state" goal of #508. The N-of-M signer model was judged sufficient.

---

## Consequences

**Positive:**

- Upgrade authority decentralizes across `N` of `M` signers while a fixed
  timelock (`delay_ledgers`) gives watchers a deterministic interception
  window.
- Delay is data-constrained per deployment (testnet/mainnet), satisfying the
  "per-network" requirement without per-env code forks.
- Backward compatible: legacy single-admin upgrades keep working until an admin
  explicitly configures governance for a given table.
- `can_execute_upgrade()` on the verifier (and the poker-table `PokerTableError`
  variants) give off-chain watchers a cheap read path to poll readiness.

**Negative:**

- Governance adds three storage keys and a proposal envelope per table (a one-time
  cost per table, plus transient pending-proposal storage during an upgrade).
- Once configured, governable upgrades require signer co-ordination; a lost
  signer set requires a new admin-configured set (which itself is an
  admin-privileged action).
- The timelock is measured in ledgers (~5 s each), so operators must map their
  desired wall-clock delays to ledger counts per network.

**Not covered (follow-up candidates):**

- Governance for `GameHub` and `CommitteeRegistry` (out of scope for #508).
- On-chain rotation of the governance configuration via a governed proposal
  rather than the admin key.
- A veto/cancellation mechanism for signers beyond the admin (currently any
  signer — or the admin — may `cancel_pending_upgrade` on poker-table only).

---

## Tests

- **poker-table**: unit tests for governance module storage/logic — threshold
  gating, timelock boundary (`delay - 1` blocked, `delay + 1` executable),
  non-signer rejection, duplicate-approval rejection, retarget-clock reset,
  config validation, and pending-proposal round-trip.
- **zk-verifier**: 4 integration tests — config validation, admin-only
  configuration, non-signer rejection + timelock boundary, threshold +
  timelock sequence, duplicate-approval rejection, and the `can_execute_upgrade`
  user-facing oracle.