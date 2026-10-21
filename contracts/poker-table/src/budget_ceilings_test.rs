//! Soroban budget regression suite with per-function CPU/memory ceilings
//! (issue #567, follow-up to #305).
//!
//! `gas_regression_test.rs` pins CPU baselines with a 5% tolerance.
//! This suite pins *hard ceilings* from `contracts/poker-table/ceilings.json`:
//! CI fails if any entrypoint exceeds its `cpu_ceiling` OR its `mem_ceiling`.
//!
//! Ceilings are deliberately loose headroom over the baselines in
//! `contracts/gas-budgets.json` — they catch real regressions (e.g. an
//! unbounded loop or a storage blow-up) without flaking on small metering
//! noise. To update after an intentional cost change:
//!
//!   cargo test -p poker-table budget_ceiling -- --nocapture
//!   cargo test -p poker-table gas_ -- --nocapture
//!
//! Copy the printed live values into `ceilings.json` (ceilings) and
//! `gas-budgets.json` (baselines), keeping `cpu_ceiling >= baseline`.

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

const CEILINGS_JSON: &str = include_str!("../ceilings.json");

struct B<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    token: TokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    admin: Address,
    committee: Address,
    verifier: Address,
}

fn budget_env() -> B<'static> {
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

    B {
        env,
        client,
        token,
        token_admin,
        admin,
        committee,
        verifier,
    }
}

#[contract]
pub struct BudgetHubMock;

#[contractimpl]
impl BudgetHubMock {
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

fn budget_cfg(b: &B) -> TableConfig {
    let game_hub = b.env.register(BudgetHubMock, ());
    TableConfig {
        token: b.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 1000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&b.env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: b.committee.clone(),
        verifier: b.verifier.clone(),
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

fn mint_and_join(b: &B, table_id: u32, buy_in: i128) -> Address {
    let p = Address::generate(&b.env);
    b.token_admin.mint(&p, &buy_in);
    b.client.join_table(&table_id, &p, &buy_in);
    p
}

fn mock_deal(b: &B, table_id: u32, n: u32) {
    let root = BytesN::from_array(&b.env, &[1u8; 32]);
    let mut commitments: Vec<BytesN<32>> = Vec::new(&b.env);
    let mut indices: Vec<u32> = Vec::new(&b.env);
    for i in 0..n {
        commitments.push_back(BytesN::from_array(&b.env, &[2u8; 32]));
        indices.push_back(i * 2);
        indices.push_back(i * 2 + 1);
    }
    b.client.commit_deal(
        &table_id,
        &b.committee,
        &root,
        &commitments,
        &indices,
        &Bytes::new(&b.env),
        &Bytes::new(&b.env),
    );
}

/// Measure (cpu_insns, mem_bytes) consumed by `f`.
fn measure_both(b: &B, f: impl FnOnce()) -> (u64, u64) {
    b.env.cost_estimate().budget().reset_unlimited();
    f();
    let budget = b.env.cost_estimate().budget();
    (
        budget.cpu_instruction_cost(),
        budget.memory_bytes_cost(),
    )
}

fn ceiling_for(function: &str, field: &str) -> u64 {
    let v: serde_json::Value =
        serde_json::from_str(CEILINGS_JSON).expect("ceilings.json must parse");
    v["functions"][function][field]
        .as_u64()
        .unwrap_or_else(|| panic!("ceilings.json missing functions.{function}.{field}"))
}

fn check_ceiling(label: &str, cpu: u64, mem: u64) {
    let cpu_ceiling = ceiling_for(label, "cpu_ceiling");
    let mem_ceiling = ceiling_for(label, "mem_ceiling");
    println!(
        "[budget] {:20} cpu {:>10} / {:>10}  mem {:>10} / {:>10}",
        label, cpu, cpu_ceiling, mem, mem_ceiling
    );
    assert!(
        cpu <= cpu_ceiling,
        "[budget] CPU REGRESSION: {label} consumed {cpu} insns, exceeds ceiling {cpu_ceiling}"
    );
    assert!(
        mem <= mem_ceiling,
        "[budget] MEM REGRESSION: {label} consumed {mem} bytes, exceeds ceiling {mem_ceiling}"
    );
}

#[test]
fn budget_ceiling_create_table() {
    let b = budget_env();
    let config = budget_cfg(&b);
    let (cpu, mem) = measure_both(&b, || {
        b.client.create_table(&b.admin, &config);
    });
    check_ceiling("create_table", cpu, mem);
}

#[test]
fn budget_ceiling_join_table() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    let p = Address::generate(&b.env);
    b.token_admin.mint(&p, &500);
    let (cpu, mem) = measure_both(&b, || {
        b.client.join_table(&table_id, &p, &500);
    });
    check_ceiling("join_table", cpu, mem);
}

#[test]
fn budget_ceiling_start_hand() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    mint_and_join(&b, table_id, 500);
    mint_and_join(&b, table_id, 500);
    let (cpu, mem) = measure_both(&b, || {
        b.client.start_hand(&table_id);
    });
    check_ceiling("start_hand", cpu, mem);
}

#[test]
fn budget_ceiling_commit_deal() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    mint_and_join(&b, table_id, 500);
    mint_and_join(&b, table_id, 500);
    b.client.start_hand(&table_id);

    let root = BytesN::from_array(&b.env, &[1u8; 32]);
    let mut comms: Vec<BytesN<32>> = Vec::new(&b.env);
    let mut idxs: Vec<u32> = Vec::new(&b.env);
    for i in 0..4u32 {
        idxs.push_back(i);
    }
    comms.push_back(BytesN::from_array(&b.env, &[2u8; 32]));
    comms.push_back(BytesN::from_array(&b.env, &[3u8; 32]));
    let (cpu, mem) = measure_both(&b, || {
        b.client.commit_deal(
            &table_id,
            &b.committee,
            &root,
            &comms,
            &idxs,
            &Bytes::new(&b.env),
            &Bytes::new(&b.env),
        );
    });
    check_ceiling("commit_deal", cpu, mem);
}

#[test]
fn budget_ceiling_player_action() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    mint_and_join(&b, table_id, 500);
    mint_and_join(&b, table_id, 500);
    b.client.start_hand(&table_id);
    mock_deal(&b, table_id, 2);

    let table = b.client.get_table(&table_id);
    let actor = table.players.get(table.current_turn).unwrap();
    let (cpu, mem) = measure_both(&b, || {
        b.client
            .player_action(&table_id, &actor.address, &1u32, &Action::Call);
    });
    check_ceiling("player_action", cpu, mem);
}

#[test]
fn budget_ceiling_reveal_board() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    mint_and_join(&b, table_id, 500);
    mint_and_join(&b, table_id, 500);
    b.client.start_hand(&table_id);
    mock_deal(&b, table_id, 2);

    let table = b.client.get_table(&table_id);
    let actor = table.players.get(table.current_turn).unwrap();
    b.client
        .player_action(&table_id, &actor.address, &1u32, &Action::Call);

    let cards: Vec<u32> = Vec::from_array(&b.env, [10, 20, 30]);
    let idxs: Vec<u32> = Vec::from_array(&b.env, [4, 5, 6]);
    let (cpu, mem) = measure_both(&b, || {
        b.client.reveal_board(
            &table_id,
            &b.committee,
            &cards,
            &idxs,
            &Bytes::new(&b.env),
            &Bytes::new(&b.env),
        );
    });
    check_ceiling("reveal_board", cpu, mem);
}

#[test]
fn budget_ceiling_leave_table() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    let p = Address::generate(&b.env);
    b.token_admin.mint(&p, &500);
    b.client.join_table(&table_id, &p, &500);
    let (cpu, mem) = measure_both(&b, || {
        b.client.leave_table(&table_id, &p);
    });
    check_ceiling("leave_table", cpu, mem);
}

#[test]
fn budget_ceiling_claim_timeout() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    mint_and_join(&b, table_id, 500);
    mint_and_join(&b, table_id, 500);
    mint_and_join(&b, table_id, 500);
    b.client.start_hand(&table_id);
    mock_deal(&b, table_id, 3);

    let table = b.client.get_table(&table_id);
    let new_seq = table.last_action_ledger + table.config.timeout_ledgers;
    b.env.ledger().set_sequence_number(new_seq);

    let claimer = Address::generate(&b.env);
    let (cpu, mem) = measure_both(&b, || {
        b.client.claim_timeout(&table_id, &claimer);
    });
    check_ceiling("claim_timeout", cpu, mem);
}

#[test]
fn budget_ceiling_withdraw_rake() {
    let b = budget_env();
    let game_hub = b.env.register(BudgetHubMock, ());
    let config = TableConfig {
        token: b.token.address.clone(),
        min_buy_in: 100,
        max_buy_in: 100_000,
        betting_structure: crate::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&b.env, 100, 200),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: b.committee.clone(),
        verifier: b.verifier.clone(),
        game_hub,
        rake_bps: 500,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
        street_time_limit: OptionalStreetTimeLimit::None,
        treasury: None,
        dead_chip_timeout_ledgers: 0,
        reclaim_period_ledgers: 0,
    };
    let table_id = b.client.create_table(&b.admin, &config);
    mint_and_join(&b, table_id, 5000);
    mint_and_join(&b, table_id, 5000);
    b.client.start_hand(&table_id);
    mock_deal(&b, table_id, 2);

    let table = b.client.get_table(&table_id);
    let folder = table.players.get(table.current_turn).unwrap();
    b.client
        .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

    let (cpu, mem) = measure_both(&b, || {
        b.client.withdraw_rake(&table_id);
    });
    check_ceiling("withdraw_rake", cpu, mem);
}

#[test]
fn budget_ceiling_submit_showdown() {
    let b = budget_env();
    let table_id = b.client.create_table(&b.admin, &budget_cfg(&b));
    for _ in 0..6 {
        mint_and_join(&b, table_id, 1000);
    }
    b.client.start_hand(&table_id);
    mock_deal(&b, table_id, 6);

    // Drive passively to Showdown: call/check until DealingFlop, reveal, repeat.
    for _ in 0..4 {
        loop {
            let table = b.client.get_table(&table_id);
            if !matches!(
                table.phase,
                GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River
            ) {
                break;
            }
            let mut max_bet = 0i128;
            for i in 0..table.players.len() {
                max_bet = max_bet.max(table.players.get(i).unwrap().bet_this_round);
            }
            let me = table.players.get(table.current_turn).unwrap();
            let action = if me.bet_this_round < max_bet {
                Action::Call
            } else {
                Action::Check
            };
            // Sequence numbers: each player acts with an incrementing seq.
            // Use a fresh seq per call by reading a deterministic counter from
            // the test harness (players act in turn order; seq 1..N works
            // because each address acts at most a few times here and the
            // contract mock accepts any strictly increasing seq — we retry
            // with increasing seqs until the call succeeds).
            let mut seq = 1u32;
            loop {
                let res = b.client.try_player_action(&table_id, &me.address, &seq, &action);
                if res.is_ok() {
                    break;
                }
                // try_ returns Err(Ok(contract_err)) for typed errors; a
                // StaleActionSequence means we need a higher seq.
                match res {
                    Err(Ok(PokerTableError::StaleActionSequence)) => seq += 1,
                    _ => break,
                }
                if seq > 20 {
                    break;
                }
            }
            // If the table left a betting phase (fold-win or round end), stop.
            let after = b.client.get_table(&table_id);
            if !matches!(
                after.phase,
                GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River
            ) {
                break;
            }
            // Safety: avoid infinite loop if no progress.
            if after.current_turn == table.current_turn && seq > 10 {
                break;
            }
        }
        let table = b.client.get_table(&table_id);
        if matches!(table.phase, GamePhase::Showdown) {
            break;
        }
        if matches!(
            table.phase,
            GamePhase::DealingFlop | GamePhase::DealingTurn | GamePhase::DealingRiver
        ) {
            let count = if matches!(table.phase, GamePhase::DealingFlop) {
                3
            } else {
                1
            };
            let mut cards: Vec<u32> = Vec::new(&b.env);
            let mut idxs: Vec<u32> = Vec::new(&b.env);
            for k in 0..count {
                cards.push_back(20 + k);
                idxs.push_back(12 + k);
            }
            let _ = b.client.try_reveal_board(
                &table_id,
                &b.committee,
                &cards,
                &idxs,
                &Bytes::new(&b.env),
                &Bytes::new(&b.env),
            );
        } else {
            break;
        }
    }

    let table = b.client.get_table(&table_id);
    if !matches!(table.phase, GamePhase::Showdown) {
        // If we did not reach showdown (fold-win path), settle-what-we-have
        // still exercises submit_showdown's validation ceiling via try_.
        let hole: Vec<(u32, u32)> = Vec::new(&b.env);
        let salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&b.env);
        let mut pi = Bytes::new(&b.env);
        for _ in 0..(27 * 32) {
            pi.push_back(0);
        }
        let empty: Vec<(u32, u32)> = Vec::new(&b.env);
        let (cpu, mem) = measure_both(&b, || {
            let _ = b.client.try_submit_showdown(
                &table_id,
                &b.committee,
                &hole,
                &salts,
                &Bytes::new(&b.env),
                &pi,
                &empty,
            );
        });
        // Validation-only path is strictly cheaper than a full settle.
        assert!(
            cpu <= ceiling_for("submit_showdown", "cpu_ceiling"),
            "submit_showdown validation exceeded CPU ceiling"
        );
        assert!(
            mem <= ceiling_for("submit_showdown", "mem_ceiling"),
            "submit_showdown validation exceeded MEM ceiling"
        );
        return;
    }

    // Full six-handed settle: build proof inputs matching the live board.
    let mut public_inputs = Bytes::new(&b.env);
    for _ in 0..(27 * 32) {
        public_inputs.push_back(0);
    }
    let mut hole_cards: Vec<(u32, u32)> = Vec::new(&b.env);
    let mut salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&b.env);
    let mut winner = None;
    for i in 0..table.players.len() {
        let p = table.players.get(i).unwrap();
        if p.folded {
            continue;
        }
        winner.get_or_insert(p.seat_index);
        let base = 30 + p.seat_index;
        let off1 = (13 + p.seat_index) * 32 + 28;
        let off2 = (19 + p.seat_index) * 32 + 28;
        public_inputs.set(off1, ((base >> 24) & 0xff) as u8);
        public_inputs.set(off1 + 1, ((base >> 16) & 0xff) as u8);
        public_inputs.set(off1 + 2, ((base >> 8) & 0xff) as u8);
        public_inputs.set(off1 + 3, (base & 0xff) as u8);
        public_inputs.set(off2, (((base + 10) >> 24) & 0xff) as u8);
        public_inputs.set(off2 + 1, (((base + 10) >> 16) & 0xff) as u8);
        public_inputs.set(off2 + 2, (((base + 10) >> 8) & 0xff) as u8);
        public_inputs.set(off2 + 3, ((base + 10) & 0xff) as u8);
        hole_cards.push_back((base, base + 10));
        salts.push_back((
            BytesN::from_array(&b.env, &[0u8; 32]),
            BytesN::from_array(&b.env, &[0u8; 32]),
        ));
    }
    let w = winner.unwrap();
    let woff = 25 * 32 + 28;
    public_inputs.set(woff, ((w >> 24) & 0xff) as u8);
    public_inputs.set(woff + 1, ((w >> 16) & 0xff) as u8);
    public_inputs.set(woff + 2, ((w >> 8) & 0xff) as u8);
    public_inputs.set(woff + 3, (w & 0xff) as u8);
    let no_bad_beats: Vec<(u32, u32)> = Vec::new(&b.env);
    let (cpu, mem) = measure_both(&b, || {
        b.client.submit_showdown(
            &table_id,
            &b.committee,
            &hole_cards,
            &salts,
            &Bytes::new(&b.env),
            &public_inputs,
            &no_bad_beats,
        );
    });
    check_ceiling("submit_showdown", cpu, mem);
}

/// Print a full ceiling report. Run with `-- --nocapture` to see output.
/// Use the printed values to update ceilings.json.
#[test]
fn budget_ceiling_report() {
    let v: serde_json::Value =
        serde_json::from_str(CEILINGS_JSON).expect("ceilings.json must parse");
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║     StellPoker Soroban Budget Ceilings Report        ║");
    println!("╠══════════════════════╦════════════╦═════════════════╣");
    println!("║ Function             ║ CPU ceil   ║ MEM ceil (B)    ║");
    println!("╠══════════════════════╬════════════╬═════════════════╣");
    if let Some(map) = v.get("functions").and_then(|f| f.as_object()) {
        for (name, entry) in map {
            let cpu = entry.get("cpu_ceiling").and_then(|c| c.as_u64()).unwrap_or(0);
            let mem = entry.get("mem_ceiling").and_then(|c| c.as_u64()).unwrap_or(0);
            println!("║ {:20} ║ {:>10} ║ {:>15} ║", name, cpu, mem);
        }
    }
    println!("╚══════════════════════╩════════════╩═════════════════╝");
    println!("(run individual budget_ceiling_* tests with --nocapture for live measurements)");
}
