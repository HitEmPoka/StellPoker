//! Property-based tests for rake calculation boundaries (issue #653).
//!
//! Rake moves real player funds, so the settlement math has to hold for every
//! legal table configuration — not just the handful of values used in example
//! tests. These properties pin down the contract's rake contract:
//!
//! * the fee is never negative and never exceeds the pot it is taken from,
//! * at any rate the fee is bounded by the maximum legal rate (`MAX_RAKE_BPS`),
//! * the fee matches the documented `floor(pot * bps / 10_000)` formula,
//! * increasing either the pot or the rate never lowers the fee,
//! * micro-stakes round down instead of collecting a chip that was not earned,
//! * a `TableConfig` whose rate exceeds `MAX_RAKE_BPS` is rejected at creation,
//!   while every rate up to and including the cap is accepted.
//!
//! The tests deliberately use the public `create_table` entrypoint for config
//! validation so the check cannot drift away from the settlement code.

#![cfg(test)]

extern crate std;
use std::format;

use crate::pot::{apply_rake, MAX_RAKE_BPS};
use crate::types::*;
use crate::{PokerTableContract, PokerTableContractClient};
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

const BPS_DENOMINATOR: i128 = 10_000;

/// Independent, overflow-safe reference for `floor(amount * bps / 10_000)`.
fn expected_rake(amount: i128, bps: u32) -> i128 {
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

fn table_config(env: &Env, admin: &Address, rake_bps: u32) -> TableConfig {
    TableConfig {
        token: admin.clone(),
        min_buy_in: 100,
        max_buy_in: 1_000,
        betting_structure: BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Rake is exact, non-negative, bounded by the pot, and bounded by the fee
    /// the maximum legal rate would take from the same pot.
    #[test]
    fn prop_rake_is_bounded_by_cap_and_spec(
        amount in 0i128..=1_000_000_000_000_000i128,
        bps in 0u32..=MAX_RAKE_BPS,
    ) {
        let env = Env::default();
        let (net, rake) = apply_rake(&env, &pot_of(&env, amount), bps).unwrap();
        let net_amount = net.get(0).unwrap().amount;

        prop_assert_eq!(rake, expected_rake(amount, bps));
        prop_assert_eq!(net_amount + rake, amount);
        prop_assert!(rake >= 0);
        prop_assert!(rake <= amount);
        prop_assert!(
            rake <= expected_rake(amount, MAX_RAKE_BPS),
            "rate {} produced more rake than the {} bps cap",
            bps,
            MAX_RAKE_BPS
        );
        prop_assert!(net_amount >= 0);
    }

    /// Raising either the pot or the rate never lowers the fee.
    #[test]
    fn prop_rake_is_monotonic_in_pot_and_rate(
        a in 0i128..=1_000_000_000_000_000i128,
        b in 0i128..=1_000_000_000_000_000i128,
        rate_a in 0u32..=MAX_RAKE_BPS,
        rate_b in 0u32..=MAX_RAKE_BPS,
    ) {
        let env = Env::default();
        let (small_pot, large_pot) = if a <= b { (a, b) } else { (b, a) };
        let (low_rate, high_rate) = if rate_a <= rate_b {
            (rate_a, rate_b)
        } else {
            (rate_b, rate_a)
        };

        let (_, rake_small) = apply_rake(&env, &pot_of(&env, small_pot), high_rate).unwrap();
        let (_, rake_large) = apply_rake(&env, &pot_of(&env, large_pot), high_rate).unwrap();
        prop_assert!(rake_small <= rake_large);

        let (_, rake_low_rate) = apply_rake(&env, &pot_of(&env, large_pot), low_rate).unwrap();
        let (_, rake_high_rate) = apply_rake(&env, &pot_of(&env, large_pot), high_rate).unwrap();
        prop_assert!(rake_low_rate <= rake_high_rate);
    }

    /// Micro-stakes always round down. A pot smaller than the rate's reciprocal
    /// pays no rake at all, and no pot pays more than `floor(amount / 20)` at
    /// the 5% cap.
    #[test]
    fn prop_micro_stakes_round_down(
        amount in 0i128..=20_000i128,
        bps in 0u32..=MAX_RAKE_BPS,
    ) {
        let env = Env::default();
        let (_, rake) = apply_rake(&env, &pot_of(&env, amount), bps).unwrap();

        prop_assert_eq!(rake, amount * bps as i128 / BPS_DENOMINATOR);
        prop_assert!(rake <= amount / 20);
        if amount < 20 && bps == MAX_RAKE_BPS {
            prop_assert_eq!(rake, 0);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Every rate inside the cap is accepted by `create_table` and stored
    /// unchanged, so settlement and configuration use the same bound.
    #[test]
    fn prop_config_inside_cap_is_accepted_and_preserved(
        bps in 0u32..=MAX_RAKE_BPS,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        let table_id = client.create_table(&admin, &table_config(&env, &admin, bps));
        let table = client.get_table(&table_id);
        prop_assert_eq!(table.config.rake_bps, bps);
    }

    /// A rate above the cap is rejected with the typed config error rather
    /// than being silently clamped (which would hide an operator mistake) or
    /// accepted (which would overcharge players).
    #[test]
    fn prop_config_above_cap_is_rejected(
        bps in (MAX_RAKE_BPS + 1)..=u32::MAX,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        let result = client.try_create_table(&admin, &table_config(&env, &admin, bps));
        match result {
            Err(Ok(err)) => prop_assert_eq!(err, PokerTableError::RakeBpsExceedsMax),
            _ => prop_assert!(
                false,
                "a rate above MAX_RAKE_BPS was not rejected with RakeBpsExceedsMax"
            ),
        }
    }
}

/// Boundary values that are easy to get wrong: zero, one, the first pot that
/// pays a chip at the cap, and pots around the 10_000 basis-point denominator.
#[test]
fn rake_cap_boundaries_are_exact() {
    let env = Env::default();
    let cases = [
        (0i128, 0i128),
        (0, 1),
        (1, 0),
        (19, 0),
        (20, 1),
        (9_999, 499),
        (10_000, 500),
        (10_001, 500),
        (19_999, 999),
        (20_000, 1_000),
    ];

    for (amount, expected) in cases {
        let (_, rake) = apply_rake(&env, &pot_of(&env, amount), MAX_RAKE_BPS).unwrap();
        assert_eq!(rake, expected, "amount={amount}");
    }
}
