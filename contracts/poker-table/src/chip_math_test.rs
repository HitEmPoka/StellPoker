//! Integer overflow / underflow property tests for chip math (issue #561).
//!
//! Chips are `i128` on-chain, and the release profile sets
//! `overflow-checks = true`, so an overflow does not silently wrap — it aborts
//! the transaction. That is still a bug when the inputs are legal: a pot near
//! `i128::MAX` must be raked, split and paid out (or cleanly rejected with a
//! `PokerTableError`), never trap the contract mid-settlement.
//!
//! These tests drive pot / rake / bet math at the magnitude boundaries — zero,
//! one, one-unit remainders, and values at and just below `i128::MAX` — and
//! assert:
//!
//!   * no arithmetic overflow (a panic fails the test),
//!   * conservation: `net + rake == gross`, payouts sum to the pot, and stacks
//!     plus rake equal the chips that entered,
//!   * rake is exactly `floor(amount * bps / 10_000)` and never exceeds the pot,
//!   * remainders ("odd chips") are never lost when a pot is split,
//!   * illegal oversized bets are rejected with an error rather than trapping.

#![cfg(test)]

extern crate std;

use crate::betting;
use crate::pot::{
    apply_rake, calculate_side_pots, distribute_pots_with_ties, split_jackpot_rake, MAX_RAKE_BPS,
};
use crate::types::*;
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Vec};
use std::format;
use std::vec::Vec as StdVec;

const MAX: i128 = i128::MAX;
const BPS_DENOMINATOR: i128 = 10_000;

/// Magnitudes that sit on a boundary of the rake / split arithmetic: zero, one,
/// remainders around the 10_000 denominator, and the values around the largest
/// pot whose naive `amount * bps` product still fits (and the first that
/// doesn't) for every legal rake and jackpot share.
fn boundary_amounts() -> StdVec<i128> {
    std::vec![
        0,
        1,
        2,
        9_999,
        10_000,
        10_001,
        19_999,
        MAX / BPS_DENOMINATOR - 1,
        MAX / BPS_DENOMINATOR,
        MAX / BPS_DENOMINATOR + 1,
        MAX / MAX_RAKE_BPS as i128 - 1,
        MAX / MAX_RAKE_BPS as i128,
        MAX / MAX_RAKE_BPS as i128 + 1,
        MAX / 2,
        MAX - 1,
        MAX,
    ]
}

/// Independent reference for `floor(amount * bps / 10_000)` (both operands
/// non-negative) that cannot overflow: split the amount into quotient and
/// remainder of the denominator first.
fn floor_bps(amount: i128, bps: u32) -> i128 {
    let bps = bps as i128;
    (amount / BPS_DENOMINATOR) * bps + (amount % BPS_DENOMINATOR) * bps / BPS_DENOMINATOR
}

fn pot_of(env: &Env, amount: i128) -> Vec<SidePot> {
    let mut pots = Vec::new(env);
    let mut eligible = Vec::new(env);
    eligible.push_back(0u32);
    pots.push_back(SidePot {
        amount,
        eligible_players: eligible,
    });
    pots
}

// ---------------------------------------------------------------------------
// Rake and jackpot split: deterministic boundary sweep
// ---------------------------------------------------------------------------

#[test]
fn apply_rake_never_overflows_at_boundaries() {
    let env = Env::default();
    for amount in boundary_amounts() {
        for bps in [0u32, 1, 250, MAX_RAKE_BPS - 1, MAX_RAKE_BPS] {
            let (net, rake) = apply_rake(&env, &pot_of(&env, amount), bps).unwrap();
            let net_amount = net.get(0).unwrap().amount;

            assert_eq!(rake, floor_bps(amount, bps), "amount={amount} bps={bps}");
            assert_eq!(net_amount + rake, amount, "amount={amount} bps={bps}");
            assert!(rake >= 0 && rake <= amount);
            assert!(net_amount >= 0);
        }
    }
}

#[test]
fn apply_rake_matches_naive_formula_where_that_does_not_overflow() {
    let env = Env::default();
    for amount in boundary_amounts() {
        for bps in [0u32, 1, 250, MAX_RAKE_BPS] {
            if let Some(product) = amount.checked_mul(bps as i128) {
                let (_, rake) = apply_rake(&env, &pot_of(&env, amount), bps).unwrap();
                assert_eq!(rake, product / BPS_DENOMINATOR);
            }
        }
    }
}

#[test]
fn apply_rake_one_unit_remainders_round_down_to_the_house_favouring_floor() {
    let env = Env::default();
    // 1 chip at 5%: floor(0.05) = 0, so the pot is untouched.
    let (net, rake) = apply_rake(&env, &pot_of(&env, 1), MAX_RAKE_BPS).unwrap();
    assert_eq!((net.get(0).unwrap().amount, rake), (1, 0));
    // 19_999 chips at 5% = 999.95 -> 999.
    let (net, rake) = apply_rake(&env, &pot_of(&env, 19_999), MAX_RAKE_BPS).unwrap();
    assert_eq!((net.get(0).unwrap().amount, rake), (19_000, 999));
    // Zero pot stays zero.
    let (net, rake) = apply_rake(&env, &pot_of(&env, 0), MAX_RAKE_BPS).unwrap();
    assert_eq!((net.get(0).unwrap().amount, rake), (0, 0));
}

#[test]
fn split_jackpot_rake_never_overflows_and_conserves() {
    for total in boundary_amounts() {
        for share_bps in [0u32, 1, 1_000, 5_000, 9_999, 10_000] {
            let (house, jackpot) = split_jackpot_rake(total, share_bps);

            assert_eq!(house + jackpot, total, "total={total} share={share_bps}");
            assert!(house >= 0 && jackpot >= 0);
            assert!(jackpot <= total);
            assert_eq!(jackpot, floor_bps(total, share_bps));
        }
    }
}

#[test]
fn split_jackpot_rake_zero_share_gives_everything_to_the_house() {
    assert_eq!(split_jackpot_rake(MAX, 0), (MAX, 0));
    assert_eq!(split_jackpot_rake(0, 5_000), (0, 0));
    assert_eq!(split_jackpot_rake(1, 5_000), (1, 0));
}

// ---------------------------------------------------------------------------
// Rake and jackpot split: generated magnitudes
// ---------------------------------------------------------------------------

/// Non-negative magnitudes biased towards the interesting edges of `0..=max`.
fn magnitude(max: i128) -> impl Strategy<Value = i128> {
    prop_oneof![
        Just(0i128),
        Just(1i128),
        Just(2i128),
        Just(max),
        Just(max - 1),
        Just(max / 2),
        0..=max,
        (max.saturating_sub(1_000))..=max,
        0..=20_000i128,
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Rake on a single pot is exact, bounded by the pot and conserves chips at
    /// every magnitude up to `i128::MAX`.
    #[test]
    fn prop_apply_rake_exact_and_conserving(
        amount in magnitude(MAX),
        bps in 0u32..=MAX_RAKE_BPS,
    ) {
        let env = Env::default();
        let (net, rake) = apply_rake(&env, &pot_of(&env, amount), bps).unwrap();
        let net_amount = net.get(0).unwrap().amount;

        prop_assert_eq!(rake, floor_bps(amount, bps));
        prop_assert_eq!(net_amount + rake, amount);
        prop_assert!(rake >= 0 && rake <= amount);
        prop_assert!(net_amount >= 0);
        if let Some(product) = amount.checked_mul(bps as i128) {
            prop_assert_eq!(rake, product / BPS_DENOMINATOR);
        }
    }

    /// A bigger pot never pays less rake than a smaller one at the same rate.
    #[test]
    fn prop_rake_is_monotonic_in_pot_size(
        a in magnitude(MAX),
        b in magnitude(MAX),
        bps in 0u32..=MAX_RAKE_BPS,
    ) {
        let env = Env::default();
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (_, rake_lo) = apply_rake(&env, &pot_of(&env, lo), bps).unwrap();
        let (_, rake_hi) = apply_rake(&env, &pot_of(&env, hi), bps).unwrap();
        prop_assert!(rake_lo <= rake_hi);
    }

    /// The house/jackpot split always sums back to the rake it was given.
    #[test]
    fn prop_split_jackpot_rake_conserves(
        total in magnitude(MAX),
        share_bps in 0u32..=10_000u32,
    ) {
        let (house, jackpot) = split_jackpot_rake(total, share_bps);
        prop_assert_eq!(house + jackpot, total);
        prop_assert!(house >= 0 && jackpot >= 0);
        prop_assert_eq!(jackpot, floor_bps(total, share_bps));
    }
}

// ---------------------------------------------------------------------------
// Side pots, rake and split payouts at large magnitudes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct GenPlayer {
    committed: i128,
    folded: bool,
    all_in: bool,
}

/// 2–6 players whose combined commitment always fits in an `i128` (each is at
/// most `MAX / 6`), with at least two contenders.
fn players_strategy() -> impl Strategy<Value = StdVec<GenPlayer>> {
    prop::collection::vec(
        (magnitude(MAX / 6), any::<bool>(), any::<bool>()).prop_map(
            |(committed, folded, all_in)| GenPlayer {
                committed,
                folded,
                all_in,
            },
        ),
        2..=6,
    )
    .prop_map(|mut players| {
        let contenders = players.iter().filter(|p| !p.folded).count();
        if contenders < 2 {
            for p in players.iter_mut().take(2) {
                p.folded = false;
            }
        }
        // Anyone still in at showdown has put chips in (at minimum a blind or a
        // call); only a folded player can have committed nothing.
        for p in players.iter_mut().filter(|p| !p.folded) {
            p.committed = p.committed.max(1);
        }
        players
    })
}

fn table_config(admin: &Address, rake_bps: u32) -> TableConfig {
    TableConfig {
        token: admin.clone(),
        min_buy_in: 0,
        max_buy_in: MAX,
        betting_structure: BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&admin.env(), 0, 0),
        min_players: 2,
        max_players: 9,
        timeout_ledgers: 0,
        committee: admin.clone(),
        verifier: admin.clone(),
        game_hub: admin.clone(),
        rake_bps,
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

/// Build a showdown-phase table. Every player starts with an empty stack so the
/// stacks after payout equal exactly what was paid out.
fn showdown_table(env: &Env, gen: &[GenPlayer]) -> (TableState, i128) {
    let mut players: Vec<PlayerState> = Vec::new(env);
    let mut total: i128 = 0;
    for (seat, g) in gen.iter().enumerate() {
        total += g.committed;
        players.push_back(PlayerState {
            address: Address::generate(env),
            stack: 0,
            bet_this_round: 0,
            committed: g.committed,
            folded: g.folded,
            all_in: g.all_in,
            sitting_out: false,
            seat_index: seat as u32,
            total_buy_in: 0,
            rebuy_count: 0,
        });
    }
    let admin = Address::generate(env);
    let table = TableState {
        id: 0,
        admin: admin.clone(),
        config: table_config(&admin, 0),
        phase: GamePhase::Showdown,
        players,
        dealer_seat: 0,
        current_turn: 0,
        pot: total,
        side_pots: Vec::new(env),
        deck_root: BytesN::from_array(env, &[0u8; 32]),
        hand_commitments: Vec::new(env),
        board_cards: Vec::new(env),
        dealt_indices: Vec::new(env),
        hand_number: 1,
        last_action_ledger: 0,
        committee: admin,
        session_id: 0,
        rake_balance: 0,
        action_deadline: 0,
        hand_actions: Vec::new(env),
        rit_state: OptionalRitState::None,
        jackpot_balance: 0,
        last_raise_size: 0,
        current_blind_level: 0,
        level_started_at: 0,
        break_ends_at: 0,
        settlement_entered_ledger: 0,
    };
    (table, total)
}

fn sum_pots(pots: &Vec<SidePot>) -> i128 {
    let mut total = 0i128;
    for i in 0..pots.len() {
        total += pots.get(i).unwrap().amount;
    }
    total
}

fn sum_stacks(table: &TableState) -> i128 {
    let mut total = 0i128;
    for i in 0..table.players.len() {
        total += table.players.get(i).unwrap().stack;
    }
    total
}

/// Seats of the non-folded players, in seat order.
fn contenders(table: &TableState) -> StdVec<u32> {
    (0..table.players.len())
        .filter(|i| !table.players.get(*i).unwrap().folded)
        .collect()
}

fn to_vec(env: &Env, seats: &[u32]) -> Vec<u32> {
    let mut v = Vec::new(env);
    for s in seats {
        v.push_back(*s);
    }
    v
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// From the moment chips are committed to the moment they are paid out —
    /// through side-pot construction, per-pot rake, and a split among tied
    /// winners — not one chip is created or destroyed, at any magnitude.
    #[test]
    fn prop_side_pot_rake_and_split_conserve_chips_at_any_magnitude(
        gen in players_strategy(),
        rake_bps in 0u32..=MAX_RAKE_BPS,
        tie_mask in 1u32..64,
    ) {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        let (mut table, total) = showdown_table(&env, &gen);

        let pots = calculate_side_pots(&env, &table).unwrap();
        prop_assert_eq!(sum_pots(&pots), total);

        let (net_pots, rake) = apply_rake(&env, &pots, rake_bps).unwrap();
        prop_assert!(rake >= 0 && rake <= total);
        prop_assert_eq!(sum_pots(&net_pots) + rake, total);

        // Any non-empty subset of the contenders "ties"; the rest are the
        // fallback ranking for pots the tied group is not eligible for.
        let all = contenders(&table);
        let tied: StdVec<u32> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| tie_mask & (1 << i) != 0)
            .map(|(_, s)| *s)
            .collect();
        let tied = if tied.is_empty() { std::vec![all[0]] } else { tied };

        let payouts = distribute_pots_with_ties(
            &env,
            &mut table,
            &net_pots,
            &to_vec(&env, &tied),
            &to_vec(&env, &all),
        )
        .unwrap();

        let mut paid = 0i128;
        for i in 0..payouts.len() {
            let amount = payouts.get(i).unwrap().1;
            prop_assert!(amount >= 0);
            paid += amount;
        }
        prop_assert_eq!(paid, sum_pots(&net_pots));
        prop_assert_eq!(sum_stacks(&table) + rake, total);
    }
}

/// One-unit remainders: a pot that doesn't divide evenly between the tied
/// winners hands the odd chips out one at a time to the earliest seats, so the
/// split always sums back to the pot and no share differs by more than one.
#[test]
fn tied_split_hands_out_every_odd_chip() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();

    for winners in 2..=6usize {
        for extra in 0..winners as i128 {
            for base in [0i128, 1, MAX / 6 - 1_000] {
                let pot_amount = base * winners as i128 + extra;
                let gen: StdVec<GenPlayer> = (0..winners)
                    .map(|_| GenPlayer {
                        committed: 0,
                        folded: false,
                        all_in: false,
                    })
                    .collect();
                let (mut table, _) = showdown_table(&env, &gen);
                let seats: StdVec<u32> = (0..winners as u32).collect();
                let mut eligible = Vec::new(&env);
                for s in &seats {
                    eligible.push_back(*s);
                }
                let mut pots = Vec::new(&env);
                pots.push_back(SidePot {
                    amount: pot_amount,
                    eligible_players: eligible,
                });

                distribute_pots_with_ties(
                    &env,
                    &mut table,
                    &pots,
                    &to_vec(&env, &seats),
                    &to_vec(&env, &seats),
                )
                .unwrap();

                let stacks: StdVec<i128> = (0..winners as u32)
                    .map(|s| table.players.get(s).unwrap().stack)
                    .collect();
                assert_eq!(stacks.iter().sum::<i128>(), pot_amount);
                assert!(stacks.iter().max().unwrap() - stacks.iter().min().unwrap() <= 1);
                // Odd chips go to the earliest seats, never the latest.
                assert!(stacks.windows(2).all(|w| w[0] >= w[1]));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Betting: oversized amounts are rejected, not trapped
// ---------------------------------------------------------------------------

/// A pre-flop heads-up table with `stack` chips behind for both players, the
/// small blind (seat 0) to act facing the big blind.
fn preflop_table(env: &Env, stack: i128, betting_structure: BettingStructure) -> (TableState, Address) {
    let (mut table, _) = showdown_table(
        env,
        &[
            GenPlayer { committed: 10, folded: false, all_in: false },
            GenPlayer { committed: 20, folded: false, all_in: false },
        ],
    );
    table.phase = GamePhase::Preflop;
    table.config.betting_structure = betting_structure;
    table.config.blinds_schedule = BlindsSchedule::fixed(env, 10, 20);
    table.current_turn = 0;
    table.last_raise_size = 20;
    for (seat, bet) in [(0u32, 10i128), (1, 20)] {
        let mut p = table.players.get(seat).unwrap();
        p.stack = stack;
        p.bet_this_round = bet;
        table.players.set(seat, p);
    }
    let actor = table.players.get(0).unwrap().address;
    (table, actor)
}

fn act(
    env: &Env,
    table: &mut TableState,
    actor: &Address,
    action: Action,
) -> Result<(), PokerTableError> {
    env.mock_all_auths();
    let contract = env.register(crate::PokerTableContract, ());
    env.as_contract(&contract, || betting::process_action(env, table, actor, &action))
}

#[test]
fn oversized_raise_is_rejected_not_overflowed() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();

    for raise in [MAX, MAX - 1, MAX - 10, MAX - 9] {
        let (mut table, actor) = preflop_table(&env, 1_000, BettingStructure::NoLimit);
        // to_call is 10, so `to_call + raise` exceeds i128::MAX for these.
        assert_eq!(
            act(&env, &mut table, &actor, Action::Raise(raise)),
            Err(PokerTableError::NotEnoughChips),
            "raise={raise}"
        );
        // A rejected action leaves the pot and stacks untouched.
        assert_eq!(table.pot, 30);
        assert_eq!(table.players.get(0).unwrap().stack, 1_000);
    }
}

#[test]
fn oversized_bet_is_rejected_not_overflowed() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();

    let (mut table, actor) = preflop_table(&env, 1_000, BettingStructure::NoLimit);
    // Seat 0 faces a bet, so a fresh `Bet` is illegal regardless of size…
    assert_eq!(
        act(&env, &mut table, &actor, Action::Bet(MAX)),
        Err(PokerTableError::CannotBetWhenOutstandingBet)
    );
    // …and with no outstanding bet, an oversized one is a chip-count error.
    let (mut table, actor) = preflop_table(&env, 1_000, BettingStructure::NoLimit);
    for i in 0..2 {
        let mut p = table.players.get(i).unwrap();
        p.bet_this_round = 0;
        table.players.set(i, p);
    }
    assert_eq!(
        act(&env, &mut table, &actor, Action::Bet(MAX)),
        Err(PokerTableError::NotEnoughChips)
    );
}

#[test]
fn pot_limit_raise_math_survives_a_near_max_pot() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();

    // Chips in play (pot + both stacks) stay within i128, as token supply guarantees.
    let (mut table, actor) = preflop_table(&env, MAX / 8, BettingStructure::PotLimit);
    table.pot = MAX / 2;
    // A raise above `pot + to_call` is refused; one at the limit goes through.
    assert_eq!(
        act(&env, &mut table, &actor, Action::Raise(MAX / 2 + 11)),
        Err(PokerTableError::NotEnoughChips)
    );
    let (mut table, actor) = preflop_table(&env, MAX / 8, BettingStructure::PotLimit);
    table.pot = MAX / 2;
    assert_eq!(act(&env, &mut table, &actor, Action::Raise(MAX / 8 - 10)), Ok(()));
    assert_eq!(table.players.get(0).unwrap().stack, 0);
}

#[test]
fn all_in_and_call_at_max_stack_conserve_chips() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();

    let stack = MAX / 4;
    let (mut table, actor) = preflop_table(&env, stack, BettingStructure::NoLimit);
    let before = table.pot + table.players.get(0).unwrap().stack + table.players.get(1).unwrap().stack;

    act(&env, &mut table, &actor, Action::AllIn).unwrap();

    let p0 = table.players.get(0).unwrap();
    assert_eq!(p0.stack, 0);
    assert!(p0.all_in);
    let after = table.pot + p0.stack + table.players.get(1).unwrap().stack;
    assert_eq!(after, before);
}
