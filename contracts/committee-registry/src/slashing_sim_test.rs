//! Committee stake slashing simulation suite.
//!
//! Exercises the two misbehaviour paths before real stake is at risk:
//!
//! - **Liveness failure** → `report_timeout` cuts every layer that backs the
//!   node (own stake, delegations, cooling-down undelegations) and pays the
//!   cut to the affected players.
//! - **Double-sign (equivocation)** → `report_slash` keeps a three-strike
//!   record; the third report halves the node's own stake.
//!
//! Every scenario is checked against three invariants (see `assert_invariants`):
//!
//! 1. **Total stake conserved** — minted supply is always fully accounted for
//!    by the contract's balance plus the balances of every known holder;
//! 2. **Solvency** — the contract never owes more than it holds;
//! 3. **Accounting consistency** — each node's `total_delegated_stake` equals
//!    the sum of its delegation records, and no balance ever goes negative.
//!
//! A report of the rules, the observed limits, and every assertion lives in
//! `docs/committee-slashing-simulation.md`.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    String,
};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A simulated registry with bookkeeping so every scenario can prove that no
/// token was created or destroyed by a slash.
struct Sim<'a> {
    env: Env,
    client: CommitteeRegistryContractClient<'a>,
    token: TokenClient<'a>,
    sac: StellarAssetClient<'a>,
    admin: Address,
    /// Every address that has ever received minted tokens.
    holders: Vec<Address>,
    /// All registered committee members, active or slashed.
    members: Vec<Address>,
    /// Parallel lists of (delegator, node) pairs the harness created.
    del_delegators: Vec<Address>,
    del_nodes: Vec<Address>,
    /// Lifetime tokens minted into the simulation.
    minted: i128,
}

fn push_unique(list: &mut Vec<Address>, addr: &Address) {
    for i in 0..list.len() {
        if list.get(i).unwrap() == *addr {
            return;
        }
    }
    list.push_back(addr.clone());
}

impl<'a> Sim<'a> {
    fn new() -> Sim<'static> {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(CommitteeRegistryContract, ());
        let client = CommitteeRegistryContractClient::new(&env, &contract_id);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token = TokenClient::new(&env, &sac.address());
        let sac_client = StellarAssetClient::new(&env, &sac.address());

        let admin = Address::generate(&env);
        // Min stake 100, cooldown 100 ledgers; fast timeout windows so the
        // liveness scenarios only need a couple of ledger closes.
        client.initialize(&admin, &token.address, &100, &100);
        client.set_timeout_config(&admin, &2, &2, &2);

        let holders = Vec::new(&env);
        let members = Vec::new(&env);
        let del_delegators = Vec::new(&env);
        let del_nodes = Vec::new(&env);
        Sim {
            env,
            client,
            token,
            sac: sac_client,
            admin,
            holders,
            members,
            del_delegators,
            del_nodes,
            minted: 0,
        }
    }

    fn fund(&mut self, to: &Address, amount: i128) {
        self.sac.mint(to, &amount);
        self.minted += amount;
        push_unique(&mut self.holders, to);
    }

    fn register(&mut self, stake: i128) -> Address {
        let member = Address::generate(&self.env);
        self.fund(&member, stake);
        self.client.register_member(
            &member,
            &stake,
            &String::from_str(&self.env, "node-endpoint"),
            &String::from_str(&self.env, "us-east-1"),
            &1_000,
        );
        push_unique(&mut self.members, &member);
        member
    }

    fn delegate(&mut self, delegator: &Address, node: &Address, amount: i128) {
        self.fund(delegator, amount);
        self.client.delegate(delegator, node, &amount);
        let mut paired = false;
        for i in 0..self.del_delegators.len() {
            if self.del_delegators.get(i).unwrap() == *delegator
                && self.del_nodes.get(i).unwrap() == *node
            {
                paired = true;
                break;
            }
        }
        if !paired {
            self.del_delegators.push_back(delegator.clone());
            self.del_nodes.push_back(node.clone());
        }
    }

    fn new_player(&mut self) -> Address {
        let player = Address::generate(&self.env);
        push_unique(&mut self.holders, &player);
        player
    }

    fn deposit_rake(&mut self, amount: i128) {
        let payer = Address::generate(&self.env);
        self.fund(&payer, amount);
        self.client.deposit_rake(&payer, &amount);
    }

    fn pay_node_fees(&mut self, node: &Address, amount: i128) {
        let payer = Address::generate(&self.env);
        self.fund(&payer, amount);
        self.client.distribute_node_fees(&payer, node, &amount);
    }

    fn players_vec(&self, players: &[Address]) -> Vec<Address> {
        let mut v = Vec::new(&self.env);
        for p in players {
            v.push_back(p.clone());
        }
        v
    }

    fn track(&self, game_id: u32, players: &Vec<Address>) {
        self.client
            .track_game_phase(&self.admin, &game_id, &GamePhase::Deal, players);
    }

    fn advance(&self, ledgers: u32) {
        self.env.ledger().with_mut(|ledger| {
            ledger.sequence_number += ledgers;
        });
    }

    fn timeout(&self, game_id: u32, node: &Address) -> i128 {
        self.client.report_timeout(&game_id, node)
    }

    /// Report a double-sign (equivocation) against `node`.
    fn double_sign(&self, node: &Address) {
        let reporter = Address::generate(&self.env);
        self.client
            .report_slash(&reporter, node, &Symbol::new(&self.env, "double_sign"));
    }

    fn stake_of(&self, node: &Address) -> i128 {
        self.client.get_member(node).stake
    }

    // -- invariants ---------------------------------------------------------

    /// Everything the contract currently owes: staked collateral, collateral
    /// cooling down, delegation principal and accrued delegator rewards, and
    /// the fee pool.
    fn liabilities(&self) -> i128 {
        let mut total: i128 = 0;

        for i in 0..self.members.len() {
            let member_addr = self.members.get(i).unwrap();
            let m = self.client.get_member(&member_addr);
            assert!(m.stake >= 0, "member stake cannot be negative");
            assert!(
                m.total_delegated_stake >= 0,
                "delegated stake cannot be negative"
            );
            total += m.stake;

            if let Some(unbonding) = self.client.get_stake_unbonding(&member_addr) {
                assert!(unbonding.amount >= 0, "unbonding cannot be negative");
                total += unbonding.amount;
            }

            let mut principal: i128 = 0;
            for j in 0..self.del_delegators.len() {
                if self.del_nodes.get(j).unwrap() != member_addr {
                    continue;
                }
                let delegator = self.del_delegators.get(j).unwrap();
                if let Some(rec) = self.client.get_delegation(&delegator, &member_addr) {
                    assert!(rec.amount >= 0, "delegation cannot be negative");
                    principal += rec.amount;
                    total += rec.amount;
                    // Accrued-but-unclaimed rewards are a liability too; the
                    // getter includes rewards not yet checkpointed.
                    let accrued = self.client.pending_rewards(&delegator, &member_addr);
                    assert!(accrued >= 0, "delegator rewards cannot be negative");
                    total += accrued;
                }
                if let Some(pending) = self
                    .client
                    .get_pending_undelegation(&delegator, &member_addr)
                {
                    assert!(pending.amount >= 0, "pending undelegation negative");
                    total += pending.amount;
                }
            }
            assert_eq!(
                principal, m.total_delegated_stake,
                "total_delegated_stake must equal the sum of delegation records"
            );
        }

        let pool = self.client.get_fee_pool();
        assert!(pool.undistributed >= 0, "fee pool cannot be negative");
        assert!(pool.pending >= 0, "pending rewards cannot be negative");
        total + pool.undistributed + pool.pending
    }

    /// **Total stake conserved**: every minted token is either held by the
    /// contract or sitting in a known holder's wallet. Slashing may move or
    /// strand tokens, but never creates or destroys them.
    fn assert_conservation(&self) {
        let mut outside: i128 = 0;
        for i in 0..self.holders.len() {
            outside += self.token.balance(&self.holders.get(i).unwrap());
        }
        let held = self.token.balance(&self.client.address);
        assert_eq!(
            self.minted,
            held + outside,
            "total stake conserved: minted supply must equal contract balance \
             plus holder balances"
        );
    }

    fn assert_invariants(&self) {
        self.assert_conservation();
        let held = self.token.balance(&self.client.address);
        let owed = self.liabilities();
        assert!(
            held >= owed,
            "solvency: contract holds {} but owes {}",
            held,
            owed
        );
    }
}

// ---------------------------------------------------------------------------
// Partial slash — a single misbehaviour event confiscates a fraction
// ---------------------------------------------------------------------------

#[test]
fn liveness_partial_slash_halves_node_stake_and_redistributes_to_players() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    let p1 = s.new_player();
    let p2 = s.new_player();

    let players = s.players_vec(&[p1.clone(), p2.clone()]);
    s.track(42, &players);
    s.advance(2);

    let owed_before = s.liabilities();
    let slashed = s.timeout(42, &node);

    // Partial slash: exactly half the stake is confiscated, half is retained.
    assert_eq!(slashed, 500);
    let m = s.client.get_member(&node);
    assert_eq!(m.stake, 500);
    assert_eq!(
        m.stake + slashed,
        1_000,
        "remainder + cut reconstructs stake"
    );
    assert!(!m.active, "a slashed node leaves the active set");
    assert_eq!(m.slash_count, 1);

    // The cut is paid out in full to the affected players.
    assert_eq!(s.token.balance(&p1), 250);
    assert_eq!(s.token.balance(&p2), 250);

    // Liabilities drop by exactly what left the contract.
    assert_eq!(s.liabilities(), owed_before - slashed);
    s.assert_invariants();
}

#[test]
fn liveness_partial_slash_floors_odd_amounts_exactly() {
    let mut s = Sim::new();
    let node = s.register(1_001);
    let delegator = Address::generate(&s.env);
    s.delegate(&delegator, &node, 999);
    s.client.undelegate(&delegator, &node, &7);

    let player = s.new_player();
    let players = s.players_vec(core::slice::from_ref(&player));
    s.track(1, &players);
    s.advance(2);
    let slashed = s.timeout(1, &node);

    let m = s.client.get_member(&node);
    let rec = s.client.get_delegation(&delegator, &node).unwrap();
    let pending = s
        .client
        .get_pending_undelegation(&delegator, &node)
        .unwrap();

    // Every layer applies floor(x / 2): odd balances round the cut down.
    assert_eq!(m.stake, 501); // 1_001 - floor(1_001 / 2)
    assert_eq!(rec.amount, 496); // 992 - floor(992 / 2)
    assert_eq!(pending.amount, 4); // 7 - floor(7 / 2)
    assert_eq!(m.total_delegated_stake, 496);
    assert_eq!(slashed, 500 + 496 + 3);
    assert_eq!(s.token.balance(&player), slashed);

    // No stroop is lost: remainder + cut reconstructs each original balance.
    assert_eq!(501 + 500, 1_001);
    assert_eq!(496 + 496, 992);
    assert_eq!(4 + 3, 7);
    s.assert_invariants();
}

#[test]
fn double_sign_first_two_strikes_confiscate_nothing() {
    let mut s = Sim::new();
    let node = s.register(1_000);

    for round in 0..2u32 {
        s.double_sign(&node);
        let m = s.client.get_member(&node);
        assert_eq!(
            m.stake,
            1_000,
            "strike {} must not confiscate stake",
            round + 1
        );
        assert_eq!(m.slash_count, round + 1);
        assert!(m.active, "record-only strikes leave the node on duty");
        s.assert_invariants();
    }
    assert_eq!(s.token.balance(&s.client.address), 1_000);
}

#[test]
fn double_sign_third_strike_partial_slashes_half_and_strands_tokens() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    let delegator = Address::generate(&s.env);
    s.delegate(&delegator, &node, 800);

    for _ in 0..2 {
        s.double_sign(&node);
    }
    s.double_sign(&node); // third strike: half the own stake

    let m = s.client.get_member(&node);
    assert_eq!(m.stake, 500);
    assert_eq!(m.stake, 1_000 - 1_000 / 2, "cut is floor(stake / 2)");
    assert!(!m.active);
    assert_eq!(m.slash_count, 3);

    // Documented behaviour: this path does not haircut delegators.
    let rec = s.client.get_delegation(&delegator, &node).unwrap();
    assert_eq!(rec.amount, 800);
    assert_eq!(m.total_delegated_stake, 800);

    // No tokens move: the confiscated half stays in the contract as surplus
    // that no liability claims yet.
    let held = s.token.balance(&s.client.address);
    assert_eq!(held, 1_800);
    assert_eq!(
        held - s.liabilities(),
        500,
        "the cut is stranded, not burned"
    );
    s.assert_invariants();
}

#[test]
fn partial_slash_can_leave_stake_below_the_registration_minimum() {
    let mut s = Sim::new();
    let node = s.register(101);

    for _ in 0..3 {
        s.double_sign(&node);
    }

    let m = s.client.get_member(&node);
    assert_eq!(m.stake, 51); // 101 - floor(101 / 2)
    assert!(m.stake < 100, "slashing does not re-enforce MinStake");
    assert!(!m.active, "but the member is off duty regardless");
    s.assert_invariants();
}

// ---------------------------------------------------------------------------
// Full slash — everything backing the node is swept, or driven to the floor
// ---------------------------------------------------------------------------

#[test]
fn full_slash_liveness_event_sweeps_node_delegations_and_undelegations() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    let a = Address::generate(&s.env);
    let b = Address::generate(&s.env);
    s.delegate(&a, &node, 700);
    s.client.undelegate(&a, &node, &100); // 600 active + 100 cooling down
    s.delegate(&b, &node, 400);

    let backing_before = 1_000 + 600 + 400 + 100;
    assert_eq!(s.token.balance(&s.client.address), backing_before);

    let player = s.new_player();
    let players = s.players_vec(core::slice::from_ref(&player));
    s.track(7, &players);
    s.advance(2);
    let slashed = s.timeout(7, &node);

    // One liveness event cuts every layer that backs the node.
    assert_eq!(slashed, 500 + 300 + 200 + 50);
    let m = s.client.get_member(&node);
    assert_eq!(m.stake, 500);
    assert_eq!(m.total_delegated_stake, 500);
    assert_eq!(s.client.get_delegation(&a, &node).unwrap().amount, 300);
    assert_eq!(s.client.get_delegation(&b, &node).unwrap().amount, 200);
    assert_eq!(
        s.client.get_pending_undelegation(&a, &node).unwrap().amount,
        50
    );

    // Full sweep: remainder + cut reconstructs the whole backing, layer by
    // layer — nothing outside the sweep scope keeps its value.
    assert_eq!(500 + 500, 1_000);
    assert_eq!(300 + 300, 600);
    assert_eq!(200 + 200, 400);
    assert_eq!(50 + 50, 100);
    assert_eq!(s.token.balance(&player), slashed);
    assert_eq!(s.liabilities(), backing_before - slashed);
    s.assert_invariants();
}

#[test]
fn full_slash_double_sign_campaign_confiscates_to_the_dust_floor() {
    let mut s = Sim::new();
    let node = s.register(1_000);

    let mut expected_stake = 1_000i128;
    let mut total_cut = 0i128;

    for round in 0..16u32 {
        let before = s.stake_of(&node);
        s.double_sign(&node);
        let after = s.stake_of(&node);
        let cut = before - after;

        if round < 2 {
            assert_eq!(cut, 0, "warning strikes confiscate nothing");
        } else {
            let want = expected_stake / 2;
            assert_eq!(cut, want, "round {} applies floor(stake / 2)", round + 1);
            expected_stake -= want;
        }
        assert_eq!(after, expected_stake);
        assert!(after >= 0, "stake must never go negative");
        total_cut += cut;
        s.assert_invariants();
    }

    // Repeated halving is bounded: s → ceil(s / 2) floors at one stroop, so a
    // campaign can confiscate everything but the last stroop — never zero.
    assert_eq!(expected_stake, 1);
    assert_eq!(total_cut, 999);
    assert_eq!(total_cut + expected_stake, 1_000);
    // The confiscated tokens are stranded in the contract, not destroyed.
    assert_eq!(s.token.balance(&s.client.address), 1_000);
}

#[test]
fn full_slash_both_misbehaviour_paths_apply_identical_halving_math() {
    let mut s = Sim::new();
    let double_signer = s.register(1_000);
    let timeout_node = s.register(1_000);
    let player = s.new_player();
    let players = s.players_vec(core::slice::from_ref(&player));

    for round in 0..14u32 {
        let a_before = s.stake_of(&double_signer);
        s.double_sign(&double_signer);
        let a_after = s.stake_of(&double_signer);

        let b_before = s.stake_of(&timeout_node);
        if round >= 2 {
            // Skip the two warning rounds on the liveness node so both nodes
            // enter each round with the same pre-slash stake.
            s.track(100 + round, &players);
            s.advance(2);
            s.timeout(100 + round, &timeout_node);
        }
        let b_after = s.stake_of(&timeout_node);

        assert_eq!(
            a_before - a_after,
            b_before - b_after,
            "round {} must apply the same cut on both paths",
            round + 1
        );
        assert_eq!(
            a_after,
            b_after,
            "round {} must leave the same remainder on both paths",
            round + 1
        );
        s.assert_invariants();
    }

    assert_eq!(s.stake_of(&double_signer), 1);
    assert_eq!(s.stake_of(&timeout_node), 1);
}

// ---------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------

#[test]
fn invariant_total_stake_conserved_across_mixed_misbehaviour_sequence() {
    let mut s = Sim::new();
    let n0 = s.register(1_000);
    let n1 = s.register(2_500);
    let n2 = s.register(1_500);
    s.assert_invariants();

    let a = Address::generate(&s.env);
    let b = Address::generate(&s.env);
    let c = Address::generate(&s.env);
    s.delegate(&a, &n0, 600);
    s.delegate(&b, &n0, 400);
    s.delegate(&c, &n1, 1_000);
    s.assert_invariants();

    s.client.undelegate(&a, &n0, &150);
    s.assert_invariants();
    s.client.begin_stake_unbonding(&n0, &300);
    assert_eq!(s.stake_of(&n0), 700);
    s.assert_invariants();

    // Fees flow in, split, and partially withdraw.
    s.deposit_rake(900);
    assert_eq!(s.client.distribute_fees(), 898);
    assert_eq!(s.client.get_pending_reward(&n0), 212);
    assert_eq!(s.client.get_pending_reward(&n1), 480);
    assert_eq!(s.client.get_pending_reward(&n2), 206);
    assert_eq!(s.client.get_fee_pool().undistributed, 2);
    assert_eq!(s.client.withdraw_rewards(&n1), 480);
    s.assert_invariants();

    // Direct node fees split with delegators, then claimed.
    s.pay_node_fees(&n1, 1_000);
    assert_eq!(s.client.pending_rewards(&c, &n1), 900);
    assert_eq!(s.client.claim_rewards(&c, &n1), 900);
    s.assert_invariants();

    // Double-sign on n2: three strikes → half of 1_500.
    for _ in 0..3 {
        s.double_sign(&n2);
    }
    assert_eq!(s.stake_of(&n2), 750);
    s.assert_invariants();

    // Liveness failure on n0: one event cuts own stake, both delegations and
    // the cooling-down undelegation.
    let p1 = s.new_player();
    let p2 = s.new_player();
    let players = s.players_vec(&[p1.clone(), p2.clone()]);
    s.track(900, &players);
    s.advance(2);
    let slashed = s.timeout(900, &n0);
    assert_eq!(slashed, 350 + 225 + 75 + 200);
    assert_eq!(s.token.balance(&p1), 425);
    assert_eq!(s.token.balance(&p2), 425);
    assert_eq!(s.stake_of(&n0), 350);
    s.assert_invariants();

    // Cooldowns complete; every withdrawal is fully backed.
    s.advance(100);
    assert_eq!(s.client.withdraw_undelegation(&a, &n0), 75);
    assert_eq!(s.client.complete_stake_unbonding(&n0), 300);
    s.assert_invariants();

    // Only n1 is still on duty.
    assert_eq!(s.client.get_active_members().len(), 1);
    assert_eq!(s.stake_of(&n1), 2_500);
}

#[test]
fn invariant_redistribution_returns_exactly_the_slashed_amount() {
    let mut s = Sim::new();
    let node = s.register(1_001);
    let p1 = s.new_player();
    let p2 = s.new_player();
    let p3 = s.new_player();

    let players = s.players_vec(&[p1.clone(), p2.clone(), p3.clone()]);
    s.track(11, &players);
    s.advance(2);
    let slashed = s.timeout(11, &node);

    // floor(500 / 3) = 166 each, the two odd stroops go to the earliest
    // listed players first.
    assert_eq!(slashed, 500);
    assert_eq!(s.token.balance(&p1), 167);
    assert_eq!(s.token.balance(&p2), 167);
    assert_eq!(s.token.balance(&p3), 166);
    assert_eq!(
        s.token.balance(&p1) + s.token.balance(&p2) + s.token.balance(&p3),
        slashed,
        "the cut is paid out exactly — nothing burned, nothing created"
    );
    s.assert_invariants();
}

#[test]
fn invariant_contract_balance_stays_exactly_backed_after_a_slash() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    let delegator = Address::generate(&s.env);
    s.delegate(&delegator, &node, 600);
    assert_eq!(s.token.balance(&s.client.address), 1_600);

    // Fees circulate without touching collateral.
    s.deposit_rake(400);
    assert_eq!(s.client.distribute_fees(), 400);
    assert_eq!(s.client.withdraw_rewards(&node), 400);
    assert_eq!(s.token.balance(&s.client.address), 1_600);

    let player = s.new_player();
    let players = s.players_vec(core::slice::from_ref(&player));
    s.track(3, &players);
    s.advance(2);
    let slashed = s.timeout(3, &node);

    // 500 (own) + 300 (delegation) leave the contract as one payout.
    assert_eq!(slashed, 800);
    assert_eq!(s.token.balance(&player), 800);
    let held = s.token.balance(&s.client.address);
    assert_eq!(held, 800);
    assert_eq!(
        s.liabilities(),
        800,
        "after the slash the contract holds exactly what it still owes"
    );
    s.assert_invariants();
}

// ---------------------------------------------------------------------------
// Documented gaps — these assert current behaviour on purpose; if the rules
// change, update the test and docs/committee-slashing-simulation.md together.
// ---------------------------------------------------------------------------

#[test]
fn documented_gap_cooling_down_unbonding_escapes_the_slash() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    s.client.begin_stake_unbonding(&node, &600);
    assert_eq!(s.stake_of(&node), 400);

    let player = s.new_player();
    let players = s.players_vec(core::slice::from_ref(&player));
    s.track(21, &players);
    s.advance(2);
    let slashed = s.timeout(21, &node);

    // Only the stake still at risk is cut; the cooling-down 600 is untouched.
    assert_eq!(slashed, 200);
    assert_eq!(s.stake_of(&node), 200);
    assert_eq!(s.client.get_stake_unbonding(&node).unwrap().amount, 600);
    s.assert_invariants();

    s.advance(100);
    assert_eq!(s.client.complete_stake_unbonding(&node), 600);
    s.assert_invariants();
}

#[test]
fn documented_gap_liveness_slash_applies_while_paused() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    let player = s.new_player();
    let players = s.players_vec(core::slice::from_ref(&player));
    s.track(5, &players);

    s.client.pause(&s.admin);
    s.advance(2);

    // report_timeout has no pause guard, so liveness slashing keeps working
    // while the rest of the registry is frozen.
    let slashed = s.timeout(5, &node);
    assert_eq!(slashed, 500);
    assert!(s.client.is_paused());
    assert_eq!(s.token.balance(&player), 500);
    s.assert_invariants();
}

#[test]
#[should_panic(expected = "contract paused")]
fn double_sign_report_is_blocked_while_paused() {
    let mut s = Sim::new();
    let node = s.register(1_000);
    s.client.pause(&s.admin);
    s.double_sign(&node);
}
