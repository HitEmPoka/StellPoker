use soroban_sdk::{contractclient, Address, Env};

/// Game Hub contract client interface.
/// In production, calls the Stellar Game Studio Game Hub at
/// CB4VZAT2U3UC6XFK3N23SKRF2NDCMP3QHJYMCHHFMZO7MRQO6DQ2EMYG.
/// For tests, use the mock in contracts/game-hub/.
#[contractclient(name = "GameHubClient")]
#[allow(dead_code)]
pub trait GameHub {
    fn start_game(
        env: Env,
        game_id: Address,
        session_id: u32,
        player1: Address,
        player2: Address,
        player1_points: i128,
        player2_points: i128,
    );

    fn end_game(env: Env, session_id: u32, player1_won: bool);
}

/// Extended Stellar Game Studio Game Hub interface for shared identity and cross-promotion.
#[contractclient(name = "StellarGameStudioHubClient")]
#[allow(dead_code)]
pub trait StellarGameStudioHub {
    fn start_game(
        env: Env,
        game_id: Address,
        session_id: u32,
        player1: Address,
        player2: Address,
        player1_points: i128,
        player2_points: i128,
    );

    fn end_game(env: Env, session_id: u32, player1_won: bool);

    fn register_game(
        env: Env,
        caller: Address,
        game_id: Address,
        name: soroban_sdk::String,
        metadata_uri: soroban_sdk::String,
    );

    fn link_player(env: Env, player: Address, studio_id: soroban_sdk::String);

    fn record_cross_promo(
        env: Env,
        caller: Address,
        player: Address,
        source_game: Address,
        target_game: Address,
        reward_points: i128,
    );

    fn claim_cross_promo(env: Env, player: Address, target_game: Address) -> i128;
}

/// Notify the game hub that a new hand is starting.
pub fn notify_start(
    env: &Env,
    game_hub: &Address,
    game_id: &Address,
    session_id: u32,
    player1: &Address,
    player2: &Address,
    player1_points: i128,
    player2_points: i128,
) {
    let client = GameHubClient::new(env, game_hub);
    client.start_game(
        game_id,
        &session_id,
        player1,
        player2,
        &player1_points,
        &player2_points,
    );
}

/// Notify the game hub that a hand has ended.
pub fn notify_end(env: &Env, game_hub: &Address, session_id: u32, player1_won: bool) {
    let client = GameHubClient::new(env, game_hub);
    client.end_game(&session_id, &player1_won);
}
