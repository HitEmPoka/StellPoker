//! Rake configuration history: recording, paging and point-in-time lookup
//! (issue #559).

#![cfg(test)]

extern crate std;

use crate::rake_history::{RakeChange, MAX_PAGE_SIZE};
use crate::state_machine_test::GameHubContract;
use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token::StellarAssetClient,
    Address, Env,
};
use std::format;

struct Fixture<'a> {
    env: Env,
    client: PokerTableContractClient<'a>,
    admin: Address,
    table_id: u32,
}

fn config(env: &Env, admin: &Address, rake_bps: u32) -> TableConfig {
    let sac = env.register_stellar_asset_contract_v2(Address::generate(env));
    let _ = StellarAssetClient::new(env, &sac.address());
    TableConfig {
        token: sac.address(),
        min_buy_in: 100,
        max_buy_in: 1_000,
        betting_structure: BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: admin.clone(),
        verifier: env.register(crate::verifier::ZkVerifierContract, ()),
        game_hub: env.register(GameHubContract, ()),
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

impl Fixture<'_> {
    fn new(initial_bps: u32) -> Fixture<'static> {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|l| {
            l.timestamp = 1_000;
            l.sequence_number = 10;
        });
        let client = PokerTableContractClient::new(&env, &env.register(PokerTableContract, ()));
        let admin = Address::generate(&env);
        let table_id = client.create_table(&admin, &config(&env, &admin, initial_bps));
        Fixture {
            env,
            client,
            admin,
            table_id,
        }
    }

    fn at(&self, timestamp: u64, sequence: u32) {
        self.env.ledger().with_mut(|l| {
            l.timestamp = timestamp;
            l.sequence_number = sequence;
        });
    }

    fn history(&self) -> std::vec::Vec<RakeChange> {
        let page = self
            .client
            .get_rake_history(&self.table_id, &0, &MAX_PAGE_SIZE);
        page.iter().collect()
    }
}

#[test]
fn creation_records_the_initial_rake_as_entry_zero() {
    let f = Fixture::new(250);

    assert_eq!(f.client.get_rake_history_len(&f.table_id), 1);
    assert_eq!(
        f.history(),
        std::vec![RakeChange {
            index: 0,
            rake_bps: 250,
            previous_bps: 250,
            effective_at_timestamp: 1_000,
            effective_at_ledger: 10,
            changed_by: f.admin.clone(),
        }]
    );
}

#[test]
fn a_change_appends_an_entry_with_the_time_it_took_effect() {
    let f = Fixture::new(250);
    f.at(5_000, 400);
    f.client.set_rake_bps(&f.table_id, &400);

    let history = f.history();
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[1],
        RakeChange {
            index: 1,
            rake_bps: 400,
            previous_bps: 250,
            effective_at_timestamp: 5_000,
            effective_at_ledger: 400,
            changed_by: f.admin.clone(),
        }
    );
    // The earlier entry is untouched, and the live config agrees with the tail.
    assert_eq!(history[0].rake_bps, 250);
    assert_eq!(f.client.get_table(&f.table_id).config.rake_bps, 400);
}

#[test]
fn a_change_emits_a_rake_config_changed_event() {
    let f = Fixture::new(0);
    f.at(2_000, 20);
    f.client.set_rake_bps(&f.table_id, &100);

    let events = format!("{:?}", f.env.events().all());
    assert!(events.contains("rake_config_changed"), "{events}");
    // The pre-existing event is still emitted for current consumers.
    assert!(events.contains("rake_bps_updated"), "{events}");
}

#[test]
fn setting_the_same_rake_records_nothing() {
    let f = Fixture::new(250);
    f.at(3_000, 30);
    f.client.set_rake_bps(&f.table_id, &250);

    assert_eq!(f.client.get_rake_history_len(&f.table_id), 1);
}

#[test]
fn a_rejected_rake_is_not_recorded() {
    let f = Fixture::new(250);
    assert!(f
        .client
        .try_set_rake_bps(&f.table_id, &(crate::pot::MAX_RAKE_BPS + 1))
        .is_err());

    assert_eq!(f.client.get_rake_history_len(&f.table_id), 1);
    assert_eq!(f.client.get_table(&f.table_id).config.rake_bps, 250);
}

#[test]
fn rake_at_a_timestamp_is_the_latest_change_at_or_before_it() {
    let f = Fixture::new(100); // effective from t=1000
    f.at(2_000, 20);
    f.client.set_rake_bps(&f.table_id, &200);
    f.at(3_000, 30);
    f.client.set_rake_bps(&f.table_id, &300);
    f.at(4_000, 40);
    f.client.set_rake_bps(&f.table_id, &0);

    let at = |t: u64| f.client.get_rake_bps_at(&f.table_id, &t);
    assert_eq!(at(999), None, "before the table existed");
    assert_eq!(at(1_000), Some(100));
    assert_eq!(at(1_999), Some(100));
    assert_eq!(
        at(2_000),
        Some(200),
        "a change is effective at its own timestamp"
    );
    assert_eq!(at(2_999), Some(200));
    assert_eq!(at(3_000), Some(300));
    assert_eq!(at(3_999), Some(300));
    assert_eq!(at(4_000), Some(0));
    assert_eq!(at(u64::MAX), Some(0));
}

#[test]
fn several_changes_in_one_ledger_resolve_to_the_last_one() {
    let f = Fixture::new(100);
    f.at(2_000, 20);
    f.client.set_rake_bps(&f.table_id, &200);
    f.client.set_rake_bps(&f.table_id, &300);

    assert_eq!(f.client.get_rake_history_len(&f.table_id), 3);
    assert_eq!(f.client.get_rake_bps_at(&f.table_id, &2_000), Some(300));
}

#[test]
fn history_is_paged_oldest_first_and_capped() {
    let f = Fixture::new(0);
    // 60 changes: entry 0 plus 60 more = 61 entries, more than one page.
    for i in 1..=60u32 {
        f.at(1_000 + u64::from(i), 10 + i);
        f.client.set_rake_bps(&f.table_id, &(i % 500 + 1));
    }
    let total = f.client.get_rake_history_len(&f.table_id);
    assert_eq!(total, 61);

    let first = f.client.get_rake_history(&f.table_id, &0, &1_000);
    assert_eq!(first.len(), MAX_PAGE_SIZE, "an oversized limit is capped");
    assert_eq!(first.get(0).unwrap().index, 0);
    assert_eq!(
        first.get(MAX_PAGE_SIZE - 1).unwrap().index,
        MAX_PAGE_SIZE - 1
    );

    let second = f
        .client
        .get_rake_history(&f.table_id, &MAX_PAGE_SIZE, &MAX_PAGE_SIZE);
    assert_eq!(second.len(), total - MAX_PAGE_SIZE);
    assert_eq!(second.get(0).unwrap().index, MAX_PAGE_SIZE);
    assert_eq!(second.get(second.len() - 1).unwrap().index, 60);

    assert_eq!(f.client.get_rake_history(&f.table_id, &total, &10).len(), 0);
    assert_eq!(
        f.client
            .get_rake_history(&f.table_id, &u32::MAX, &u32::MAX)
            .len(),
        0
    );
    assert_eq!(f.client.get_rake_history(&f.table_id, &5, &0).len(), 0);
}

#[test]
fn point_in_time_lookup_stays_correct_over_a_long_history() {
    let f = Fixture::new(1);
    for i in 1..=40u32 {
        f.at(1_000 + u64::from(i) * 10, 10 + i);
        f.client.set_rake_bps(&f.table_id, &(i + 1));
    }
    for i in 0..=40u32 {
        let t = 1_000 + u64::from(i) * 10;
        assert_eq!(f.client.get_rake_bps_at(&f.table_id, &t), Some(i + 1));
        assert_eq!(f.client.get_rake_bps_at(&f.table_id, &(t + 9)), Some(i + 1));
    }
}

#[test]
fn history_views_reject_an_unknown_table() {
    let f = Fixture::new(0);
    assert!(f.client.try_get_rake_history(&99, &0, &10).is_err());
    assert!(f.client.try_get_rake_history_len(&99).is_err());
    assert!(f.client.try_get_rake_bps_at(&99, &0).is_err());
}

#[test]
fn a_table_that_predates_history_keeps_its_old_rake_as_entry_zero() {
    let f = Fixture::new(250);
    // Simulate a table created before this feature: drop the creation entry.
    f.env.as_contract(&f.client.address, || {
        f.env
            .storage()
            .persistent()
            .remove(&DataKey::RakeHistoryLen(f.table_id));
        f.env
            .storage()
            .persistent()
            .remove(&DataKey::RakeHistory(f.table_id, 0));
    });
    assert_eq!(f.client.get_rake_history_len(&f.table_id), 0);
    assert_eq!(f.client.get_rake_bps_at(&f.table_id, &5_000), None);

    f.at(5_000, 50);
    f.client.set_rake_bps(&f.table_id, &400);

    let history = f.history();
    assert_eq!(history.len(), 2);
    // Entry 0 stands for everything before the first recorded change (time 0).
    assert_eq!(
        (history[0].rake_bps, history[0].effective_at_timestamp),
        (250, 0)
    );
    assert_eq!((history[1].rake_bps, history[1].previous_bps), (400, 250));
    assert_eq!(f.client.get_rake_bps_at(&f.table_id, &4_999), Some(250));
    assert_eq!(f.client.get_rake_bps_at(&f.table_id, &5_000), Some(400));
}

#[test]
fn a_change_is_also_logged_in_the_config_version_history() {
    let f = Fixture::new(0);
    assert_eq!(f.client.get_config_version(&f.table_id), 0);
    f.at(2_000, 20);
    f.client.set_rake_bps(&f.table_id, &100);
    assert_eq!(f.client.get_config_version(&f.table_id), 1);
    // A no-op change does not bump the version.
    f.client.set_rake_bps(&f.table_id, &100);
    assert_eq!(f.client.get_config_version(&f.table_id), 1);
}
