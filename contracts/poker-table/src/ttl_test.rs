//! Storage TTL policy tests (issue #547).
//!
//! Each test drives a public entry point and then reads the TTL the SDK
//! reports for the entry it wrote, checking it matches the policy in
//! `ttl.rs`.

#![cfg(test)]

extern crate std;

use crate::history::HAND_HISTORY_CAPACITY;
use crate::ttl;
use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{
        storage::{Instance as _, Persistent as _},
        Address as _,
    },
    token::StellarAssetClient,
    Address, Bytes, BytesN, Env, Vec,
};

#[contract]
pub struct TtlHubMock;

#[contractimpl]
impl TtlHubMock {
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
    token_admin: StellarAssetClient<'a>,
    committee: Address,
    table_id: u32,
    players: std::vec::Vec<Address>,
    seqs: std::vec::Vec<u32>,
}

fn setup() -> T<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    let contract_id = env.register(PokerTableContract, ());
    let client = PokerTableContractClient::new(&env, &contract_id);
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token_admin = StellarAssetClient::new(&env, &sac.address());
    let admin = Address::generate(&env);
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
        game_hub: env.register(TtlHubMock, ()),
        rake_bps: 0,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
    };
    let table_id = client.create_table(&admin, &config);

    let mut t = T {
        env,
        client,
        token_admin,
        committee,
        table_id,
        players: std::vec::Vec::new(),
        seqs: std::vec::Vec::new(),
    };
    for _ in 0..2 {
        let p = Address::generate(&t.env);
        t.token_admin.mint(&p, &5_000);
        t.client.join_table(&t.table_id, &p, &5_000);
        t.players.push(p);
        t.seqs.push(0);
    }
    t
}

fn deal(t: &T) {
    let mut commitments: Vec<BytesN<32>> = Vec::new(&t.env);
    let mut indices: Vec<u32> = Vec::new(&t.env);
    for i in 0..t.players.len() as u32 {
        commitments.push_back(BytesN::from_array(&t.env, &[2u8; 32]));
        indices.push_back(i * 2);
        indices.push_back(i * 2 + 1);
    }
    t.client.commit_deal(
        &t.table_id,
        &t.committee,
        &BytesN::from_array(&t.env, &[1u8; 32]),
        &commitments,
        &indices,
        &Bytes::new(&t.env),
        &Bytes::new(&t.env),
    );
}

/// Play one hand that ends when the first player to act folds.
fn play_fold_hand(t: &mut T) {
    t.client.start_hand(&t.table_id);
    deal(t);
    let table = t.client.get_table(&t.table_id);
    let seat = table.current_turn as usize;
    t.seqs[seat] += 1;
    t.client
        .player_action(&t.table_id, &t.players[seat], &t.seqs[seat], &Action::Fold);
}

fn persistent_ttl(t: &T, key: &DataKey) -> u32 {
    t.env.as_contract(&t.client.address, || {
        t.env.storage().persistent().get_ttl(key)
    })
}

fn persistent_has(t: &T, key: &DataKey) -> bool {
    t.env
        .as_contract(&t.client.address, || t.env.storage().persistent().has(key))
}

#[test]
fn policies_are_ordered() {
    assert!(ttl::TABLE.threshold < ttl::TABLE.extend);
    assert!(ttl::HAND.threshold < ttl::HAND.extend);
    assert_eq!(ttl::HISTORY, ttl::TABLE);
    assert!(ttl::HAND.extend < ttl::TABLE.extend);
}

#[test]
fn table_and_instance_use_table_policy() {
    let t = setup();
    assert_eq!(
        persistent_ttl(&t, &DataKey::Table(t.table_id)),
        ttl::TABLE.extend
    );
    let instance_ttl = t
        .env
        .as_contract(&t.client.address, || t.env.storage().instance().get_ttl());
    assert_eq!(instance_ttl, ttl::TABLE.extend);
}

#[test]
fn action_counter_uses_table_policy() {
    let mut t = setup();
    play_fold_hand(&mut t);
    let table = t.client.get_table(&t.table_id);
    let folder = t
        .players
        .iter()
        .zip(t.seqs.iter())
        .find(|(_, seq)| **seq > 0)
        .map(|(p, _)| p.clone())
        .unwrap();
    assert_eq!(
        persistent_ttl(&t, &DataKey::PlayerActionCounter(table.id, folder)),
        ttl::TABLE.extend
    );
}

#[test]
fn archived_hand_and_meta_use_history_policy() {
    let mut t = setup();
    play_fold_hand(&mut t);
    assert_eq!(
        persistent_ttl(&t, &DataKey::HandRecord(t.table_id, 0)),
        ttl::HISTORY.extend
    );
    assert_eq!(
        persistent_ttl(&t, &DataKey::HandHistoryMeta(t.table_id)),
        ttl::HISTORY.extend
    );
}

#[test]
fn action_commitment_uses_hand_policy() {
    let t = setup();
    t.client.start_hand(&t.table_id);
    deal(&t);
    let table = t.client.get_table(&t.table_id);
    let seat = table.current_turn;
    let hash = Bytes::from_array(&t.env, &[7u8; 32]);
    t.client.commit_action(
        &t.table_id,
        &t.players[seat as usize],
        &hash,
        &Bytes::from_array(&t.env, &[8u8; 32]),
    );
    assert_eq!(
        persistent_ttl(
            &t,
            &DataKey::ActionCommitmentHash(t.table_id, table.hand_number, seat)
        ),
        ttl::HAND.extend
    );
}

/// Old hands live only in the circular history buffer: archiving past its
/// capacity reuses slots instead of adding new keys.
#[test]
fn old_hands_are_kept_only_in_the_history_buffer() {
    let mut t = setup();
    for _ in 0..(HAND_HISTORY_CAPACITY + 2) {
        play_fold_hand(&mut t);
    }

    for slot in 0..HAND_HISTORY_CAPACITY {
        assert!(persistent_has(&t, &DataKey::HandRecord(t.table_id, slot)));
    }
    assert!(!persistent_has(
        &t,
        &DataKey::HandRecord(t.table_id, HAND_HISTORY_CAPACITY)
    ));

    let meta = t.client.get_hand_history_meta(&t.table_id);
    assert_eq!(meta.stored, HAND_HISTORY_CAPACITY);
    assert_eq!(meta.total_archived, HAND_HISTORY_CAPACITY + 2);
}
