//! Gas cost regression tests for poker-table contract functions (issue #305).
//!
//! Each test measures the CPU instruction count consumed by one contract
//! entry point and asserts it stays within the approved budget defined in
//! contracts/gas-budgets.json. Budgets are capped at baseline * 1.05 —
//! any >5% increase breaks the build.
//!
//! To update baselines after an intentional cost change:
//!   cargo test -p poker-table gas_ -- --nocapture
//! Copy the printed values into contracts/gas-budgets.json and this file.

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
use std::println;

// ---------------------------------------------------------------------------
// Approved CPU-instruction budgets per function.
// Derived from a baseline run; any increase >5% fails CI.
// ---------------------------------------------------------------------------

const BUDGET_CREATE_TABLE: u64 = 6_000_000;
const BUDGET_JOIN_TABLE: u64 = 6_000_000;
const BUDGET_START_HAND: u64 = 8_000_000;
const BUDGET_COMMIT_DEAL: u64 = 8_000_000;
const BUDGET_PLAYER_ACTION: u64 = 8_000_000;
const BUDGET_REVEAL_BOARD: u64 = 8_000_000;
const BUDGET_LEAVE_TABLE: u64 = 6_000_000;
const BUDGET_CLAIM_TIMEOUT: u64 = 8_000_000;
const BUDGET_WITHDRAW_RAKE: u64 = 6_000_000;
const BUDGET_SUBMIT_SHOWDOWN: u64 = 10_000_000;

const MAX_REGRESSION_PCT: u64 = 5;

// ---------------------------------------------------------------------------
// Minimal mock GameHub (satisfies the interface without doing any work)
// ---------------------------------------------------------------------------

#[contract]
pub struct GasHubMock;

#[contractimpl]
impl GasHubMock {
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
    /// End game.
    ///
    /// # Parameters
    /// - `_session_id`: parameter
    /// - `_p1_won`: parameter
    ///
    /// # Returns
    /// - `()`
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    ///
    /// # Authorization
    /// Requires appropriate authorization.
    pub fn end_game(_env: Env, _session_id: u32, _p1_won: bool) {}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct G<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    token: TokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    admin: Address,
    committee: Address,
    verifier: Address,
}

fn gas_env() -> G<'static> {
    let env = Env::default();
    env.mock_all_auths();
    // unlimited so we measure rather than hit default limits
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

    G {
        env,
        client,
        token,
        token_admin,
        admin,
        committee,
        verifier,
    }
}

fn cfg(g: &G) -> TableConfig {
    let game_hub = g.env.register(GasHubMock, ());
    TableConfig {
        token: g.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 1000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&g.env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: g.committee.clone(),
        verifier: g.verifier.clone(),
        game_hub,
        rake_bps: 0,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
    }
}

fn mint_and_join(g: &G, table_id: u32, buy_in: i128) -> Address {
    let p = Address::generate(&g.env);
    g.token_admin.mint(&p, &buy_in);
    g.client.join_table(&table_id, &p, &buy_in);
    p
}

fn mock_deal(g: &G, table_id: u32, n: u32) {
    let root = BytesN::from_array(&g.env, &[1u8; 32]);
    let mut commitments: Vec<BytesN<32>> = Vec::new(&g.env);
    let mut indices: Vec<u32> = Vec::new(&g.env);
    for i in 0..n {
        commitments.push_back(BytesN::from_array(&g.env, &[2u8; 32]));
        indices.push_back(i * 2);
        indices.push_back(i * 2 + 1);
    }
    g.client.commit_deal(
        &table_id,
        &g.committee,
        &root,
        &commitments,
        &indices,
        &Bytes::new(&g.env),
        &Bytes::new(&g.env),
    );
}

/// Snapshot current CPU instruction count, run `f`, return delta.
fn measure<F: FnOnce()>(g: &G, f: F) -> u64 {
    g.env.cost_estimate().budget().reset_unlimited();
    f();
    g.env.cost_estimate().budget().cpu_instruction_cost()
}

fn check(label: &str, cost: u64, budget: u64) {
    let ceiling = budget + budget * MAX_REGRESSION_PCT / 100;
    println!("[gas] {:30} {:>12} insns  (budget {})", label, cost, budget);
    assert!(
        cost <= ceiling,
        "[gas] REGRESSION: {} consumed {} insns, exceeds budget {} by >{}%",
        label,
        cost,
        budget,
        MAX_REGRESSION_PCT
    );
}

// ---------------------------------------------------------------------------
// One test per entry point
// ---------------------------------------------------------------------------

#[test]
fn gas_create_table() {
    let g = gas_env();
    let config = cfg(&g);
    let cost = measure(&g, || {
        g.client.create_table(&g.admin, &config);
    });
    check("create_table", cost, BUDGET_CREATE_TABLE);
}

#[test]
fn gas_join_table() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    let p = Address::generate(&g.env);
    g.token_admin.mint(&p, &500);

    let cost = measure(&g, || {
        g.client.join_table(&table_id, &p, &500);
    });
    check("join_table", cost, BUDGET_JOIN_TABLE);
}

#[test]
fn gas_start_hand() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);

    let cost = measure(&g, || {
        g.client.start_hand(&table_id);
    });
    check("start_hand", cost, BUDGET_START_HAND);
}

#[test]
fn gas_commit_deal() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);
    g.client.start_hand(&table_id);

    let root = BytesN::from_array(&g.env, &[1u8; 32]);
    let mut comms: Vec<BytesN<32>> = Vec::new(&g.env);
    let mut idxs: Vec<u32> = Vec::new(&g.env);
    for i in 0..4u32 {
        idxs.push_back(i);
    }
    comms.push_back(BytesN::from_array(&g.env, &[2u8; 32]));
    comms.push_back(BytesN::from_array(&g.env, &[3u8; 32]));

    let cost = measure(&g, || {
        g.client.commit_deal(
            &table_id,
            &g.committee,
            &root,
            &comms,
            &idxs,
            &Bytes::new(&g.env),
            &Bytes::new(&g.env),
        );
    });
    check("commit_deal", cost, BUDGET_COMMIT_DEAL);
}

#[test]
fn gas_player_action_call() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);
    g.client.start_hand(&table_id);
    mock_deal(&g, table_id, 2);

    let table = g.client.get_table(&table_id);
    let actor = table.players.get(table.current_turn).unwrap();

    let cost = measure(&g, || {
        g.client
            .player_action(&table_id, &actor.address, &1u32, &Action::Call);
    });
    check("player_action(Call)", cost, BUDGET_PLAYER_ACTION);
}

#[test]
fn gas_player_action_fold() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);
    g.client.start_hand(&table_id);
    mock_deal(&g, table_id, 2);

    let table = g.client.get_table(&table_id);
    let actor = table.players.get(table.current_turn).unwrap();

    let cost = measure(&g, || {
        g.client
            .player_action(&table_id, &actor.address, &1u32, &Action::Fold);
    });
    check("player_action(Fold)", cost, BUDGET_PLAYER_ACTION);
}

#[test]
fn gas_reveal_board() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);
    g.client.start_hand(&table_id);
    mock_deal(&g, table_id, 2);

    // advance to DealingFlop: SB calls
    let table = g.client.get_table(&table_id);
    let actor = table.players.get(table.current_turn).unwrap();
    g.client
        .player_action(&table_id, &actor.address, &1u32, &Action::Call);

    let cards: Vec<u32> = Vec::from_array(&g.env, [10, 20, 30]);
    let idxs: Vec<u32> = Vec::from_array(&g.env, [4, 5, 6]);

    let cost = measure(&g, || {
        g.client.reveal_board(
            &table_id,
            &g.committee,
            &cards,
            &idxs,
            &Bytes::new(&g.env),
            &Bytes::new(&g.env),
        );
    });
    check("reveal_board", cost, BUDGET_REVEAL_BOARD);
}

#[test]
fn gas_leave_table() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    let p = Address::generate(&g.env);
    g.token_admin.mint(&p, &500);
    g.client.join_table(&table_id, &p, &500);

    let cost = measure(&g, || {
        g.client.leave_table(&table_id, &p);
    });
    check("leave_table", cost, BUDGET_LEAVE_TABLE);
}

#[test]
fn gas_claim_timeout() {
    let g = gas_env();
    let table_id = g.client.create_table(&g.admin, &cfg(&g));
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);
    mint_and_join(&g, table_id, 500);
    g.client.start_hand(&table_id);
    mock_deal(&g, table_id, 3);

    let table = g.client.get_table(&table_id);
    let new_seq = table.last_action_ledger + table.config.timeout_ledgers;
    g.env.ledger().set_sequence_number(new_seq);

    let claimer = Address::generate(&g.env);
    let cost = measure(&g, || {
        g.client.claim_timeout(&table_id, &claimer);
    });
    check("claim_timeout", cost, BUDGET_CLAIM_TIMEOUT);
}

#[test]
fn gas_withdraw_rake() {
    let g = gas_env();
    let game_hub = g.env.register(GasHubMock, ());
    let config = TableConfig {
        token: g.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 100_000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&g.env, 100, 200),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: g.committee.clone(),
        verifier: g.verifier.clone(),
        game_hub,
        rake_bps: 500,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
    };
    let table_id = g.client.create_table(&g.admin, &config);
    mint_and_join(&g, table_id, 5000);
    mint_and_join(&g, table_id, 5000);
    g.client.start_hand(&table_id);
    mock_deal(&g, table_id, 2);

    // fold to generate rake
    let table = g.client.get_table(&table_id);
    let folder = table.players.get(table.current_turn).unwrap();
    g.client
        .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

    let cost = measure(&g, || {
        g.client.withdraw_rake(&table_id);
    });
    check("withdraw_rake", cost, BUDGET_WITHDRAW_RAKE);
}

// ---------------------------------------------------------------------------
// Six-handed hot paths (issue #548): bet, reveal and settle with a full table,
// where the per-player loops cost the most. Run with `-- --nocapture` to
// compare numbers before and after a change.
// ---------------------------------------------------------------------------

struct Hand {
    table_id: u32,
    players: std::vec::Vec<Address>,
    seqs: std::vec::Vec<u32>,
    next_board_index: u32,
}

fn six_handed(g: &G) -> Hand {
    let table_id = g.client.create_table(&g.admin, &cfg(g));
    let mut players = std::vec::Vec::new();
    for _ in 0..6 {
        players.push(mint_and_join(g, table_id, 1000));
    }
    g.client.start_hand(&table_id);
    mock_deal(g, table_id, 6);
    Hand {
        table_id,
        players,
        seqs: std::vec![0; 6],
        next_board_index: 12,
    }
}

/// The action the player to act takes to keep everyone in: call or check.
fn passive_action(table: &TableState) -> Action {
    let mut max_bet = 0i128;
    for i in 0..table.players.len() {
        max_bet = max_bet.max(table.players.get(i).unwrap().bet_this_round);
    }
    let me = table.players.get(table.current_turn).unwrap();
    if me.bet_this_round < max_bet {
        Action::Call
    } else {
        Action::Check
    }
}

/// Who acts next and with which sequence number.
fn next_actor(g: &G, h: &mut Hand) -> (Address, u32) {
    let seat = g.client.get_table(&h.table_id).current_turn as usize;
    h.seqs[seat] += 1;
    (h.players[seat].clone(), h.seqs[seat])
}

fn act(g: &G, h: &mut Hand, action: &Action) {
    let (player, seq) = next_actor(g, h);
    g.client.player_action(&h.table_id, &player, &seq, action);
}

/// Call or check around the table until the betting round ends.
fn finish_round(g: &G, h: &mut Hand) {
    loop {
        let table = g.client.get_table(&h.table_id);
        if !matches!(
            table.phase,
            GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River
        ) {
            return;
        }
        act(g, h, &passive_action(&table));
    }
}

fn next_reveal(g: &G, h: &mut Hand) -> (Vec<u32>, Vec<u32>) {
    let table = g.client.get_table(&h.table_id);
    let count = if matches!(table.phase, GamePhase::DealingFlop) {
        3
    } else {
        1
    };
    let mut cards: Vec<u32> = Vec::new(&g.env);
    let mut idxs: Vec<u32> = Vec::new(&g.env);
    for _ in 0..count {
        cards.push_back(20 + h.next_board_index);
        idxs.push_back(h.next_board_index);
        h.next_board_index += 1;
    }
    (cards, idxs)
}

fn reveal_with(g: &G, h: &Hand, cards: &Vec<u32>, idxs: &Vec<u32>) {
    g.client.reveal_board(
        &h.table_id,
        &g.committee,
        cards,
        idxs,
        &Bytes::new(&g.env),
        &Bytes::new(&g.env),
    );
}

fn reveal(g: &G, h: &mut Hand) {
    let (cards, idxs) = next_reveal(g, h);
    reveal_with(g, h, &cards, &idxs);
}

fn write_u32_field(bytes: &mut Bytes, field_index: u32, value: u32) {
    let start = field_index * 32 + 28;
    bytes.set(start, ((value >> 24) & 0xff) as u8);
    bytes.set(start + 1, ((value >> 16) & 0xff) as u8);
    bytes.set(start + 2, ((value >> 8) & 0xff) as u8);
    bytes.set(start + 3, (value & 0xff) as u8);
}

type ShowdownArgs = (Vec<(u32, u32)>, Vec<(BytesN<32>, BytesN<32>)>, Bytes);

fn showdown_args(g: &G, h: &Hand) -> ShowdownArgs {
    let table = g.client.get_table(&h.table_id);
    let mut public_inputs = Bytes::new(&g.env);
    for _ in 0..(27 * 32) {
        public_inputs.push_back(0);
    }
    let mut hole_cards: Vec<(u32, u32)> = Vec::new(&g.env);
    let mut salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&g.env);
    let mut winner = None;
    for i in 0..table.players.len() {
        let p = table.players.get(i).unwrap();
        if p.folded {
            continue;
        }
        winner.get_or_insert(p.seat_index);
        write_u32_field(&mut public_inputs, 13 + p.seat_index, 30 + p.seat_index);
        write_u32_field(&mut public_inputs, 19 + p.seat_index, 40 + p.seat_index);
        hole_cards.push_back((30 + p.seat_index, 40 + p.seat_index));
        salts.push_back((
            BytesN::from_array(&g.env, &[0u8; 32]),
            BytesN::from_array(&g.env, &[0u8; 32]),
        ));
    }
    write_u32_field(&mut public_inputs, 25, winner.unwrap());
    write_u32_field(&mut public_inputs, 26, 0);
    (hole_cards, salts, public_inputs)
}

#[test]
fn gas_six_handed_bet() {
    let g = gas_env();
    let mut h = six_handed(&g);
    finish_round(&g, &mut h);
    reveal(&g, &mut h);

    // First to act on the flop bets into five opponents.
    let (player, seq) = next_actor(&g, &mut h);
    let cost = measure(&g, || {
        g.client
            .player_action(&h.table_id, &player, &seq, &Action::Bet(10));
    });
    check("six_handed bet", cost, BUDGET_PLAYER_ACTION);
}

#[test]
fn gas_six_handed_reveal() {
    let g = gas_env();
    let mut h = six_handed(&g);
    finish_round(&g, &mut h);

    let (cards, idxs) = next_reveal(&g, &mut h);
    let cost = measure(&g, || reveal_with(&g, &h, &cards, &idxs));
    check("six_handed reveal_board", cost, BUDGET_REVEAL_BOARD);
}

#[test]
fn gas_six_handed_settle() {
    let g = gas_env();
    let mut h = six_handed(&g);
    for _ in 0..3 {
        finish_round(&g, &mut h);
        reveal(&g, &mut h);
    }
    finish_round(&g, &mut h);
    assert_eq!(g.client.get_table(&h.table_id).phase, GamePhase::Showdown);

    let (hole_cards, salts, public_inputs) = showdown_args(&g, &h);
    let no_bad_beats: Vec<(u32, u32)> = Vec::new(&g.env);
    let cost = measure(&g, || {
        g.client.submit_showdown(
            &h.table_id,
            &g.committee,
            &hole_cards,
            &salts,
            &Bytes::new(&g.env),
            &public_inputs,
            &no_bad_beats,
        );
    });
    check("six_handed submit_showdown", cost, BUDGET_SUBMIT_SHOWDOWN);
}

/// Print a full gas cost report. Run with `-- --nocapture` to see output.
/// Use the printed values to update gas-budgets.json and the constants above.
#[test]
fn gas_report() {
    println!("\n╔════════════════════════════════════════╗");
    println!("║       StellPoker Gas Cost Report       ║");
    println!("╠══════════════════════════╦═════════════╣");
    println!("║ Function                 ║ CPU insns   ║");
    println!("╠══════════════════════════╬═════════════╣");

    macro_rules! row {
        ($label:expr, $budget:expr) => {
            println!("║ {:<24} ║ {:>11} ║", $label, $budget);
        };
    }
    row!("create_table", BUDGET_CREATE_TABLE);
    row!("join_table", BUDGET_JOIN_TABLE);
    row!("start_hand", BUDGET_START_HAND);
    row!("commit_deal", BUDGET_COMMIT_DEAL);
    row!("player_action", BUDGET_PLAYER_ACTION);
    row!("reveal_board", BUDGET_REVEAL_BOARD);
    row!("leave_table", BUDGET_LEAVE_TABLE);
    row!("claim_timeout", BUDGET_CLAIM_TIMEOUT);
    row!("withdraw_rake", BUDGET_WITHDRAW_RAKE);
    row!("submit_showdown", BUDGET_SUBMIT_SHOWDOWN);
    println!("╚══════════════════════════╩═════════════╝");
    println!("(run individual gas_* tests with --nocapture for live measurements)");
}
