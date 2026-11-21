#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String,
    Vec,
};

/// Production Game Hub address on Stellar testnet:
/// CB4VZAT2U3UC6XFK3N23SKRF2NDCMP3QHJYMCHHFMZO7MRQO6DQ2EMYG
#[contract]
pub struct MockGameHub;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameInfo {
    pub game_id: Address,
    pub name: String,
    pub author: Address,
    pub metadata_uri: String,
    pub active: bool,
    pub registered_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerProfile {
    pub player: Address,
    pub studio_id: String,
    pub games_played: u32,
    pub games_won: u32,
    pub total_points: i128,
    pub first_seen_ledger: u32,
    pub last_active_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameSession {
    pub session_id: u32,
    pub game_id: Address,
    pub player1: Address,
    pub player2: Address,
    pub player1_points: i128,
    pub player2_points: i128,
    pub active: bool,
    pub start_ledger: u32,
    pub player1_won: Option<bool>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossPromoRecord {
    pub player: Address,
    pub source_game: Address,
    pub target_game: Address,
    pub reward_points: i128,
    pub claimed: bool,
    pub registered_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Game(Address),
    GameList,
    Player(Address),
    Session(u32),
    CrossPromo(Address, Address), // (player, target_game)
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GameHubError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    GameNotFound = 4,
    GameInactive = 5,
    SessionNotFound = 6,
    SessionAlreadyActive = 7,
    SessionAlreadyEnded = 8,
    PromoAlreadyClaimed = 9,
    PromoNotFound = 10,
    InvalidPoints = 11,
}

#[contractevent]
pub struct GameRegistered {
    pub game_id: Address,
    pub author: Address,
}

#[contractevent]
pub struct PlayerLinked {
    pub player: Address,
    pub studio_id: String,
}

#[contractevent]
pub struct GameStarted {
    pub session_id: u32,
    pub game_id: Address,
    /// The authenticated game contract that reported the session.
    pub caller: Address,
    pub player1: Address,
    pub player2: Address,
    pub player1_points: i128,
    pub player2_points: i128,
}

#[contractevent]
pub struct GameEnded {
    pub session_id: u32,
    /// The authenticated game contract that reported the result.
    pub caller: Address,
    pub player1_won: bool,
}

#[contractevent]
pub struct CrossPromoAwarded {
    pub player: Address,
    pub source_game: Address,
    pub target_game: Address,
    pub reward_points: i128,
}

const TTL_THRESHOLD: u32 = 17_280;
const TTL_EXTEND: u32 = 518_400;

#[contractimpl]
impl MockGameHub {
    /// Initialize the Game Hub with an admin address.
    pub fn initialize(env: Env, admin: Address) -> Result<(), GameHubError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(GameHubError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::GameList, &Vec::<Address>::new(&env));
        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
        Ok(())
    }

    /// Register a game in the Stellar Game Studio catalog for cross-promotion.
    /// Admin only: registration grants the game contract the right to report
    /// sessions, so an untrusted account must not be able to self-register.
    pub fn register_game(
        env: Env,
        caller: Address,
        game_id: Address,
        name: String,
        metadata_uri: String,
    ) -> Result<(), GameHubError> {
        caller.require_auth();
        Self::require_admin(&env, &caller)?;
        let ledger = env.ledger().sequence();
        let game_info = GameInfo {
            game_id: game_id.clone(),
            name,
            author: caller.clone(),
            metadata_uri,
            active: true,
            registered_ledger: ledger,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Game(game_id.clone()), &game_info);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Game(game_id.clone()), TTL_THRESHOLD, TTL_EXTEND);

        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::GameList)
            .unwrap_or_else(|| Vec::new(&env));
        let mut exists = false;
        for i in 0..list.len() {
            if list.get(i).unwrap() == game_id {
                exists = true;
                break;
            }
        }
        if !exists {
            list.push_back(game_id.clone());
            env.storage()
                .persistent()
                .set(&DataKey::GameList, &list);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::GameList, TTL_THRESHOLD, TTL_EXTEND);
        }

        GameRegistered {
            game_id,
            author: caller,
        }
        .publish(&env);
        Ok(())
    }

    /// Get registered game metadata.
    pub fn get_game(env: Env, game_id: Address) -> Option<GameInfo> {
        env.storage().persistent().get(&DataKey::Game(game_id))
    }

    /// List all registered games in the hub catalog.
    pub fn list_games(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::GameList)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Link a player's on-chain address with their Stellar Game Studio ID / handle.
    pub fn link_player(env: Env, player: Address, studio_id: String) -> Result<(), GameHubError> {
        player.require_auth();
        let ledger = env.ledger().sequence();
        let mut profile = env
            .storage()
            .persistent()
            .get(&DataKey::Player(player.clone()))
            .unwrap_or_else(|| PlayerProfile {
                player: player.clone(),
                studio_id: studio_id.clone(),
                games_played: 0,
                games_won: 0,
                total_points: 0,
                first_seen_ledger: ledger,
                last_active_ledger: ledger,
            });

        profile.studio_id = studio_id.clone();
        profile.last_active_ledger = ledger;

        env.storage()
            .persistent()
            .set(&DataKey::Player(player.clone()), &profile);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Player(player.clone()), TTL_THRESHOLD, TTL_EXTEND);

        PlayerLinked { player, studio_id }.publish(&env);
        Ok(())
    }

    /// Fetch a player's shared Game Studio profile.
    pub fn get_player_profile(env: Env, player: Address) -> Option<PlayerProfile> {
        env.storage().persistent().get(&DataKey::Player(player))
    }

    /// Record a cross-promotion referral (e.g. player referred from another game to StellPoker).
    pub fn record_cross_promo(
        env: Env,
        caller: Address,
        player: Address,
        source_game: Address,
        target_game: Address,
        reward_points: i128,
    ) -> Result<(), GameHubError> {
        caller.require_auth();
        if reward_points <= 0 {
            return Err(GameHubError::InvalidPoints);
        }
        let ledger = env.ledger().sequence();
        let key = DataKey::CrossPromo(player.clone(), target_game.clone());
        let record = CrossPromoRecord {
            player: player.clone(),
            source_game: source_game.clone(),
            target_game: target_game.clone(),
            reward_points,
            claimed: false,
            registered_ledger: ledger,
        };
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);

        CrossPromoAwarded {
            player,
            source_game,
            target_game,
            reward_points,
        }
        .publish(&env);
        Ok(())
    }

    /// Claim a cross-promotion bonus points reward once the player joins the target game.
    pub fn claim_cross_promo(
        env: Env,
        player: Address,
        target_game: Address,
    ) -> Result<i128, GameHubError> {
        player.require_auth();
        let key = DataKey::CrossPromo(player.clone(), target_game);
        let mut promo: CrossPromoRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(GameHubError::PromoNotFound)?;
        if promo.claimed {
            return Err(GameHubError::PromoAlreadyClaimed);
        }
        promo.claimed = true;
        env.storage().persistent().set(&key, &promo);

        // Update player's total points
        let ledger = env.ledger().sequence();
        let mut profile = env
            .storage()
            .persistent()
            .get(&DataKey::Player(player.clone()))
            .unwrap_or_else(|| PlayerProfile {
                player: player.clone(),
                studio_id: String::from_str(&env, "guest"),
                games_played: 0,
                games_won: 0,
                total_points: 0,
                first_seen_ledger: ledger,
                last_active_ledger: ledger,
            });
        profile.total_points = profile.total_points.saturating_add(promo.reward_points);
        profile.last_active_ledger = ledger;
        env.storage()
            .persistent()
            .set(&DataKey::Player(player), &profile);

        Ok(promo.reward_points)
    }

    /// View cross promotion status for a player and target game.
    pub fn get_cross_promo(
        env: Env,
        player: Address,
        target_game: Address,
    ) -> Option<CrossPromoRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::CrossPromo(player, target_game))
    }

    /// Start game session (compatible with Stellar Game Studio standard).
    ///
    /// Only a game contract registered by the hub admin may report a session.
    /// A direct contract-to-contract call from that game automatically
    /// authorizes its own address, while arbitrary accounts cannot authorize
    /// as a contract address.
    pub fn start_game(
        env: Env,
        game_id: Address,
        session_id: u32,
        player1: Address,
        player2: Address,
        player1_points: i128,
        player2_points: i128,
    ) -> Result<(), GameHubError> {
        game_id.require_auth();
        let game: GameInfo = env
            .storage()
            .persistent()
            .get(&DataKey::Game(game_id.clone()))
            .ok_or(GameHubError::GameNotFound)?;
        if !game.active {
            return Err(GameHubError::GameInactive);
        }

        let key = DataKey::Session(session_id);
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, GameSession>(&key)
        {
            if existing.active {
                return Err(GameHubError::SessionAlreadyActive);
            }
        }

        let ledger = env.ledger().sequence();
        let session = GameSession {
            session_id,
            game_id: game_id.clone(),
            player1: player1.clone(),
            player2: player2.clone(),
            player1_points,
            player2_points,
            active: true,
            start_ledger: ledger,
            player1_won: None,
        };

        env.storage().persistent().set(&key, &session);
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);

        // Update stats for both players
        Self::update_player_activity(&env, &player1, player1_points);
        Self::update_player_activity(&env, &player2, player2_points);

        GameStarted {
            session_id,
            caller: game_id,
            game_id: game.game_id,
            player1,
            player2,
            player1_points,
            player2_points,
        }
        .publish(&env);

        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
        Ok(())
    }

    /// End game session (compatible with Stellar Game Studio standard).
    /// Only the game contract that started the session may end it.
    pub fn end_game(env: Env, session_id: u32, player1_won: bool) -> Result<(), GameHubError> {
        let key = DataKey::Session(session_id);
        let mut session: GameSession = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(GameHubError::SessionNotFound)?;
        session.game_id.require_auth();
        if !session.active {
            return Err(GameHubError::SessionAlreadyEnded);
        }

        session.active = false;
        session.player1_won = Some(player1_won);
        env.storage().persistent().set(&key, &session);
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);

        let winner = if player1_won {
            &session.player1
        } else {
            &session.player2
        };
        Self::record_player_win(&env, winner);

        GameEnded {
            session_id,
            caller: session.game_id,
            player1_won,
        }
        .publish(&env);
        Ok(())
    }

    /// Query a game session record.
    pub fn get_session(env: Env, session_id: u32) -> Option<GameSession> {
        env.storage().persistent().get(&DataKey::Session(session_id))
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), GameHubError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(GameHubError::NotInitialized)?;
        if admin != *caller {
            return Err(GameHubError::Unauthorized);
        }
        Ok(())
    }

    fn update_player_activity(env: &Env, player: &Address, points: i128) {
        let ledger = env.ledger().sequence();
        let mut profile = env
            .storage()
            .persistent()
            .get(&DataKey::Player(player.clone()))
            .unwrap_or_else(|| PlayerProfile {
                player: player.clone(),
                studio_id: String::from_str(env, "guest"),
                games_played: 0,
                games_won: 0,
                total_points: 0,
                first_seen_ledger: ledger,
                last_active_ledger: ledger,
            });
        profile.games_played = profile.games_played.saturating_add(1);
        profile.total_points = profile.total_points.saturating_add(points);
        profile.last_active_ledger = ledger;

        env.storage()
            .persistent()
            .set(&DataKey::Player(player.clone()), &profile);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Player(player.clone()), TTL_THRESHOLD, TTL_EXTEND);
    }

    fn record_player_win(env: &Env, player: &Address) {
        let ledger = env.ledger().sequence();
        if let Some(mut profile) = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerProfile>(&DataKey::Player(player.clone()))
        {
            profile.games_won = profile.games_won.saturating_add(1);
            profile.last_active_ledger = ledger;
            env.storage()
                .persistent()
                .set(&DataKey::Player(player.clone()), &profile);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, IntoVal, Symbol};

    fn setup_registered_game<'a>(client: &MockGameHubClient<'a>, env: &Env) -> Address {
        let admin = Address::generate(env);
        client.initialize(&admin);
        let game_id = Address::generate(env);
        client.register_game(
            &admin,
            &game_id,
            &String::from_str(env, "StellPoker"),
            &String::from_str(env, "ipfs://stellpoker-meta"),
        );
        game_id
    }

    #[test]
    fn test_start_and_end_game() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let game_id = setup_registered_game(&client, &env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        client.start_game(&game_id, &1, &player1, &player2, &1000, &1000);

        let s1 = client.get_session(&1).unwrap();
        assert!(s1.active);
        assert_eq!(s1.player1_points, 1000);
        assert_eq!(s1.game_id, game_id);

        client.end_game(&1, &true);
        let s2 = client.get_session(&1).unwrap();
        assert!(!s2.active);
        assert_eq!(s2.player1_won, Some(true));

        let p1_profile = client.get_player_profile(&player1).unwrap();
        assert_eq!(p1_profile.games_played, 1);
        assert_eq!(p1_profile.games_won, 1);

        let p2_profile = client.get_player_profile(&player2).unwrap();
        assert_eq!(p2_profile.games_played, 1);
        assert_eq!(p2_profile.games_won, 0);

        // The authenticated game contract is recorded as the caller in both
        // lifecycle events so indexers can verify provenance.
        let events = env.events().all();
        assert_eq!(events.len(), 3); // GameRegistered, GameStarted, GameEnded
        let start_event = events.get(1).unwrap();
        let end_event = events.get(2).unwrap();
        assert_eq!(
            start_event.1.get(0).unwrap(),
            Symbol::new(&env, "game_started").into_val(&env)
        );
        assert_eq!(
            end_event.1.get(0).unwrap(),
            Symbol::new(&env, "game_ended").into_val(&env)
        );
        let expected_caller: soroban_sdk::Val = game_id.into_val(&env);
        let start_data: soroban_sdk::Map<Symbol, soroban_sdk::Val> = start_event.2.into_val(&env);
        let end_data: soroban_sdk::Map<Symbol, soroban_sdk::Val> = end_event.2.into_val(&env);
        assert_eq!(start_data.get(Symbol::new(&env, "caller")), Some(expected_caller.clone()));
        assert_eq!(end_data.get(Symbol::new(&env, "caller")), Some(expected_caller));
    }

    #[test]
    fn unregistered_game_cannot_start_session() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let _ = setup_registered_game(&client, &env);

        let stranger_game = Address::generate(&env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        assert_eq!(
            client.try_start_game(&stranger_game, &2, &player1, &player2, &1, &1),
            Err(Ok(GameHubError::GameNotFound))
        );
        assert!(client.get_session(&2).is_none());
    }

    #[test]
    fn unauthenticated_cannot_start_session() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let game_id = setup_registered_game(&client, &env);

        // Without a signature/authorization for `game_id`, the call is rejected
        // even though the game is registered.
        env.set_auths(&[]);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        assert!(client
            .try_start_game(&game_id, &3, &player1, &player2, &1, &1)
            .is_err());
        assert!(client.get_session(&3).is_none());
    }

    #[test]
    fn unauthenticated_cannot_end_session() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let game_id = setup_registered_game(&client, &env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        client.start_game(&game_id, &4, &player1, &player2, &1, &1);

        env.set_auths(&[]);
        assert!(client.try_end_game(&4, &true).is_err());
        assert!(client.get_session(&4).unwrap().active);
    }

    #[test]
    fn end_game_rejects_unknown_and_finished_sessions() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let game_id = setup_registered_game(&client, &env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);

        assert_eq!(client.try_end_game(&99, &true), Err(Ok(GameHubError::SessionNotFound)));
        client.start_game(&game_id, &5, &player1, &player2, &1, &1);
        client.end_game(&5, &true);
        assert_eq!(client.try_end_game(&5, &true), Err(Ok(GameHubError::SessionAlreadyEnded)));
    }

    #[test]
    fn test_game_registry_and_catalog() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let game_addr = Address::generate(&env);

        client.initialize(&admin);
        client.register_game(
            &admin,
            &game_addr,
            &String::from_str(&env, "StellPoker"),
            &String::from_str(&env, "ipfs://stellpoker-meta"),
        );

        let game = client.get_game(&game_addr).unwrap();
        assert_eq!(game.name, String::from_str(&env, "StellPoker"));
        assert!(game.active);

        let list = client.list_games();
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).unwrap(), game_addr);
    }

    #[test]
    fn test_player_identity_linking_and_cross_promo() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);

        let player = Address::generate(&env);
        let game_source = Address::generate(&env);
        let game_target = Address::generate(&env);

        // Link identity with Game Studio handle
        client.link_player(&player, &String::from_str(&env, "stellar_ninja_99"));
        let profile = client.get_player_profile(&player).unwrap();
        assert_eq!(profile.studio_id, String::from_str(&env, "stellar_ninja_99"));

        // Record cross promotion bonus for referral from source to target
        client.record_cross_promo(&game_source, &player, &game_source, &game_target, &500);
        let promo = client.get_cross_promo(&player, &game_target).unwrap();
        assert!(!promo.claimed);
        assert_eq!(promo.reward_points, 500);

        // Player claims reward upon joining target game
        let claimed = client.claim_cross_promo(&player, &game_target);
        assert_eq!(claimed, 500);

        // Verify promo is now claimed and points added to profile
        let promo_after = client.get_cross_promo(&player, &game_target).unwrap();
        assert!(promo_after.claimed);
        let updated_profile = client.get_player_profile(&player).unwrap();
        assert_eq!(updated_profile.total_points, 500);

        // Claiming again fails
        assert!(client.try_claim_cross_promo(&player, &game_target).is_err());
    }
}
