//! Checks emitted events against the schemas in `docs/events/` (issue #546).
//!
//! The lifecycle test plays a showdown hand and a fold-win hand, decodes every
//! event the contract emits with [`crate::event_schema`], and fails on any
//! event that does not match its schema or any schema that was never emitted.

#![cfg(test)]

extern crate std;

use crate::event_schema::{decode_event, event_name, json_type_for, schema_for, SCHEMAS};
use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use serde_json::Value;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events as _},
    token::StellarAssetClient,
    Address, Bytes, BytesN, Env, Vec,
};
use std::collections::BTreeSet;
use std::string::String;

#[contract]
pub struct EventHubMock;

#[contractimpl]
impl EventHubMock {
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

struct T<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    committee: Address,
    table_id: u32,
    players: std::vec::Vec<Address>,
    seqs: std::vec::Vec<u32>,
    seen: BTreeSet<String>,
}

impl T<'_> {
    /// Decode every event from the call that just ran.
    fn capture(&mut self) {
        let events = self
            .env
            .events()
            .all()
            .filter_by_contract(&self.client.address);
        for event in events.events() {
            match decode_event(event) {
                Ok(Some(_)) => {
                    self.seen.insert(event_name(event).unwrap());
                }
                Ok(None) => {}
                Err(msg) => panic!("event does not match its schema: {msg}"),
            }
        }
    }

    fn act(&mut self, choose: impl Fn(&TableState) -> Action) {
        let table = self.client.get_table(&self.table_id);
        let seat = table.current_turn as usize;
        let action = choose(&table);
        self.seqs[seat] += 1;
        self.client.player_action(
            &self.table_id,
            &self.players[seat],
            &self.seqs[seat],
            &action,
        );
        self.capture();
    }

    fn phase(&self) -> GamePhase {
        self.client.get_table(&self.table_id).phase
    }

    fn call_or_check_round(&mut self) {
        while matches!(
            self.phase(),
            GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River
        ) {
            self.act(call_or_check);
        }
    }

    fn deal(&mut self) {
        let n = self.players.len() as u32;
        let mut commitments: Vec<BytesN<32>> = Vec::new(&self.env);
        let mut indices: Vec<u32> = Vec::new(&self.env);
        for i in 0..n {
            commitments.push_back(BytesN::from_array(&self.env, &[2u8; 32]));
            indices.push_back(i * 2);
            indices.push_back(i * 2 + 1);
        }
        self.client.commit_deal(
            &self.table_id,
            &self.committee,
            &BytesN::from_array(&self.env, &[1u8; 32]),
            &commitments,
            &indices,
            &Bytes::new(&self.env),
            &Bytes::new(&self.env),
        );
        self.capture();
    }

    fn reveal(&mut self, first_index: u32, count: u32) {
        let mut cards: Vec<u32> = Vec::new(&self.env);
        let mut indices: Vec<u32> = Vec::new(&self.env);
        for i in first_index..first_index + count {
            cards.push_back(20 + i);
            indices.push_back(i);
        }
        self.client.reveal_board(
            &self.table_id,
            &self.committee,
            &cards,
            &indices,
            &Bytes::new(&self.env),
            &Bytes::new(&self.env),
        );
        self.capture();
    }

    fn showdown(&mut self) {
        let table = self.client.get_table(&self.table_id);
        let mut public_inputs = Bytes::new(&self.env);
        for _ in 0..(27 * 32) {
            public_inputs.push_back(0);
        }
        let mut hole_cards: Vec<(u32, u32)> = Vec::new(&self.env);
        let mut salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&self.env);
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
                BytesN::from_array(&self.env, &[0u8; 32]),
                BytesN::from_array(&self.env, &[0u8; 32]),
            ));
        }
        write_u32_field(&mut public_inputs, 25, winner.unwrap());
        write_u32_field(&mut public_inputs, 26, 0);
        self.client.submit_showdown(
            &self.table_id,
            &self.committee,
            &hole_cards,
            &salts,
            &Bytes::new(&self.env),
            &public_inputs,
            &Vec::new(&self.env),
        );
        self.capture();
    }
}

fn call_or_check(table: &TableState) -> Action {
    let mut max_bet = 0i128;
    for i in 0..table.players.len() {
        max_bet = max_bet.max(table.players.get(i).unwrap().bet_this_round);
    }
    if table
        .players
        .get(table.current_turn)
        .unwrap()
        .bet_this_round
        < max_bet
    {
        Action::Call
    } else {
        Action::Check
    }
}

fn write_u32_field(bytes: &mut Bytes, field_index: u32, value: u32) {
    let start = field_index * 32 + 28;
    bytes.set(start, ((value >> 24) & 0xff) as u8);
    bytes.set(start + 1, ((value >> 16) & 0xff) as u8);
    bytes.set(start + 2, ((value >> 8) & 0xff) as u8);
    bytes.set(start + 3, (value & 0xff) as u8);
}

fn setup() -> T<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    let contract_id = env.register(PokerTableContract, ());
    let client = PokerTableContractClient::new(&env, &contract_id);
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token_admin = StellarAssetClient::new(&env, &sac.address());
    let committee = Address::generate(&env);

    let config = TableConfig {
        token: sac.address(),
        min_buy_in: 100,
        max_buy_in: 5_000,
        betting_structure: BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: committee.clone(),
        verifier: env.register(crate::verifier::ZkVerifierContract, ()),
        game_hub: env.register(EventHubMock, ()),
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
    let table_id = client.create_table(&Address::generate(&env), &config);

    let mut t = T {
        env,
        client,
        committee,
        table_id,
        players: std::vec::Vec::new(),
        seqs: std::vec::Vec::new(),
        seen: BTreeSet::new(),
    };
    t.capture();

    for _ in 0..3 {
        let p = Address::generate(&t.env);
        token_admin.mint(&p, &1_000);
        t.client.join_table(&t.table_id, &p, &1_000);
        t.capture();
        t.players.push(p);
        t.seqs.push(0);
    }
    t
}

#[test]
fn emitted_events_match_their_schemas() {
    let mut t = setup();

    // Hand 1: a bet on the flop, then call and check down to a showdown.
    t.client.start_hand(&t.table_id);
    t.capture();
    t.deal();
    t.call_or_check_round();
    t.reveal(6, 3);
    t.act(|_| Action::Bet(10));
    t.call_or_check_round();
    t.reveal(9, 1);
    t.call_or_check_round();
    t.reveal(10, 1);
    t.call_or_check_round();
    assert_eq!(t.phase(), GamePhase::Showdown);
    t.showdown();

    // Hand 2: a raise that everyone folds to, so the pot is big enough to rake.
    t.client.start_hand(&t.table_id);
    t.capture();
    t.deal();
    t.act(|_| Action::Raise(100));
    t.act(|_| Action::Fold);
    t.act(|_| Action::Fold);
    assert_eq!(t.phase(), GamePhase::Settlement);

    let leaver = t.players[0].clone();
    t.client.leave_table(&t.table_id, &leaver);
    t.capture();

    for (name, _) in SCHEMAS {
        assert!(t.seen.contains(*name), "no {name} event was emitted");
    }
}

fn check_node(node: &Value, path: &str) {
    if let Some(options) = node.get("oneOf").and_then(Value::as_array) {
        for (i, option) in options.iter().enumerate() {
            check_node(option, &std::format!("{path}.oneOf[{i}]"));
        }
        return;
    }
    let kind = node["x-soroban"]
        .as_str()
        .unwrap_or_else(|| panic!("{path}: no x-soroban tag"));
    let expected = json_type_for(kind).unwrap_or_else(|| panic!("{path}: unknown tag {kind}"));
    assert_eq!(node["type"].as_str(), Some(expected), "{path}: JSON type");
    if kind == "vec" {
        check_node(&node["items"], &std::format!("{path}.items"));
    }
    if kind == "tuple" {
        let prefix = node["prefixItems"].as_array().unwrap();
        assert_eq!(
            node["minItems"].as_u64(),
            Some(prefix.len() as u64),
            "{path}"
        );
        assert_eq!(
            node["maxItems"].as_u64(),
            Some(prefix.len() as u64),
            "{path}"
        );
        for (i, child) in prefix.iter().enumerate() {
            check_node(child, &std::format!("{path}[{i}]"));
        }
    }
}

/// The JSON Schema keywords and the `x-soroban` tags in each file agree, and
/// each file is named after the event it describes.
#[test]
fn schemas_are_consistent() {
    for (name, _) in SCHEMAS {
        let schema = schema_for(name).unwrap();
        assert_eq!(schema["title"].as_str(), Some(*name));
        let topics = &schema["properties"]["topics"];
        assert_eq!(topics["prefixItems"][0]["const"].as_str(), Some(*name));
        check_node(topics, &std::format!("{name}.topics"));
        check_node(&schema["properties"]["data"], &std::format!("{name}.data"));
    }
}
