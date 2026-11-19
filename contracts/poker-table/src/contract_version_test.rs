#![cfg(test)]

mod contract_version_test {
    use crate::types::*;
    use crate::{PokerTableContract, PokerTableContractClient};
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger as _},
        token::StellarAssetClient,
        Address, Bytes, BytesN, Env,
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
        env.cost_estimate().budget().reset_unlimited();

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
            betting_structure: BettingStructure::NoLimit,
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
            street_time_limit: OptionalStreetTimeLimit::None,
            treasury: None,
            dead_chip_timeout_ledgers: 0,
            reclaim_period_ledgers: 0,
        };
        let table_id = client.create_table(&admin, &config);
        Setup {
            env,
            client,
            table_id,
        }
    }

    fn uploaded_hash(env: &Env) -> BytesN<32> {
        env.deployer()
            .upload_contract_wasm(Bytes::from_slice(
                env,
                include_bytes!("../tests/fixtures/poker_table.wasm"),
            ))
    }

    fn execute_at(s: &Setup, timestamp: u64) {
        s.env.ledger().set_timestamp(timestamp);
        s.client.execute_upgrade(&s.table_id);
    }

    fn execute(s: &Setup) {
        execute_at(s, 86_400);
    }

    #[test]
    fn legacy_tables_start_at_version_one() {
        let s = setup();
        assert_eq!(s.client.get_contract_version(&s.table_id), 1);
        assert_eq!(s.client.get_contract_version_record(&s.table_id), None);
        assert_eq!(s.client.get_contract_migration_window(&s.table_id), None);
    }

    #[test]
    fn plain_upgrade_increments_version_and_opens_migration_window() {
        let s = setup();
        let new_hash = uploaded_hash(&s.env);
        s.client.propose_upgrade(&s.table_id, &new_hash, &86_400);
        execute(&s);

        assert_eq!(s.client.get_contract_version(&s.table_id), 2);
        let record = s.client.get_contract_version_record(&s.table_id).unwrap();
        assert_eq!(record.version, 2);
        assert_eq!(record.wasm_hash, new_hash);

        let window = s
            .client
            .get_contract_migration_window(&s.table_id)
            .unwrap();
        assert_eq!(window.from_version, 1);
        assert_eq!(window.to_version, 2);
        assert_eq!(window.started_at, 86_400);
        assert_eq!(window.ends_at, 86_400 + 86_400);
    }

    #[test]
    fn versioned_upgrade_targets_the_committed_version() {
        let s = setup();
        let new_hash = uploaded_hash(&s.env);
        s.client
            .propose_versioned_upgrade(&s.table_id, &new_hash, &4, &86_400);
        assert_eq!(s.client.get_pending_contract_version(&s.table_id), Some(4));
        execute(&s);

        assert_eq!(s.client.get_contract_version(&s.table_id), 4);
        assert_eq!(s.client.get_pending_contract_version(&s.table_id), None);
        let window = s
            .client
            .get_contract_migration_window(&s.table_id)
            .unwrap();
        assert_eq!((window.from_version, window.to_version), (1, 4));
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #122)")]
    fn versioned_upgrade_rejects_a_stale_target() {
        let s = setup();
        s.client
            .propose_versioned_upgrade(&s.table_id, &uploaded_hash(&s.env), &1, &86_400);
    }

    #[test]
    fn migration_window_expires_and_can_be_closed_early() {
        let s = setup();
        s.client.propose_upgrade(&s.table_id, &uploaded_hash(&s.env), &86_400);
        execute(&s);

        s.env.ledger().set_timestamp(86_400 + 86_401);
        assert_eq!(s.client.get_contract_migration_window(&s.table_id), None);

        let s2 = setup();
        s2.client
            .propose_upgrade(&s2.table_id, &uploaded_hash(&s2.env), &86_400);
        execute(&s2);
        assert!(s2.client.close_contract_migration_window(&s2.table_id));
        assert_eq!(s2.client.get_contract_migration_window(&s2.table_id), None);
        assert!(!s2.client.close_contract_migration_window(&s2.table_id));
    }

    #[test]
    fn rollback_restores_previous_version_and_closes_window() {
        let s = setup();
        let hash = uploaded_hash(&s.env);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);
        execute(&s);
        assert_eq!(s.client.get_contract_version(&s.table_id), 2);

        s.client
            .propose_versioned_upgrade(&s.table_id, &hash, &3, &86_400);
        execute_at(&s, 172_800);
        assert_eq!(s.client.get_contract_version(&s.table_id), 3);

        s.client.revert_last_upgrade(&s.table_id);
        assert_eq!(s.client.get_contract_version(&s.table_id), 2);
        assert_eq!(
            s.client.get_contract_version_record(&s.table_id).unwrap().version,
            2
        );
        assert_eq!(s.client.get_contract_migration_window(&s.table_id), None);
    }
}
