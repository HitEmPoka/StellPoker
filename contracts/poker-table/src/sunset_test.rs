//! Table sunset and residual balance handling (issue #564).
//!
//! The runbook in `docs/contract-sunset-runbook.md` ends a table's life in
//! three moves: `propose_table_closure` / `execute_table_closure` return every
//! player's chips, then `finalize_sunset` pays out what the contract still
//! holds for the table (rake, jackpot pool, waiting-list escrow) and freezes it.
//! These tests follow that path and check the token accounting at every step:
//! nothing is paid twice, nothing is left behind, and a frozen table takes no
//! new deposits.

#![cfg(test)]

extern crate std;

use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, Vec,
};

const BUY_IN: i128 = 5_000;
const QUEUE_BUY_IN: i128 = 300;

#[contract]
pub struct SunsetHubMock;

#[contractimpl]
impl SunsetHubMock {
    pub fn start_game(
        _env: Env,
        _game_id: Address,
        _session_id: u32,
        _player1: Address,
        _player2: Address,
        _p1_pts: i128,
        _p2_pts: i128,
    ) {
    }

    pub fn end_game(_env: Env, _session_id: u32, _p1_won: bool) {}
}

struct Setup<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    token: TokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    admin: Address,
    committee: Address,
    verifier: Address,
}

fn setup() -> Setup<'static> {
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

    Setup {
        env,
        client,
        token,
        token_admin,
        admin,
        committee,
        verifier,
    }
}

/// A heads-up table with 5% rake, half of it going to the jackpot pool.
fn create_table(s: &Setup) -> u32 {
    let game_hub = s.env.register(SunsetHubMock, ());
    let config = TableConfig {
        token: s.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 100_000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&s.env, 100, 200),
        min_players: 2,
        max_players: 2,
        timeout_ledgers: 100,
        committee: s.committee.clone(),
        verifier: s.verifier.clone(),
        game_hub,
        rake_bps: 500,
        max_rebuys: 0,
        jackpot_rake_share_bps: 5_000,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
        street_time_limit: OptionalStreetTimeLimit::None,
        treasury: None,
        dead_chip_timeout_ledgers: 0,
        reclaim_period_ledgers: 0,
    };
    s.client.create_table(&s.admin, &config)
}

fn join(s: &Setup, table_id: u32, buy_in: i128) -> Address {
    let player = Address::generate(&s.env);
    s.token_admin.mint(&player, &buy_in);
    s.client.join_table(&table_id, &player, &buy_in);
    player
}

fn commit_deal(s: &Setup, table_id: u32, players: u32) {
    let deck_root = BytesN::from_array(&s.env, &[7u8; 32]);
    let mut commitments: Vec<BytesN<32>> = Vec::new(&s.env);
    let mut dealt_indices: Vec<u32> = Vec::new(&s.env);
    for i in 0..players {
        commitments.push_back(BytesN::from_array(&s.env, &[10u8 + i as u8; 32]));
        dealt_indices.push_back(i * 2);
        dealt_indices.push_back(i * 2 + 1);
    }
    s.client.commit_deal(
        &table_id,
        &s.committee,
        &deck_root,
        &commitments,
        &dealt_indices,
        &Bytes::new(&s.env),
        &Bytes::new(&s.env),
    );
}

fn write_u32_field(bytes: &mut Bytes, field_index: u32, value: u32) {
    let start = field_index * 32 + 28;
    bytes.set(start, ((value >> 24) & 0xff) as u8);
    bytes.set(start + 1, ((value >> 16) & 0xff) as u8);
    bytes.set(start + 2, ((value >> 8) & 0xff) as u8);
    bytes.set(start + 3, (value & 0xff) as u8);
}

/// Showdown public inputs naming `winner_seat`, with hole cards for every seat.
fn showdown_inputs(env: &Env, table: &TableState, winner_seat: u32) -> Bytes {
    let mut bytes = Bytes::new(env);
    for _ in 0..(27 * 32) {
        bytes.push_back(0);
    }
    for i in 0..table.players.len() {
        let p = table.players.get(i).unwrap();
        if p.folded {
            continue;
        }
        write_u32_field(&mut bytes, 13 + p.seat_index, 30 + p.seat_index);
        write_u32_field(&mut bytes, 19 + p.seat_index, 40 + p.seat_index);
    }
    write_u32_field(&mut bytes, 25, winner_seat);
    write_u32_field(&mut bytes, 26, 0);
    bytes
}

fn submit_showdown(s: &Setup, table_id: u32) {
    let table = s.client.get_table(&table_id);
    let mut hole_cards: Vec<(u32, u32)> = Vec::new(&s.env);
    let mut salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&s.env);
    for i in 0..table.players.len() {
        let p = table.players.get(i).unwrap();
        if p.folded {
            continue;
        }
        hole_cards.push_back((30 + p.seat_index, 40 + p.seat_index));
        salts.push_back((
            BytesN::from_array(&s.env, &[0u8; 32]),
            BytesN::from_array(&s.env, &[0u8; 32]),
        ));
    }
    s.client.submit_showdown(
        &table_id,
        &s.committee,
        &hole_cards,
        &salts,
        &Bytes::new(&s.env),
        &showdown_inputs(&s.env, &table, 0),
        &Vec::new(&s.env),
    );
}

fn reveal_board(s: &Setup, table_id: u32, phase: &GamePhase, next_index: &mut u32) {
    let count = match phase {
        GamePhase::DealingFlop => 3,
        _ => 1,
    };
    let mut cards: Vec<u32> = Vec::new(&s.env);
    let mut indices: Vec<u32> = Vec::new(&s.env);
    for _ in 0..count {
        cards.push_back(20 + *next_index);
        indices.push_back(*next_index);
        *next_index += 1;
    }
    s.client.reveal_board(
        &table_id,
        &s.committee,
        &cards,
        &indices,
        &Bytes::new(&s.env),
        &Bytes::new(&s.env),
    );
}

fn call_or_check(table: &TableState) -> Action {
    let player = table.players.get(table.current_turn).unwrap();
    let mut current_bet = 0i128;
    for i in 0..table.players.len() {
        let bet = table.players.get(i).unwrap().bet_this_round;
        if bet > current_bet {
            current_bet = bet;
        }
    }
    if current_bet - player.bet_this_round > 0 {
        Action::Call
    } else {
        Action::Check
    }
}

/// Play one hand to a showdown that seat 0 wins. Every street is called or
/// checked, so the 400 chip pot pays 20 chips of rake, split between the house
/// balance and the jackpot pool.
fn play_showdown_hand(s: &Setup, table_id: u32, players: &[Address]) {
    let mut seqs = std::vec![0u32; players.len()];
    let mut next_board_index = players.len() as u32 * 2;

    s.client.start_hand(&table_id);
    commit_deal(s, table_id, players.len() as u32);

    for _ in 0..64 {
        let table = s.client.get_table(&table_id);
        match table.phase {
            GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River => {
                let seat = table.current_turn as usize;
                seqs[seat] += 1;
                s.client.player_action(
                    &table_id,
                    &players[seat],
                    &seqs[seat],
                    &call_or_check(&table),
                );
            }
            GamePhase::DealingFlop | GamePhase::DealingTurn | GamePhase::DealingRiver => {
                reveal_board(s, table_id, &table.phase, &mut next_board_index);
            }
            GamePhase::Showdown => submit_showdown(s, table_id),
            GamePhase::Settlement => return,
            _ => panic!("unexpected phase {:?}", table.phase),
        }
    }
    panic!("hand did not settle");
}

/// Run the closure procedure: propose, wait out the notice period, execute.
fn close_table(s: &Setup, table_id: u32) -> i128 {
    s.client.propose_table_closure(&table_id, &s.admin);
    let ready_at = s.env.ledger().timestamp() + 86_400 + 1;
    s.env.ledger().set_timestamp(ready_at);
    s.client.execute_table_closure(&table_id)
}

/// A heads-up table that has played one showdown hand: two players seated, a
/// third waiting in the queue with escrowed chips, rake and jackpot accrued.
struct Played {
    table_id: u32,
    players: std::vec::Vec<Address>,
    queued: Address,
}

fn played_table(s: &Setup) -> Played {
    let table_id = create_table(s);
    let players = std::vec![join(s, table_id, BUY_IN), join(s, table_id, BUY_IN)];
    let queued = join(s, table_id, QUEUE_BUY_IN);
    play_showdown_hand(s, table_id, &players);
    Played {
        table_id,
        players,
        queued,
    }
}

// ---------------------------------------------------------------------------
// Closure: the chips players hold
// ---------------------------------------------------------------------------

#[test]
fn the_played_hand_leaves_rake_and_a_jackpot_pool_in_the_contract() {
    let s = setup();
    let played = played_table(&s);

    let table = s.client.get_table(&played.table_id);
    assert_eq!(table.rake_balance + table.jackpot_balance, 20);
    assert!(table.jackpot_balance > 0);
    assert_eq!(table.phase, GamePhase::Settlement);
}

#[test]
fn closing_a_settled_table_refunds_stacks_only() {
    // `committed` is stale once a hand settles. Refunding it as well would
    // pay the pot out a second time and exhaust the contract's balance.
    let s = setup();
    let played = played_table(&s);
    let table = s.client.get_table(&played.table_id);
    assert!(table.players.get(0).unwrap().committed > 0);
    let held = table.rake_balance + table.jackpot_balance + QUEUE_BUY_IN;

    let refunded = close_table(&s, played.table_id);

    assert_eq!(refunded, 2 * BUY_IN - 20);
    let returned = s.token.balance(&played.players[0]) + s.token.balance(&played.players[1]);
    assert_eq!(returned, 2 * BUY_IN - 20);
    // What stays behind is exactly rake, jackpot pool and queue escrow.
    assert_eq!(s.token.balance(&s.client.address), held);
}

#[test]
fn closing_a_table_mid_hand_refunds_stacks_and_live_bets() {
    let s = setup();
    let table_id = create_table(&s);
    let players = std::vec![join(&s, table_id, BUY_IN), join(&s, table_id, BUY_IN)];
    s.client.start_hand(&table_id);
    commit_deal(&s, table_id, 2);
    // Blinds are in the pot, so the stacks alone are short of the buy-ins.
    assert!(s.client.get_table(&table_id).pot > 0);

    let refunded = close_table(&s, table_id);

    assert_eq!(refunded, 2 * BUY_IN);
    assert_eq!(s.token.balance(&players[0]), BUY_IN);
    assert_eq!(s.token.balance(&players[1]), BUY_IN);
    assert_eq!(s.token.balance(&s.client.address), 0);
}

// ---------------------------------------------------------------------------
// Finalize: residual balances
// ---------------------------------------------------------------------------

#[test]
fn finalize_pays_out_every_residual_balance() {
    let s = setup();
    let played = played_table(&s);
    let table = s.client.get_table(&played.table_id);
    let rake = table.rake_balance;
    let jackpot = table.jackpot_balance;
    close_table(&s, played.table_id);

    let record = s.client.finalize_sunset(&played.table_id);

    assert_eq!(record.rake_swept, rake);
    assert_eq!(record.jackpot_swept, jackpot);
    assert_eq!(record.queue_refunded, QUEUE_BUY_IN);
    // Rake and the jackpot pool go to the table admin; the queued player is
    // made whole.
    assert_eq!(s.token.balance(&s.admin), rake + jackpot);
    assert_eq!(s.token.balance(&played.queued), QUEUE_BUY_IN);
    // Nothing is left behind, and every chip minted is accounted for.
    assert_eq!(s.token.balance(&s.client.address), 0);
    let everyone = s.token.balance(&played.players[0])
        + s.token.balance(&played.players[1])
        + s.token.balance(&played.queued)
        + s.token.balance(&s.admin);
    assert_eq!(everyone, 2 * BUY_IN + QUEUE_BUY_IN);

    let table = s.client.get_table(&played.table_id);
    assert_eq!(table.rake_balance, 0);
    assert_eq!(table.jackpot_balance, 0);
    assert_eq!(table.players.len(), 0);
    assert_eq!(s.client.get_queue(&played.table_id).len(), 0);
    assert_eq!(s.client.get_sunset_record(&played.table_id), Some(record));
    assert!(s.client.is_table_sunset(&played.table_id));
}

#[test]
fn finalize_on_an_idle_table_sweeps_nothing() {
    let s = setup();
    let table_id = create_table(&s);

    let record = s.client.finalize_sunset(&table_id);

    assert_eq!(record.rake_swept, 0);
    assert_eq!(record.jackpot_swept, 0);
    assert_eq!(record.queue_refunded, 0);
    assert!(s.client.is_table_sunset(&table_id));
    assert_eq!(s.token.balance(&s.client.address), 0);
}

#[test]
fn finalize_clears_the_seat_index_and_seat_metric() {
    let s = setup();
    let played = played_table(&s);
    assert_eq!(s.client.get_contract_metrics().active_seats, 2);
    assert_eq!(s.client.get_player_tables(&played.players[0]).len(), 1);
    close_table(&s, played.table_id);

    s.client.finalize_sunset(&played.table_id);

    assert_eq!(s.client.get_contract_metrics().active_seats, 0);
    assert_eq!(s.client.get_player_tables(&played.players[0]).len(), 0);
    assert_eq!(s.client.get_player_tables(&played.players[1]).len(), 0);
}

#[test]
fn finalize_leaves_other_tables_alone() {
    let s = setup();
    let played = played_table(&s);
    let other = played_table(&s);
    let other_rake = s.client.get_rake_balance(&other.table_id);
    close_table(&s, played.table_id);

    s.client.finalize_sunset(&played.table_id);

    assert!(!s.client.is_table_sunset(&other.table_id));
    assert_eq!(s.client.get_rake_balance(&other.table_id), other_rake);
    assert_eq!(s.client.get_queue(&other.table_id).len(), 1);
    assert_eq!(s.client.get_table(&other.table_id).players.len(), 2);
}

#[test]
fn finalize_requires_the_table_admin() {
    let s = setup();
    let table_id = create_table(&s);

    s.client.finalize_sunset(&table_id);

    assert!(s.env.auths().iter().any(|(who, _)| *who == s.admin));
}

// ---------------------------------------------------------------------------
// Finalize: preconditions
// ---------------------------------------------------------------------------

#[test]
fn finalize_is_refused_while_players_still_hold_chips() {
    let s = setup();
    let played = played_table(&s);

    assert_eq!(
        s.client.try_finalize_sunset(&played.table_id),
        Err(Ok(PokerTableError::SunsetNotReady))
    );
    assert!(!s.client.is_table_sunset(&played.table_id));
}

#[test]
fn finalize_is_refused_during_a_hand() {
    let s = setup();
    let table_id = create_table(&s);
    join(&s, table_id, BUY_IN);
    join(&s, table_id, BUY_IN);
    s.client.start_hand(&table_id);
    commit_deal(&s, table_id, 2);

    assert_eq!(
        s.client.try_finalize_sunset(&table_id),
        Err(Ok(PokerTableError::SunsetNotReady))
    );
}

#[test]
fn finalize_cannot_run_twice() {
    let s = setup();
    let table_id = create_table(&s);
    s.client.finalize_sunset(&table_id);

    assert_eq!(
        s.client.try_finalize_sunset(&table_id),
        Err(Ok(PokerTableError::TableSunset))
    );
}

#[test]
fn finalize_reports_an_unknown_table() {
    let s = setup();

    assert_eq!(
        s.client.try_finalize_sunset(&42),
        Err(Ok(PokerTableError::TableNotFound))
    );
}

// ---------------------------------------------------------------------------
// The freeze
// ---------------------------------------------------------------------------

#[test]
fn a_sunset_table_takes_no_new_deposits() {
    let s = setup();
    let table_id = create_table(&s);
    s.client.finalize_sunset(&table_id);

    let newcomer = Address::generate(&s.env);
    s.token_admin.mint(&newcomer, &BUY_IN);

    assert_eq!(
        s.client.try_join_table(&table_id, &newcomer, &BUY_IN),
        Err(Ok(PokerTableError::TableSunset))
    );
    assert_eq!(
        s.client.try_rebuy(&table_id, &newcomer, &BUY_IN),
        Err(Ok(PokerTableError::TableSunset))
    );
    assert_eq!(
        s.client.try_start_hand(&table_id),
        Err(Ok(PokerTableError::TableSunset))
    );
    // The rejected join took no chips.
    assert_eq!(s.token.balance(&newcomer), BUY_IN);
    assert_eq!(s.token.balance(&s.client.address), 0);
}

#[test]
fn a_closed_and_finalized_table_cannot_be_restarted() {
    // Closure alone leaves the table in `Settlement` with its seats still
    // assigned, which is a state `rebuy` and `start_hand` accept. Finalizing
    // freezes it, so neither can bring the table back to life.
    let s = setup();
    let played = played_table(&s);
    close_table(&s, played.table_id);
    s.client.finalize_sunset(&played.table_id);

    assert_eq!(
        s.client
            .try_rebuy(&played.table_id, &played.players[0], &BUY_IN),
        Err(Ok(PokerTableError::TableSunset))
    );
    assert_eq!(
        s.client.try_start_hand(&played.table_id),
        Err(Ok(PokerTableError::TableSunset))
    );
}

#[test]
fn a_sunset_table_still_answers_read_calls() {
    let s = setup();
    let played = played_table(&s);
    close_table(&s, played.table_id);
    s.client.finalize_sunset(&played.table_id);

    // History and metrics outlive the table so dashboards keep working.
    assert_eq!(s.client.get_hand_history(&played.table_id, &0).len(), 1);
    assert_eq!(s.client.get_table_metrics(&played.table_id).hands_played, 1);
    assert_eq!(s.client.get_table_metrics(&played.table_id).total_rake, 20);
    assert_eq!(s.client.get_rake_balance(&played.table_id), 0);
}
