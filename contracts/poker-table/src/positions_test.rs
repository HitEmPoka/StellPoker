//! Batched per-wallet position reads across tables (issue #560).

#![cfg(test)]

extern crate std;

use crate::state_machine_test::GameHubContract;
use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient, MAX_POSITIONS_BATCH};
use soroban_sdk::{
    testutils::Address as _, token::StellarAssetClient, Address, Bytes, BytesN, Env, Vec,
};

struct Fixture<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    token: StellarAssetClient<'a>,
    committee: Address,
    config: TableConfig,
}

impl Fixture<'_> {
    fn new() -> Fixture<'static> {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        let client = PokerTableContractClient::new(&env, &env.register(PokerTableContract, ()));
        let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
        let committee = Address::generate(&env);
        let config = TableConfig {
            token: sac.address(),
            min_buy_in: 100,
            max_buy_in: 1_000,
            betting_structure: BettingStructure::NoLimit,
            blinds_schedule: BlindsSchedule::fixed(&env, 5, 10),
            min_players: 2,
            max_players: 6,
            timeout_ledgers: 100,
            committee: committee.clone(),
            verifier: env.register(crate::verifier::ZkVerifierContract, ()),
            game_hub: env.register(GameHubContract, ()),
            rake_bps: 0,
            max_rebuys: 0,
            jackpot_rake_share_bps: 0,
            min_bad_beat_category: 7,
            min_bad_beat_rank: 12,
            street_time_limit: OptionalStreetTimeLimit::None,
            treasury: None,
            dead_chip_timeout_ledgers: 0,
            reclaim_period_ledgers: 0,
        };
        Fixture {
            token: StellarAssetClient::new(&env, &sac.address()),
            env,
            client,
            committee,
            config,
        }
    }

    fn table(&self) -> u32 {
        self.client
            .create_table(&Address::generate(&self.env), &self.config)
    }

    fn join(&self, table_id: u32, player: &Address, buy_in: i128) -> u32 {
        self.token.mint(player, &buy_in);
        self.client.join_table(&table_id, player, &buy_in)
    }

    fn ids(&self, ids: &[u32]) -> Vec<u32> {
        let mut v = Vec::new(&self.env);
        for id in ids {
            v.push_back(*id);
        }
        v
    }
}

#[test]
fn reports_the_position_at_every_requested_table_in_order() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let (a, b, c) = (f.table(), f.table(), f.table());
    f.join(a, &Address::generate(&f.env), 200); // someone else takes seat 0 at `a`
    assert_eq!(f.join(a, &me, 300), 1);
    assert_eq!(f.join(b, &me, 500), 0);
    assert_eq!(f.join(c, &me, 1_000), 0);

    let positions = f.client.get_player_positions(&me, &f.ids(&[c, a, b]));

    assert_eq!(positions.len(), 3);
    let pos = |i| positions.get(i).unwrap();
    assert_eq!(
        (pos(0).table_id, pos(0).stack, pos(0).seat_index),
        (c, 1_000, 0)
    );
    assert_eq!(
        (pos(1).table_id, pos(1).stack, pos(1).seat_index),
        (a, 300, 1)
    );
    assert_eq!(
        (pos(2).table_id, pos(2).stack, pos(2).seat_index),
        (b, 500, 0)
    );
    for i in 0..3 {
        let p = pos(i);
        assert!(p.exists && p.seated);
        assert_eq!(p.total_buy_in, p.stack);
        assert_eq!(p.committed, 0);
        assert_eq!(p.phase, GamePhase::Waiting);
    }
}

#[test]
fn matches_what_the_single_table_reads_report() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let ids: std::vec::Vec<u32> = (0..3).map(|_| f.table()).collect();
    for (i, id) in ids.iter().enumerate() {
        f.join(*id, &me, 200 + 100 * i as i128);
    }

    let batch = f.client.get_player_positions(&me, &f.ids(&ids));

    for (i, id) in ids.iter().enumerate() {
        let table = f.client.get_table(id);
        let seat = table
            .players
            .get(batch.get(i as u32).unwrap().seat_index)
            .unwrap();
        assert_eq!(seat.address, me);
        assert_eq!(batch.get(i as u32).unwrap().stack, seat.stack);
        let (buy_in, _rebuys) = f.client.get_player_buy_in(id, &me);
        assert_eq!(batch.get(i as u32).unwrap().total_buy_in, buy_in);
    }
}

#[test]
fn a_table_the_wallet_is_not_seated_at_gets_an_unseated_entry() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let (mine, theirs) = (f.table(), f.table());
    f.join(mine, &me, 400);
    f.join(theirs, &Address::generate(&f.env), 400);

    let positions = f.client.get_player_positions(&me, &f.ids(&[mine, theirs]));

    let other = positions.get(1).unwrap();
    assert_eq!(
        other,
        PlayerPosition {
            table_id: theirs,
            exists: true,
            seated: false,
            seat_index: 0,
            stack: 0,
            committed: 0,
            total_buy_in: 0,
            phase: GamePhase::Waiting,
            hand_number: 0,
        }
    );
    assert!(positions.get(0).unwrap().seated);
}

#[test]
fn an_unknown_table_does_not_fail_the_batch() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let real = f.table();
    f.join(real, &me, 400);

    let positions = f.client.get_player_positions(&me, &f.ids(&[999, real]));

    let missing = positions.get(0).unwrap();
    assert_eq!(
        (missing.table_id, missing.exists, missing.seated),
        (999, false, false)
    );
    assert!(positions.get(1).unwrap().seated);
}

#[test]
fn an_empty_request_returns_an_empty_list() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    assert_eq!(f.client.get_player_positions(&me, &f.ids(&[])).len(), 0);
}

#[test]
fn duplicate_ids_are_answered_one_for_one() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let t = f.table();
    f.join(t, &me, 400);

    let positions = f.client.get_player_positions(&me, &f.ids(&[t, t, t]));

    assert_eq!(positions.len(), 3);
    assert_eq!(positions.get(0), positions.get(2));
}

#[test]
fn the_batch_limit_is_inclusive_and_over_it_is_a_clear_error() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let t = f.table();
    f.join(t, &me, 400);

    let at_limit: std::vec::Vec<u32> = (0..MAX_POSITIONS_BATCH).map(|_| t).collect();
    assert_eq!(
        f.client.get_player_positions(&me, &f.ids(&at_limit)).len(),
        MAX_POSITIONS_BATCH
    );

    let over: std::vec::Vec<u32> = (0..=MAX_POSITIONS_BATCH).map(|_| t).collect();
    assert_eq!(
        f.client.try_get_player_positions(&me, &f.ids(&over)),
        Err(Ok(PokerTableError::BatchTooLarge))
    );
}

#[test]
fn mid_hand_positions_show_committed_chips_and_the_table_phase() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let other = Address::generate(&f.env);
    let t = f.table();
    f.join(t, &other, 500);
    f.join(t, &me, 500);
    f.client.start_hand(&t);

    // Heads-up, the button (seat 1 after the first rotation) posts the small blind.
    let mut commitments: Vec<BytesN<32>> = Vec::new(&f.env);
    let mut dealt: Vec<u32> = Vec::new(&f.env);
    for i in 0..2u32 {
        commitments.push_back(BytesN::from_array(&f.env, &[2u8; 32]));
        dealt.push_back(2 * i);
        dealt.push_back(2 * i + 1);
    }
    let empty = Bytes::new(&f.env);
    f.client.commit_deal(
        &t,
        &f.committee,
        &BytesN::from_array(&f.env, &[1u8; 32]),
        &commitments,
        &dealt,
        &empty,
        &empty,
    );

    let p = f
        .client
        .get_player_positions(&me, &f.ids(&[t]))
        .get(0)
        .unwrap();
    let table = f.client.get_table(&t);
    assert_eq!(p.phase, GamePhase::Preflop);
    assert_eq!(p.hand_number, 1);
    assert!(
        p.committed == 5 || p.committed == 10,
        "posted a blind: {}",
        p.committed
    );
    assert_eq!(p.stack + p.committed, 500);
    assert_eq!(table.pot, 15);
}

#[test]
fn a_seat_renumbered_after_someone_leaves_is_reported_at_its_new_index() {
    let f = Fixture::new();
    let me = Address::generate(&f.env);
    let leaver = Address::generate(&f.env);
    let t = f.table();
    f.join(t, &leaver, 300);
    f.join(t, &me, 300);
    assert_eq!(
        f.client
            .get_player_positions(&me, &f.ids(&[t]))
            .get(0)
            .unwrap()
            .seat_index,
        1
    );

    f.client.leave_table(&t, &leaver);

    assert_eq!(
        f.client
            .get_player_positions(&me, &f.ids(&[t]))
            .get(0)
            .unwrap()
            .seat_index,
        0
    );
    // And the wallet that left now reads as unseated.
    assert!(
        !f.client
            .get_player_positions(&leaver, &f.ids(&[t]))
            .get(0)
            .unwrap()
            .seated
    );
}
