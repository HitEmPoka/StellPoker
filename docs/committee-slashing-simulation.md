# Committee Slashing Simulation

Before real stake goes behind the committee, the registry's slashing rules are
exercised by a simulation suite that replays the two misbehaviour models a node
can actually be punished for, asserts the arithmetic of every cut, and proves
that no token is ever created or destroyed by a slash.

```
misbehaviour            entry point                          effect
────────────────────    ─────────────────────────────────    ──────────────────────────────
liveness failure        report_timeout(game_id, node_id)     halve every layer backing the
                                                             node, pay the cut to players
double-sign /           report_slash(reporter, member,       three-strike record; the third
equivocation            reason)                              report halves the node's stake
```

The suite lives in `contracts/committee-registry/src/slashing_sim_test.rs` and
is written against the contract's public interface, so it tests exactly what a
poker table or an off-chain reporter can trigger. Every scenario runs the same
three invariants after each state change (see Invariants below).

## Liveness failure

```rust
report_timeout(game_id: u32, node_id: Address) -> i128   // returns the total cut
```

A node that stops producing progress for a tracked phase is cut immediately,
and the cut is split among the affected players:

```
cut_node      = floor(node_stake / 2)
cut_deleg     = floor(delegation / 2)              per delegator
cut_cooling   = floor(pending_undelegation / 2)    per cooling-down request
slashed       = cut_node + Σ cut_deleg + Σ cut_cooling
```

Every layer rounds the cut **down**, so odd balances keep their odd stroop:
`1_001 → 501` retained (`liveness_partial_slash_floors_odd_amounts_exactly`).
Remainder plus cut reconstructs each original balance — no stroop is lost. The
node is deactivated (`active = false`) and `slash_count` increments in the same
call. Delegator rewards are checkpointed before their principal is haircut, so
accrued fees survive the slash untouched.

The payout is exact: `share = floor(slashed / players)`, with the odd stroops
going to the earliest listed players first
(`invariant_redistribution_returns_exactly_the_slashed_amount`).

## Double-sign (equivocation)

```rust
report_slash(reporter: Address, member: Address, reason: Symbol)
```

Reports accumulate a strike; only the third report moves funds:

```
report 1, 2  →  slash_count += 1        no funds move
report ≥ 3   →  cut = floor(stake / 2); stake -= cut; active = false
```

Strikes one and two confiscate nothing
(`double_sign_first_two_strikes_confiscate_nothing`). The third strike is a
**partial slash of the node's own stake only**: delegator principal is not
haircut and no payout happens — the confiscated half stays in the contract as
surplus that no liability claims yet
(`double_sign_third_strike_partial_slashes_half_and_strands_tokens`).

## Partial and full slash

The suite draws the line like this:

- **Partial slash** — one misbehaviour event confiscates a fraction of a
  balance; something is always retained. Covers the first liveness event
  (`liveness_partial_slash_halves_node_stake_and_redistributes_to_players`),
  floor behaviour on odd balances, the third-strike cut, and the edge case
  where the remainder falls below `MinStake`
  (`partial_slash_can_leave_stake_below_the_registration_minimum`).
- **Full slash** — the misbehaviour event covers everything the node has at
  risk, or a campaign of reports drives the stake to the floor the rule can
  reach:
  - one liveness event sweeps node stake, both delegations and the
    cooling-down undelegation together — a `2_100` backing becomes `1_050`
    retained plus `1_050` paid out, layer by layer
    (`full_slash_liveness_event_sweeps_node_delegations_and_undelegations`);
  - a repeated double-sign campaign takes a `1_000` stake down to `1` stroop,
    confiscating `999` in total while every cut remains exactly
    `floor(stake / 2)`
    (`full_slash_double_sign_campaign_confiscates_to_the_dust_floor`);
  - both misbehaviour paths apply byte-identical halving math to the same
    pre-state
    (`full_slash_both_misbehaviour_paths_apply_identical_halving_math`).

A literal 100 % confiscation of a member's own stake is **not reachable**: the
rule is `s → ceil(s / 2)`, which floors at one stroop no matter how many
reports arrive. The campaign test pins that limit down.

## Invariants

Asserted by `assert_invariants()` after every step of every scenario, and
directly by name in the tests:

1. **Total stake conserved** — `minted supply == contract balance + Σ holder
   balances`. Slashing may move tokens to players or strand them in the
   contract; it can never create or destroy one. Pinned by
   `invariant_total_stake_conserved_across_mixed_misbehaviour_sequence`,
   which interleaves delegations, an undelegation, a stake unbonding, rake
   distribution and withdrawal, node-fee distribution and claims, then a
   double-sign campaign and a liveness failure, checking after each step.
2. **Solvency** — the contract never owes more than it holds:

   ```
   contract balance ≥ Σ member stakes + Σ stake unbonding
                    + Σ delegation principal + Σ accrued delegator rewards
                    + Σ pending undelegations
                    + fee_pool.undistributed + fee_pool.pending
   ```

   In the clean case the two sides are exactly equal
   (`invariant_contract_balance_stays_exactly_backed_after_a_slash`); the only
   permitted surplus is a stranded strike cut.
3. **Accounting consistency** — each node's `total_delegated_stake` equals the
   sum of its delegation records, and no stake, delegation, reward or pool
   balance ever goes negative. Checked inside `liabilities()` on every pass.

## Limits and gaps

Current behaviour, each pinned by a test so a rule change has to update this
document too:

- **Halving never reaches zero.** Repeated reports floor at one stroop; from
  `1_000` a campaign can confiscate `999`, never `1_000`.
- **The third-strike cut is stranded.** `report_slash` neither pays out nor
  credits the confiscated half, and does not haircut delegators — the contract
  holds tokens that no liability claims.
- **Cooling-down stake escapes the slash.** `begin_stake_unbonding` removes
  the amount from `stake` immediately, and the liveness slash only reads
  `stake`, delegations and pending undelegations — an amount already unbonding
  is returned in full after a slash
  (`documented_gap_cooling_down_unbonding_escapes_the_slash`).
- **Asymmetric pause guard.** `report_timeout` has no auth and no pause check,
  so liveness slashing keeps working while the registry is paused
  (`documented_gap_liveness_slash_applies_while_paused`), whereas
  `report_slash` reverts while paused
  (`double_sign_report_is_blocked_while_paused`).
- **`MinStake` is not re-enforced after a cut** — a slashed member can be left
  below the registration minimum (it is inactive regardless).
- **Governance cannot tune the cut.** `protocol-governance` can vote on
  `committee_slash_penalty_bps`, but the registry never reads it; all four cut
  sites hard-code `floor(x / 2)`.

## Running the suite

```
cargo test -p committee-registry slashing_sim   # this suite only
cargo test -p committee-registry                # all registry tests
```

The suite is part of the CI matrix job for `committee-registry`.

## Events

```
topics: ("slash_reported", slash_count)   data: (member, reason)
topics: ("timeout_reported", game_id)     data: (node, phase, slashed)
```
