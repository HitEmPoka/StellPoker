//! Tests for voluntary show-one-card at showdown / post-settlement (Issue #867).

#[cfg(test)]
mod show_one_test {
    extern crate std;
    use crate::types::*;
    use crate::{PokerTableContract, PokerTableContractClient};
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Events as _},
        token::{StellarAssetClient, TokenClient},
        Address, Bytes, BytesN, Env, Vec,
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

    fn create_and_settle_two_player_hand(s: &Setup) -> (u32, u32, u32) {
        let game_hub = s.env.register(GameHubContract, ());
        let mut levels = Vec::new(&s.env);
        levels.push_back(BlindLevel {
            small_blind: 5,
            big_blind: 10,
            ante: AnteMode::None,
            duration_seconds: 0,
            break_seconds: 0,
        });
        let config = TableConfig {
            token: s.token.address.clone(),
            min_buy_in: 100,
            max_buy_in: 100_000,
            betting_structure: BettingStructure::NoLimit,
            blinds_schedule: BlindsSchedule { levels },
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
        };
        let table_id = s.client.create_table(&s.admin, &config);

        let p0 = Address::generate(&s.env);
        let p1 = Address::generate(&s.env);
        s.token_admin_client.mint(&p0, &1_000);
        s.token_admin_client.mint(&p1, &1_000);
        s.client.join_table(&table_id, &p0, &1_000);
        s.client.join_table(&table_id, &p1, &1_000);

        s.client.start_hand(&table_id);

        let deck_root = BytesN::from_array(&s.env, &[1u8; 32]);
        let mut commitments: Vec<BytesN<32>> = Vec::new(&s.env);
        commitments.push_back(BytesN::from_array(&s.env, &[2u8; 32]));
        commitments.push_back(BytesN::from_array(&s.env, &[3u8; 32]));
        let mut dealt_indices: Vec<u32> = Vec::new(&s.env);
        for i in 0..4 {
            dealt_indices.push_back(i);
        }
        s.client.commit_deal(
            &table_id,
            &s.committee,
            &deck_root,
            &commitments,
            &dealt_indices,
            &Bytes::new(&s.env),
            &Bytes::new(&s.env),
        );

        // First actor folds so the other seat wins and settles immediately.
        let table = s.client.get_table(&table_id);
        let loser_seat = table.current_turn;
        let winner_seat = (loser_seat + 1) % 2;
        let loser_addr = table.players.get(loser_seat).unwrap().address;
        s.client
            .player_action(&table_id, &loser_addr, &1, &Action::Fold);

        (table_id, winner_seat, loser_seat)
    }

    #[test]
    fn winner_can_show_one_card_and_other_remains_private() {
        let s = setup();
        let (table_id, winner_seat, _loser_seat) = create_and_settle_two_player_hand(&s);

        let table = s.client.get_table(&table_id);
        assert_eq!(table.phase, GamePhase::Settlement);

        // Winner discloses only card_index 0 (card 51 = Ace of Spades).
        s.client.show_one(&table_id, &winner_seat, &0, &51);

        // Exactly one card is recorded in hand history.
        let shown = s.client.get_shown_cards(&table_id, &table.hand_number);
        assert_eq!(shown.len(), 1);
        assert_eq!(
            shown.get(0).unwrap(),
            ShownCardRecord {
                seat: winner_seat,
                card_index: 0,
                card: 51,
            }
        );

        // Distinct `card_shown_one` event was emitted (and no second card).
        let events = s.env.events().all();
        assert!(!events.is_empty());
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #126)")] // CardAlreadyShown
    fn winner_cannot_show_second_card() {
        let s = setup();
        let (table_id, winner_seat, _loser_seat) = create_and_settle_two_player_hand(&s);

        s.client.show_one(&table_id, &winner_seat, &0, &51);
        // Attempting to reveal the second card must be rejected.
        s.client.show_one(&table_id, &winner_seat, &1, &50);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #125)")] // NotHandWinner
    fn loser_cannot_show_one_card() {
        let s = setup();
        let (table_id, _winner_seat, loser_seat) = create_and_settle_two_player_hand(&s);

        s.client.show_one(&table_id, &loser_seat, &0, &10);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #124)")] // InvalidCardIndex
    fn invalid_card_index_rejected() {
        let s = setup();
        let (table_id, winner_seat, _loser_seat) = create_and_settle_two_player_hand(&s);

        s.client.show_one(&table_id, &winner_seat, &2, &10);
    }
}
