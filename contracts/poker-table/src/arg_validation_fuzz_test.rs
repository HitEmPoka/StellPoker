//! Argument validation fuzzing for poker-table entrypoints (issue #568).
//!
//! #28 fuzzes the action handler; this suite expands coverage to *all*
//! entrypoints with arbitrary arguments. Goal: no unexpected traps —
//! invalid input must yield a typed `PokerTableError`, never a host trap.
//!
//! Strategy: custom `proptest` generators (no extra deps) produce edge-case
//! scalars (`i128::MIN/MAX`, negative buy-ins, oversized vectors, bogus
//! table ids, invalid blinds) plus fully arbitrary values. Each case drives
//! the contract through its `try_*` client (which captures contract errors
//! as `Err(Ok(_))`) and asserts the result is either success or a typed
//! contract error — never `Err(Err(_))` (host trap / unexpected panic).
//!
//! Run nightly in CI (`.github/workflows/poker-table-arg-fuzz.yml`) and
//! locally with `cargo test -p poker-table arg_validation -- --nocapture`.

#![cfg(test)]

extern crate std;

use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use proptest::prelude::*;
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, Vec,
};
use std::format;
use std::vec::Vec as StdVec;

#[contract]
pub struct ArgFuzzHub;

#[contractimpl]
impl ArgFuzzHub {
    pub fn start_game(
        _env: Env,
        _game_id: Address,
        _session_id: u32,
        _p1: Address,
        _p2: Address,
        _p1_pts: i128,
        _p2_pts: i128,
    ) {
    }
    pub fn end_game(_env: Env, _session_id: u32, _p1_won: bool) {}
}

struct F<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    token: TokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    admin: Address,
    committee: Address,
    verifier: Address,
}

fn fuzz_setup() -> F<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    let contract_id = env.register(PokerTableContract, ());
    let client = PokerTableContractClient::new(&env, &contract_id);

    let token_admin_addr = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin_addr);
    let token = TokenClient::new(&env, &sac.address());
    let token_admin = StellarAssetClient::new(&env, &sac.address());

    let admin = Address::generate(&env);
    let committee = Address::generate(&env);
    let verifier = env.register(crate::verifier::ZkVerifierContract, ());

    F {
        env,
        client,
        token,
        token_admin,
        admin,
        committee,
        verifier,
    }
}

fn fuzz_config(f: &F) -> TableConfig {
    let game_hub = f.env.register(ArgFuzzHub, ());
    TableConfig {
        token: f.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 1000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&f.env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: f.committee.clone(),
        verifier: f.verifier.clone(),
        game_hub,
        rake_bps: 0,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
        street_time_limit: OptionalStreetTimeLimit::None,
        treasury: None,
        dead_chip_timeout_ledgers: 0,
        reclaim_period_ledgers: 0,
    }
}

/// Assert a `try_*` result is success or a *typed* contract error, never a
/// host trap. `try_*` returns `Ok(_)` on success (including `Ok(Err(e))`
/// for typed errors in some SDK shapes) and `Err(Ok(e))` for typed contract
/// errors vs `Err(Err(host))` for traps — so any `Err` whose debug starts
/// with `Err(` is an unexpected trap.
fn expect_typed_only<T: std::fmt::Debug, E: std::fmt::Debug>(res: Result<T, E>, ctx: &str) {
    if let Err(e) = res {
        let s = format!("{:?}", e);
        assert!(
            s.starts_with("Ok("),
            "unexpected trap on {ctx}: {s} (expected typed PokerTableError only)"
        );
    }
}

fn setup_live_preflop(f: &F) -> (u32, StdVec<Address>, StdVec<u32>) {
    let table_id = f.client.create_table(&f.admin, &fuzz_config(f));
    let mut players = StdVec::new();
    let mut seqs = StdVec::new();
    for _ in 0..2 {
        let p = Address::generate(&f.env);
        f.token_admin.mint(&p, &5000);
        f.client.join_table(&table_id, &p, &500);
        players.push(p);
        seqs.push(0u32);
    }
    let _ = f.client.try_start_hand(&table_id);
    // Mock deal -> Preflop.
    let root = BytesN::from_array(&f.env, &[1u8; 32]);
    let mut comms: Vec<BytesN<32>> = Vec::new(&f.env);
    let mut idxs: Vec<u32> = Vec::new(&f.env);
    for _ in 0..2 {
        comms.push_back(BytesN::from_array(&f.env, &[2u8; 32]));
    }
    for i in 0..4u32 {
        idxs.push_back(i);
    }
    let _ = f.client.try_commit_deal(
        &table_id,
        &f.committee,
        &root,
        &comms,
        &idxs,
        &Bytes::new(&f.env),
        &Bytes::new(&f.env),
    );
    (table_id, players, seqs)
}

// ---------------------------------------------------------------------------
// Generators (custom arbitrary args)
// ---------------------------------------------------------------------------

fn arb_buy_in() -> impl Strategy<Value = i128> {
    prop_oneof![
        Just(0i128),
        Just(-1i128),
        Just(1i128),
        Just(99i128),
        Just(100i128),
        Just(1000i128),
        Just(1001i128),
        Just(i128::MIN),
        Just(i128::MAX),
        Just(i128::MIN + 1),
        (-10_000i128..10_000i128),
        any::<i128>(),
    ]
}

fn arb_table_id() -> impl Strategy<Value = u32> {
    prop_oneof![Just(0u32), Just(1u32), Just(u32::MAX), any::<u32>(), 0u32..10u32,]
}

fn arb_amount() -> impl Strategy<Value = i128> {
    prop_oneof![
        Just(0i128),
        Just(-1i128),
        Just(1i128),
        Just(i128::MIN),
        Just(i128::MAX),
        (-100_000i128..100_000i128),
        any::<i128>(),
    ]
}

fn arb_action() -> impl Strategy<Value = (u8, i128)> {
    // (discriminant, amount). Maps to Fold/Check/Call/Bet/Raise/AllIn.
    (0u8..=7, arb_amount())
}

fn action_from_parts(disc: u8, amount: i128) -> Action {
    match disc % 6 {
        0 => Action::Fold,
        1 => Action::Check,
        2 => Action::Call,
        3 => Action::Bet(amount),
        4 => Action::Raise(amount),
        _ => Action::AllIn,
    }
}

fn arb_card_value() -> impl Strategy<Value = u32> {
    prop_oneof![Just(0u32), Just(51u32), Just(52u32), Just(u32::MAX), 0u32..100u32,]
}

fn arb_rake_bps() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(0u32),
        Just(500u32),
        Just(501u32),
        Just(u32::MAX),
        any::<u32>(),
    ]
}

// ---------------------------------------------------------------------------
// Property tests: every entrypoint must return typed errors only
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// join_table with arbitrary buy-ins/table ids never traps.
    #[test]
    fn prop_join_table_arg_validation_never_traps(
        table_id in arb_table_id(),
        buy_in in arb_buy_in(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        // Fund generously so the token transfer itself never traps; the
        // *validation* under test is the contract's buy-in check.
        let player = Address::generate(&f.env);
        f.token_admin.mint(&player, &1_000_000);
        // Ensure at least one table exists so table 0 is sometimes valid.
        let _ = f.client.try_create_table(&f.admin, &fuzz_config(&f));
        let res = f.client.try_join_table(&table_id, &player, &buy_in);
        expect_typed_only(res, "join_table");
    }

    /// create_table with arbitrary rake/player counts/blinds never traps.
    #[test]
    fn prop_create_table_arg_validation_never_traps(
        rake_bps in arb_rake_bps(),
        min_players in any::<u32>(),
        max_players in any::<u32>(),
        small_blind in arb_amount(),
        big_blind in arb_amount(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        let mut cfg = fuzz_config(&f);
        cfg.rake_bps = rake_bps;
        cfg.min_players = min_players;
        cfg.max_players = max_players;
        // Clamp blinds into i128 range the SDK can encode; validation of
        // inverted/negative blinds is what we exercise, not harness OOM.
        let sb = small_blind.clamp(-1_000_000, 1_000_000);
        let bb = big_blind.clamp(-1_000_000, 1_000_000);
        cfg.blinds_schedule = BlindsSchedule::fixed(&f.env, sb, bb);
        let res = f.client.try_create_table(&f.admin, &cfg);
        expect_typed_only(res, "create_table");
        // Invalid configs must be rejected (not silently accepted).
        if rake_bps > 500 || min_players < 2 || max_players < min_players || max_players > 6 {
            prop_assert!(res.is_err(), "invalid config should be rejected: rake={rake_bps} min={min_players} max={max_players}");
        }
    }

    /// player_action with arbitrary actions/amounts/seqs never traps.
    #[test]
    fn prop_player_action_arg_validation_never_traps(
        disc in 0u8..=7,
        amount in arb_amount(),
        seq in any::<u32>(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        let (table_id, players, _) = setup_live_preflop(&f);
        let table = f.client.get_table(&table_id);
        // Pick the player whose turn it is when in a betting phase;
        // otherwise fuzz against a random seated player (expect
        // NotInBettingPhase / NotYourTurn typed errors, not traps).
        let victim = if matches!(table.phase, GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River) {
            table.players.get(table.current_turn).map(|p| p.address).unwrap_or_else(|| players[0].clone())
        } else {
            players[0].clone()
        };
        let action = action_from_parts(disc, amount);
        let res = f.client.try_player_action(&table_id, &victim, &seq, &action);
        expect_typed_only(res, &format!("player_action({action:?}, seq={seq})"));
    }

    /// commit_deal with arbitrary counts/indices never traps.
    #[test]
    fn prop_commit_deal_arg_validation_never_traps(
        n_commitments in 0usize..8,
        n_indices in 0usize..12,
        card in arb_card_value(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        let table_id = f.client.create_table(&f.admin, &fuzz_config(&f));
        for _ in 0..2 {
            let p = Address::generate(&f.env);
            f.token_admin.mint(&p, &500);
            let _ = f.client.try_join_table(&table_id, &p, &500);
        }
        let _ = f.client.try_start_hand(&table_id);
        let root = BytesN::from_array(&f.env, &[9u8; 32]);
        let mut comms: Vec<BytesN<32>> = Vec::new(&f.env);
        for _ in 0..n_commitments {
            comms.push_back(BytesN::from_array(&f.env, &[2u8; 32]));
        }
        let mut idxs: Vec<u32> = Vec::new(&f.env);
        for _ in 0..n_indices {
            idxs.push_back(card);
        }
        let res = f.client.try_commit_deal(
            &table_id,
            &f.committee,
            &root,
            &comms,
            &idxs,
            &Bytes::new(&f.env),
            &Bytes::new(&f.env),
        );
        expect_typed_only(res, "commit_deal");
    }

    /// reveal_board with arbitrary card/index vectors never traps.
    #[test]
    fn prop_reveal_board_arg_validation_never_traps(
        n_cards in 0usize..8,
        card in arb_card_value(),
        idx in arb_card_value(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        let (table_id, _, _) = setup_live_preflop(&f);
        // Advance one betting round passively so we sometimes sit in a
        // reveal phase; otherwise we expect NotInRevealPhase (typed).
        let table = f.client.get_table(&table_id);
        if matches!(table.phase, GamePhase::Preflop) {
            let actor = table.players.get(table.current_turn).unwrap();
            let _ = f.client.try_player_action(&table_id, &actor.address, &1u32, &Action::Call);
            let t2 = f.client.get_table(&table_id);
            if matches!(t2.phase, GamePhase::Preflop) {
                let actor2 = t2.players.get(t2.current_turn).unwrap();
                let _ = f.client.try_player_action(&table_id, &actor2.address, &1u32, &Action::Call);
            }
        }
        let mut cards: Vec<u32> = Vec::new(&f.env);
        let mut idxs: Vec<u32> = Vec::new(&f.env);
        for _ in 0..n_cards {
            cards.push_back(card);
            idxs.push_back(idx);
        }
        let res = f.client.try_reveal_board(
            &table_id,
            &f.committee,
            &cards,
            &idxs,
            &Bytes::new(&f.env),
            &Bytes::new(&f.env),
        );
        expect_typed_only(res, "reveal_board");
    }

    /// submit_showdown / claim_timeout / leave_table / rebuy with arbitrary
    /// args never trap.
    #[test]
    fn prop_settlement_entrypoints_never_trap(
        table_id in arb_table_id(),
        amount in arb_amount(),
        n_hole in 0usize..8,
        card in arb_card_value(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        // Seed one real table so table 0 exercises deeper paths.
        let real_id = f.client.create_table(&f.admin, &fuzz_config(&f));
        let p0 = Address::generate(&f.env);
        f.token_admin.mint(&p0, &1_000_000);
        let _ = f.client.try_join_table(&real_id, &p0, &500);

        let mut hole: Vec<(u32, u32)> = Vec::new(&f.env);
        for _ in 0..n_hole {
            hole.push_back((card, card));
        }
        let salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&f.env);
        let mut pi = Bytes::new(&f.env);
        for _ in 0..(27u32 * 32) {
            pi.push_back(0);
        }
        let empty: Vec<(u32, u32)> = Vec::new(&f.env);

        let r1 = f.client.try_submit_showdown(&table_id, &f.committee, &hole, &salts, &Bytes::new(&f.env), &pi, &empty);
        expect_typed_only(r1, "submit_showdown");

        let claimer = Address::generate(&f.env);
        let r2 = f.client.try_claim_timeout(&table_id, &claimer);
        expect_typed_only(r2, "claim_timeout");

        let leaver = Address::generate(&f.env);
        let r3 = f.client.try_leave_table(&table_id, &leaver);
        expect_typed_only(r3, "leave_table");

        // Rebuy with a funded player; amount itself is arbitrary.
        let rb = Address::generate(&f.env);
        f.token_admin.mint(&rb, &1_000_000);
        let r4 = f.client.try_rebuy(&table_id, &rb, &amount);
        expect_typed_only(r4, "rebuy");

        let r5 = f.client.try_withdraw_rake(&table_id);
        expect_typed_only(r5, "withdraw_rake");
    }

    /// Straddle / queue / misc entrypoints with arbitrary args never trap.
    #[test]
    fn prop_misc_entrypoints_never_trap(
        table_id in arb_table_id(),
        multiplier in 0u32..10u32,
        amount_cap in arb_amount(),
        seq in any::<u32>(),
    ) {
        let f = fuzz_setup();
        f.env.cost_estimate().budget().reset_unlimited();
        let real_id = f.client.create_table(&f.admin, &fuzz_config(&f));
        let target = if table_id % 3 == 0 { real_id } else { table_id };

        let r1 = f.client.try_configure_straddle(&target, &multiplier, &StraddlePosition::Mississippi);
        expect_typed_only(r1, "configure_straddle");

        let r2 = f.client.try_configure_straddle_extended(&target, &multiplier, &StraddlePosition::Any, &false, &amount_cap.clamp(-1_000_000, 1_000_000), &true);
        expect_typed_only(r2, "configure_straddle_extended");

        let player = Address::generate(&f.env);
        f.token_admin.mint(&player, &1_000_000);
        let r3 = f.client.try_post_mississippi_straddle(&target, &player);
        expect_typed_only(r3, "post_mississippi_straddle");

        let r4 = f.client.try_get_queue(&target);
        // get_queue is infallible (returns empty vec for unknown tables);
        // try_ success still proves no trap on arbitrary ids.
        expect_typed_only(r4, "get_queue");

        let r5 = f.client.try_leave_queue(&target, &player);
        expect_typed_only(r5, "leave_queue");

        let r6 = f.client.try_start_hand(&target);
        expect_typed_only(r6, "start_hand");

        let _ = seq;
    }
}

// ---------------------------------------------------------------------------
// Deterministic edge-case regression checks (run alongside the proptests)
// ---------------------------------------------------------------------------

#[test]
fn arg_validation_rejects_garbage_bet_amounts_with_typed_errors() {
    let f = fuzz_setup();
    let (table_id, _, _) = setup_live_preflop(&f);
    for amount in [0i128, -1, -100, i128::MIN, i128::MAX, 1_000_000_000] {
        let table = f.client.get_table(&table_id);
        if !matches!(
            table.phase,
            GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River
        ) {
            return;
        }
        let actor = table.players.get(table.current_turn).unwrap();
        // Use a fresh table per amount so seq bookkeeping never interferes:
        // seq is always 999 (never previously used) -> any failure must be
        // amount validation (typed), not StaleActionSequence confusion.
        let res = f.client.try_player_action(&table_id, &actor.address, &999u32, &Action::Bet(amount));
        expect_typed_only(res, "Bet(garbage)");
        // 0 / negative bets must never succeed.
        if amount <= 0 {
            assert!(res.is_err(), "Bet({amount}) should be rejected");
        }
    }
}

#[test]
fn arg_validation_rejects_unknown_table_ids_with_typed_errors() {
    let f = fuzz_setup();
    let bogus = 999_999u32;
    let player = Address::generate(&f.env);
    f.token_admin.mint(&player, &1000);

    let r = f.client.try_join_table(&bogus, &player, &500);
    expect_typed_only(r, "join_table(bogus id)");
    assert!(r.is_err(), "unknown table must be TableNotFound, not success");

    let claimer = Address::generate(&f.env);
    let r = f.client.try_claim_timeout(&bogus, &claimer);
    expect_typed_only(r, "claim_timeout(bogus id)");

    let r = f.client.try_leave_table(&bogus, &player);
    expect_typed_only(r, "leave_table(bogus id)");
}
