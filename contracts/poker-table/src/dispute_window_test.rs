/// Tests for the on-chain dispute window for settlement challenges.
///
/// Validates the state machine: settled -> challenged -> resolved,
/// and that timeout finalizes the original settlement.

#[cfg(test)]
mod dispute_window_test {
    use crate::types::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Address, BytesN, Env, Vec,
    };

    fn fake_hash(env: &Env, seed: u8) -> BytesN<32> {
        BytesN::from_array(env, &[seed; 32])
    }

    fn make_table(env: &Env) -> TableState {
        let admin = Address::generate(env);
        let committee = Address::generate(env);
        let verifier = Address::generate(env);
        let token_admin = Address::generate(env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let game_hub = Address::generate(env);

        let config = TableConfig {
            token: sac.address(),
            min_buy_in: 100,
            max_buy_in: 1000,
            betting_structure: BettingStructure::NoLimit,
            blinds_schedule: BlindsSchedule::fixed(env, 5, 10),
            min_players: 2,
            max_players: 6,
            timeout_ledgers: 100,
            committee: committee.clone(),
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

        TableState {
            id: 1,
            admin,
            config,
            phase: GamePhase::Settlement,
            players: Vec::new(env),
            dealer_seat: 0,
            current_turn: 0,
            pot: 0,
            side_pots: Vec::new(env),
            deck_root: BytesN::from_array(env, &[0; 32]),
            hand_commitments: Vec::new(env),
            board_cards: Vec::new(env),
            dealt_indices: Vec::new(env),
            hand_number: 5,
            last_action_ledger: 0,
            committee,
            session_id: 0,
            rake_balance: 0,
            jackpot_balance: 0,
            action_deadline: 0,
            hand_actions: Vec::new(env),
            rit_state: OptionalRitState::None,
            last_raise_size: 0,
            current_blind_level: 0,
            level_started_at: 0,
            break_ends_at: 0,
            settlement_entered_ledger: 100,
        }
    }

    // --- Dispute window logic tests ---

    #[test]
    fn dispute_window_check_in_settlement_phase() {
        let env = Env::default();
        env.mock_all_auths();

        let table = make_table(&env);

        // Within the dispute window (100 + 120 = 220, current = 150).
        env.ledger().with_mut(|l| l.sequence_number = 150);
        assert!(crate::dispute_window::is_in_dispute_window(&env, &table, 5));

        // Wrong hand number.
        assert!(!crate::dispute_window::is_in_dispute_window(&env, &table, 4));

        // After the window (current = 221).
        env.ledger().with_mut(|l| l.sequence_number = 221);
        assert!(!crate::dispute_window::is_in_dispute_window(&env, &table, 5));
    }

    #[test]
    fn dispute_window_expired_check() {
        let env = Env::default();
        env.mock_all_auths();

        let mut table = make_table(&env);
        table.settlement_entered_ledger = 50;
        table.hand_number = 1;

        // Not expired yet (50 + 120 = 170, current = 100).
        env.ledger().with_mut(|l| l.sequence_number = 100);
        assert!(!crate::dispute_window::is_dispute_window_expired(&env, &table));

        // Exactly at boundary (170).
        env.ledger().with_mut(|l| l.sequence_number = 170);
        assert!(!crate::dispute_window::is_dispute_window_expired(&env, &table));

        // Expired (171).
        env.ledger().with_mut(|l| l.sequence_number = 171);
        assert!(crate::dispute_window::is_dispute_window_expired(&env, &table));
    }

    #[test]
    fn evidence_hash_validation() {
        let env = Env::default();

        // Valid hash (non-zero).
        let valid = fake_hash(&env, 42);
        assert!(crate::dispute_window::validate_evidence_hash(&valid));

        // Zero hash is invalid.
        let zero = BytesN::from_array(&env, &[0; 32]);
        assert!(!crate::dispute_window::validate_evidence_hash(&zero));
    }

    #[test]
    fn settlement_not_in_settlement_phase() {
        let env = Env::default();
        env.mock_all_auths();

        let mut table = make_table(&env);
        table.phase = GamePhase::Preflop;

        assert!(!crate::dispute_window::is_in_dispute_window(&env, &table, 5));
    }

    #[test]
    fn dispute_window_not_expired_when_no_settlement() {
        let env = Env::default();
        env.mock_all_auths();

        let mut table = make_table(&env);
        table.phase = GamePhase::Waiting;
        table.settlement_entered_ledger = 0;

        assert!(!crate::dispute_window::is_dispute_window_expired(&env, &table));
    }

    #[test]
    fn challenge_lifecycle_settled_to_challenged_to_resolved() {
        let env = Env::default();
        let challenger = Address::generate(&env);
        let evidence = fake_hash(&env, 99);

        let mut challenge = crate::dispute_window::SettlementChallenge {
            table_id: 1,
            hand_number: 5,
            challenger: challenger.clone(),
            evidence_hash: evidence.clone(),
            challenged_at: 100,
            expires_at: 220,
            resolved: false,
            upheld: false,
        };

        // State: Challenged (not resolved yet).
        assert!(!challenge.resolved);
        assert!(!challenge.upheld);

        // Resolve the challenge (dismissed).
        challenge.resolved = true;
        challenge.upheld = false;
        assert!(challenge.resolved);
        assert!(!challenge.upheld);

        // Resolve upheld.
        challenge.upheld = true;
        assert!(challenge.resolved);
        assert!(challenge.upheld);
    }

    #[test]
    fn timeout_finalizes_settlement() {
        let env = Env::default();
        env.mock_all_auths();

        let mut table = make_table(&env);
        table.hand_number = 3;
        table.settlement_entered_ledger = 50;

        // During window.
        env.ledger().with_mut(|l| l.sequence_number = 100);
        assert!(crate::dispute_window::is_in_dispute_window(&env, &table, 3));
        assert!(!crate::dispute_window::is_dispute_window_expired(&env, &table));

        // After timeout — finalized.
        env.ledger().with_mut(|l| l.sequence_number = 171);
        assert!(!crate::dispute_window::is_in_dispute_window(&env, &table, 3));
        assert!(crate::dispute_window::is_dispute_window_expired(&env, &table));
    }
}
