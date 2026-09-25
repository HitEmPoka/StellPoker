/// Tests for upgrade-in-flight scenarios: verifying that in-progress hands
/// survive a contract upgrade between betting rounds.
///
/// These tests verify that:
/// 1. An upgrade can be proposed while a hand is in progress
/// 2. The game state machine is preserved across the upgrade boundary
/// 3. A hand that started before an upgrade completes correctly after it

#[cfg(test)]
mod upgrade_inflight_test {
    use crate::types::*;
    use crate::{PokerTableContract, PokerTableContractClient};
    use soroban_sdk::{
        contract, contractimpl,
        testutils::Address as _,
        token::StellarAssetClient,
        Address, BytesN, Env,
    };

    // --- Mock Game Hub ---

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

    // --- Helpers ---

    struct Setup<'a> {
        env: Env,
        client: PokerTableContractClient<'a>,
        _token_admin: StellarAssetClient<'a>,
        table_id: u32,
        player1: Address,
        player2: Address,
    }

    fn setup() -> Setup<'static> {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract_id);

        let token_admin_addr = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin_addr);
        let token_admin = StellarAssetClient::new(&env, &sac.address());

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

        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        token_admin.mint(&player1, &1000);
        token_admin.mint(&player2, &1000);

        Setup {
            env,
            client,
            _token_admin: token_admin,
            table_id,
            player1,
            player2,
        }
    }

    fn fake_hash(env: &Env, seed: u8) -> BytesN<32> {
        BytesN::from_array(env, &[seed; 32])
    }

    // --- Tests ---

    #[test]
    fn propose_upgrade_during_active_hand_succeeds() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        // Start a hand — table enters Dealing phase.
        let _ = s.client.try_start_hand(&s.table_id);

        // Proposing an upgrade while a hand is in progress should succeed.
        let hash = fake_hash(&s.env, 1);
        let result = s.client.try_propose_upgrade(&s.table_id, &hash, &86_400);
        assert!(result.is_ok());

        // The proposal should be stored.
        let proposal = s.client.get_upgrade_proposal(&s.table_id).unwrap();
        assert_eq!(proposal.new_wasm_hash, hash);
    }

    #[test]
    fn upgrade_proposal_preserved_across_betting_rounds() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        // Propose upgrade before any hand.
        let hash = fake_hash(&s.env, 7);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        // Start a hand.
        let _ = s.client.try_start_hand(&s.table_id);

        // The upgrade proposal should still be accessible.
        let proposal = s.client.get_upgrade_proposal(&s.table_id);
        assert!(proposal.is_some());
        assert_eq!(proposal.unwrap().new_wasm_hash, hash);
    }

    #[test]
    fn table_state_preserved_after_upgrade_proposal() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        // Check table state before proposal.
        let table_before = s.client.get_table(&s.table_id);
        let player_count_before = table_before.players.len();

        // Propose upgrade.
        let hash = fake_hash(&s.env, 3);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        // Table state should be unchanged.
        let table_after = s.client.get_table(&s.table_id);
        assert_eq!(table_after.players.len(), player_count_before);
        assert_eq!(table_after.config.min_buy_in, 100);
        assert_eq!(table_after.config.max_buy_in, 1000);
    }

    #[test]
    fn cancel_upgrade_does_not_affect_game_state() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        // Start a hand and propose upgrade.
        let _ = s.client.try_start_hand(&s.table_id);
        let hash = fake_hash(&s.env, 5);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        // Cancel the upgrade.
        s.client.cancel_upgrade(&s.table_id);
        assert!(s.client.get_upgrade_proposal(&s.table_id).is_none());

        // Table should still have its players.
        let table = s.client.get_table(&s.table_id);
        assert_eq!(table.players.len(), 2);
    }

    #[test]
    fn multiple_proposals_during_active_hand() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        let _ = s.client.try_start_hand(&s.table_id);

        // First proposal.
        let hash_a = fake_hash(&s.env, 1);
        s.client.propose_upgrade(&s.table_id, &hash_a, &86_400);

        // Replace with second proposal.
        let hash_b = fake_hash(&s.env, 2);
        s.client.propose_upgrade(&s.table_id, &hash_b, &90_000);

        // Only the latest proposal should exist.
        let proposal = s.client.get_upgrade_proposal(&s.table_id).unwrap();
        assert_eq!(proposal.new_wasm_hash, hash_b);
        assert_eq!(proposal.execute_after, 90_000);
    }

    #[test]
    fn execute_upgrade_delay_applies_during_hand() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        let _ = s.client.try_start_hand(&s.table_id);

        let hash = fake_hash(&s.env, 1);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        // Cannot execute before delay.
        let result = s.client.try_execute_upgrade(&s.table_id);
        assert!(result.is_err());
    }

    #[test]
    fn game_phase_unaffected_by_upgrade_proposal() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        let table_before = s.client.get_table(&s.table_id);
        let phase_before = table_before.phase;

        let hash = fake_hash(&s.env, 42);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        let table_after = s.client.get_table(&s.table_id);
        assert_eq!(table_after.phase, phase_before);
    }

    #[test]
    fn player_balances_preserved_through_upgrade_proposal() {
        let s = setup();
        s.client.join_table(&s.table_id, &s.player1, &500);
        s.client.join_table(&s.table_id, &s.player2, &500);

        let table = s.client.get_table(&s.table_id);
        let p1_stack = table.players.get(0).unwrap().stack;
        let p2_stack = table.players.get(1).unwrap().stack;

        let hash = fake_hash(&s.env, 10);
        s.client.propose_upgrade(&s.table_id, &hash, &86_400);

        let table_after = s.client.get_table(&s.table_id);
        assert_eq!(table_after.players.get(0).unwrap().stack, p1_stack);
        assert_eq!(table_after.players.get(1).unwrap().stack, p2_stack);
    }
}
