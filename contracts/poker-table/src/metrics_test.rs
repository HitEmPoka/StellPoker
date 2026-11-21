//! Aggregate metrics views (issue #563).
//!
//! `get_contract_metrics` and `get_table_metrics` are read from counters that
//! the settlement and seating entrypoints keep up to date, so a dashboard read
//! never scans tables or history. These tests check that every counter moves
//! exactly when it should, that the totals agree with the archived hand
//! history, and that a read costs the same however much the contract has done.

#![cfg(test)]

extern crate std;

use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, Vec,
};

#[contract]
pub struct MetricsHubMock;

#[contractimpl]
impl MetricsHubMock {
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

/// A table with 100/200 blinds, so a fold-win pot of 300 yields real rake.
fn create_table(s: &Setup, max_players: u32, rake_bps: u32) -> u32 {
    let game_hub = s.env.register(MetricsHubMock, ());
    let config = TableConfig {
        token: s.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 100_000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&s.env, 100, 200),
        min_players: 2,
        max_players,
        timeout_ledgers: 100,
        committee: s.committee.clone(),
        verifier: s.verifier.clone(),
        game_hub,
        rake_bps,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
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
    let deck_root = BytesN::from_array(&s.env, &[1u8; 32]);
    let mut commitments: Vec<BytesN<32>> = Vec::new(&s.env);
    let mut dealt_indices: Vec<u32> = Vec::new(&s.env);
    for i in 0..players {
        commitments.push_back(BytesN::from_array(&s.env, &[2u8; 32]));
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

/// Play one hand of a heads-up table to settlement: the player on the clock
/// folds. With 100/200 blinds the pot is 300, so 5% rake is 15 chips.
fn play_fold_win(s: &Setup, table_id: u32, players: &[Address], seqs: &mut [u32]) {
    s.client.start_hand(&table_id);
    commit_deal(s, table_id, players.len() as u32);

    let seat = s.client.get_table(&table_id).current_turn as usize;
    seqs[seat] += 1;
    s.client
        .player_action(&table_id, &players[seat], &seqs[seat], &Action::Fold);
}

/// CPU instructions consumed by `f`.
fn cpu_of(s: &Setup, f: impl FnOnce()) -> u64 {
    s.env.cost_estimate().budget().reset_unlimited();
    f();
    s.env.cost_estimate().budget().cpu_instruction_cost()
}

// ---------------------------------------------------------------------------
// Starting point
// ---------------------------------------------------------------------------

#[test]
fn fresh_contract_reports_zero_metrics() {
    let s = setup();

    assert_eq!(
        s.client.get_contract_metrics(),
        ContractMetrics {
            tables_created: 0,
            hands_played: 0,
            total_rake: 0,
            active_seats: 0,
        }
    );
}

#[test]
fn table_metrics_for_an_unknown_table_is_a_typed_error() {
    let s = setup();

    assert_eq!(
        s.client.try_get_table_metrics(&99),
        Err(Ok(PokerTableError::TableNotFound))
    );
}

#[test]
fn tables_created_counts_every_table() {
    let s = setup();

    create_table(&s, 6, 0);
    create_table(&s, 2, 0);
    create_table(&s, 6, 500);

    assert_eq!(s.client.get_contract_metrics().tables_created, 3);
}

// ---------------------------------------------------------------------------
// Active seats
// ---------------------------------------------------------------------------

#[test]
fn active_seats_follow_joins_and_leaves() {
    let s = setup();
    let table_id = create_table(&s, 6, 0);

    let a = join(&s, table_id, 500);
    let b = join(&s, table_id, 500);
    let c = join(&s, table_id, 500);
    assert_eq!(s.client.get_contract_metrics().active_seats, 3);
    assert_eq!(s.client.get_table_metrics(&table_id).active_seats, 3);

    s.client.leave_table(&table_id, &b);
    assert_eq!(s.client.get_contract_metrics().active_seats, 2);
    assert_eq!(s.client.get_table_metrics(&table_id).active_seats, 2);

    s.client.leave_table(&table_id, &a);
    s.client.leave_table(&table_id, &c);
    assert_eq!(s.client.get_contract_metrics().active_seats, 0);
    assert_eq!(s.client.get_table_metrics(&table_id).active_seats, 0);
}

#[test]
fn active_seats_sum_across_tables() {
    let s = setup();
    let first = create_table(&s, 6, 0);
    let second = create_table(&s, 6, 0);

    join(&s, first, 500);
    join(&s, first, 500);
    join(&s, second, 500);

    assert_eq!(s.client.get_contract_metrics().active_seats, 3);
    assert_eq!(s.client.get_table_metrics(&first).active_seats, 2);
    assert_eq!(s.client.get_table_metrics(&second).active_seats, 1);
}

#[test]
fn queued_players_are_not_active_seats_until_seated() {
    let s = setup();
    let table_id = create_table(&s, 2, 0);

    let a = join(&s, table_id, 500);
    join(&s, table_id, 500);
    // The table is full, so this player waits in the queue.
    join(&s, table_id, 300);
    assert_eq!(s.client.get_queue(&table_id).len(), 1);
    assert_eq!(s.client.get_contract_metrics().active_seats, 2);

    // A seat opens and the queued player takes it: one out, one in.
    s.client.leave_table(&table_id, &a);
    assert_eq!(s.client.get_queue(&table_id).len(), 0);
    assert_eq!(s.client.get_contract_metrics().active_seats, 2);
    assert_eq!(s.client.get_table_metrics(&table_id).active_seats, 2);
}

// ---------------------------------------------------------------------------
// Hands played and total rake
// ---------------------------------------------------------------------------

#[test]
fn settled_hands_count_hands_and_rake() {
    let s = setup();
    let table_id = create_table(&s, 2, 500);
    let players = std::vec![join(&s, table_id, 5_000), join(&s, table_id, 5_000)];
    let mut seqs = std::vec![0u32; players.len()];

    play_fold_win(&s, table_id, &players, &mut seqs);
    let after_one = s.client.get_contract_metrics();
    assert_eq!(after_one.hands_played, 1);
    assert_eq!(after_one.total_rake, 15);

    play_fold_win(&s, table_id, &players, &mut seqs);
    let after_two = s.client.get_contract_metrics();
    assert_eq!(after_two.hands_played, 2);
    assert_eq!(after_two.total_rake, 30);

    let table_metrics = s.client.get_table_metrics(&table_id);
    assert_eq!(table_metrics.hands_played, 2);
    assert_eq!(table_metrics.total_rake, 30);
}

#[test]
fn a_hand_in_progress_is_not_counted_yet() {
    let s = setup();
    let table_id = create_table(&s, 2, 500);
    join(&s, table_id, 5_000);
    join(&s, table_id, 5_000);

    s.client.start_hand(&table_id);
    commit_deal(&s, table_id, 2);

    assert_eq!(s.client.get_contract_metrics().hands_played, 0);
    assert_eq!(s.client.get_table_metrics(&table_id).hands_played, 0);
}

#[test]
fn zero_rake_hands_still_count() {
    let s = setup();
    let table_id = create_table(&s, 2, 0);
    let players = std::vec![join(&s, table_id, 5_000), join(&s, table_id, 5_000)];
    let mut seqs = std::vec![0u32; players.len()];

    play_fold_win(&s, table_id, &players, &mut seqs);

    let metrics = s.client.get_contract_metrics();
    assert_eq!(metrics.hands_played, 1);
    assert_eq!(metrics.total_rake, 0);
}

#[test]
fn total_rake_is_cumulative_and_survives_withdrawal() {
    let s = setup();
    let table_id = create_table(&s, 2, 500);
    let players = std::vec![join(&s, table_id, 5_000), join(&s, table_id, 5_000)];
    let mut seqs = std::vec![0u32; players.len()];

    play_fold_win(&s, table_id, &players, &mut seqs);
    assert_eq!(s.client.withdraw_rake(&table_id), 15);
    assert_eq!(s.client.get_rake_balance(&table_id), 0);

    // The balance is spendable rake; the metric is lifetime rake.
    assert_eq!(s.client.get_contract_metrics().total_rake, 15);
    assert_eq!(s.client.get_table_metrics(&table_id).total_rake, 15);

    play_fold_win(&s, table_id, &players, &mut seqs);
    assert_eq!(s.client.get_contract_metrics().total_rake, 30);
}

#[test]
fn table_metrics_are_kept_per_table() {
    let s = setup();
    let busy = create_table(&s, 2, 500);
    let idle = create_table(&s, 2, 500);
    let players = std::vec![join(&s, busy, 5_000), join(&s, busy, 5_000)];
    let mut seqs = std::vec![0u32; players.len()];

    play_fold_win(&s, busy, &players, &mut seqs);

    assert_eq!(s.client.get_table_metrics(&busy).hands_played, 1);
    assert_eq!(s.client.get_table_metrics(&busy).total_rake, 15);
    assert_eq!(s.client.get_table_metrics(&idle).hands_played, 0);
    assert_eq!(s.client.get_table_metrics(&idle).total_rake, 0);
    // The contract-wide view is the sum over tables.
    assert_eq!(s.client.get_contract_metrics().hands_played, 1);
    assert_eq!(s.client.get_contract_metrics().total_rake, 15);
}

#[test]
fn table_totals_match_the_archived_hand_history() {
    let s = setup();
    let table_id = create_table(&s, 2, 500);
    let players = std::vec![join(&s, table_id, 5_000), join(&s, table_id, 5_000)];
    let mut seqs = std::vec![0u32; players.len()];

    for _ in 0..3 {
        play_fold_win(&s, table_id, &players, &mut seqs);
    }

    let history = s.client.get_hand_history(&table_id, &0);
    let mut archived_rake = 0i128;
    for record in history.iter() {
        archived_rake += record.rake;
    }

    let metrics = s.client.get_table_metrics(&table_id);
    assert_eq!(metrics.hands_played, history.len() as u64);
    assert_eq!(metrics.total_rake, archived_rake);
    assert_eq!(
        metrics.hands_played,
        s.client.get_hand_history_meta(&table_id).total_archived as u64
    );
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

#[test]
fn reading_metrics_costs_the_same_however_much_the_contract_has_done() {
    let s = setup();
    let table_id = create_table(&s, 2, 500);
    let players = std::vec![join(&s, table_id, 5_000), join(&s, table_id, 5_000)];
    let mut seqs = std::vec![0u32; players.len()];
    // Baseline once the counters exist, so both reads decode the same record.
    play_fold_win(&s, table_id, &players, &mut seqs);

    let before = cpu_of(&s, || {
        s.client.get_contract_metrics();
    });

    // Grow the contract: more tables, more seats, more settled hands.
    for _ in 0..12 {
        let extra = create_table(&s, 6, 500);
        join(&s, extra, 500);
    }
    for _ in 0..4 {
        play_fold_win(&s, table_id, &players, &mut seqs);
    }

    let after = cpu_of(&s, || {
        s.client.get_contract_metrics();
    });

    // A scan would grow with the 13 tables and 5 hands; a counter read does not.
    assert!(
        after <= before + before / 4,
        "get_contract_metrics cost grew from {before} to {after} CPU instructions"
    );
}
