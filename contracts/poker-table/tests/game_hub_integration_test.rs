#![cfg(test)]

use game_hub::{MockGameHub, MockGameHubClient};
use poker_table::game_hub::StellarGameStudioHubClient;
use poker_table::types::{BlindsSchedule, TableConfig};
use poker_table::{PokerTableContract, PokerTableContractClient};
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env, String,
};

#[test]
fn test_game_hub_integration_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    // 1. Deploy & initialize Stellar Game Studio Game Hub
    let hub_id = env.register(MockGameHub, ());
    let hub_client = MockGameHubClient::new(&env, &hub_id);
    let hub_admin = Address::generate(&env);
    hub_client.initialize(&hub_admin);

    // 2. Register StellPoker in Game Hub catalog
    let poker_id = env.register(PokerTableContract, ());
    let poker_client = PokerTableContractClient::new(&env, &poker_id);
    let game_name = String::from_str(&env, "StellPoker");
    let metadata_uri = String::from_str(&env, "ipfs://bafybeistellpokergamehubmeta");
    hub_client.register_game(&hub_admin, &poker_id, &game_name, &metadata_uri);

    let registered_game = hub_client.get_game(&poker_id).unwrap();
    assert_eq!(registered_game.name, game_name);
    assert!(registered_game.active);

    // 3. Link shared player identities between Stellar Game Studio and wallet addresses
    let player1 = Address::generate(&env);
    let player2 = Address::generate(&env);
    let p1_handle = String::from_str(&env, "ace_high_99");
    let p2_handle = String::from_str(&env, "bluff_master");

    hub_client.link_player(&player1, &p1_handle);
    hub_client.link_player(&player2, &p2_handle);

    assert_eq!(
        hub_client.get_player_profile(&player1).unwrap().studio_id,
        p1_handle
    );
    assert_eq!(
        hub_client.get_player_profile(&player2).unwrap().studio_id,
        p2_handle
    );

    // 4. Cross-promotion: Player 1 was referred from "StellarArcade" to StellPoker
    let referrer_game = Address::generate(&env);
    let promo_bonus_points: i128 = 250;
    hub_client.record_cross_promo(
        &referrer_game,
        &player1,
        &referrer_game,
        &poker_id,
        &promo_bonus_points,
    );

    // Verify cross promo is recorded
    let promo_record = hub_client.get_cross_promo(&player1, &poker_id).unwrap();
    assert!(!promo_record.claimed);
    assert_eq!(promo_record.reward_points, promo_bonus_points);

    // Player claims cross-promotion upon joining StellPoker
    let claimed = hub_client.claim_cross_promo(&player1, &poker_id);
    assert_eq!(claimed, promo_bonus_points);
    let p1_profile = hub_client.get_player_profile(&player1).unwrap();
    assert_eq!(p1_profile.total_points, promo_bonus_points);

    // 5. Setup Token, Committee, and Verifier for Poker Table
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let _token = TokenClient::new(&env, &sac.address());
    let token_admin_client = StellarAssetClient::new(&env, &sac.address());

    let committee = Address::generate(&env);
    let verifier = Address::generate(&env);
    let table_admin = Address::generate(&env);

    let config = TableConfig {
        token: sac.address(),
        min_buy_in: 100,
        max_buy_in: 1000,
        betting_structure: poker_table::types::BettingStructure::NoLimit,
        blinds_schedule: BlindsSchedule::fixed(&env, 5, 10),
        min_players: 2,
        max_players: 6,
        timeout_ledgers: 100,
        committee: committee.clone(),
        verifier: verifier.clone(),
        game_hub: hub_id.clone(),
        rake_bps: 0,
        max_rebuys: 0,
        jackpot_rake_share_bps: 0,
        min_bad_beat_category: 7,
        min_bad_beat_rank: 12,
        street_time_limit: poker_table::types::OptionalStreetTimeLimit::None,
        treasury: None,
        dead_chip_timeout_ledgers: 0,
        reclaim_period_ledgers: 0,
    };

    let table_id = poker_client.create_table(&table_admin, &config);

    // Fund players and join table
    token_admin_client.mint(&player1, &1000);
    token_admin_client.mint(&player2, &1000);
    poker_client.join_table(&table_id, &player1, &500);
    poker_client.join_table(&table_id, &player2, &500);

    // 6. Start hand on table -> triggers GameHub start_game
    poker_client.start_hand(&table_id);

    let table = poker_client.get_table(&table_id);
    let session_id = table.session_id;
    assert!(session_id > 0);

    // 7. Verify Game Hub session was created and active
    let session = hub_client.get_session(&session_id).unwrap();
    assert!(session.active);
    assert_eq!(session.game_id, poker_id);
    assert_eq!(session.player1, player1);
    assert_eq!(session.player2, player2);

    // Both players had their activity tracked in Game Hub
    let p1_stats = hub_client.get_player_profile(&player1).unwrap();
    assert_eq!(p1_stats.games_played, 1);

    // 8. Test notifying end of game session via Game Hub interface
    let studio_client = StellarGameStudioHubClient::new(&env, &hub_id);
    studio_client.end_game(&session_id, &true);

    let settled_session = hub_client.get_session(&session_id).unwrap();
    assert!(!settled_session.active);
    assert_eq!(settled_session.player1_won, Some(true));

    let winner_profile = hub_client.get_player_profile(&player1).unwrap();
    assert_eq!(winner_profile.games_won, 1);
}
