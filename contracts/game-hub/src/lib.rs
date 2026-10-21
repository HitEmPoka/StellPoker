#![no_std]

use soroban_sdk::{contract, contractevent, contractimpl, Address, Env};

/// Mock Game Hub contract matching the Stellar Game Studio interface.
/// In production, the real Game Hub lives at CB4VZAT2U3UC6XFK3N23SKRF2NDCMP3QHJYMCHHFMZO7MRQO6DQ2EMYG
#[contract]
pub struct MockGameHub;

#[contractevent]
pub struct GameStarted {
    pub session_id: u32,
    pub game_id: Address,
    pub player1: Address,
    pub player2: Address,
    pub player1_points: i128,
    pub player2_points: i128,
}

#[contractevent]
pub struct GameEnded {
    pub session_id: u32,
    pub player1_won: bool,
}

#[contractimpl]
impl MockGameHub {
    pub fn start_game(
        env: Env,
        game_id: Address,
        session_id: u32,
        player1: Address,
        player2: Address,
        player1_points: i128,
        player2_points: i128,
    ) {
        GameStarted {
            session_id,
            game_id,
            player1,
            player2,
            player1_points,
            player2_points,
        }
        .publish(&env);
        env.storage().instance().extend_ttl(17_280, 518_400);
    }

    /// End game.
    ///
    /// # Parameters
    /// - `session_id`: parameter
    /// - `player1_won`: parameter
    ///
    /// # Returns
    /// - `()`
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    ///
    /// # Authorization
    /// Requires appropriate authorization.
    pub fn end_game(env: Env, session_id: u32, player1_won: bool) {
        GameEnded {
            session_id,
            player1_won,
        }
        .publish(&env);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_start_and_end_game() {
        let env = Env::default();
        let contract_id = env.register(MockGameHub, ());
        let client = MockGameHubClient::new(&env, &contract_id);
        let game_id = Address::generate(&env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        client.start_game(&game_id, &1, &player1, &player2, &1000, &1000);
        client.end_game(&1, &true);
    }
}
