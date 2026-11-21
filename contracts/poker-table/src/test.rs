#[cfg(test)]
mod test {
    use crate::types::*;
    use crate::{PokerTableContract, PokerTableContractClient};
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger as _},
        token::{StellarAssetClient, TokenClient},
        Address, BytesN, Env, Vec,
    };

    // ---------------------------------------------------------------------------
    // Mock Game Hub
    //
    // The poker-table contract calls a Game Hub via `start_game`/`end_game`. The
    // real hub lives in a separate crate (`contracts/game-hub`), so tests register
    // this minimal mock that satisfies the same interface.
    // ---------------------------------------------------------------------------

    #[contract]
    pub struct GameHubContract;

    #[contractimpl]
    impl GameHubContract {
        pub fn start_game(
            _env: Env,
            _game_id: Address,
            _session_id: u32,
            _player1: Address,
            _player2: Address,
            _player1_points: i128,
            _player2_points: i128,
        ) {
        }

        /// End game.
        ///
        /// # Parameters
        /// - `_session_id`: parameter
        /// - `_player1_won`: parameter
        ///
        /// # Returns
        /// - `()`
        ///
        /// # Errors
        /// Returns an error if the operation fails.
        ///
        /// # Authorization
        /// Requires appropriate authorization.
        pub fn end_game(_env: Env, _session_id: u32, _player1_won: bool) {}
    }

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    /// Deploy a Stellar Asset Contract and mint tokens to the given addresses.
    fn create_token<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, StellarAssetClient<'a>) {
        let sac = env.register_stellar_asset_contract_v2(admin.clone());
        (
            TokenClient::new(env, &sac.address()),
            StellarAssetClient::new(env, &sac.address()),
        )
    }

    /// Build a standard table config for tests.
    fn default_config(
        env: &Env,
        token: &Address,
        committee: &Address,
        verifier: &Address,
    ) -> TableConfig {
        // Register a mock game hub contract
        let game_hub = env.register(GameHubContract, ());
        TableConfig {
            token: token.clone(),
            min_buy_in: 100,
            max_buy_in: 1000,
            betting_structure: crate::types::BettingStructure::NoLimit,
            blinds_schedule: BlindsSchedule::fixed(env, 5, 10),
            min_players: 2,
            max_players: 6,
            timeout_ledgers: 100,
            committee: committee.clone(),
            verifier: verifier.clone(),
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

    struct TestSetup<'a> {
        env: Env,
        client: PokerTableContractClient<'a>,
        token: TokenClient<'a>,
        token_admin_client: StellarAssetClient<'a>,
        admin: Address,
        committee: Address,
        verifier: Address,
    }

    /// Spin up an environment with a poker-table contract and a token contract.
    fn setup() -> TestSetup<'static> {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract_id);

        let token_admin = Address::generate(&env);
        let (token, token_admin_client) = create_token(&env, &token_admin);

        let admin = Address::generate(&env);
        let committee = Address::generate(&env);
        let verifier = env.register(crate::verifier::ZkVerifierContract, ());

        TestSetup {
            env,
            client,
            token,
            token_admin_client,
            admin,
            committee,
            verifier,
        }
    }

    /// Create a table using the default config and return its id.
    fn create_default_table(s: &TestSetup) -> u32 {
        let config = default_config(&s.env, &s.token.address, &s.committee, &s.verifier);
        s.client.create_table(&s.admin, &config)
    }

    /// Build a table config with larger blinds and a configurable rake, used
    /// by the rake tests below to make the fee clearly visible.
    fn rake_config(
        env: &Env,
        token: &Address,
        committee: &Address,
        verifier: &Address,
        rake_bps: u32,
    ) -> TableConfig {
        let game_hub = env.register(GameHubContract, ());
        TableConfig {
            token: token.clone(),
            min_buy_in: 100,
            max_buy_in: 100_000,
            betting_structure: crate::types::BettingStructure::NoLimit,
            blinds_schedule: BlindsSchedule::fixed(env, 100, 200),
            min_players: 2,
            max_players: 6,
            timeout_ledgers: 100,
            committee: committee.clone(),
            verifier: verifier.clone(),
            game_hub,
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

    /// Mint tokens, join the table, and return the assigned seat index.
    fn join_player(s: &TestSetup, table_id: u32, player: &Address, buy_in: i128) -> u32 {
        s.token_admin_client.mint(player, &buy_in);
        s.client.join_table(&table_id, player, &buy_in)
    }

    /// Helper to move a table from Dealing -> Preflop by committing a mock deal.
    fn commit_mock_deal(s: &TestSetup, table_id: u32, num_players: u32) {
        let deck_root = BytesN::from_array(&s.env, &[1u8; 32]);
        let mut commitments: Vec<BytesN<32>> = Vec::new(&s.env);
        for _ in 0..num_players {
            commitments.push_back(BytesN::from_array(&s.env, &[2u8; 32]));
        }
        let mut dealt_indices: Vec<u32> = Vec::new(&s.env);
        for i in 0..(num_players * 2) {
            dealt_indices.push_back(i);
        }
        let proof = soroban_sdk::Bytes::new(&s.env);
        let public_inputs = soroban_sdk::Bytes::new(&s.env);

        s.client.commit_deal(
            &table_id,
            &s.committee,
            &deck_root,
            &commitments,
            &dealt_indices,
            &proof,
            &public_inputs,
        );
    }

    // ---------------------------------------------------------------------------
    // 1. Create table
    // ---------------------------------------------------------------------------

    #[test]
    fn test_create_table() {
        let s = setup();
        let table_id = create_default_table(&s);
        assert_eq!(table_id, 0);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.id, 0);
        assert_eq!(table.admin, s.admin);
        assert_eq!(table.config.min_buy_in, 100);
        assert_eq!(table.config.max_buy_in, 1000);
        let level = table.config.blinds_schedule.levels.get(0).unwrap();
        assert_eq!(level.small_blind, 5);
        assert_eq!(level.big_blind, 10);
        assert_eq!(table.config.min_players, 2);
        assert_eq!(table.config.max_players, 6);
        assert_eq!(table.phase, GamePhase::Waiting);
        assert_eq!(table.players.len(), 0);
        assert_eq!(table.pot, 0);
    }

    #[test]
    fn test_create_multiple_tables() {
        let s = setup();
        let id0 = create_default_table(&s);
        let id1 = create_default_table(&s);
        let id2 = create_default_table(&s);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_export_table_state_versioned_snapshot() {
        use soroban_sdk::testutils::Address as _;
        use soroban_sdk::Vec as SdkVec;
        let s = setup();
        let table_id = create_default_table(&s);
        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 700);
        s.client.start_hand(&table_id);

        let export = s.client.export_table_state(&table_id);
        // Stable versioned format.
        assert_eq!(export.version, crate::types::TABLE_EXPORT_VERSION);
        assert_eq!(export.version, 1);
        assert_eq!(export.table_id, table_id);
        // Seated stacks, seats, phase, button and config.
        assert_eq!(export.phase, GamePhase::Dealing);
        assert_eq!(export.dealer_seat, 1);
        assert_eq!(export.hand_number, 1);
        assert_eq!(export.seats.len(), 2);
        assert_eq!(export.seats.get(0).unwrap().address, p1);
        assert_eq!(export.seats.get(0).unwrap().seat_index, 0);
        assert_eq!(export.seats.get(0).unwrap().stack, 495); // 500 - sb 5
        assert_eq!(export.seats.get(1).unwrap().address, p2);
        assert_eq!(export.seats.get(1).unwrap().stack, 690); // 700 - bb 10
        assert_eq!(export.pot, 15);
        assert_eq!(export.board_cards, SdkVec::new(&s.env));
        assert_eq!(export.config.max_players, 6);
        let level = export.config.blinds_schedule.levels.get(0).unwrap();
        assert_eq!(level.small_blind, 5);
        assert_eq!(level.big_blind, 10);
    }

    // ---------------------------------------------------------------------------
    // 2. Join table with buy-in
    // ---------------------------------------------------------------------------

    #[test]
    fn test_join_table() {
        let s = setup();
        let table_id = create_default_table(&s);

        let player = Address::generate(&s.env);
        let seat = join_player(&s, table_id, &player, 500);
        assert_eq!(seat, 0);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.players.len(), 1);

        let p = table.players.get(0).unwrap();
        assert_eq!(p.address, player);
        assert_eq!(p.stack, 500);
        assert_eq!(p.seat_index, 0);
        assert!(!p.folded);
        assert!(!p.all_in);

        // Token should have been transferred to the contract
        assert_eq!(s.token.balance(&player), 0);
        assert_eq!(s.token.balance(&s.client.address), 500);
    }

    #[test]
    fn test_join_multiple_players() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        let p3 = Address::generate(&s.env);

        assert_eq!(join_player(&s, table_id, &p1, 200), 0);
        assert_eq!(join_player(&s, table_id, &p2, 300), 1);
        assert_eq!(join_player(&s, table_id, &p3, 500), 2);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.players.len(), 3);
        assert_eq!(s.token.balance(&s.client.address), 1000);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_join_table_buy_in_too_low() {
        let s = setup();
        let table_id = create_default_table(&s);
        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 50); // min is 100
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #4)")]
    fn test_join_table_buy_in_too_high() {
        let s = setup();
        let table_id = create_default_table(&s);
        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 2000); // max is 1000
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #5)")]
    fn test_join_table_already_seated() {
        let s = setup();
        let table_id = create_default_table(&s);
        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 500);
        // Mint more so the transfer wouldn't fail first
        s.token_admin_client.mint(&player, &500);
        s.client.join_table(&table_id, &player, &500);
    }

    #[test]
    fn test_join_table_above_max_players_queues_instead_of_erroring() {
        // Joining a full table no longer errors — the player is queued
        // instead (see the waiting-list feature), with their buy-in
        // escrowed immediately so they can be auto-seated later.
        let s = setup();
        let config = TableConfig {
            max_players: 2,
            ..default_config(&s.env, &s.token.address, &s.committee, &s.verifier)
        };
        let table_id = s.client.create_table(&s.admin, &config);

        for _ in 0..2 {
            let player = Address::generate(&s.env);
            join_player(&s, table_id, &player, 500);
        }

        let extra = Address::generate(&s.env);
        let position = join_player(&s, table_id, &extra, 500);
        assert_eq!(position, 0); // first queue slot, not a seat index

        assert_eq!(s.client.get_table(&table_id).players.len(), 2);
        let queue = s.client.get_queue(&table_id);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.get(0).unwrap().player, extra);
    }

    // ---------------------------------------------------------------------------
    // 3. Start hand
    // ---------------------------------------------------------------------------

    #[test]
    fn test_start_hand() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Dealing);
        assert_eq!(table.hand_number, 1);

        // Blinds should be posted (dealer rotated to seat 1, sb=seat 0, bb=seat 1
        // for 2 players: dealer_seat = (0+1)%2 = 1, sb = (1+1)%2 = 0, bb = (1+2)%2 = 1)
        let sb_player = table.players.get(0).unwrap();
        let bb_player = table.players.get(1).unwrap();
        assert_eq!(sb_player.bet_this_round, 5); // small blind
        assert_eq!(sb_player.stack, 495);
        assert_eq!(bb_player.bet_this_round, 10); // big blind
        assert_eq!(bb_player.stack, 490);
        assert_eq!(table.pot, 15); // 5 + 10
    }

    #[test]
    fn test_start_hand_exactly_min_players_allowed() {
        let s = setup();
        let config = TableConfig {
            min_players: 3,
            max_players: 3,
            ..default_config(&s.env, &s.token.address, &s.committee, &s.verifier)
        };
        let table_id = s.client.create_table(&s.admin, &config);

        for _ in 0..3 {
            let player = Address::generate(&s.env);
            join_player(&s, table_id, &player, 500);
        }

        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Dealing);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #9)")]
    fn test_start_hand_not_enough_players() {
        let s = setup();
        let config = TableConfig {
            min_players: 3,
            max_players: 6,
            ..default_config(&s.env, &s.token.address, &s.committee, &s.verifier)
        };
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
    }

    #[test]
    fn test_admin_can_change_min_players_between_hands() {
        let s = setup();
        let table_id = create_default_table(&s);

        s.client.set_min_players(&table_id, &3);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.config.min_players, 3);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #38)")]
    fn test_admin_cannot_change_min_players_mid_hand() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);
        s.client.start_hand(&table_id);

        s.client.set_min_players(&table_id, &3);
    }

    // ---------------------------------------------------------------------------
    // 4. Full betting round (check, bet, call, fold)
    // ---------------------------------------------------------------------------

    /// Helper: set up a 3-player table and get it into Preflop betting phase.
    fn setup_preflop_3p() -> (TestSetup<'static>, u32, Address, Address, Address) {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        let p3 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);
        join_player(&s, table_id, &p3, 500);

        // Start hand -> Dealing
        s.client.start_hand(&table_id);

        // Commit deal -> Preflop
        commit_mock_deal(&s, table_id, 3);

        (s, table_id, p1, p2, p3)
    }

    #[test]
    fn test_commit_deal_transitions_to_preflop() {
        let (s, table_id, _, _, _) = setup_preflop_3p();
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Preflop);
    }

    #[test]
    fn test_player_fold() {
        let (s, table_id, _p1, _p2, _p3) = setup_preflop_3p();

        // After start_hand with 3 players:
        //   dealer_seat = 1, sb = seat 2, bb = seat 0
        //   current_turn set by commit_deal = (1+3)%3 = 1
        //   So seat 1 acts first.
        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let acting_player = table.players.get(current).unwrap();

        s.client
            .player_action(&table_id, &acting_player.address, &1u32, &Action::Fold);

        let table = s.client.get_table(&table_id);
        let folded_player = table.players.get(current).unwrap();
        assert!(folded_player.folded);
    }

    #[test]
    fn test_player_call() {
        let (s, table_id, _p1, _p2, _p3) = setup_preflop_3p();

        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let acting_player = table.players.get(current).unwrap();
        let stack_before = acting_player.stack;
        let pot_before = table.pot;

        // The current bet is the big blind (10). The acting player's bet_this_round
        // could be 0 or some blind amount depending on seat. We need to call.
        let to_call = {
            let mut max_bet: i128 = 0;
            for i in 0..table.players.len() {
                let p = table.players.get(i).unwrap();
                if p.bet_this_round > max_bet {
                    max_bet = p.bet_this_round;
                }
            }
            max_bet - acting_player.bet_this_round
        };

        s.client
            .player_action(&table_id, &acting_player.address, &1u32, &Action::Call);

        let table_after = s.client.get_table(&table_id);
        let player_after = table_after.players.get(current).unwrap();
        assert_eq!(player_after.stack, stack_before - to_call);
        assert_eq!(table_after.pot, pot_before + to_call);
    }

    #[test]
    fn test_player_bet() {
        // To test Bet, we need a situation where current_bet == 0 (post-flop).
        // In the current contract, the preflop round ends as soon as all active
        // players have matching bets (SB calls -> all at BB level -> round over).
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // 2-player: dealer_seat = 1, sb = seat 0, bb = seat 1
        // commit_deal sets current_turn = (1+3)%2 = 0 (the SB)
        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let acting = table.players.get(current).unwrap();

        // SB calls the big blind. Once bets match, round ends automatically.
        s.client
            .player_action(&table_id, &acting.address, &1u32, &Action::Call);

        // Round completes -> DealingFlop
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::DealingFlop);

        // Reveal flop
        let flop_cards: Vec<u32> = Vec::from_array(&s.env, [10, 20, 30]);
        let flop_indices: Vec<u32> = Vec::from_array(&s.env, [4, 5, 6]);
        let proof = soroban_sdk::Bytes::new(&s.env);
        let pub_inputs = soroban_sdk::Bytes::new(&s.env);
        s.client.reveal_board(
            &table_id,
            &s.committee,
            &flop_cards,
            &flop_indices,
            &proof,
            &pub_inputs,
        );

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Flop);

        // Now current_bet is 0 after reset_round. First active player can Bet.
        let current = table.current_turn;
        let acting = table.players.get(current).unwrap();
        let stack_before = acting.stack;
        let pot_before = table.pot;
        let bet_amount: i128 = 20;

        s.client
            .player_action(&table_id, &acting.address, &1u32, &Action::Bet(bet_amount));

        let table = s.client.get_table(&table_id);
        let player_after = table.players.get(current).unwrap();
        assert_eq!(player_after.stack, stack_before - bet_amount);
        assert_eq!(player_after.bet_this_round, bet_amount);
        assert_eq!(table.pot, pot_before + bet_amount);
    }

    #[test]
    fn test_fold_wins_pot() {
        // 2-player: one folds, the other wins the pot.
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        let table = s.client.get_table(&table_id);
        let pot = table.pot; // Should be 15 (sb 5 + bb 10)
        assert_eq!(pot, 15);

        let current = table.current_turn;
        let folder = table.players.get(current).unwrap();
        let other_seat = if current == 0 { 1u32 } else { 0u32 };
        let winner_before = table.players.get(other_seat).unwrap();
        let winner_stack_before = winner_before.stack;

        // Player folds
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        // Table should be in Settlement with pot awarded to remaining player
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Settlement);
        assert_eq!(table.pot, 0);

        let winner_after = table.players.get(other_seat).unwrap();
        assert_eq!(winner_after.stack, winner_stack_before + pot);
    }

    #[test]
    fn test_full_preflop_round_call_call() {
        // 3-player hand: two players call the big blind, round completes.
        // Note: due to is_round_complete logic, the round ends as soon as all
        // active players have matching bets. The BB does not get an extra action.
        let (s, table_id, _p1, _p2, _p3) = setup_preflop_3p();

        // Preflop: dealer=1, sb=2, bb=0, first_to_act = (1+3)%3 = 1
        // Blinds: sb(seat 2) = 5, bb(seat 0) = 10
        let table = s.client.get_table(&table_id);
        let pot_start = table.pot; // 15 (5 + 10)
        assert_eq!(pot_start, 15);

        // Seat 1 (first_to_act) calls the BB (bet 10)
        let turn1 = table.current_turn;
        assert_eq!(turn1, 1);
        let player1 = table.players.get(turn1).unwrap();
        s.client
            .player_action(&table_id, &player1.address, &1u32, &Action::Call);

        // Seat 2 (SB, bet was 5) calls (adds 5 to match BB at 10)
        let table = s.client.get_table(&table_id);
        let turn2 = table.current_turn;
        assert_eq!(turn2, 2);
        let player2 = table.players.get(turn2).unwrap();
        s.client
            .player_action(&table_id, &player2.address, &1u32, &Action::Call);

        // All bets now match at 10 -> round ends automatically -> DealingFlop
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::DealingFlop);
        // Pot: 15 (blinds) + 10 (seat 1 call) + 5 (seat 2 call) = 30
        assert_eq!(table.pot, 30);
    }

    #[test]
    fn test_raise_and_call_sequence() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // 2 players: dealer=1, sb=0, bb=1
        // current_turn = (1+3)%2 = 0
        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let raiser = table.players.get(current).unwrap();

        // Player raises by 20 on top of calling the big blind
        s.client
            .player_action(&table_id, &raiser.address, &1u32, &Action::RaiseBy(20));

        // Other player calls the raise
        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let caller = table.players.get(current).unwrap();
        s.client
            .player_action(&table_id, &caller.address, &1u32, &Action::Call);

        // Round should advance to DealingFlop
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::DealingFlop);
    }

    #[test]
    fn test_all_in_action() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 200);
        join_player(&s, table_id, &p2, 200);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let player = table.players.get(current).unwrap();

        // Go all-in
        s.client
            .player_action(&table_id, &player.address, &1u32, &Action::AllIn);

        let table = s.client.get_table(&table_id);
        let p = table.players.get(current).unwrap();
        assert!(p.all_in);
        assert_eq!(p.stack, 0);
    }

    // ---------------------------------------------------------------------------
    // 5. Leave table and withdraw
    // ---------------------------------------------------------------------------

    #[test]
    fn test_leave_table_waiting_phase() {
        let s = setup();
        let table_id = create_default_table(&s);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 500);

        // Verify player has 0 tokens (transferred to contract)
        assert_eq!(s.token.balance(&player), 0);

        // Leave table (in Waiting phase)
        let withdrawn = s.client.leave_table(&table_id, &player);
        assert_eq!(withdrawn, 500);

        // Tokens returned to player
        assert_eq!(s.token.balance(&player), 500);
        assert_eq!(s.token.balance(&s.client.address), 0);

        // Player removed from table
        let table = s.client.get_table(&table_id);
        assert_eq!(table.players.len(), 0);
    }

    #[test]
    fn test_leave_table_settlement_phase() {
        // Get to Settlement by having one player fold.
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // One player folds -> Settlement
        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let folder = table.players.get(current).unwrap();
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Settlement);

        // Now a player can leave
        let winner = if current == 0 {
            table.players.get(1).unwrap()
        } else {
            table.players.get(0).unwrap()
        };
        let winner_stack = winner.stack;

        let withdrawn = s.client.leave_table(&table_id, &winner.address);
        assert_eq!(withdrawn, winner_stack);
        assert_eq!(s.token.balance(&winner.address), winner_stack);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #7)")]
    fn test_cannot_leave_during_active_hand() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // In Preflop phase, leaving should panic
        s.client.leave_table(&table_id, &p1);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #6)")]
    fn test_leave_table_not_seated() {
        let s = setup();
        let table_id = create_default_table(&s);
        let stranger = Address::generate(&s.env);
        s.client.leave_table(&table_id, &stranger);
    }

    // ---------------------------------------------------------------------------
    // Additional edge-case tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_reveal_board_flop() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // SB calls -> all bets match -> round ends automatically
        let table = s.client.get_table(&table_id);
        let c = table.current_turn;
        let actor = table.players.get(c).unwrap();
        s.client
            .player_action(&table_id, &actor.address, &1u32, &Action::Call);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::DealingFlop);

        // Reveal flop
        let cards: Vec<u32> = Vec::from_array(&s.env, [10, 20, 30]);
        let indices: Vec<u32> = Vec::from_array(&s.env, [4, 5, 6]);
        let proof = soroban_sdk::Bytes::new(&s.env);
        let pub_in = soroban_sdk::Bytes::new(&s.env);
        s.client
            .reveal_board(&table_id, &s.committee, &cards, &indices, &proof, &pub_in);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Flop);
        assert_eq!(table.board_cards.len(), 3);
        assert_eq!(table.board_cards.get(0).unwrap(), 10);
        assert_eq!(table.board_cards.get(1).unwrap(), 20);
        assert_eq!(table.board_cards.get(2).unwrap(), 30);

        // Bets should be reset
        for i in 0..table.players.len() {
            let p = table.players.get(i).unwrap();
            assert_eq!(p.bet_this_round, 0);
        }
    }

    #[test]
    fn test_timeout_auto_folds_player() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        let p3 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);
        join_player(&s, table_id, &p3, 500);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 3);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Preflop);
        let stalling_seat = table.current_turn;
        let stalling_player = table.players.get(stalling_seat).unwrap();

        // Advance the ledger past the timeout
        let new_seq = table.last_action_ledger + table.config.timeout_ledgers;
        s.env.ledger().set_sequence_number(new_seq);

        // Claim timeout
        let claimer = Address::generate(&s.env);
        s.client.claim_timeout(&table_id, &claimer);

        // The stalling player should be auto-folded
        let table = s.client.get_table(&table_id);
        let folded = table.players.get(stalling_seat).unwrap();
        assert!(folded.folded);
        // Current turn should have advanced
        assert_ne!(table.current_turn, stalling_seat);

        // Verify the stalling player address matches
        assert_eq!(folded.address, stalling_player.address);
    }

    #[test]
    fn test_second_hand_after_settlement() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        // Hand 1
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // One folds -> Settlement
        let table = s.client.get_table(&table_id);
        let c = table.current_turn;
        let folder = table.players.get(c).unwrap();
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Settlement);
        assert_eq!(table.hand_number, 1);

        // Hand 2
        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Dealing);
        assert_eq!(table.hand_number, 2);
        assert_eq!(table.pot, 15); // blinds posted again

        // Verify all players are reset
        for i in 0..table.players.len() {
            let p = table.players.get(i).unwrap();
            assert!(!p.folded);
            assert!(!p.all_in);
        }
    }

    // ---------------------------------------------------------------------------
    // 6. Rake / fee mechanism (issue #31)
    // ---------------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "Error(Contract, #36)")]
    fn test_create_table_rejects_rake_above_max() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 501);
        s.client.create_table(&s.admin, &config);
    }

    #[test]
    fn test_create_table_accepts_max_rake() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 500);
        let table_id = s.client.create_table(&s.admin, &config);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.config.rake_bps, 500);
        assert_eq!(table.rake_balance, 0);
    }

    #[test]
    fn test_fold_win_deducts_rake() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 500); // 5%
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 5000);
        join_player(&s, table_id, &p2, 5000);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        let table = s.client.get_table(&table_id);
        let pot = table.pot; // small_blind 100 + big_blind 200 = 300
        assert_eq!(pot, 300);

        let current = table.current_turn;
        let folder = table.players.get(current).unwrap();
        let other_seat = if current == 0 { 1u32 } else { 0u32 };
        let winner_stack_before = table.players.get(other_seat).unwrap().stack;

        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let table = s.client.get_table(&table_id);
        // SB committed 100 and folded; BB committed 200, so 100 uncalled excess
        // is returned before rake (#869). Contested pot = 200; 5% of 200 = 10 rake;
        // winner receives 100 uncalled + 190 net pot = 290.
        let expected_rake = 10;
        assert_eq!(table.rake_balance, expected_rake);
        let winner_after = table.players.get(other_seat).unwrap();
        assert_eq!(
            winner_after.stack,
            winner_stack_before + pot - expected_rake
        );
        assert_eq!(s.client.get_rake_balance(&table_id), expected_rake);
    }

    #[test]
    fn test_fold_win_rake_rounds_down_on_small_pot() {
        // Smallest possible non-zero pot (blinds 1 + 2) where rake floors to
        // zero — the whole pot goes to the winner and no chips are burned.
        let s = setup();
        let config = TableConfig {
            blinds_schedule: BlindsSchedule::fixed(&s.env, 1, 2),
            ..rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 100) // 1%
        };
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 5000);
        join_player(&s, table_id, &p2, 5000);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.pot, 3); // 1 + 2

        let current = table.current_turn;
        let folder = table.players.get(current).unwrap();
        let other_seat = if current == 0 { 1u32 } else { 0u32 };
        let winner_stack_before = table.players.get(other_seat).unwrap().stack;

        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let table = s.client.get_table(&table_id);
        // floor(3 * 100 / 10_000) = 0 -> no rake taken, full pot to winner.
        assert_eq!(table.rake_balance, 0);
        let winner_after = table.players.get(other_seat).unwrap();
        assert_eq!(winner_after.stack, winner_stack_before + 3);
    }

    #[test]
    fn test_withdraw_rake_transfers_to_admin() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 500);
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 5000);
        join_player(&s, table_id, &p2, 5000);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let folder = table.players.get(current).unwrap();
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let accrued = s.client.get_rake_balance(&table_id);
        assert_eq!(accrued, 15);
        assert_eq!(s.token.balance(&s.admin), 0);

        let withdrawn = s.client.withdraw_rake(&table_id);
        assert_eq!(withdrawn, 15);
        assert_eq!(s.token.balance(&s.admin), 15);
        assert_eq!(s.client.get_rake_balance(&table_id), 0);

        // Withdrawing again with nothing accrued is a no-op, not an error.
        let withdrawn_again = s.client.withdraw_rake(&table_id);
        assert_eq!(withdrawn_again, 0);
        assert_eq!(s.token.balance(&s.admin), 15);
    }

    #[test]
    fn test_set_rake_bps_updates_config() {
        let s = setup();
        let table_id = create_default_table(&s);

        s.client.set_rake_bps(&table_id, &250);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.config.rake_bps, 250);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #36)")]
    fn test_set_rake_bps_rejects_above_max() {
        let s = setup();
        let table_id = create_default_table(&s);
        s.client.set_rake_bps(&table_id, &501);
    }

    // ---------------------------------------------------------------------------
    // 7. Emergency pause mechanism (issue #33)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_pause_and_unpause() {
        let s = setup();
        let table_id = create_default_table(&s);

        assert!(!s.client.is_paused(&table_id));

        s.client.pause(&table_id);
        assert!(s.client.is_paused(&table_id));

        s.client.unpause(&table_id);
        assert!(!s.client.is_paused(&table_id));
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #39)")]
    fn test_paused_blocks_join_table() {
        let s = setup();
        let table_id = create_default_table(&s);
        s.client.pause(&table_id);

        let player = Address::generate(&s.env);
        s.token_admin_client.mint(&player, &500);
        s.client.join_table(&table_id, &player, &500);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #39)")]
    fn test_paused_blocks_start_hand() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.pause(&table_id);
        s.client.start_hand(&table_id);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #39)")]
    fn test_paused_blocks_player_action() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        s.client.pause(&table_id);

        let table = s.client.get_table(&table_id);
        let current = table.current_turn;
        let actor = table.players.get(current).unwrap();
        s.client
            .player_action(&table_id, &actor.address, &1u32, &Action::Fold);
    }

    #[test]
    fn test_admin_can_read_while_paused() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        s.client.pause(&table_id);

        // Admin read functions must not revert while paused
        let table = s.client.get_table(&table_id);
        assert_eq!(table.players.len(), 2);
        let admin = s.client.get_admin(&table_id);
        assert_eq!(admin, s.admin);
        assert_eq!(s.client.get_rake_balance(&table_id), 0);
    }

    #[test]
    fn test_unpause_allows_operations_again() {
        let s = setup();
        let table_id = create_default_table(&s);

        s.client.pause(&table_id);
        s.client.unpause(&table_id);

        // join_table should succeed after unpause
        let player = Address::generate(&s.env);
        let seat = join_player(&s, table_id, &player, 500);
        assert_eq!(seat, 0);
    }

    // ---------------------------------------------------------------------------
    // Hand history (#71)
    // ---------------------------------------------------------------------------

    /// Play one heads-up hand that ends with a fold, leaving the table in
    /// Settlement and one hand in the history buffer.
    fn play_fold_hand(s: &TestSetup, table_id: u32) {
        s.client.start_hand(&table_id);
        let n = s.client.get_table(&table_id).players.len();
        commit_mock_deal(s, table_id, n);

        let table = s.client.get_table(&table_id);
        let folder = table.players.get(table.current_turn).unwrap();
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);
    }

    #[test]
    fn test_hand_history_records_settled_hand() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        assert_eq!(s.client.get_hand_history(&table_id, &0).len(), 0);

        play_fold_hand(&s, table_id);

        let history = s.client.get_hand_history(&table_id, &0);
        assert_eq!(history.len(), 1);

        let record = history.get(0).unwrap();
        assert_eq!(record.hand_number, 1);
        assert_eq!(record.players.len(), 2);
        assert!(!record.showdown, "fold win is not a showdown");
        assert_eq!(record.total_pot, 15); // sb 5 + bb 10
        assert_eq!(record.rake, 0);

        // The whole pot goes to the one seat still standing.
        assert_eq!(record.payouts.len(), 1);
        let payout = record.payouts.get(0).unwrap();
        assert_eq!(payout.amount, 15);

        // The fold that ended the hand is in the action summary.
        assert_eq!(record.actions.len(), 1);
        let action = record.actions.get(0).unwrap();
        assert_eq!(action.kind, ActionKind::Fold);
        assert_eq!(action.amount, 0);
        assert_eq!(action.phase, GamePhase::Preflop);
    }

    #[test]
    fn test_hand_history_is_newest_first_and_queryable_by_number() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        for _ in 0..3 {
            play_fold_hand(&s, table_id);
        }

        let history = s.client.get_hand_history(&table_id, &0);
        assert_eq!(history.len(), 3);
        assert_eq!(history.get(0).unwrap().hand_number, 3);
        assert_eq!(history.get(1).unwrap().hand_number, 2);
        assert_eq!(history.get(2).unwrap().hand_number, 1);

        // A limit pages in only the latest entries.
        let latest = s.client.get_hand_history(&table_id, &2);
        assert_eq!(latest.len(), 2);
        assert_eq!(latest.get(0).unwrap().hand_number, 3);

        // Direct lookup by hand number.
        let hand2 = s.client.get_hand(&table_id, &2).unwrap();
        assert_eq!(hand2.hand_number, 2);
        assert!(s.client.get_hand(&table_id, &99).is_none());

        let meta = s.client.get_hand_history_meta(&table_id);
        assert_eq!(meta.stored, 3);
        assert_eq!(meta.total_archived, 3);
    }

    #[test]
    fn test_hand_history_buffer_evicts_oldest() {
        let s = setup();
        let table_id = create_default_table(&s);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 500);
        join_player(&s, table_id, &p2, 500);

        let capacity = s.client.hand_history_capacity();
        for _ in 0..(capacity + 2) {
            play_fold_hand(&s, table_id);
        }

        let meta = s.client.get_hand_history_meta(&table_id);
        assert_eq!(meta.stored, capacity, "buffer never grows past capacity");
        assert_eq!(meta.total_archived, capacity + 2);

        let history = s.client.get_hand_history(&table_id, &0);
        assert_eq!(history.len(), capacity);
        assert_eq!(history.get(0).unwrap().hand_number, capacity + 2);

        // The two oldest hands have been overwritten.
        assert!(s.client.get_hand(&table_id, &1).is_none());
        assert!(s.client.get_hand(&table_id, &2).is_none());
        assert!(s.client.get_hand(&table_id, &3).is_some());
    }

    #[test]
    fn test_hand_history_records_bets_and_rake() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 500); // 5%
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 5000);
        join_player(&s, table_id, &p2, 5000);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // The small blind raises (a call would end the round outright heads-up),
        // then the big blind folds and the hand settles.
        let table = s.client.get_table(&table_id);
        let raiser = table.players.get(table.current_turn).unwrap();
        s.client
            .player_action(&table_id, &raiser.address, &1u32, &Action::RaiseBy(200));

        let table = s.client.get_table(&table_id);
        let folder = table.players.get(table.current_turn).unwrap();
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let record = s.client.get_hand(&table_id, &1).unwrap();
        assert_eq!(record.actions.len(), 2);

        let raise = record.actions.get(0).unwrap();
        assert_eq!(raise.kind, ActionKind::Raise);
        // 100 to call the big blind plus the 200 raise on top.
        assert_eq!(raise.amount, 300);

        let fold = record.actions.get(1).unwrap();
        assert_eq!(fold.kind, ActionKind::Fold);
        assert_eq!(fold.amount, 0);

        // SB committed 400 (100 SB + 300 raise) and BB committed 200 before folding.
        // 200 uncalled excess is returned to SB before rake (#869), leaving a
        // contested pot of 400; 5% rake = 20, payout = 380.
        assert_eq!(record.total_pot, 400);
        assert_eq!(record.rake, 20);
        assert_eq!(record.payouts.get(0).unwrap().amount, 380);
    }

    // ---------------------------------------------------------------------------
    // Partial buy-ins and rebuys (#75)
    // ---------------------------------------------------------------------------

    /// A table with `max_rebuys` configured. Buy-in band is the default
    /// 100..=1000, so a rebuy has plenty of room between the two bounds.
    fn rebuy_table(s: &TestSetup, max_rebuys: u32) -> u32 {
        let mut config = default_config(&s.env, &s.token.address, &s.committee, &s.verifier);
        config.max_rebuys = max_rebuys;
        s.client.create_table(&s.admin, &config)
    }

    /// Mint `amount` to `player` and rebuy for it.
    fn rebuy(s: &TestSetup, table_id: u32, player: &Address, amount: i128) -> i128 {
        s.token_admin_client.mint(player, &amount);
        s.client.rebuy(&table_id, player, &amount)
    }

    #[test]
    fn test_rebuy_tops_up_a_partial_amount() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 200);

        // A partial top-up well under a full buy-in.
        let new_stack = rebuy(&s, table_id, &player, 150);
        assert_eq!(new_stack, 350);

        let table = s.client.get_table(&table_id);
        let p = table.players.get(0).unwrap();
        assert_eq!(p.stack, 350);
        assert_eq!(p.total_buy_in, 350);
        assert_eq!(p.rebuy_count, 1);

        // The chips actually moved into the contract.
        assert_eq!(s.token.balance(&player), 0);

        let (total, count) = s.client.get_player_buy_in(&table_id, &player);
        assert_eq!(total, 350);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_rebuy_allows_repeated_partial_top_ups() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 100);

        assert_eq!(rebuy(&s, table_id, &player, 50), 150);
        assert_eq!(rebuy(&s, table_id, &player, 50), 200);
        assert_eq!(rebuy(&s, table_id, &player, 800), 1000);

        let (total, count) = s.client.get_player_buy_in(&table_id, &player);
        assert_eq!(total, 1000);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_rebuy_respects_the_per_session_limit() {
        let s = setup();
        let table_id = rebuy_table(&s, 2);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 200);

        rebuy(&s, table_id, &player, 100);
        rebuy(&s, table_id, &player, 100);

        s.token_admin_client.mint(&player, &100);
        let err = s
            .client
            .try_rebuy(&table_id, &player, &100)
            .expect_err("third rebuy is over the limit");
        assert_eq!(err, Ok(PokerTableError::RebuyLimitReached));
    }

    #[test]
    fn test_rebuy_cannot_exceed_max_buy_in() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 900);

        // 900 + 200 would put the stack over the 1000 maximum.
        s.token_admin_client.mint(&player, &200);
        let err = s
            .client
            .try_rebuy(&table_id, &player, &200)
            .expect_err("rebuy past max_buy_in is rejected");
        assert_eq!(err, Ok(PokerTableError::InvalidRebuyAmount));

        // Exactly reaching the maximum is fine.
        assert_eq!(s.client.rebuy(&table_id, &player, &100), 1000);
    }

    #[test]
    fn test_rebuy_must_reach_min_buy_in() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 100);
        let other = Address::generate(&s.env);
        join_player(&s, table_id, &other, 500);

        // Lose the stack down to a stub, then try to top up below the minimum.
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);
        let table = s.client.get_table(&table_id);
        let folder = table.players.get(table.current_turn).unwrap();
        s.client
            .player_action(&table_id, &folder.address, &1u32, &Action::Fold);

        let table = s.client.get_table(&table_id);
        let short = table
            .players
            .iter()
            .find(|p| p.stack < 100)
            .expect("one player is now under the table minimum");

        // A top-up that lands one chip short of the minimum is refused.
        let needed = 100 - short.stack;
        assert!(needed > 1, "the short stack has room to fall short");
        s.token_admin_client.mint(&short.address, &needed);

        let err = s
            .client
            .try_rebuy(&table_id, &short.address, &(needed - 1))
            .expect_err("a top-up that stays under min_buy_in is rejected");
        assert_eq!(err, Ok(PokerTableError::InvalidRebuyAmount));

        // Topping up exactly to the minimum works.
        assert_eq!(s.client.rebuy(&table_id, &short.address, &needed), 100);
    }

    #[test]
    fn test_rebuy_rejects_non_positive_amount() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 500);

        let err = s
            .client
            .try_rebuy(&table_id, &player, &0)
            .expect_err("zero rebuy is rejected");
        assert_eq!(err, Ok(PokerTableError::InvalidRebuyAmount));
    }

    #[test]
    fn test_rebuy_rejected_during_an_active_hand() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 200);
        join_player(&s, table_id, &p2, 200);

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        s.token_admin_client.mint(&p1, &100);
        let err = s
            .client
            .try_rebuy(&table_id, &p1, &100)
            .expect_err("chips cannot enter mid-hand");
        assert_eq!(err, Ok(PokerTableError::CannotRebuyDuringActiveHand));
    }

    #[test]
    fn test_rebuy_allowed_in_settlement_between_hands() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 200);
        join_player(&s, table_id, &p2, 200);

        play_fold_hand(&s, table_id);
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Settlement);

        let stack_before = s.client.get_table(&table_id).players.get(0).unwrap().stack;
        let new_stack = rebuy(&s, table_id, &p1, 100);
        assert_eq!(new_stack, stack_before + 100);
    }

    #[test]
    fn test_rebuy_requires_a_seat() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);

        let stranger = Address::generate(&s.env);
        s.token_admin_client.mint(&stranger, &500);
        let err = s
            .client
            .try_rebuy(&table_id, &stranger, &500)
            .expect_err("an unseated wallet cannot rebuy");
        assert_eq!(err, Ok(PokerTableError::PlayerNotAtTable));
    }

    #[test]
    fn test_rebuy_preserves_chip_conservation() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);
        let contract_addr = s.client.address.clone();

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 200);
        join_player(&s, table_id, &p2, 300);
        assert_eq!(s.token.balance(&contract_addr), 500);

        rebuy(&s, table_id, &p1, 250);
        assert_eq!(s.token.balance(&contract_addr), 750);

        // Contract balance still equals stacks + pot + rake.
        let table = s.client.get_table(&table_id);
        let stacks: i128 = table.players.iter().map(|p| p.stack).sum();
        assert_eq!(
            s.token.balance(&contract_addr),
            stacks + table.pot + table.rake_balance
        );
    }

    #[test]
    fn test_set_max_rebuys_updates_the_limit() {
        let s = setup();
        let table_id = rebuy_table(&s, 1);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 200);

        rebuy(&s, table_id, &player, 100);

        s.token_admin_client.mint(&player, &100);
        let err = s
            .client
            .try_rebuy(&table_id, &player, &100)
            .expect_err("limit of 1 is used up");
        assert_eq!(err, Ok(PokerTableError::RebuyLimitReached));

        // Raising the limit lets the player continue.
        s.client.set_max_rebuys(&table_id, &3);
        assert_eq!(s.client.rebuy(&table_id, &player, &100), 400);
        assert_eq!(s.client.get_table(&table_id).config.max_rebuys, 3);
    }

    #[test]
    fn test_rebuy_counter_resets_after_rejoining() {
        let s = setup();
        let table_id = rebuy_table(&s, 1);

        let player = Address::generate(&s.env);
        join_player(&s, table_id, &player, 200);
        rebuy(&s, table_id, &player, 100);
        assert_eq!(s.client.get_player_buy_in(&table_id, &player).1, 1);

        s.client.leave_table(&table_id, &player);
        s.client.join_table(&table_id, &player, &300);

        let (total, count) = s.client.get_player_buy_in(&table_id, &player);
        assert_eq!(total, 300, "a fresh session starts a fresh buy-in total");
        assert_eq!(count, 0);
    }

    // ---------------------------------------------------------------------------
    // Multiple simultaneous tables per player (#72)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_one_wallet_can_sit_at_many_tables_at_once() {
        let s = setup();
        let player = Address::generate(&s.env);

        // Enough tables to make clear there is no small cap hiding anywhere.
        let mut table_ids: Vec<u32> = Vec::new(&s.env);
        for _ in 0..8u32 {
            let table_id = create_default_table(&s);
            join_player(&s, table_id, &player, 300);
            table_ids.push_back(table_id);
        }

        let seated = s.client.get_player_tables(&player);
        assert_eq!(seated.len(), 8);
        assert_eq!(s.client.get_player_table_count(&player), 8);
        for i in 0..table_ids.len() {
            let table_id = table_ids.get(i).unwrap();
            assert_eq!(seated.get(i).unwrap(), table_id);
            let table = s.client.get_table(&table_id);
            assert_eq!(table.players.get(0).unwrap().address, player);
        }
    }

    #[test]
    fn test_player_table_index_is_empty_by_default() {
        let s = setup();
        let stranger = Address::generate(&s.env);
        assert_eq!(s.client.get_player_tables(&stranger).len(), 0);
        assert_eq!(s.client.get_player_table_count(&stranger), 0);
    }

    #[test]
    fn test_leaving_one_table_keeps_the_others() {
        let s = setup();
        let player = Address::generate(&s.env);

        let a = create_default_table(&s);
        let b = create_default_table(&s);
        let c = create_default_table(&s);
        for table_id in [a, b, c] {
            join_player(&s, table_id, &player, 300);
        }

        s.client.leave_table(&b, &player);

        let seated = s.client.get_player_tables(&player);
        assert_eq!(seated.len(), 2);
        assert_eq!(seated.get(0).unwrap(), a);
        assert_eq!(seated.get(1).unwrap(), c);
    }

    #[test]
    fn test_leaving_the_last_table_clears_the_index() {
        let s = setup();
        let player = Address::generate(&s.env);

        let table_id = create_default_table(&s);
        join_player(&s, table_id, &player, 300);
        s.client.leave_table(&table_id, &player);

        assert_eq!(s.client.get_player_tables(&player).len(), 0);
    }

    #[test]
    fn test_tables_stay_independent_across_concurrent_seats() {
        let s = setup();
        let hero = Address::generate(&s.env);
        let villain_a = Address::generate(&s.env);
        let villain_b = Address::generate(&s.env);

        let a = create_default_table(&s);
        let b = create_default_table(&s);
        join_player(&s, a, &hero, 400);
        join_player(&s, a, &villain_a, 400);
        join_player(&s, b, &hero, 400);
        join_player(&s, b, &villain_b, 400);

        // A hand on table A leaves table B untouched.
        play_fold_hand(&s, a);

        assert_eq!(s.client.get_table(&a).phase, GamePhase::Settlement);
        assert_eq!(s.client.get_table(&b).phase, GamePhase::Waiting);
        assert_eq!(s.client.get_hand_history(&a, &0).len(), 1);
        assert_eq!(s.client.get_hand_history(&b, &0).len(), 0);

        // Hero is still seated at both.
        assert_eq!(s.client.get_player_tables(&hero).len(), 2);
    }

    #[test]
    fn test_rejoining_does_not_duplicate_the_index_entry() {
        let s = setup();
        let player = Address::generate(&s.env);

        let table_id = create_default_table(&s);
        join_player(&s, table_id, &player, 300);
        s.client.leave_table(&table_id, &player);
        join_player(&s, table_id, &player, 300);

        let seated = s.client.get_player_tables(&player);
        assert_eq!(seated.len(), 1);
        assert_eq!(seated.get(0).unwrap(), table_id);
    }

    #[test]
    fn test_hand_history_is_per_table() {
        let s = setup();
        let table_a = create_default_table(&s);
        let table_b = create_default_table(&s);

        for table_id in [table_a, table_b] {
            let p1 = Address::generate(&s.env);
            let p2 = Address::generate(&s.env);
            join_player(&s, table_id, &p1, 500);
            join_player(&s, table_id, &p2, 500);
        }

        play_fold_hand(&s, table_a);
        play_fold_hand(&s, table_a);
        play_fold_hand(&s, table_b);

        assert_eq!(s.client.get_hand_history(&table_a, &0).len(), 2);
        assert_eq!(s.client.get_hand_history(&table_b, &0).len(), 1);
    }

    // ---------------------------------------------------------------------------
    // All-in run-out (#855)
    // ---------------------------------------------------------------------------

    /// Committee reveal of the next street from fresh deck indices.
    fn reveal_next_street(s: &TestSetup, table_id: u32) {
        let table = s.client.get_table(&table_id);
        let count = if table.phase == GamePhase::DealingFlop {
            3
        } else {
            1
        };
        let first = table.dealt_indices.len();
        let mut cards: Vec<u32> = Vec::new(&s.env);
        let mut indices: Vec<u32> = Vec::new(&s.env);
        for i in first..first + count {
            cards.push_back(i);
            indices.push_back(i);
        }
        let empty = soroban_sdk::Bytes::new(&s.env);
        s.client
            .reveal_board(&table_id, &s.committee, &cards, &indices, &empty, &empty);
    }

    /// Drive the hand to `Showdown`: on `shove_street` players with at most 200
    /// behind go all in, everyone else calls or checks, and every street is
    /// revealed. Fails if a betting round opens after the shove round.
    /// Returns how many streets were revealed.
    fn play_to_showdown(
        s: &TestSetup,
        table_id: u32,
        seqs: &mut [u32; 6],
        shove_street: Option<GamePhase>,
    ) -> u32 {
        let (mut shoving, mut shoved, mut reveals) = (false, false, 0);
        loop {
            let table = s.client.get_table(&table_id);
            match table.phase {
                GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River => {
                    assert!(!shoved, "{:?} opened betting after the all-in", table.phase);
                    shoving = shove_street.as_ref() == Some(&table.phase);
                    let mut max_bet = 0;
                    for p in table.players.iter() {
                        max_bet = max_bet.max(p.bet_this_round);
                    }
                    let seat = table.current_turn;
                    let p = table.players.get(seat).unwrap();
                    let action = if shoving && p.stack <= 200 {
                        Action::AllIn
                    } else if p.bet_this_round < max_bet {
                        Action::Call
                    } else {
                        Action::Check
                    };
                    seqs[seat as usize] += 1;
                    s.client
                        .player_action(&table_id, &p.address, &seqs[seat as usize], &action);
                }
                GamePhase::DealingFlop | GamePhase::DealingTurn | GamePhase::DealingRiver => {
                    shoved |= shoving;
                    reveal_next_street(s, table_id);
                    reveals += 1;
                }
                GamePhase::Showdown => return reveals,
                phase => panic!("unexpected phase {:?}", phase),
            }
        }
    }

    /// Three-handed: the two 200-chip stacks shove on `shove_street` and the
    /// 1,000-chip stack calls. No one can bet after that, so the remaining
    /// streets are only revealed, each with its own reveal proof.
    fn assert_all_in_runs_out(shove_street: GamePhase) {
        let s = setup();
        let table_id = create_default_table(&s);
        for buy_in in [1_000, 200, 200] {
            join_player(&s, table_id, &Address::generate(&s.env), buy_in);
        }
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 3);

        let reveals = play_to_showdown(&s, table_id, &mut [0; 6], Some(shove_street));

        let table = s.client.get_table(&table_id);
        assert_eq!(reveals, 3, "flop, turn and river each need a reveal");
        assert_eq!(table.board_cards.len(), 5);
        assert_eq!(table.players.iter().filter(|p| p.all_in).count(), 2);
        assert_eq!(table.action_deadline, 0, "no player timer left running");
    }

    #[test]
    fn test_all_in_preflop_runs_out_the_board() {
        assert_all_in_runs_out(GamePhase::Preflop);
    }

    #[test]
    fn test_all_in_on_flop_runs_out_the_board() {
        assert_all_in_runs_out(GamePhase::Flop);
    }

    #[test]
    fn test_all_in_on_turn_runs_out_the_board() {
        assert_all_in_runs_out(GamePhase::Turn);
    }

    #[test]
    fn test_deal_runs_out_the_board_when_blinds_leave_no_one_to_act() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 0);
        let table_id = s.client.create_table(&s.admin, &config);
        // Blinds 100/200. Seat 0 is the big blind and all in for 100; seat 1
        // (button, small blind 100) has already matched it.
        join_player(&s, table_id, &Address::generate(&s.env), 100);
        join_player(&s, table_id, &Address::generate(&s.env), 1_000);
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::DealingFlop);

        assert_eq!(play_to_showdown(&s, table_id, &mut [0; 6], None), 3);
        assert_eq!(s.client.get_table(&table_id).board_cards.len(), 5);
    }

    #[test]
    fn test_deal_passes_the_turn_past_an_all_in_big_blind() {
        let s = setup();
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 0);
        let table_id = s.client.create_table(&s.admin, &config);
        // Blinds 100/200. Seat 0 is the big blind, all in for 150: more than
        // the small blind's 100, so seat 1 still has to call or fold.
        join_player(&s, table_id, &Address::generate(&s.env), 150);
        let small_blind = Address::generate(&s.env);
        join_player(&s, table_id, &small_blind, 1_000);
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Preflop);
        assert_eq!(table.current_turn, 1);
        assert!(table.action_deadline > 0);

        s.client
            .player_action(&table_id, &small_blind, &1, &Action::Call);
        assert_eq!(
            play_to_showdown(&s, table_id, &mut [0, 1, 0, 0, 0, 0], None),
            3
        );
    }

    // ---------------------------------------------------------------------------
    // Seven-deuce bounty (#856)
    // ---------------------------------------------------------------------------

    // Card values are `suit * 13 + rank`, rank 0 = deuce.
    const SEVEN_CLUBS: u32 = 5;
    const DEUCE_CLUBS: u32 = 0;
    const DEUCE_DIAMONDS: u32 = 13;
    const KING_SPADES: u32 = 50;
    const ACE_SPADES: u32 = 51;

    /// Write `value` into BN254 field `field_index` of a proof's public inputs.
    fn write_field(bytes: &mut soroban_sdk::Bytes, field_index: u32, value: u32) {
        let start = field_index * 32 + 28;
        for (i, b) in value.to_be_bytes().iter().enumerate() {
            bytes.set(start + i as u32, *b);
        }
    }

    /// Showdown arguments in which seat `winner` (plus every seat in
    /// `tie_mask`) wins, with `holes[seat]` as each seat's proved hole cards.
    fn showdown_inputs(
        s: &TestSetup,
        winner: u32,
        tie_mask: u32,
        holes: &[(u32, u32)],
    ) -> (
        Vec<(u32, u32)>,
        Vec<(BytesN<32>, BytesN<32>)>,
        soroban_sdk::Bytes,
    ) {
        let mut public_inputs = soroban_sdk::Bytes::new(&s.env);
        for _ in 0..(27 * 32) {
            public_inputs.push_back(0);
        }
        let mut hole_cards: Vec<(u32, u32)> = Vec::new(&s.env);
        let mut salts: Vec<(BytesN<32>, BytesN<32>)> = Vec::new(&s.env);
        for (seat, &(c1, c2)) in holes.iter().enumerate() {
            write_field(&mut public_inputs, 13 + seat as u32, c1);
            write_field(&mut public_inputs, 19 + seat as u32, c2);
            hole_cards.push_back((c1, c2));
            let salt = BytesN::from_array(&s.env, &[0u8; 32]);
            salts.push_back((salt.clone(), salt));
        }
        write_field(&mut public_inputs, 25, winner);
        write_field(&mut public_inputs, 26, tie_mask);
        (hole_cards, salts, public_inputs)
    }

    /// Submit a committee showdown built by [`showdown_inputs`].
    fn mock_showdown(
        s: &TestSetup,
        table_id: u32,
        winner: u32,
        tie_mask: u32,
        holes: &[(u32, u32)],
    ) {
        let (seat_cards, salts, public_inputs) = showdown_inputs(s, winner, tie_mask, holes);
        // The contract takes the cards in showdown reveal order (#866).
        let mut hole_cards: Vec<(u32, u32)> = Vec::new(&s.env);
        for seat in s.client.get_showdown_order(&table_id).iter() {
            hole_cards.push_back(seat_cards.get(seat).unwrap());
        }
        s.client.submit_showdown(
            &table_id,
            &s.committee,
            &hole_cards,
            &salts,
            &soroban_sdk::Bytes::new(&s.env),
            &public_inputs,
            &Vec::new(&s.env),
        );
    }

    /// Heads-up table (blinds 100/200, 5% rake) with `bounty` funded by the
    /// admin and a hand checked down to the showdown.
    fn bounty_hand_at_showdown(s: &TestSetup, bounty: i128) -> u32 {
        let config = rake_config(&s.env, &s.token.address, &s.committee, &s.verifier, 500);
        let table_id = s.client.create_table(&s.admin, &config);
        for _ in 0..2 {
            join_player(s, table_id, &Address::generate(&s.env), 1_000);
        }
        s.token_admin_client.mint(&s.admin, &bounty);
        assert_eq!(s.client.fund_seven_deuce_bounty(&table_id, &bounty), bounty);
        s.client.start_hand(&table_id);
        commit_mock_deal(s, table_id, 2);
        play_to_showdown(s, table_id, &mut [0; 6], None);
        table_id
    }

    #[test]
    fn test_seven_deuce_bounty_paid_to_outright_winner_and_not_raked() {
        let s = setup();
        let table_id = bounty_hand_at_showdown(&s, 300);
        assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 300);
        assert_eq!(s.token.balance(&s.admin), 0);

        let before = s.client.get_table(&table_id);
        let stack = before.players.get(0).unwrap().stack;
        mock_showdown(
            &s,
            table_id,
            0,
            0,
            &[(SEVEN_CLUBS, DEUCE_DIAMONDS), (ACE_SPADES, KING_SPADES)],
        );

        let after = s.client.get_table(&table_id);
        let rake = after.rake_balance + after.jackpot_balance;
        assert_eq!(
            rake,
            before.pot * 500 / 10_000,
            "rake comes from the pot only"
        );
        assert_eq!(
            after.players.get(0).unwrap().stack,
            stack + before.pot - rake + 300
        );
        assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 0);
    }

    #[test]
    fn test_seven_deuce_bounty_needs_an_outright_offsuit_seven_deuce_win() {
        let seven_deuce = (SEVEN_CLUBS, DEUCE_DIAMONDS);
        let ace_king = (ACE_SPADES, KING_SPADES);
        // (winner, tie mask, hole cards by seat)
        let cases = [
            (0, 0, [(SEVEN_CLUBS, DEUCE_CLUBS), ace_king]), // suited
            (1, 0, [seven_deuce, ace_king]),                // 7-2 loses
            (0, 0b10, [seven_deuce, ace_king]),             // 7-2 only chops
        ];
        for (winner, tie_mask, holes) in cases {
            let s = setup();
            let table_id = bounty_hand_at_showdown(&s, 300);
            mock_showdown(&s, table_id, winner, tie_mask, &holes);
            assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 300);
        }
    }

    #[test]
    fn test_seven_deuce_bounty_not_paid_on_a_fold_win() {
        let s = setup();
        let table_id = create_default_table(&s);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        s.token_admin_client.mint(&s.admin, &300);
        s.client.fund_seven_deuce_bounty(&table_id, &300);

        play_fold_hand(&s, table_id);

        assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 300);
    }

    #[test]
    fn test_seven_deuce_bounty_cannot_be_claimed_twice_for_one_hand() {
        let s = setup();
        let table_id = bounty_hand_at_showdown(&s, 300);
        let holes = [(SEVEN_CLUBS, DEUCE_DIAMONDS), (ACE_SPADES, KING_SPADES)];
        mock_showdown(&s, table_id, 0, 0, &holes);

        // Refund the bounty before the next hand and replay the showdown.
        s.token_admin_client.mint(&s.admin, &300);
        s.client.fund_seven_deuce_bounty(&table_id, &300);
        let (hole_cards, salts, public_inputs) = showdown_inputs(&s, 0, 0, &holes);
        assert_eq!(
            s.client.try_submit_showdown(
                &table_id,
                &s.committee,
                &hole_cards,
                &salts,
                &soroban_sdk::Bytes::new(&s.env),
                &public_inputs,
                &Vec::new(&s.env),
            ),
            Err(Ok(PokerTableError::NotInShowdownPhase))
        );
        assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 300);
    }

    #[test]
    fn test_fund_seven_deuce_bounty_rejects_non_positive_amounts() {
        let s = setup();
        let table_id = create_default_table(&s);
        for amount in [0, -1] {
            assert_eq!(
                s.client.try_fund_seven_deuce_bounty(&table_id, &amount),
                Err(Ok(PokerTableError::InvalidBountyAmount))
            );
        }
        assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 0);
    }

    #[test]
    fn test_finalize_sunset_returns_unclaimed_bounty_to_admin() {
        let s = setup();
        let table_id = create_default_table(&s);
        s.token_admin_client.mint(&s.admin, &300);
        s.client.fund_seven_deuce_bounty(&table_id, &300);
        assert_eq!(s.token.balance(&s.admin), 0);

        s.client.finalize_sunset(&table_id);

        assert_eq!(s.token.balance(&s.admin), 300);
        assert_eq!(s.client.get_seven_deuce_bounty(&table_id), 0);
    }

    // ---------------------------------------------------------------------------
    // Bomb pots (#857)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_bomb_pot_collects_antes_and_skips_preflop() {
        let s = setup();
        let table_id = create_default_table(&s);
        for _ in 0..3 {
            join_player(&s, table_id, &Address::generate(&s.env), 500);
        }
        s.client.configure_bomb_pot(&table_id, &2, &50);
        let mut seqs = [0u32; 6];

        // Hand 1 is a normal hand: blinds 5/10 and preflop betting.
        s.client.start_hand(&table_id);
        assert_eq!(s.client.get_table(&table_id).pot, 15);
        commit_mock_deal(&s, table_id, 3);
        for _ in 0..2 {
            let table = s.client.get_table(&table_id);
            assert_eq!(table.phase, GamePhase::Preflop);
            let seat = table.current_turn;
            seqs[seat as usize] += 1;
            let folder = table.players.get(seat).unwrap().address;
            s.client
                .player_action(&table_id, &folder, &seqs[seat as usize], &Action::Fold);
        }
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Settlement);

        // Hand 2 is a bomb pot: three antes, no blinds, no preflop betting.
        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.pot, 150);
        for p in table.players.iter() {
            assert_eq!(p.committed, 50);
            assert_eq!(p.bet_this_round, 0);
        }
        commit_mock_deal(&s, table_id, 3);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::DealingFlop);
        let seat = table.current_turn as usize;
        assert_eq!(
            s.client.try_player_action(
                &table_id,
                &table.players.get(seat as u32).unwrap().address,
                &(seqs[seat] + 1),
                &Action::Check
            ),
            Err(Ok(PokerTableError::NotInBettingPhase))
        );

        reveal_next_street(&s, table_id);
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Flop);
    }

    #[test]
    fn test_bomb_pot_side_pots_add_up_to_the_antes() {
        let s = setup();
        let table_id = create_default_table(&s);
        for buy_in in [500, 500, 100] {
            join_player(&s, table_id, &Address::generate(&s.env), buy_in);
        }
        s.client.configure_bomb_pot(&table_id, &1, &150);

        // Seat 2 can only ante 100 of the 150 and is all in.
        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.pot, 400);
        assert!(table.players.get(2).unwrap().all_in);
        commit_mock_deal(&s, table_id, 3);
        assert_eq!(play_to_showdown(&s, table_id, &mut [0; 6], None), 3);
        mock_showdown(
            &s,
            table_id,
            2,
            0,
            &[
                (ACE_SPADES, KING_SPADES),
                (SEVEN_CLUBS, DEUCE_CLUBS),
                (40, 41),
            ],
        );

        // Main pot: 3 x 100 to seat 2. Side pot: 2 x 50 to the next best seat.
        let table = s.client.get_table(&table_id);
        assert_eq!(table.side_pots.len(), 2);
        assert_eq!(table.side_pots.get(0).unwrap().amount, 300);
        assert_eq!(table.side_pots.get(1).unwrap().amount, 100);
        assert_eq!(table.players.get(2).unwrap().stack, 300);
    }

    #[test]
    fn test_bomb_pot_starts_when_the_ante_leaves_one_player_with_chips() {
        let s = setup();
        let table_id = create_default_table(&s);
        join_player(&s, table_id, &Address::generate(&s.env), 100);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        s.client.configure_bomb_pot(&table_id, &1, &150);

        // Seat 0 is all in on the ante, so seat 1 has no one to bet against.
        s.client.start_hand(&table_id);
        assert_eq!(s.client.get_table(&table_id).pot, 250);
        commit_mock_deal(&s, table_id, 2);
        assert_eq!(play_to_showdown(&s, table_id, &mut [0; 6], None), 3);
    }

    #[test]
    fn test_bomb_pot_ante_of_a_whole_stack_is_all_in_and_wins_the_main_pot() {
        let s = setup();
        let table_id = create_default_table(&s);
        for buy_in in [500, 500, 150] {
            join_player(&s, table_id, &Address::generate(&s.env), buy_in);
        }
        s.client.configure_bomb_pot(&table_id, &1, &150);
        s.client.start_hand(&table_id);
        assert!(s.client.get_table(&table_id).players.get(2).unwrap().all_in);
        commit_mock_deal(&s, table_id, 3);
        reveal_next_street(&s, table_id);

        // The two players with chips put 100 more in on the flop.
        let mut seqs = [0u32; 6];
        for action in [Action::Bet(100), Action::Call] {
            let table = s.client.get_table(&table_id);
            let seat = table.current_turn;
            seqs[seat as usize] += 1;
            let player = table.players.get(seat).unwrap().address;
            s.client
                .player_action(&table_id, &player, &seqs[seat as usize], &action);
        }
        play_to_showdown(&s, table_id, &mut seqs, None);
        mock_showdown(
            &s,
            table_id,
            2,
            0,
            &[
                (ACE_SPADES, KING_SPADES),
                (SEVEN_CLUBS, DEUCE_CLUBS),
                (40, 41),
            ],
        );

        // Seat 2 wins the 3 x 150 main pot; the 2 x 100 side pot is not its.
        let table = s.client.get_table(&table_id);
        assert_eq!(table.players.get(2).unwrap().stack, 450);
    }

    #[test]
    fn test_configure_bomb_pot_validates_and_waits_for_the_hand_to_end() {
        let s = setup();
        let table_id = create_default_table(&s);
        assert_eq!(
            s.client.try_configure_bomb_pot(&table_id, &3, &0),
            Err(Ok(PokerTableError::InvalidBombPotConfig))
        );
        assert_eq!(s.client.get_bomb_pot_config(&table_id), (0, 0));
        s.client.configure_bomb_pot(&table_id, &3, &25);
        assert_eq!(s.client.get_bomb_pot_config(&table_id), (3, 25));

        join_player(&s, table_id, &Address::generate(&s.env), 500);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        s.client.start_hand(&table_id);
        assert_eq!(
            s.client.try_configure_bomb_pot(&table_id, &0, &0),
            Err(Ok(PokerTableError::HandAlreadyInProgress))
        );
    }

    // ---------------------------------------------------------------------------
    // Rabbit hunt (#858)
    // ---------------------------------------------------------------------------

    /// Committee rabbit hunt of the given deck indices (card value = index).
    fn try_rabbit_hunt(
        s: &TestSetup,
        table_id: u32,
        deck_indices: &[u32],
    ) -> Result<(), PokerTableError> {
        let mut cards: Vec<u32> = Vec::new(&s.env);
        for &i in deck_indices {
            cards.push_back(i);
        }
        let empty = soroban_sdk::Bytes::new(&s.env);
        match s
            .client
            .try_rabbit_hunt(&table_id, &s.committee, &cards, &cards, &empty, &empty)
        {
            Ok(_) => Ok(()),
            Err(Ok(e)) => Err(e),
            Err(Err(e)) => panic!("rabbit_hunt failed to invoke: {:?}", e),
        }
    }

    #[test]
    fn test_rabbit_hunt_reveals_the_rest_of_the_board_after_a_fold() {
        let s = setup();
        let table_id = create_default_table(&s);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        // Preflop fold: hole cards used deck indices 0..4, no board.
        play_fold_hand(&s, table_id);
        let settled = s.client.get_table(&table_id);

        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[4, 5, 6, 7, 8, 9]),
            Err(PokerTableError::WrongCardCount)
        );
        assert_eq!(try_rabbit_hunt(&s, table_id, &[4, 5, 6]), Ok(()));
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[8]),
            Err(PokerTableError::InvalidRabbitHuntIndices),
            "the turn card comes before the river card"
        );
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[0]),
            Err(PokerTableError::InvalidRabbitHuntIndices),
            "hole cards are never revealed"
        );
        assert_eq!(try_rabbit_hunt(&s, table_id, &[7]), Ok(()));
        assert_eq!(try_rabbit_hunt(&s, table_id, &[8]), Ok(()));
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[9]),
            Err(PokerTableError::RabbitHuntNotAvailable),
            "five board cards at most"
        );

        // The settled hand and the next one are untouched.
        let table = s.client.get_table(&table_id);
        assert_eq!(table.board_cards, settled.board_cards);
        assert_eq!(table.dealt_indices, settled.dealt_indices);
        let record = s.client.get_hand(&table_id, &table.hand_number).unwrap();
        assert_eq!(record.board.len(), 0);
        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.board_cards.len(), 0);
        assert_eq!(table.dealt_indices.len(), 0);
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[4, 5, 6]),
            Err(PokerTableError::RabbitHuntNotAvailable)
        );
    }

    #[test]
    fn test_rabbit_hunt_only_after_a_hand_settles_without_showdown() {
        let s = setup();
        let table_id = create_default_table(&s);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[0, 1, 2]),
            Err(PokerTableError::RabbitHuntNotAvailable),
            "no hand played yet"
        );

        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[4, 5, 6]),
            Err(PokerTableError::RabbitHuntNotAvailable),
            "hand still live"
        );

        play_to_showdown(&s, table_id, &mut [0; 6], None);
        mock_showdown(
            &s,
            table_id,
            0,
            0,
            &[(ACE_SPADES, KING_SPADES), (SEVEN_CLUBS, DEUCE_CLUBS)],
        );
        assert_eq!(
            try_rabbit_hunt(&s, table_id, &[9]),
            Err(PokerTableError::RabbitHuntNotAvailable),
            "showdown hands have a full board"
        );
    }

    // ---------------------------------------------------------------------------
    // Between-hands guard (#860, #861), sit-out (#859), showdown order (#866)
    // ---------------------------------------------------------------------------

    /// The player whose turn it is takes `action`.
    fn act(s: &TestSetup, table_id: u32, seqs: &mut [u32; 6], action: Action) {
        let table = s.client.get_table(&table_id);
        let seat = table.current_turn;
        seqs[seat as usize] += 1;
        let p = table.players.get(seat).unwrap();
        s.client
            .player_action(&table_id, &p.address, &seqs[seat as usize], &action);
    }

    /// Call or check (and reveal each street) until the hand reaches `stop`.
    fn call_down_to(s: &TestSetup, table_id: u32, seqs: &mut [u32; 6], stop: GamePhase) {
        loop {
            let table = s.client.get_table(&table_id);
            if table.phase == stop {
                return;
            }
            match table.phase {
                GamePhase::Preflop | GamePhase::Flop | GamePhase::Turn | GamePhase::River => {
                    let max_bet = table
                        .players
                        .iter()
                        .map(|p| p.bet_this_round)
                        .max()
                        .unwrap();
                    let p = table.players.get(table.current_turn).unwrap();
                    let action = if p.bet_this_round < max_bet {
                        Action::Call
                    } else {
                        Action::Check
                    };
                    act(s, table_id, seqs, action);
                }
                GamePhase::DealingFlop | GamePhase::DealingTurn | GamePhase::DealingRiver => {
                    reveal_next_street(s, table_id)
                }
                phase => panic!("unexpected phase {:?}", phase),
            }
        }
    }

    /// Contract token balance equals every stack plus the pot and rake.
    fn assert_chips_conserved(s: &TestSetup, table_id: u32) {
        let table = s.client.get_table(&table_id);
        let stacks: i128 = table.players.iter().map(|p| p.stack).sum();
        assert_eq!(
            s.token.balance(&s.client.address),
            stacks + table.pot + table.rake_balance
        );
    }

    /// `player`'s seat view at `table_id`.
    fn position(s: &TestSetup, table_id: u32, player: &Address) -> PlayerPosition {
        let ids = Vec::from_array(&s.env, [table_id]);
        s.client.get_player_positions(player, &ids).get(0).unwrap()
    }

    #[test]
    fn test_rebuy_rejected_in_every_live_phase_and_chips_conserved() {
        let s = setup();
        let table_id = rebuy_table(&s, 0);
        let p1 = Address::generate(&s.env);
        join_player(&s, table_id, &p1, 200);
        join_player(&s, table_id, &Address::generate(&s.env), 200);
        s.token_admin_client.mint(&p1, &100);

        let assert_rejected = |s: &TestSetup| {
            let stack = s.client.get_table(&table_id).players.get(0).unwrap().stack;
            assert_eq!(
                s.client.try_rebuy(&table_id, &p1, &100),
                Err(Ok(PokerTableError::CannotRebuyDuringActiveHand))
            );
            assert_eq!(
                s.client.get_table(&table_id).players.get(0).unwrap().stack,
                stack
            );
            assert_eq!(s.token.balance(&p1), 100);
            assert_chips_conserved(s, table_id);
        };

        s.client.start_hand(&table_id);
        assert_rejected(&s); // Dealing
        commit_mock_deal(&s, table_id, 2);
        let mut seqs = [0u32; 6];
        for phase in [
            GamePhase::Preflop,
            GamePhase::DealingFlop,
            GamePhase::Flop,
            GamePhase::DealingTurn,
            GamePhase::Turn,
            GamePhase::DealingRiver,
            GamePhase::River,
            GamePhase::Showdown,
        ] {
            call_down_to(&s, table_id, &mut seqs, phase);
            assert_rejected(&s);
        }

        // Between hands the same top-up goes through.
        mock_showdown(&s, table_id, 0, 0, &[(0, 1), (2, 3)]);
        let stack = s.client.get_table(&table_id).players.get(0).unwrap().stack;
        assert_eq!(s.client.rebuy(&table_id, &p1, &100), stack + 100);
        assert_chips_conserved(&s, table_id);
    }

    #[test]
    fn test_folded_player_cannot_leave_mid_hand_and_their_chips_stay_in_the_pot() {
        let s = setup();
        let table_id = create_default_table(&s);
        let players = [(); 3].map(|_| Address::generate(&s.env));
        for p in &players {
            join_player(&s, table_id, p, 500);
        }
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 3);

        // Button (seat 1) raises to 30, small blind (seat 2) re-raises to 70,
        // big blind (seat 0) calls, and the button folds with 30 committed.
        let mut seqs = [0u32; 6];
        assert_eq!(s.client.get_table(&table_id).current_turn, 1);
        act(&s, table_id, &mut seqs, Action::RaiseBy(20));
        act(&s, table_id, &mut seqs, Action::RaiseBy(40));
        act(&s, table_id, &mut seqs, Action::Call);
        act(&s, table_id, &mut seqs, Action::Fold);
        let folder = s.client.get_table(&table_id).players.get(1).unwrap();
        assert!(folder.folded);
        assert_eq!((folder.stack, folder.committed), (470, 30));

        // No chips leave while the hand is live.
        assert_eq!(
            s.client.try_leave_table(&table_id, &players[1]),
            Err(Ok(PokerTableError::CannotLeaveDuringActiveHand))
        );
        assert_eq!(s.token.balance(&players[1]), 0);
        assert_eq!(s.client.get_table(&table_id).pot, 170);
        assert_chips_conserved(&s, table_id);

        // Seat 0 wins at showdown and takes the folder's 30 as dead money.
        call_down_to(&s, table_id, &mut seqs, GamePhase::Showdown);
        mock_showdown(&s, table_id, 0, 0, &[(0, 1), (2, 3), (4, 6)]);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Settlement);
        assert_eq!(table.pot, 0);
        let stack = |seat: u32| table.players.get(seat).unwrap().stack;
        assert_eq!([stack(0), stack(1), stack(2)], [600, 470, 430]);
        assert_chips_conserved(&s, table_id);

        // After settlement the folder leaves with their stack only.
        assert_eq!(s.client.leave_table(&table_id, &players[1]), 470);
        assert_eq!(s.token.balance(&players[1]), 470);
        assert_chips_conserved(&s, table_id);
    }

    #[test]
    fn test_sit_out_next_big_blind_skips_one_hand_and_returns_with_a_dead_blind() {
        let s = setup();
        let table_id = create_default_table(&s);
        let players = [(); 3].map(|_| Address::generate(&s.env));
        for p in &players {
            join_player(&s, table_id, p, 500);
        }
        s.client.sit_out_next_big_blind(&table_id, &players[0]);
        assert!(position(&s, table_id, &players[0]).sit_out_next_bb);

        // Hand 1: the button is seat 1, so the big blind would be seat 0. Seat 0
        // sits out and seats 1 and 2 play it heads-up (seat 1 small, seat 2 big).
        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        let p0 = table.players.get(0).unwrap();
        assert!(p0.sitting_out && p0.folded);
        assert_eq!((p0.stack, p0.committed), (500, 0));
        assert_eq!(table.players.len(), 3, "the seat is kept");
        assert_eq!(table.pot, 15);
        assert_eq!(table.players.get(2).unwrap().bet_this_round, 10);
        let pos = position(&s, table_id, &players[0]);
        assert!(pos.seated && pos.sitting_out);

        // The sitting-out seat never gets the turn; one fold ends the hand.
        commit_mock_deal(&s, table_id, 3);
        assert_ne!(s.client.get_table(&table_id).current_turn, 0);
        let mut seqs = [0u32; 6];
        act(&s, table_id, &mut seqs, Action::Fold);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Settlement);
        assert_eq!(table.players.get(0).unwrap().stack, 500);

        // Hand 2: seat 0 is dealt back in as small blind and also posts the
        // skipped big blind dead: pot = 5 + 10 (big blind) + 10 (dead).
        s.client.start_hand(&table_id);
        let table = s.client.get_table(&table_id);
        let p0 = table.players.get(0).unwrap();
        assert!(!p0.sitting_out && !p0.folded);
        assert_eq!((p0.stack, p0.committed, p0.bet_this_round), (485, 15, 5));
        assert_eq!(table.players.get(1).unwrap().bet_this_round, 10);
        assert_eq!(table.pot, 25);
        let pos = position(&s, table_id, &players[0]);
        assert!(!pos.sitting_out && !pos.sit_out_next_bb);
        assert_chips_conserved(&s, table_id);
    }

    #[test]
    fn test_sit_out_next_big_blind_moves_first_preflop_action_with_the_blind() {
        let s = setup();
        let table_id = create_default_table(&s);
        let players = [(); 4].map(|_| Address::generate(&s.env));
        for p in &players {
            join_player(&s, table_id, p, 500);
        }
        s.client.sit_out_next_big_blind(&table_id, &players[3]);

        // Button seat 1, small blind seat 2. Seat 3 sits out, so seat 0 posts the
        // big blind and the button, not the big blind, opens the betting.
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 4);
        let table = s.client.get_table(&table_id);
        assert!(table.players.get(3).unwrap().sitting_out);
        assert_eq!(table.players.get(0).unwrap().bet_this_round, 10);
        assert_eq!(table.current_turn, 1);
    }

    #[test]
    fn test_sit_out_next_big_blind_waits_while_heads_up() {
        let s = setup();
        let table_id = create_default_table(&s);
        let p0 = Address::generate(&s.env);
        join_player(&s, table_id, &p0, 500);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        s.client.sit_out_next_big_blind(&table_id, &p0);

        // Seat 0 is the big blind, but sitting out would leave one player.
        s.client.start_hand(&table_id);
        let p = s.client.get_table(&table_id).players.get(0).unwrap();
        assert!(!p.sitting_out && !p.folded);
        assert_eq!(p.committed, 10);
        let pos = position(&s, table_id, &p0);
        assert!(pos.sit_out_next_bb, "the request is still pending");
    }

    #[test]
    fn test_sit_out_request_is_dropped_when_the_player_leaves() {
        let s = setup();
        let table_id = create_default_table(&s);
        let p0 = Address::generate(&s.env);
        join_player(&s, table_id, &p0, 500);
        s.client.sit_out_next_big_blind(&table_id, &p0);
        s.client.leave_table(&table_id, &p0);

        join_player(&s, table_id, &p0, 500);
        assert!(!position(&s, table_id, &p0).sit_out_next_bb);
    }

    #[test]
    fn test_showdown_last_river_aggressor_reveals_first() {
        let s = setup();
        let table_id = create_default_table(&s);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        join_player(&s, table_id, &Address::generate(&s.env), 500);
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        // River: seat 0 (left of the button) bets, seat 1 raises, seat 0 calls.
        let mut seqs = [0u32; 6];
        call_down_to(&s, table_id, &mut seqs, GamePhase::River);
        assert_eq!(s.client.get_table(&table_id).current_turn, 0);
        act(&s, table_id, &mut seqs, Action::Bet(20));
        act(&s, table_id, &mut seqs, Action::RaiseBy(20));
        act(&s, table_id, &mut seqs, Action::Call);
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Showdown);
        assert_eq!(
            s.client.get_showdown_order(&table_id),
            Vec::from_array(&s.env, [1, 0])
        );

        // Seat order puts seat 0 first: rejected.
        let holes = [(0, 1), (2, 3)];
        let (seat_order, salts, public_inputs) = showdown_inputs(&s, 1, 0, &holes);
        assert_eq!(
            s.client.try_submit_showdown(
                &table_id,
                &s.committee,
                &seat_order,
                &salts,
                &soroban_sdk::Bytes::new(&s.env),
                &public_inputs,
                &Vec::new(&s.env),
            ),
            Err(Ok(PokerTableError::HoleCardMismatch))
        );

        // The aggressor first is accepted.
        mock_showdown(&s, table_id, 1, 0, &holes);
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Settlement);
    }

    #[test]
    fn test_showdown_order_starts_left_of_button_when_river_is_checked() {
        let s = setup();
        let table_id = create_default_table(&s);
        for _ in 0..3 {
            join_player(&s, table_id, &Address::generate(&s.env), 500);
        }
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 3);

        // A flop bet does not count: only river aggression sets the order.
        let mut seqs = [0u32; 6];
        call_down_to(&s, table_id, &mut seqs, GamePhase::Flop);
        act(&s, table_id, &mut seqs, Action::Bet(20));
        call_down_to(&s, table_id, &mut seqs, GamePhase::Showdown);
        assert_eq!(s.client.get_table(&table_id).dealer_seat, 1);
        assert_eq!(
            s.client.get_showdown_order(&table_id),
            Vec::from_array(&s.env, [2, 0, 1])
        );
        mock_showdown(&s, table_id, 2, 0, &[(0, 1), (2, 3), (4, 6)]);
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Settlement);
    }
}
