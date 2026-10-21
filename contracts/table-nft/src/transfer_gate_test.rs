//! Pending-hand safety for table ownership transfers.
//!
//! A table NFT may change hands only while the table is idle, or through the
//! explicit hand-completion gate (`queue_transfer`), which settles the transfer
//! once the in-flight hand is reported complete.

#![cfg(test)]

extern crate std;

use crate::{TableAesthetics, TableNftContract, TableNftContractClient, TableNftError, TableRules};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    xdr::{ContractEventBody, ScVal},
    Address, Env, String, Vec,
};

const START_TIME: u64 = 1_000_000;
const TABLE: u32 = 1;

struct Fixture<'a> {
    env: Env,
    client: TableNftContractClient<'a>,
    admin: Address,
    owner: Address,
    reporter: Address,
    token: Address,
}

fn default_rules(env: &Env, token: &Address) -> TableRules {
    let mut allowed = Vec::new(env);
    allowed.push_back(token.clone());
    TableRules {
        min_buy_in: 100,
        max_buy_in: 1000,
        min_players: 2,
        max_players: 6,
        rake_bps: 250,
        jackpot_share_bps: 500,
        allowed_tokens: allowed,
        action_timeout_seconds: 30,
        is_private: false,
        allow_straddle: true,
        allow_run_it_twice: true,
    }
}

fn default_aesthetics(env: &Env) -> TableAesthetics {
    TableAesthetics {
        table_name: String::from_str(env, "High Stakes Cyber Lounge"),
        felt_color: String::from_str(env, "#1A2B3C"),
        card_back_uri: String::from_str(env, "ipfs://QmCyberBack"),
        background_uri: String::from_str(env, "ipfs://QmCyberBg"),
        avatar_frame_uri: String::from_str(env, "ipfs://QmFrame"),
        soundtrack_theme: String::from_str(env, "synthwave_night"),
        custom_metadata_uri: String::from_str(env, "https://stellpoker.io/meta/1"),
    }
}

/// A deployed contract with table `TABLE` minted to `owner` and a hand
/// reporter configured.
fn setup() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START_TIME);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let reporter = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register(TableNftContract, ());
    let client = TableNftContractClient::new(&env, &contract_id);
    client.initialize(
        &admin,
        &String::from_str(&env, "StellPoker Tables"),
        &String::from_str(&env, "SPTAB"),
    );
    client.mint(
        &admin,
        &owner,
        &TABLE,
        &default_rules(&env, &token),
        &default_aesthetics(&env),
    );
    client.set_hand_reporter(&admin, &reporter);

    Fixture {
        env,
        client,
        admin,
        owner,
        reporter,
        token,
    }
}

/// True if a contract event whose first topic is `name` has been emitted.
fn emitted(env: &Env, name: &str) -> bool {
    env.events().all().events().iter().any(|event| {
        let ContractEventBody::V0(body) = &event.body;
        match body.topics.first() {
            Some(ScVal::Symbol(s)) => s.to_utf8_string_lossy() == name,
            _ => false,
        }
    })
}

// ---------------------------------------------------------------------------
// Idle path
// ---------------------------------------------------------------------------

#[test]
fn idle_table_transfers_directly() {
    let f = setup();
    let to = Address::generate(&f.env);

    assert!(!f.client.is_hand_active(&TABLE));
    f.client.transfer(&f.owner, &f.owner, &to, &TABLE);

    assert_eq!(f.client.owner_of(&TABLE), to);
    assert_eq!(f.client.balance_of(&f.owner), 0);
    assert_eq!(f.client.balance_of(&to), 1);
    assert!(emitted(&f.env, "table_transferred"));
}

#[test]
fn table_without_a_reporter_still_transfers() {
    // The gate is opt-in: a deployment that never configures a reporter keeps
    // the original transfer behavior.
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let to = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register(TableNftContract, ());
    let client = TableNftContractClient::new(&env, &contract_id);
    client.initialize(
        &admin,
        &String::from_str(&env, "StellPoker Tables"),
        &String::from_str(&env, "SPTAB"),
    );
    client.mint(
        &admin,
        &owner,
        &TABLE,
        &default_rules(&env, &token),
        &default_aesthetics(&env),
    );

    assert_eq!(client.get_hand_reporter(), None);
    client.transfer(&owner, &owner, &to, &TABLE);
    assert_eq!(client.owner_of(&TABLE), to);
}

#[test]
fn transfer_is_allowed_again_once_the_hand_completes() {
    let f = setup();
    let to = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    assert!(f.client.is_hand_active(&TABLE));
    f.client.report_hand_completed(&f.reporter, &TABLE);
    assert!(!f.client.is_hand_active(&TABLE));

    f.client.transfer(&f.owner, &f.owner, &to, &TABLE);
    assert_eq!(f.client.owner_of(&TABLE), to);
}

// ---------------------------------------------------------------------------
// Direct transfer is refused mid-hand
// ---------------------------------------------------------------------------

#[test]
fn direct_transfer_is_rejected_while_a_hand_is_in_flight() {
    let f = setup();
    let to = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);

    assert_eq!(
        f.client.try_transfer(&f.owner, &f.owner, &to, &TABLE),
        Err(Ok(TableNftError::HandInProgress))
    );
    assert_eq!(f.client.owner_of(&TABLE), f.owner);
    assert_eq!(f.client.balance_of(&f.owner), 1);
    assert_eq!(f.client.balance_of(&to), 0);
}

#[test]
fn approved_operator_cannot_bypass_the_hand_gate() {
    let f = setup();
    let operator = Address::generate(&f.env);
    let to = Address::generate(&f.env);

    f.client.approve(&f.owner, &Some(operator.clone()), &TABLE);
    f.client.report_hand_started(&f.reporter, &TABLE);

    assert_eq!(
        f.client.try_transfer(&operator, &f.owner, &to, &TABLE),
        Err(Ok(TableNftError::HandInProgress))
    );
    assert_eq!(f.client.owner_of(&TABLE), f.owner);
}

#[test]
fn a_hand_on_one_table_does_not_lock_another() {
    let f = setup();
    let other = 2u32;
    let to = Address::generate(&f.env);
    f.client.mint(
        &f.admin,
        &f.owner,
        &other,
        &default_rules(&f.env, &f.token),
        &default_aesthetics(&f.env),
    );

    f.client.report_hand_started(&f.reporter, &TABLE);

    f.client.transfer(&f.owner, &f.owner, &to, &other);
    assert_eq!(f.client.owner_of(&other), to);
    assert_eq!(f.client.owner_of(&TABLE), f.owner);
}

// ---------------------------------------------------------------------------
// Hand-completion gate
// ---------------------------------------------------------------------------

#[test]
fn queued_transfer_executes_when_the_hand_completes() {
    let f = setup();
    let to = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &to, &TABLE);

    // Nothing moves while the hand is running.
    assert_eq!(f.client.owner_of(&TABLE), f.owner);
    let pending = f.client.get_pending_transfer(&TABLE).unwrap();
    assert_eq!(pending.from, f.owner);
    assert_eq!(pending.to, to);
    assert_eq!(pending.requested_by, f.owner);
    assert_eq!(pending.requested_at, START_TIME);
    assert!(emitted(&f.env, "transfer_queued"));

    f.client.report_hand_completed(&f.reporter, &TABLE);

    assert_eq!(f.client.owner_of(&TABLE), to);
    assert_eq!(f.client.balance_of(&f.owner), 0);
    assert_eq!(f.client.balance_of(&to), 1);
    assert!(f.client.get_pending_transfer(&TABLE).is_none());
    assert!(!f.client.is_hand_active(&TABLE));
    assert!(emitted(&f.env, "queued_transfer_executed"));
    assert!(emitted(&f.env, "table_transferred"));
}

#[test]
fn queued_transfer_clears_approvals_and_listings_like_a_direct_transfer() {
    let f = setup();
    let operator = Address::generate(&f.env);
    let to = Address::generate(&f.env);

    f.client.approve(&f.owner, &Some(operator), &TABLE);
    f.client
        .list_for_rent(&f.owner, &TABLE, &500, &3_600, &86_400, &f.token);

    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &to, &TABLE);
    f.client.report_hand_completed(&f.reporter, &TABLE);

    assert_eq!(f.client.get_approved(&TABLE), None);
    assert!(f.client.get_rental_listing(&TABLE).is_none());
}

#[test]
fn approved_address_can_queue_a_transfer() {
    let f = setup();
    let operator = Address::generate(&f.env);
    let to = Address::generate(&f.env);

    f.client.approve(&f.owner, &Some(operator.clone()), &TABLE);
    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&operator, &f.owner, &to, &TABLE);
    assert_eq!(
        f.client.get_pending_transfer(&TABLE).unwrap().requested_by,
        operator
    );

    f.client.report_hand_completed(&f.reporter, &TABLE);
    assert_eq!(f.client.owner_of(&TABLE), to);
}

#[test]
fn stranger_cannot_queue_a_transfer() {
    let f = setup();
    let stranger = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    assert_eq!(
        f.client
            .try_queue_transfer(&stranger, &f.owner, &stranger, &TABLE),
        Err(Ok(TableNftError::Unauthorized))
    );
    assert!(f.client.get_pending_transfer(&TABLE).is_none());
}

#[test]
fn queueing_needs_a_hand_in_flight() {
    // An idle table should use `transfer` directly.
    let f = setup();
    let to = Address::generate(&f.env);

    assert_eq!(
        f.client.try_queue_transfer(&f.owner, &f.owner, &to, &TABLE),
        Err(Ok(TableNftError::NoHandInProgress))
    );
}

#[test]
fn only_one_transfer_can_be_queued_per_table() {
    let f = setup();
    let first = Address::generate(&f.env);
    let second = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &first, &TABLE);

    assert_eq!(
        f.client.try_queue_transfer(&f.owner, &f.owner, &second, &TABLE),
        Err(Ok(TableNftError::TransferAlreadyPending))
    );
    assert_eq!(f.client.get_pending_transfer(&TABLE).unwrap().to, first);
}

#[test]
fn cancelled_transfer_leaves_ownership_untouched() {
    let f = setup();
    let to = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &to, &TABLE);
    f.client.cancel_queued_transfer(&f.owner, &TABLE);

    assert!(f.client.get_pending_transfer(&TABLE).is_none());
    assert!(emitted(&f.env, "transfer_cancelled"));

    f.client.report_hand_completed(&f.reporter, &TABLE);
    assert_eq!(f.client.owner_of(&TABLE), f.owner);
    assert_eq!(f.client.balance_of(&to), 0);

    assert_eq!(
        f.client.try_cancel_queued_transfer(&f.owner, &TABLE),
        Err(Ok(TableNftError::NoPendingTransfer))
    );
}

#[test]
fn stranger_cannot_cancel_a_queued_transfer() {
    let f = setup();
    let to = Address::generate(&f.env);
    let stranger = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &to, &TABLE);

    assert_eq!(
        f.client.try_cancel_queued_transfer(&stranger, &TABLE),
        Err(Ok(TableNftError::Unauthorized))
    );
    assert!(f.client.get_pending_transfer(&TABLE).is_some());
}

// ---------------------------------------------------------------------------
// Leases and the gate
// ---------------------------------------------------------------------------

#[test]
fn new_leases_are_rejected_while_a_transfer_is_queued() {
    let f = setup();
    let to = Address::generate(&f.env);
    let renter = Address::generate(&f.env);

    f.client
        .list_for_rent(&f.owner, &TABLE, &500, &3_600, &86_400, &f.token);
    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &to, &TABLE);

    assert_eq!(
        f.client.try_rent_table(&renter, &TABLE, &3_600),
        Err(Ok(TableNftError::TransferPending))
    );
    assert_eq!(
        f.client
            .try_direct_lease(&f.owner, &TABLE, &renter, &3_600, &0, &f.token),
        Err(Ok(TableNftError::TransferPending))
    );
    assert!(!f.client.is_rented(&TABLE));
}

#[test]
fn rented_table_cannot_queue_a_transfer() {
    // A lease outlives the hand, so the transfer could never complete.
    let f = setup();
    let to = Address::generate(&f.env);
    let renter = Address::generate(&f.env);

    f.client
        .direct_lease(&f.owner, &TABLE, &renter, &86_400, &0, &f.token);
    f.client.report_hand_started(&f.reporter, &TABLE);

    assert_eq!(
        f.client.try_queue_transfer(&f.owner, &f.owner, &to, &TABLE),
        Err(Ok(TableNftError::CannotTransferRentedTable))
    );
}

// ---------------------------------------------------------------------------
// Reporter authority and hand bookkeeping
// ---------------------------------------------------------------------------

#[test]
fn only_the_reporter_can_report_hands() {
    let f = setup();
    let stranger = Address::generate(&f.env);

    assert_eq!(
        f.client.try_report_hand_started(&stranger, &TABLE),
        Err(Ok(TableNftError::Unauthorized))
    );

    f.client.report_hand_started(&f.reporter, &TABLE);
    assert_eq!(
        f.client.try_report_hand_completed(&stranger, &TABLE),
        Err(Ok(TableNftError::Unauthorized))
    );
    assert!(f.client.is_hand_active(&TABLE));
}

#[test]
fn only_the_admin_can_set_the_reporter() {
    let f = setup();
    let stranger = Address::generate(&f.env);

    assert_eq!(
        f.client.try_set_hand_reporter(&stranger, &stranger),
        Err(Ok(TableNftError::Unauthorized))
    );
    assert_eq!(f.client.get_hand_reporter(), Some(f.reporter.clone()));
}

#[test]
fn hand_reports_must_alternate_start_and_complete() {
    let f = setup();

    assert_eq!(
        f.client.try_report_hand_completed(&f.reporter, &TABLE),
        Err(Ok(TableNftError::NoHandInProgress))
    );

    f.client.report_hand_started(&f.reporter, &TABLE);
    assert_eq!(
        f.client.try_report_hand_started(&f.reporter, &TABLE),
        Err(Ok(TableNftError::HandAlreadyInProgress))
    );
}

#[test]
fn hands_cannot_be_reported_for_unminted_tables() {
    let f = setup();

    assert_eq!(
        f.client.try_report_hand_started(&f.reporter, &99),
        Err(Ok(TableNftError::TokenNotFound))
    );
}

// ---------------------------------------------------------------------------
// Admin escape hatch
// ---------------------------------------------------------------------------

#[test]
fn admin_can_force_clear_a_stuck_hand_and_run_the_queued_transfer() {
    let f = setup();
    let to = Address::generate(&f.env);
    let stranger = Address::generate(&f.env);

    f.client.report_hand_started(&f.reporter, &TABLE);
    f.client.queue_transfer(&f.owner, &f.owner, &to, &TABLE);

    assert_eq!(
        f.client.try_force_clear_hand(&stranger, &TABLE),
        Err(Ok(TableNftError::Unauthorized))
    );

    f.client.force_clear_hand(&f.admin, &TABLE);

    assert!(!f.client.is_hand_active(&TABLE));
    assert_eq!(f.client.owner_of(&TABLE), to);
    assert!(f.client.get_pending_transfer(&TABLE).is_none());
    assert!(emitted(&f.env, "hand_force_cleared"));
}

#[test]
fn force_clear_needs_a_hand_in_flight() {
    let f = setup();

    assert_eq!(
        f.client.try_force_clear_hand(&f.admin, &TABLE),
        Err(Ok(TableNftError::NoHandInProgress))
    );
}
