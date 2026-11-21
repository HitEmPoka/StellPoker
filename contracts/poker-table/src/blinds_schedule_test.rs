//! Tests for the configurable blinds structure (fixed / escalating / ante),
//! exercised against the real contract rather than a disconnected pure
//! function — see the note in `tournament_lifecycle_test.rs` this replaces
//! the intent of.

#[cfg(test)]
mod blinds_schedule_test {
    extern crate std;
    use crate::types::*;
    use crate::{PokerTableContract, PokerTableContractClient};
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger as _},
        token::{StellarAssetClient, TokenClient},
        Address, BytesN, Env, Vec,
    };
    use std::vec::Vec as StdVec;

    /// Tracks each player's next expected `player_action` sequence number,
    /// which the contract requires to increase by exactly one per accepted
    /// action for a given (table, player) pair.
    fn get_seq(seqs: &mut StdVec<(Address, u32)>, player: &Address) -> u32 {
        for entry in seqs.iter_mut() {
            if entry.0 == *player {
                entry.1 += 1;
                return entry.1;
            }
        }
        seqs.push((player.clone(), 1));
        1
    }

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
        /// End game.
        ///
        /// # Parameters
        /// - `_session_id`: parameter
        /// - `_p1_won`: parameter
        ///
        /// # Returns
        /// - `()`
        ///
        /// # Errors
        /// Returns an error if the operation fails.
        ///
        /// # Authorization
        /// Requires appropriate authorization.
        pub fn end_game(_env: Env, _session_id: u32, _p1_won: bool) {}
    }

    struct Setup<'a> {
        env: Env,
        client: PokerTableContractClient<'a>,
        token: TokenClient<'a>,
        token_admin_client: StellarAssetClient<'a>,
        admin: Address,
        committee: Address,
        verifier: Address,
    }

    fn setup() -> Setup<'static> {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(PokerTableContract, ());
        let client = PokerTableContractClient::new(&env, &contract_id);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token = TokenClient::new(&env, &sac.address());
        let token_admin_client = StellarAssetClient::new(&env, &sac.address());

        let admin = Address::generate(&env);
        let committee = Address::generate(&env);
        let verifier = env.register(crate::verifier::ZkVerifierContract, ());

        Setup {
            env,
            client,
            token,
            token_admin_client,
            admin,
            committee,
            verifier,
        }
    }

    fn config_with_schedule(s: &Setup, schedule: BlindsSchedule) -> TableConfig {
        let game_hub = s.env.register(GameHubContract, ());
        TableConfig {
            token: s.token.address.clone(),
            min_buy_in: 100,
            max_buy_in: 100_000,
            betting_structure: crate::types::BettingStructure::NoLimit,
            blinds_schedule: schedule,
            min_players: 2,
            max_players: 6,
            timeout_ledgers: 100,
            committee: s.committee.clone(),
            verifier: s.verifier.clone(),
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

    fn join(s: &Setup, table_id: u32, player: &Address, buy_in: i128) -> u32 {
        s.token_admin_client.mint(player, &buy_in);
        s.client.join_table(&table_id, player, &buy_in)
    }

    fn commit_mock_deal(s: &Setup, table_id: u32, num_players: u32) {
        let deck_root = BytesN::from_array(&s.env, &[1u8; 32]);
        let mut commitments: Vec<BytesN<32>> = Vec::new(&s.env);
        for _ in 0..num_players {
            commitments.push_back(BytesN::from_array(&s.env, &[2u8; 32]));
        }
        let mut dealt_indices: Vec<u32> = Vec::new(&s.env);
        for i in 0..(num_players * 2) {
            dealt_indices.push_back(i);
        }
        s.client.commit_deal(
            &table_id,
            &s.committee,
            &deck_root,
            &commitments,
            &dealt_indices,
            &soroban_sdk::Bytes::new(&s.env),
            &soroban_sdk::Bytes::new(&s.env),
        );
    }

    /// Drive a fresh hand (already in Dealing after `start_hand` +
    /// `commit_mock_deal`) straight to Settlement via a fold-win, the
    /// cheapest path to a completed hand.
    fn play_fold_win_hand(s: &Setup, table_id: u32, seqs: &mut StdVec<(Address, u32)>) {
        s.client.start_hand(&table_id);
        commit_mock_deal(s, table_id, 2);
        let table = s.client.get_table(&table_id);
        let folder = table.players.get(table.current_turn).unwrap();
        let seq = get_seq(seqs, &folder.address);
        s.client.player_action(
            &table_id,
            &folder.address,
            &seq,
            &crate::types::Action::Fold,
        );
        assert_eq!(s.client.get_table(&table_id).phase, GamePhase::Settlement);
    }

    #[test]
    fn fixed_single_level_blinds_never_advance() {
        let s = setup();
        let config = config_with_schedule(&s, BlindsSchedule::fixed(&s.env, 5, 10));
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join(&s, table_id, &p1, 1000);
        join(&s, table_id, &p2, 1000);

        let mut seqs: StdVec<(Address, u32)> = StdVec::new();
        play_fold_win_hand(&s, table_id, &mut seqs);
        assert_eq!(s.client.get_table(&table_id).current_blind_level, 0);

        // Advance a huge amount of wall-clock time — a single-level (fixed)
        // schedule must never advance regardless of elapsed time.
        s.env.ledger().set_timestamp(1_000_000);
        play_fold_win_hand(&s, table_id, &mut seqs);
        assert_eq!(s.client.get_table(&table_id).current_blind_level, 0);
    }

    #[test]
    fn escalating_blinds_advance_after_level_duration_elapses() {
        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 5,
            big_blind: 10,
            ante: crate::types::AnteMode::None,
            duration_seconds: 100,
            break_seconds: 0,
        });
        levels.push_back(BlindLevel {
            small_blind: 10,
            big_blind: 20,
            ante: crate::types::AnteMode::None,
            duration_seconds: 0, // final level: lasts indefinitely
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join(&s, table_id, &p1, 1000);
        join(&s, table_id, &p2, 1000);

        let mut seqs: StdVec<(Address, u32)> = StdVec::new();

        // Hand 1 at t=0: level 0 still active (5/10 blinds, pot = 15).
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);
        let table = s.client.get_table(&table_id);
        assert_eq!(table.current_blind_level, 0);
        assert_eq!(table.pot, 15);
        let folder = table.players.get(table.current_turn).unwrap();
        let seq = get_seq(&mut seqs, &folder.address);
        s.client.player_action(
            &table_id,
            &folder.address,
            &seq,
            &crate::types::Action::Fold,
        );

        // Not enough time has passed yet — still level 0.
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);
        assert_eq!(s.client.get_table(&table_id).current_blind_level, 0);
        let table = s.client.get_table(&table_id);
        let folder = table.players.get(table.current_turn).unwrap();
        let seq = get_seq(&mut seqs, &folder.address);
        s.client.player_action(
            &table_id,
            &folder.address,
            &seq,
            &crate::types::Action::Fold,
        );

        // Advance past the 100s level duration, then start a new hand.
        s.env.ledger().set_timestamp(150);
        s.client.start_hand(&table_id);
        commit_mock_deal(&s, table_id, 2);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.current_blind_level, 1);
        assert_eq!(table.pot, 30); // 10 + 20
    }

    #[test]
    fn ante_collected_from_every_player_without_affecting_bet_this_round() {
        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 5,
            big_blind: 10,
            ante: crate::types::AnteMode::Fixed(2),
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        let table_id = s.client.create_table(&s.admin, &config);

        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        let p3 = Address::generate(&s.env);
        join(&s, table_id, &p1, 1000);
        join(&s, table_id, &p2, 1000);
        join(&s, table_id, &p3, 1000);

        s.client.start_hand(&table_id);

        let table = s.client.get_table(&table_id);
        // Pot = 3 antes (2 each = 6) + small blind (5) + big blind (10) = 21.
        assert_eq!(table.pot, 21);

        // Every seat paid the ante on top of whatever else they owed; total
        // deducted from stacks must equal the antes + blinds collected.
        let mut total_deducted = 0i128;
        for i in 0..table.players.len() {
            let p = table.players.get(i).unwrap();
            total_deducted += 1000 - p.stack;
        }
        assert_eq!(total_deducted, 21);

        // The ante must not be folded into bet_this_round — only the actual
        // blind (or 0 for a player who's neither SB nor BB) should show
        // there, since bet_this_round drives call-amount logic, not the pot
        // contribution accounting (which lives in `committed`).
        let sb_seat = (table.dealer_seat + 1) % table.players.len();
        let bb_seat = (table.dealer_seat + 2) % table.players.len();
        for i in 0..table.players.len() {
            let p = table.players.get(i).unwrap();
            let expected_bet = if i == sb_seat {
                5
            } else if i == bb_seat {
                10
            } else {
                0
            };
            assert_eq!(p.bet_this_round, expected_bet, "seat {i}");
            // committed must include the ante regardless of seat.
            let expected_committed = expected_bet + 2;
            assert_eq!(p.committed, expected_committed, "seat {i}");
        }
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #42)")] // EmptyBlindsSchedule
    fn create_table_rejects_empty_blinds_schedule() {
        let s = setup();
        let config = config_with_schedule(
            &s,
            BlindsSchedule {
                levels: Vec::new(&s.env),
            },
        );
        s.client.create_table(&s.admin, &config);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #43)")] // InvalidBlindLevel
    fn create_table_rejects_level_with_small_blind_gte_big_blind() {
        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 10,
            big_blind: 10,
            ante: crate::types::AnteMode::None,
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        s.client.create_table(&s.admin, &config);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #43)")] // InvalidBlindLevel
    fn create_table_rejects_nonfinal_level_with_zero_duration() {
        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 5,
            big_blind: 10,
            ante: crate::types::AnteMode::None,
            duration_seconds: 0, // invalid: not the final level
            break_seconds: 0,
        });
        levels.push_back(BlindLevel {
            small_blind: 10,
            big_blind: 20,
            ante: crate::types::AnteMode::None,
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        s.client.create_table(&s.admin, &config);
    }

    #[test]
    fn ante_only_all_in_clamps_stack_marks_all_in_and_skips_preflop_turn() {
        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 50,
            big_blind: 100,
            ante: crate::types::AnteMode::Fixed(150),
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        let table_id = s.client.create_table(&s.admin, &config);

        // Seat 0: micro-stack (100 < ante 150) — goes all-in from partial ante.
        // Seat 1 & 2: deep stacks (10_000).
        let p0 = Address::generate(&s.env);
        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join(&s, table_id, &p0, 100);
        join(&s, table_id, &p1, 10_000);
        join(&s, table_id, &p2, 10_000);

        s.client.start_hand(&table_id);

        let table = s.client.get_table(&table_id);
        let s0 = table.players.get(0).unwrap();
        assert_eq!(s0.stack, 0);
        assert!(s0.all_in, "player exhausted by ante must be marked all-in");
        assert_eq!(s0.committed, 100, "committed must equal partial ante only");

        // Total pot = partial ante (100) + 2 full antes (300) + SB (50) + BB (100) = 550.
        assert_eq!(table.pot, 550);

        // Commit deal and verify preflop turn skips any all-in seat.
        commit_mock_deal(&s, table_id, 3);
        let after_deal = s.client.get_table(&table_id);
        assert_eq!(after_deal.phase, GamePhase::Preflop);
        let turn_player = after_deal.players.get(after_deal.current_turn).unwrap();
        assert!(
            !turn_player.all_in && turn_player.stack > 0,
            "preflop current_turn must skip ante-all-in player"
        );
    }

    #[test]
    fn exact_ante_all_in_marks_player_all_in_without_underflow() {
        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 50,
            big_blind: 100,
            ante: crate::types::AnteMode::Fixed(100),
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        let table_id = s.client.create_table(&s.admin, &config);

        // Seat 0 has stack == ante (100), Seat 1 and Seat 2 have 10_000.
        let p0 = Address::generate(&s.env);
        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join(&s, table_id, &p0, 100);
        join(&s, table_id, &p1, 10_000);
        join(&s, table_id, &p2, 10_000);

        s.client.start_hand(&table_id);

        let table = s.client.get_table(&table_id);
        let s0 = table.players.get(0).unwrap();
        assert_eq!(s0.stack, 0);
        assert!(s0.all_in, "player with stack == ante must be marked all-in");
        assert_eq!(s0.committed, 100);
        assert_eq!(s0.bet_this_round, 0);
        // Pot = 3 * 100 (antes) + 50 (SB) + 100 (BB) = 450.
        assert_eq!(table.pot, 450);
    }

    #[test]
    fn color_up_conserves_total_chips_with_mixed_odd_stacks() {
        use crate::game::apply_color_up;
        use crate::types::{ColorUpRounding, ColorUpRule};
        use soroban_sdk::testutils::Events as _;

        let s = setup();
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 5,
            big_blind: 10,
            ante: AnteMode::None,
            duration_seconds: 60,
            break_seconds: 0,
        });
        levels.push_back(BlindLevel {
            small_blind: 25,
            big_blind: 50,
            ante: AnteMode::None,
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = config_with_schedule(&s, BlindsSchedule { levels });
        let table_id = s.client.create_table(&s.admin, &config);

        let p0 = Address::generate(&s.env);
        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        let p3 = Address::generate(&s.env);
        join(&s, table_id, &p0, 103);
        join(&s, table_id, &p1, 217);
        join(&s, table_id, &p2, 109);
        join(&s, table_id, &p3, 301);
        // Total chips in play: 103 + 217 + 109 + 301 = 730.
        // Remainders mod 10: [3, 7, 9, 1], sum = 20 -> 2 chips of 10, 0 residual.
        let total_before = 730i128;

        let mut table = s.client.get_table(&table_id);
        table.dealer_seat = 0;

        // HighestRemainderClockwise: remainders are seat 2 (9), seat 1 (7), seat 0 (3), seat 3 (1).
        // Seats 2 and 1 each win a 10-chip color-up.
        let rule = ColorUpRule {
            level_index: 1,
            denomination: 10,
            rounding: ColorUpRounding::HighestRemainderClockwise,
        };
        apply_color_up(&s.env, &mut table, &rule).unwrap();

        let s0 = table.players.get(0).unwrap().stack;
        let s1 = table.players.get(1).unwrap().stack;
        let s2 = table.players.get(2).unwrap().stack;
        let s3 = table.players.get(3).unwrap().stack;
        assert_eq!(s0, 100);
        assert_eq!(s1, 220);
        assert_eq!(s2, 110);
        assert_eq!(s3, 300);
        assert_eq!(s0 + s1 + s2 + s3, total_before);

        // Verify color_up event was emitted.
        let events = s.env.events().all();
        assert!(!events.is_empty());
    }

    #[test]
    fn color_up_clockwise_from_button_and_residual_conservation() {
        use crate::game::apply_color_up;
        use crate::types::{ColorUpRounding, ColorUpRule};

        let s = setup();
        let config = config_with_schedule(&s, BlindsSchedule::fixed(&s.env, 5, 10));
        let table_id = s.client.create_table(&s.admin, &config);

        let p0 = Address::generate(&s.env);
        let p1 = Address::generate(&s.env);
        let p2 = Address::generate(&s.env);
        join(&s, table_id, &p0, 103);
        join(&s, table_id, &p1, 104);
        join(&s, table_id, &p2, 105);
        // Total = 312. Remainders mod 10: [3, 4, 5], sum = 12 -> 1 chip of 10 + 2 residual.
        let mut table = s.client.get_table(&table_id);
        table.dealer_seat = 1; // Clockwise from 1 is: 2, 0, 1.

        let rule = ColorUpRule {
            level_index: 1,
            denomination: 10,
            rounding: ColorUpRounding::ClockwiseFromButton,
        };
        apply_color_up(&s.env, &mut table, &rule).unwrap();

        let s0 = table.players.get(0).unwrap().stack;
        let s1 = table.players.get(1).unwrap().stack;
        let s2 = table.players.get(2).unwrap().stack;
        // Seat 2 (first left of button 1) receives the 10-chip + 2 residual = 112.
        assert_eq!(s2, 112);
        assert_eq!(s0, 100);
        assert_eq!(s1, 100);
        assert_eq!(s0 + s1 + s2, 312);
    }
}
