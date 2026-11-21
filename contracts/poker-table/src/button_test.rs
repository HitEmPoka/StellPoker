//! Deterministic seat-rotation and button-tracking tests (issue #558).
//!
//! `invariants_test` / `state_machine_test` prove action order *within* a
//! hand. This module covers the contract-side button logic *between* hands:
//!
//!   * for every table size from 2 (heads-up) to 6 seats and **every**
//!     combination of active / sitting-out / busted seats and **every**
//!     starting button position, `start_new_hand` moves the button to the
//!     nearest active seat clockwise, never skips an active player, and posts
//!     the blinds from the right seats (button is the small blind heads-up);
//!   * over consecutive hands the button visits every active seat exactly once
//!     per orbit, in seat order;
//!   * a table with fewer than two active players refuses to start a hand;
//!   * property tests interleave hands with sit-outs and busts;
//!   * contract-level tests drive the public `join_table` / `start_hand` /
//!     `player_action` / `leave_table` entrypoints, including a player leaving
//!     between hands (which renumbers seats) at every table size and button
//!     position, and the 2-max table shrinking to one player.

#![cfg(test)]

extern crate std;

use crate::game::start_new_hand;
use crate::state_machine_test::GameHubContract;
use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, BytesN, Env, Vec};
use std::cell::RefCell;
use std::format;
use std::vec::Vec as StdVec;

const SMALL_BLIND: i128 = 5;
const BIG_BLIND: i128 = 10;
const STACK: i128 = 1_000;

/// What a seat is doing when the next hand is about to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seat {
    /// Has chips and is dealt in.
    Active,
    /// Has chips but is sitting out.
    SitOut,
    /// Dealt in, but out of chips.
    Busted,
}

impl Seat {
    fn is_active(self) -> bool {
        self == Seat::Active
    }
}

/// Every assignment of `Active` / `SitOut` / `Busted` to `n` seats.
fn all_seat_states(n: usize) -> StdVec<StdVec<Seat>> {
    let mut all: StdVec<StdVec<Seat>> = std::vec![std::vec![]];
    for _ in 0..n {
        all = all
            .into_iter()
            .flat_map(|prefix| {
                [Seat::Active, Seat::SitOut, Seat::Busted].map(|s| {
                    let mut next = prefix.clone();
                    next.push(s);
                    next
                })
            })
            .collect();
    }
    all
}

/// Independent reference: the nearest seat clockwise from `from` (exclusive)
/// that is active, scanning the ring once. Written as a plain loop over the
/// model so it does not share code with `game::next_active_seat`.
fn nearest_active_after(states: &[Seat], from: usize) -> Option<usize> {
    let n = states.len();
    (1..=n)
        .map(|step| (from + step) % n)
        .find(|s| states[*s].is_active())
}

fn table_config(admin: &Address) -> TableConfig {
    TableConfig {
        token: admin.clone(),
        min_buy_in: 0,
        max_buy_in: i128::MAX,
        betting_structure: BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&admin.env(), SMALL_BLIND, BIG_BLIND),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 0,
        committee: admin.clone(),
        verifier: admin.clone(),
        game_hub: admin.clone(),
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

/// A `Waiting`-phase table with the given seat states and button position.
fn build_table(env: &Env, states: &[Seat], dealer: u32) -> TableState {
    let mut players: Vec<PlayerState> = Vec::new(env);
    for (seat, state) in states.iter().enumerate() {
        players.push_back(PlayerState {
            address: Address::generate(env),
            stack: if *state == Seat::Busted { 0 } else { STACK },
            bet_this_round: 0,
            committed: 0,
            folded: false,
            all_in: false,
            sitting_out: *state == Seat::SitOut,
            seat_index: seat as u32,
            total_buy_in: 0,
            rebuy_count: 0,
        });
    }
    let admin = Address::generate(env);
    TableState {
        id: 0,
        admin: admin.clone(),
        config: table_config(&admin),
        phase: GamePhase::Waiting,
        players,
        dealer_seat: dealer,
        current_turn: 0,
        pot: 0,
        side_pots: Vec::new(env),
        deck_root: BytesN::from_array(env, &[0u8; 32]),
        hand_commitments: Vec::new(env),
        board_cards: Vec::new(env),
        dealt_indices: Vec::new(env),
        hand_number: 0,
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
    }
}

/// Run `start_new_hand` inside the contract's frame (it touches storage and
/// events), exactly as `start_hand` does on-chain.
fn start(env: &Env, contract: &Address, table: &mut TableState) -> Result<(), PokerTableError> {
    env.as_contract(contract, || start_new_hand(env, table))
}

/// Put the table back into the "between hands" shape: chips returned, no
/// bets, empty pot — but keep the button, hand counter and seat flags.
fn reset_for_next_hand(table: &mut TableState, states: &[Seat]) {
    for (seat, state) in states.iter().enumerate() {
        let mut p = table.players.get(seat as u32).unwrap();
        p.stack = if *state == Seat::Busted { 0 } else { STACK };
        p.bet_this_round = 0;
        p.committed = 0;
        p.all_in = false;
        table.players.set(seat as u32, p);
    }
    table.pot = 0;
    table.phase = GamePhase::Waiting;
}

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let contract = env.register(PokerTableContract, ());
    (env, contract)
}

// ---------------------------------------------------------------------------
// Exhaustive: one hand, every table size / seat-state mix / button position
// ---------------------------------------------------------------------------

#[test]
fn button_moves_to_nearest_active_seat_for_every_layout() {
    let (env, contract) = setup();

    for n in 2..=6usize {
        for states in all_seat_states(n) {
            let active = states.iter().filter(|s| s.is_active()).count();
            for dealer in 0..n {
                let mut table = build_table(&env, &states, dealer as u32);
                let result = start(&env, &contract, &mut table);
                let ctx = std::format!("n={n} states={states:?} dealer={dealer}");

                if active < 2 {
                    assert_eq!(result, Err(PokerTableError::NotEnoughPlayers), "{ctx}");
                    continue;
                }
                result.unwrap_or_else(|e| panic!("{ctx}: {e:?}"));

                // The button lands on the nearest active seat clockwise…
                let expected = nearest_active_after(&states, dealer).unwrap();
                assert_eq!(table.dealer_seat as usize, expected, "{ctx}");
                // …which is always an active seat, and never the old button
                // (with two or more active seats there is always somewhere to go).
                assert!(states[expected].is_active(), "{ctx}");
                assert_ne!(expected, dealer, "{ctx}");
                // No active seat was jumped over on the way.
                let mut seat = (dealer + 1) % n;
                while seat != expected {
                    assert!(
                        !states[seat].is_active(),
                        "skipped active seat {seat}: {ctx}"
                    );
                    seat = (seat + 1) % n;
                }
                assert_eq!(table.hand_number, 1, "{ctx}");
                assert_eq!(table.phase, GamePhase::Dealing, "{ctx}");
            }
        }
    }
}

#[test]
fn blinds_follow_the_button_for_every_layout() {
    let (env, contract) = setup();

    for n in 2..=6usize {
        for states in all_seat_states(n) {
            let active: StdVec<usize> = (0..n).filter(|s| states[*s].is_active()).collect();
            if active.len() < 2 {
                continue;
            }
            for dealer in 0..n {
                let mut table = build_table(&env, &states, dealer as u32);
                start(&env, &contract, &mut table).unwrap();
                let button = table.dealer_seat as usize;
                let ctx = std::format!("n={n} states={states:?} dealer={dealer}");

                let (sb, bb) = if active.len() == 2 {
                    // Heads-up: the button posts the small blind.
                    (button, nearest_active_after(&states, button).unwrap())
                } else {
                    let sb = nearest_active_after(&states, button).unwrap();
                    (sb, nearest_active_after(&states, sb).unwrap())
                };

                for seat in 0..n {
                    let p = table.players.get(seat as u32).unwrap();
                    let expected_bet = if seat == sb {
                        SMALL_BLIND
                    } else if seat == bb {
                        BIG_BLIND
                    } else {
                        0
                    };
                    assert_eq!(p.bet_this_round, expected_bet, "seat {seat}: {ctx}");
                    assert_eq!(
                        p.stack + p.bet_this_round,
                        if states[seat] == Seat::Busted {
                            0
                        } else {
                            STACK
                        }
                    );
                }
                assert_eq!(table.pot, SMALL_BLIND + BIG_BLIND, "{ctx}");
                assert_ne!(sb, bb, "{ctx}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Orbit: consecutive hands with a stable set of players
// ---------------------------------------------------------------------------

#[test]
fn button_visits_every_active_seat_once_per_orbit() {
    let (env, contract) = setup();

    for n in 2..=6usize {
        for states in all_seat_states(n) {
            let active: StdVec<usize> = (0..n).filter(|s| states[*s].is_active()).collect();
            if active.len() < 2 {
                continue;
            }
            // First, middle and last button positions cover the wrap-around.
            for first_dealer in [0, n / 2, n - 1] {
                let mut table = build_table(&env, &states, first_dealer as u32);
                let mut visited: StdVec<usize> = std::vec![];
                for _ in 0..active.len() {
                    start(&env, &contract, &mut table).unwrap();
                    visited.push(table.dealer_seat as usize);
                    reset_for_next_hand(&mut table, &states);
                }
                let ctx = std::format!("n={n} states={states:?} start={first_dealer}");

                // Each active seat held the button exactly once…
                let mut sorted = visited.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, active, "{ctx}");
                // …in clockwise order…
                for pair in visited.windows(2) {
                    assert_eq!(
                        pair[1],
                        nearest_active_after(&states, pair[0]).unwrap(),
                        "{ctx}"
                    );
                }
                // …and the next hand starts a new orbit from the same place.
                start(&env, &contract, &mut table).unwrap();
                assert_eq!(table.dealer_seat as usize, visited[0], "{ctx}");
                assert_eq!(table.hand_number as usize, active.len() + 1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property: interleaved sit-outs and busts between hands
// ---------------------------------------------------------------------------

fn seat_strategy() -> impl Strategy<Value = Seat> {
    prop_oneof![
        6 => Just(Seat::Active),
        2 => Just(Seat::SitOut),
        1 => Just(Seat::Busted),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Across a random sequence of hands where players sit out, sit back in and
    /// bust between hands, the button always lands on the nearest active seat
    /// clockwise of the previous button, and a hand only refuses to start when
    /// fewer than two seats are active.
    #[test]
    fn prop_button_tracks_changing_seat_states(
        n in 2usize..=6,
        start_dealer in 0usize..6,
        rounds in prop::collection::vec(prop::collection::vec(seat_strategy(), 6), 1..12),
    ) {
        let (env, contract) = setup();
        let dealer0 = start_dealer % n;
        let first: StdVec<Seat> = rounds[0][..n].to_vec();
        let mut table = build_table(&env, &first, dealer0 as u32);

        for round in &rounds {
            let states: StdVec<Seat> = round[..n].to_vec();
            // Apply this hand's seat changes.
            for (seat, state) in states.iter().enumerate() {
                let mut p = table.players.get(seat as u32).unwrap();
                p.sitting_out = *state == Seat::SitOut;
                table.players.set(seat as u32, p);
            }
            reset_for_next_hand(&mut table, &states);

            let previous = table.dealer_seat as usize;
            let active = states.iter().filter(|s| s.is_active()).count();
            let result = start(&env, &contract, &mut table);

            if active < 2 {
                prop_assert_eq!(result, Err(PokerTableError::NotEnoughPlayers));
                // A refused start must not have moved the button off the ring.
                prop_assert!((table.dealer_seat as usize) < n);
                // Re-seat the button where it was so the model stays in sync.
                table.dealer_seat = previous as u32;
                continue;
            }
            prop_assert!(result.is_ok());
            let expected = nearest_active_after(&states, previous).unwrap();
            prop_assert_eq!(table.dealer_seat as usize, expected);
            prop_assert!(states[expected].is_active());
        }
    }
}

// ---------------------------------------------------------------------------
// Contract level: real join / start / play / leave
// ---------------------------------------------------------------------------

struct Fixture<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    token_admin: StellarAssetClient<'a>,
    committee: Address,
    table_id: u32,
    /// Last action sequence number used by each player (`player_action` replay guard).
    seqs: RefCell<StdVec<(Address, u32)>>,
}

impl<'a> Fixture<'a> {
    fn new() -> Fixture<'static> {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();

        let contract = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract);

        let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
        let token_admin = StellarAssetClient::new(&env, &sac.address());

        let admin = Address::generate(&env);
        let committee = Address::generate(&env);
        let mut config = table_config(&admin);
        config.token = sac.address();
        config.min_buy_in = 100;
        config.max_buy_in = STACK;
        config.committee = committee.clone();
        config.verifier = env.register(crate::verifier::ZkVerifierContract, ());
        config.game_hub = env.register(GameHubContract, ());
        let table_id = client.create_table(&admin, &config);

        Fixture {
            env,
            client,
            token_admin,
            committee,
            table_id,
            seqs: RefCell::new(std::vec![]),
        }
    }

    fn join(&self) -> Address {
        let player = Address::generate(&self.env);
        self.token_admin.mint(&player, &STACK);
        self.client.join_table(&self.table_id, &player, &STACK);
        player
    }

    /// Start a hand: the button moves and the blinds are posted.
    fn begin_hand(&self) {
        self.client.start_hand(&self.table_id);
    }

    /// Finish the hand that `begin_hand` started: commit a mock deal, then have
    /// each player fold in turn until one is left and the pot is awarded. Stacks
    /// survive (unlike `cancel_hand`, which cashes everyone out), so the table
    /// ends up genuinely between hands, in `Settlement`.
    fn finish_hand(&self) {
        let players = self.table().players.len();
        let mut commitments: Vec<BytesN<32>> = Vec::new(&self.env);
        let mut dealt: Vec<u32> = Vec::new(&self.env);
        for i in 0..players {
            commitments.push_back(BytesN::from_array(&self.env, &[2u8; 32]));
            dealt.push_back(2 * i);
            dealt.push_back(2 * i + 1);
        }
        let empty = soroban_sdk::Bytes::new(&self.env);
        self.client.commit_deal(
            &self.table_id,
            &self.committee,
            &BytesN::from_array(&self.env, &[1u8; 32]),
            &commitments,
            &dealt,
            &empty,
            &empty,
        );
        loop {
            let table = self.table();
            if table.phase == GamePhase::Settlement {
                break;
            }
            let actor = table.players.get(table.current_turn).unwrap().address;
            let seq = self.next_seq(&actor);
            self.client
                .player_action(&self.table_id, &actor, &seq, &Action::Fold);
        }
    }

    fn next_seq(&self, who: &Address) -> u32 {
        let mut seqs = self.seqs.borrow_mut();
        if let Some(entry) = seqs.iter_mut().find(|(a, _)| a == who) {
            entry.1 += 1;
            return entry.1;
        }
        seqs.push((who.clone(), 1));
        1
    }

    /// A whole hand, start to settlement.
    fn play_hand(&self) {
        self.begin_hand();
        self.finish_hand();
    }

    fn table(&self) -> TableState {
        self.client.get_table(&self.table_id)
    }

    fn dealer(&self) -> u32 {
        self.table().dealer_seat
    }
}

fn seat_of(table: &TableState, who: &Address) -> Option<u32> {
    (0..table.players.len()).find(|i| table.players.get(*i).unwrap().address == *who)
}

#[test]
fn contract_button_advances_one_seat_per_hand_for_every_table_size() {
    for n in 2..=6u32 {
        let f = Fixture::new();
        for _ in 0..n {
            f.join();
        }
        assert_eq!(f.dealer(), 0, "button starts at seat 0 (n={n})");

        // Two full orbits: 0 -> 1 -> ... -> n-1 -> 0 -> ...
        for hand in 1..=(2 * n) {
            f.play_hand();
            assert_eq!(f.dealer(), hand % n, "n={n} hand={hand}");
        }
        assert_eq!(f.table().hand_number, 2 * n);
    }
}

#[test]
fn contract_heads_up_button_posts_small_blind_and_alternates() {
    let f = Fixture::new();
    let a = f.join();
    let b = f.join();

    let mut small_blinds: StdVec<Address> = std::vec![];
    for _ in 0..4 {
        f.begin_hand();
        let table = f.table();
        let button = table.players.get(table.dealer_seat).unwrap();
        assert_eq!(
            button.bet_this_round, SMALL_BLIND,
            "button is the SB heads-up"
        );
        let other = table.players.get((table.dealer_seat + 1) % 2).unwrap();
        assert_eq!(other.bet_this_round, BIG_BLIND);
        small_blinds.push(button.address);
        f.finish_hand();
    }
    // Heads-up the button strictly alternates between the two players.
    assert_eq!(small_blinds[0], b);
    assert_eq!(small_blinds[1], a);
    assert_eq!(small_blinds[2], b);
    assert_eq!(small_blinds[3], a);
}

/// A player leaving between hands shifts every later seat down by one and the
/// contract keeps the button index modulo the new seat count. This pins that
/// behaviour for every table size, leaver and button position: seats stay
/// contiguous, the button stays on the ring, and the next hand advances the
/// button by exactly one *index* from the wrapped position.
#[test]
fn contract_leaving_between_hands_keeps_seats_contiguous_and_button_on_the_ring() {
    for n in 3..=6u32 {
        for leaver in 0..n {
            // Button at its start position, one seat in, and on the last seat.
            for hands_before in [0, 1, n - 1] {
                let f = Fixture::new();
                let players: StdVec<Address> = (0..n).map(|_| f.join()).collect();
                for _ in 0..hands_before {
                    f.play_hand();
                }
                let ctx = std::format!("n={n} leaver={leaver} hands_before={hands_before}");
                let button_before = f.dealer();

                f.client.leave_table(&f.table_id, &players[leaver as usize]);

                let after = f.table();
                let remaining = n - 1;
                assert_eq!(after.players.len(), remaining, "{ctx}");
                assert!(
                    seat_of(&after, &players[leaver as usize]).is_none(),
                    "{ctx}"
                );
                for i in 0..remaining {
                    assert_eq!(after.players.get(i).unwrap().seat_index, i, "{ctx}");
                }
                // Button index is carried over modulo the new seat count.
                assert_eq!(after.dealer_seat, button_before % remaining, "{ctx}");

                // The next hand moves the button exactly one index further.
                f.play_hand();
                assert_eq!(
                    f.dealer(),
                    (button_before % remaining + 1) % remaining,
                    "{ctx}"
                );
                assert_eq!(f.table().players.len(), remaining, "{ctx}");
            }
        }
    }
}

#[test]
fn contract_two_max_table_shrinks_to_one_player() {
    let f = Fixture::new();
    let a = f.join();
    let b = f.join();
    f.play_hand();
    assert_eq!(f.dealer(), 1);

    // Player `a` leaves between hands: one seat left, button wraps to seat 0.
    f.client.leave_table(&f.table_id, &a);
    let table = f.table();
    assert_eq!(table.players.len(), 1);
    assert_eq!(table.dealer_seat, 0);
    assert_eq!(table.players.get(0).unwrap().address, b);

    // One player cannot start a hand, and the failed attempt leaves the button alone.
    assert!(f.client.try_start_hand(&f.table_id).is_err());
    assert_eq!(f.dealer(), 0);
    assert_eq!(f.table().hand_number, 1);
}

#[test]
fn contract_last_two_players_leaving_resets_the_button() {
    let f = Fixture::new();
    let a = f.join();
    let b = f.join();
    f.play_hand();

    f.client.leave_table(&f.table_id, &a);
    f.client.leave_table(&f.table_id, &b);

    let table = f.table();
    assert_eq!(table.players.len(), 0);
    assert_eq!(table.dealer_seat, 0);
}
