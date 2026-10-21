# ADR-008: Committee Parameter Governance via Propose → Vote → Timelock → Execute

**Status**: Accepted

---

## Context

Committee parameters (timeout windows, minimum withdrawal, fee rates) are currently changed by a single admin address via direct setters on `CommitteeRegistry`. Issue #181 calls for decentralized governance so parameter changes go through a transparent, auditable lifecycle rather than a unilateral admin action. The design must specify the full propose → vote → timelock → execute flow and establish quorum/threshold rules before any implementation lands.

---

## Decision

Adopt a **propose → vote → timelock → execute** lifecycle for committee parameter changes, implemented in a standalone `governance` contract that is decoupled from (but targets) the committee registry.

### Lifecycle

1. **Propose** — any registered committee member submits a proposal naming a parameter key and its new value. A proposal ID is assigned.
2. **Vote** — each registered member casts one vote (For / Against) on the proposal. Double-voting is rejected.
3. **Timelock** — after voting closes (or immediately, if all members have voted), a fixed timelock delay must elapse before execution. This gives operators and players a window to react or exit.
4. **Execute** — any member may execute a proposal once (a) the quorum of votes is met, (b) the approval threshold is met, and (c) the timelock has elapsed. Execution applies the new parameter value.

### Quorum and threshold

- **Quorum**: more than `quorum_bps` of registered members must have voted. Default 5000 (50%).
- **Threshold**: more than `approval_threshold_bps` of votes cast must be For. Default 5000 (50%).
- Both are set at initialize time and can be updated by a successful governance proposal (self-amendment).

### Parameter scope (MVP)

- `timeout.deal_ledgers`, `timeout.reveal_ledgers`, `timeout.showdown_ledgers`
- `min_withdrawal`

The governance contract stores `(key, value)` pairs and emits events; a follow-up integration binds these to `CommitteeRegistry` setters once the registry's delegation module is restored.

---

## Options Considered

### Option A: Multisig-only (status quo)

Keep the single admin but require a Gnosis-style multisig to sign parameter changes.

**Pros:**
- No new contract surface.
- Immediate to deploy.

**Cons:**
- No transparency into who voted or when.
- No timelock — a compromised key can change parameters instantly.
- Does not meet the #181 requirement for an on-chain proposal lifecycle.

### Option B: Full on-chain governance with delegation and vote weighting

Token-weighted voting with delegation (like Compound Governor).

**Pros:**
- Scales beyond the fixed committee.
- Familiar pattern.

**Cons:**
- Far more complexity than needed for a 3–5 member committee.
- Token-weighted voting is inappropriate for a security-critical MPC committee where stake is already bonded.
- Slower to ship.

### Option C: Committee-member propose → vote → timelock → execute — **chosen**

One-member-one-vote among registered committee members, with a fixed timelock.

**Pros:**
- Matches the trust model: the committee members are the governed parties.
- One-member-one-vote is simple and resistant to stake-buying.
- Timelock provides the required safety window.
- Minimal contract surface; quorum/threshold are two basis-point constants.

**Cons:**
- Only committee members can propose/vote — excludes token holders.
- If the committee is captured, governance is captured. (Mitigated by the existing slashing/epoch-rotation mechanism.)

---

## Consequences

- Parameter changes become transparent: every proposal, vote, and execution is an on-chain event.
- The timelock introduces a delay between approval and effect; operators must monitor the proposal queue.
- The governance contract must be kept in sync with the committee's active member set (via epoch rotation or re-initialization).
- Self-amendment of quorum/threshold requires the same lifecycle, preventing silent rule changes.
- This ADR specifies the design; the MVP implementation lives in `contracts/governance` with quorum/threshold tests. Binding to `CommitteeRegistry` setters is deferred until the registry's delegation module is restored.
