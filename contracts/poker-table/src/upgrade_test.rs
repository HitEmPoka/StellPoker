//! Tests for the admin-only propose/execute/cancel contract-upgrade
//! mechanism: minimum timelock delay enforcement, and the commit-reveal
//! hash guarantee (execute always uses the hash committed at propose time).

#[cfg(test)]
mod upgrade_test {
    use crate::types::*;
    use crate::{PokerTableContract, PokerTableContractClient};
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger as _},
        token::StellarAssetClient,
        Address, BytesN, Env,
    };

    #[contract]
    pub struct GameHubContract;

    #[contractimpl]
    impl GameHubContract {
        pub fn start_game(
            _env: Env,
            _game_id: Address,
            _session_id: u32,
            _p1: Address,
            _p2: Address,
            _p1_pts: i128,
            _p2_pts: i128,
        ) {
        }
        pub fn end_game(_env: Env, _session_id: u32, _p1_won: bool) {}
    }

    struct Setup<'a> {
        env: Env,
        client: PokerTableContractClient<'a>,
        table_id: u32,
    }

    fn setup() -> Setup<'static> {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract_id);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let _token_admin_client = StellarAssetClient::new(&env, &sac.address());

        let admin = Address::generate(&env);
        let committee = Address::generate(&env);
        let verifier = env.register(crate::verifier::ZkVerifierContract, ());
        let game_hub = env.register(GameHubContract, ());

        let config = TableConfig {
            token: sac.address(),
            min_buy_in: 100,
            max_buy_in: 1000,
            betting_structure: crate::types::BettingStructure::NoLimit,
            blinds_schedule: BlindsSchedule::fixed(&env, 5, 10),
            min_players: 2,
            max_players: 6,
            timeout_ledgers: 100,
            committee,
            verifier,
            game_hub,
            rake_bps: 0,
            max_rebuys: 0,
            jackpot_rake_share_bps: 0,
            min_bad_beat_category: 7,
            min_bad_beat_rank: 12,
        };
        let table_id = client.create_table(&admin, &config);

        Setup {
            env,
            client,
            table_id,
        }
    }

    fn fake_hash(env: &Env, seed: u8) -> BytesN<32> {
        BytesN::from_array(env, &[seed; 32])
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #49)")] // UpgradeDelayTooShort
    fn propose_upgrade_rejects_delay_below_minimum() {
        let s = setup();
        let hash = fake_hash(&s.env, 7);
        // One day minus one second — just under MIN_UPGRADE_DELAY_SECONDS.
        s.client.propose_upgrade(&s.table_id, &hash, &86_399);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #47)")] // NoUpgradeProposal
    fn execute_upgrade_rejects_without_proposal() {
        let s = setup();
        s.client.execute_upgrade(&s.table_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #47)")] // NoUpgradeProposal
    fn cancel_upgrade_rejects_without_proposal() {
        let s = setup();
        s.client.cancel_upgrade(&s.table_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #48)")] // UpgradeDelayNotElapsed
    fn execute_upgrade_rejects_before_delay_elapses() {
        let s = setup();
        let hash = fake_hash(&s.env, 7);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);
        // No time has passed — must not be executable yet.
        s.client.execute_upgrade(&s.table_id);
    }

    #[test]
    fn execute_upgrade_rejects_right_up_until_the_delay_boundary() {
        let s = setup();
        let hash = fake_hash(&s.env, 7);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        // One second before the deadline: still not executable.
        s.env.ledger().set_timestamp(86_399);
        let result = s.client.try_execute_upgrade(&s.table_id);
        assert!(result.is_err());
    }

    #[test]
    fn propose_then_cancel_clears_the_pending_proposal() {
        let s = setup();
        let hash = fake_hash(&s.env, 3);
        s.client.propose_upgrade(&s.table_id, &hash, &90_000);

        let proposal = s.client.get_upgrade_proposal(&s.table_id).unwrap();
        assert_eq!(proposal.new_wasm_hash, hash);
        assert_eq!(proposal.execute_after, 90_000);

        s.client.cancel_upgrade(&s.table_id);
        assert_eq!(s.client.get_upgrade_proposal(&s.table_id), None);
    }

    #[test]
    fn a_new_proposal_replaces_the_previous_one() {
        let s = setup();
        let hash_a = fake_hash(&s.env, 1);
        let hash_b = fake_hash(&s.env, 2);

        s.client.propose_upgrade(&s.table_id, &hash_a, &90_000);
        s.client.propose_upgrade(&s.table_id, &hash_b, &100_000);

        let proposal = s.client.get_upgrade_proposal(&s.table_id).unwrap();
        // The later proposal's hash and delay win — no trace of hash_a's
        // commitment lingers for `execute_upgrade` to fall back on.
        assert_eq!(proposal.new_wasm_hash, hash_b);
        assert_eq!(proposal.execute_after, 100_000);
    }

    #[test]
    fn get_upgrade_proposal_is_none_when_nothing_pending() {
        let s = setup();
        assert_eq!(s.client.get_upgrade_proposal(&s.table_id), None);
    }

    // ─── revert_last_upgrade (issue #348) ─────────────────────────────────

    /// Propose, fast-forward past the delay, and execute — the shared setup
    /// for the revert tests below.
    fn propose_and_execute(s: &Setup, hash: &BytesN<32>) {
        s.client.propose_upgrade(&s.table_id, hash, &86_400);
        s.env.ledger().set_timestamp(86_400);
        s.client.execute_upgrade(&s.table_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #70)")] // NoUpgradeToRevert
    fn revert_rejects_when_no_upgrade_has_ever_executed() {
        let s = setup();
        s.client.revert_last_upgrade(&s.table_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #70)")] // NoUpgradeToRevert
    fn revert_rejects_after_the_first_ever_executed_upgrade() {
        // The very first upgrade tracked by this mechanism has no recorded
        // "previous" hash to revert to — its genesis wasm was never
        // recorded on-chain.
        let s = setup();
        let hash = fake_hash(&s.env, 1);
        propose_and_execute(&s, &hash);

        s.client.revert_last_upgrade(&s.table_id);
    }

    #[test]
    fn revert_restores_the_hash_from_before_the_most_recent_upgrade() {
        let s = setup();
        let hash_a = fake_hash(&s.env, 1);
        let hash_b = fake_hash(&s.env, 2);

        propose_and_execute(&s, &hash_a);

        // Second upgrade: propose again (from the new post-upgrade
        // timestamp) and execute once its own delay has elapsed.
        s.client.propose_upgrade(&s.table_id, &hash_b, &86_400);
        s.env.ledger().set_timestamp(86_400 + 86_400);
        s.client.execute_upgrade(&s.table_id);

        let record = s.client.get_last_upgrade(&s.table_id).unwrap();
        assert_eq!(record.new_wasm_hash, hash_b);
        assert_eq!(record.previous_wasm_hash, Some(hash_a));

        // Reverting the second upgrade must succeed now that there's a
        // recorded previous hash to fall back to.
        s.client.revert_last_upgrade(&s.table_id);

        // The record is consumed on revert — no double-revert, no redo.
        assert_eq!(s.client.get_last_upgrade(&s.table_id), None);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #70)")] // NoUpgradeToRevert
    fn reverting_twice_in_a_row_fails_the_second_time() {
        let s = setup();
        let hash_a = fake_hash(&s.env, 1);
        let hash_b = fake_hash(&s.env, 2);

        propose_and_execute(&s, &hash_a);
        s.client.propose_upgrade(&s.table_id, &hash_b, &86_400);
        s.env.ledger().set_timestamp(86_400 + 86_400);
        s.client.execute_upgrade(&s.table_id);

        s.client.revert_last_upgrade(&s.table_id);
        // Nothing left to revert to — the record was consumed above, and
        // there is no "redo" of a revert.
        s.client.revert_last_upgrade(&s.table_id);
    }

    #[test]
    fn revert_succeeds_right_up_until_the_rollback_window_boundary() {
        let s = setup();
        let hash_a = fake_hash(&s.env, 1);
        let hash_b = fake_hash(&s.env, 2);

        propose_and_execute(&s, &hash_a);
        s.client.propose_upgrade(&s.table_id, &hash_b, &86_400);
        s.env.ledger().set_timestamp(86_400 + 86_400);
        s.client.execute_upgrade(&s.table_id);

        // Exactly at the boundary (ROLLBACK_WINDOW_SECONDS = 21_600) —
        // still within the window (elapsed > window is the rejection
        // condition, so elapsed == window must still succeed).
        s.env.ledger().set_timestamp(86_400 + 86_400 + 21_600);
        s.client.revert_last_upgrade(&s.table_id);

        let cfg = s.client.get_last_upgrade(&s.table_id);
        assert_eq!(cfg, None);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #71)")] // RollbackWindowExpired
    fn revert_rejects_once_the_rollback_window_has_passed() {
        let s = setup();
        let hash_a = fake_hash(&s.env, 1);
        let hash_b = fake_hash(&s.env, 2);

        propose_and_execute(&s, &hash_a);
        s.client.propose_upgrade(&s.table_id, &hash_b, &86_400);
        s.env.ledger().set_timestamp(86_400 + 86_400);
        s.client.execute_upgrade(&s.table_id);

        // One second past the window.
        s.env.ledger().set_timestamp(86_400 + 86_400 + 21_600 + 1);
        s.client.revert_last_upgrade(&s.table_id);
    }

    #[test]
    fn get_last_upgrade_is_none_when_no_upgrade_has_executed() {
        let s = setup();
        assert_eq!(s.client.get_last_upgrade(&s.table_id), None);
    }
}
