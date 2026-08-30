#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol, Vec,
};

pub const DEFAULT_EMERGENCY_TIMEOUT_LEDGERS: u32 = 17_280; // ~1 day (5s per ledger)

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerBalance {
    pub total_deposited: i128,
    pub available_balance: i128,
    pub locked_balance: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableSubAllocation {
    pub table_id: u32,
    pub allocated_amount: i128,
    pub last_action_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    EmergencyTimeoutLedgers,
    AuthorizedCaller(Address), // Table contract or coordinator
    Balance(Address, Address), // (Player, Token) -> PlayerBalance
    TableAllocation(Address, Address, u32), // (Player, Token, TableId) -> i128
    PlayerTables(Address, Address), // (Player, Token) -> Vec<u32>
    TablePlayers(Address, u32), // (Token, TableId) -> Vec<Address>
    TableTotalAllocation(Address, u32), // (Token, TableId) -> i128
    TableLastActivity(u32), // TableId -> u32 (ledger sequence)
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LiquidityPoolError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InsufficientAvailableBalance = 4,
    InsufficientTableAllocation = 5,
    InvalidAmount = 6,
    TableNotFound = 7,
    SettlementMismatch = 8,
    EmergencyTimeoutNotElapsed = 9,
}

#[contract]
pub struct CrossTableLiquidityPool;

#[contractimpl]
impl CrossTableLiquidityPool {
    /// Initialize the cross-table shared liquidity pool
    pub fn initialize(
        env: Env,
        admin: Address,
        emergency_timeout_ledgers: Option<u32>,
    ) -> Result<(), LiquidityPoolError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(LiquidityPoolError::AlreadyInitialized);
        }

        let timeout = emergency_timeout_ledgers.unwrap_or(DEFAULT_EMERGENCY_TIMEOUT_LEDGERS);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::EmergencyTimeoutLedgers, &timeout);
        env.storage().instance().extend_ttl(100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "pool_initialized"),),
            (admin, timeout),
        );
        Ok(())
    }

    /// Admin authorizes a table contract or coordinator to allocate and settle funds
    pub fn set_authorized_caller(
        env: Env,
        admin: Address,
        caller: Address,
        authorized: bool,
    ) -> Result<(), LiquidityPoolError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let key = DataKey::AuthorizedCaller(caller.clone());
        if authorized {
            env.storage().persistent().set(&key, &true);
            env.storage().persistent().extend_ttl(&key, 100_000, 100_000);
        } else {
            env.storage().persistent().remove(&key);
        }

        env.events().publish(
            (Symbol::new(&env, "authorized_caller_set"),),
            (caller, authorized),
        );
        Ok(())
    }

    /// Check if caller is authorized
    pub fn is_authorized_caller(env: Env, caller: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::AuthorizedCaller(caller))
            .unwrap_or(false)
    }

    // ==========================================
    // Player Deposit & Withdrawal
    // ==========================================

    /// Deposit tokens (XLM or SAC token) into the shared bankroll pool
    pub fn deposit(
        env: Env,
        player: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), LiquidityPoolError> {
        player.require_auth();
        if amount <= 0 {
            return Err(LiquidityPoolError::InvalidAmount);
        }

        // Transfer funds from player to liquidity pool contract
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&player, &env.current_contract_address(), &amount);

        // Update balance
        let key = DataKey::Balance(player.clone(), token.clone());
        let mut bal = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerBalance>(&key)
            .unwrap_or(PlayerBalance {
                total_deposited: 0,
                available_balance: 0,
                locked_balance: 0,
            });

        bal.total_deposited = bal.total_deposited.checked_add(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        bal.available_balance = bal.available_balance.checked_add(amount).ok_or(LiquidityPoolError::InvalidAmount)?;

        env.storage().persistent().set(&key, &bal);
        env.storage().persistent().extend_ttl(&key, 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "pool_deposit"),),
            (player, token, amount, bal.available_balance),
        );
        Ok(())
    }

    /// Withdraw unlocked funds from the shared pool back to player's wallet
    pub fn withdraw(
        env: Env,
        player: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), LiquidityPoolError> {
        player.require_auth();
        if amount <= 0 {
            return Err(LiquidityPoolError::InvalidAmount);
        }

        let key = DataKey::Balance(player.clone(), token.clone());
        let mut bal = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerBalance>(&key)
            .ok_or(LiquidityPoolError::InsufficientAvailableBalance)?;

        if bal.available_balance < amount {
            return Err(LiquidityPoolError::InsufficientAvailableBalance);
        }

        bal.available_balance = bal.available_balance.checked_sub(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        bal.total_deposited = bal.total_deposited.checked_sub(amount).ok_or(LiquidityPoolError::InvalidAmount)?;

        env.storage().persistent().set(&key, &bal);
        env.storage().persistent().extend_ttl(&key, 100_000, 100_000);

        // Transfer funds from contract to player
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &player, &amount);

        env.events().publish(
            (Symbol::new(&env, "pool_withdraw"),),
            (player, token, amount, bal.available_balance),
        );
        Ok(())
    }

    // ==========================================
    // Cross-Table Sub-Allocations
    // ==========================================

    /// Allocate bankroll from player's available pool balance to a specific poker table
    pub fn allocate_to_table(
        env: Env,
        caller: Address,
        player: Address,
        token: Address,
        table_id: u32,
        amount: i128,
    ) -> Result<(), LiquidityPoolError> {
        caller.require_auth();
        Self::require_player_or_authorized(&env, &caller, &player)?;

        if amount <= 0 {
            return Err(LiquidityPoolError::InvalidAmount);
        }

        let bal_key = DataKey::Balance(player.clone(), token.clone());
        let mut bal = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerBalance>(&bal_key)
            .ok_or(LiquidityPoolError::InsufficientAvailableBalance)?;

        if bal.available_balance < amount {
            return Err(LiquidityPoolError::InsufficientAvailableBalance);
        }

        bal.available_balance = bal.available_balance.checked_sub(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        bal.locked_balance = bal.locked_balance.checked_add(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        env.storage().persistent().set(&bal_key, &bal);
        env.storage().persistent().extend_ttl(&bal_key, 100_000, 100_000);

        // Update table sub-allocation
        let alloc_key = DataKey::TableAllocation(player.clone(), token.clone(), table_id);
        let cur_alloc = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&alloc_key)
            .unwrap_or(0);
        let new_alloc = cur_alloc.checked_add(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        env.storage().persistent().set(&alloc_key, &new_alloc);
        env.storage().persistent().extend_ttl(&alloc_key, 100_000, 100_000);

        // Update player's active tables index
        Self::add_player_table(&env, &player, &token, table_id);
        // Update table's active players index
        Self::add_table_player(&env, &token, table_id, &player);

        // Update table total allocation
        let total_alloc_key = DataKey::TableTotalAllocation(token.clone(), table_id);
        let cur_total = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&total_alloc_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&total_alloc_key, &(cur_total + amount));

        // Mark table activity
        env.storage()
            .persistent()
            .set(&DataKey::TableLastActivity(table_id), &env.ledger().sequence());

        env.events().publish(
            (Symbol::new(&env, "table_allocated"),),
            (player, token, table_id, amount, new_alloc),
        );
        Ok(())
    }

    /// Deallocate funds from a table back to player's available bankroll
    pub fn deallocate_from_table(
        env: Env,
        caller: Address,
        player: Address,
        token: Address,
        table_id: u32,
        amount: i128,
    ) -> Result<(), LiquidityPoolError> {
        caller.require_auth();
        Self::require_player_or_authorized(&env, &caller, &player)?;

        if amount <= 0 {
            return Err(LiquidityPoolError::InvalidAmount);
        }

        let alloc_key = DataKey::TableAllocation(player.clone(), token.clone(), table_id);
        let cur_alloc = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&alloc_key)
            .unwrap_or(0);

        if cur_alloc < amount {
            return Err(LiquidityPoolError::InsufficientTableAllocation);
        }

        let new_alloc = cur_alloc.checked_sub(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        if new_alloc == 0 {
            env.storage().persistent().remove(&alloc_key);
            Self::remove_player_table(&env, &player, &token, table_id);
            Self::remove_table_player(&env, &token, table_id, &player);
        } else {
            env.storage().persistent().set(&alloc_key, &new_alloc);
        }

        // Update balances
        let bal_key = DataKey::Balance(player.clone(), token.clone());
        let mut bal = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerBalance>(&bal_key)
            .ok_or(LiquidityPoolError::InsufficientAvailableBalance)?;

        bal.locked_balance = bal.locked_balance.checked_sub(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        bal.available_balance = bal.available_balance.checked_add(amount).ok_or(LiquidityPoolError::InvalidAmount)?;
        env.storage().persistent().set(&bal_key, &bal);

        // Update table total
        let total_alloc_key = DataKey::TableTotalAllocation(token.clone(), table_id);
        let cur_total = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&total_alloc_key)
            .unwrap_or(0);
        let next_total = if cur_total >= amount { cur_total - amount } else { 0 };
        env.storage().persistent().set(&total_alloc_key, &next_total);

        // Mark table activity
        env.storage()
            .persistent()
            .set(&DataKey::TableLastActivity(table_id), &env.ledger().sequence());

        env.events().publish(
            (Symbol::new(&env, "table_deallocated"),),
            (player, token, table_id, amount, new_alloc),
        );
        Ok(())
    }

    /// Settle a completed hand across multiple players' sub-allocations directly in the shared pool
    pub fn settle_table_hand(
        env: Env,
        caller: Address,
        token: Address,
        table_id: u32,
        winners: Vec<(Address, i128)>,
        losers: Vec<(Address, i128)>,
        rake: i128,
        rake_recipient: Option<Address>,
    ) -> Result<(), LiquidityPoolError> {
        caller.require_auth();
        Self::require_authorized(&env, &caller)?;

        let mut total_lost: i128 = 0;
        let mut total_won: i128 = 0;

        // Process losers: reduce table allocation, locked balance, and total deposited
        for i in 0..losers.len() {
            let (loser, loss) = losers.get(i).unwrap();
            if loss <= 0 {
                continue;
            }
            total_lost = total_lost.checked_add(loss).ok_or(LiquidityPoolError::InvalidAmount)?;

            let alloc_key = DataKey::TableAllocation(loser.clone(), token.clone(), table_id);
            let cur_alloc = env
                .storage()
                .persistent()
                .get::<DataKey, i128>(&alloc_key)
                .unwrap_or(0);
            if cur_alloc < loss {
                return Err(LiquidityPoolError::InsufficientTableAllocation);
            }
            let next_alloc = cur_alloc - loss;
            if next_alloc == 0 {
                env.storage().persistent().remove(&alloc_key);
                Self::remove_player_table(&env, &loser, &token, table_id);
                Self::remove_table_player(&env, &token, table_id, &loser);
            } else {
                env.storage().persistent().set(&alloc_key, &next_alloc);
            }

            let bal_key = DataKey::Balance(loser.clone(), token.clone());
            let mut bal = env
                .storage()
                .persistent()
                .get::<DataKey, PlayerBalance>(&bal_key)
                .unwrap();
            bal.locked_balance = bal.locked_balance.saturating_sub(loss);
            bal.total_deposited = bal.total_deposited.saturating_sub(loss);
            env.storage().persistent().set(&bal_key, &bal);
        }

        // Process winners: increase table allocation, locked balance, and total deposited
        for i in 0..winners.len() {
            let (winner, win) = winners.get(i).unwrap();
            if win <= 0 {
                continue;
            }
            total_won = total_won.checked_add(win).ok_or(LiquidityPoolError::InvalidAmount)?;

            let alloc_key = DataKey::TableAllocation(winner.clone(), token.clone(), table_id);
            let cur_alloc = env
                .storage()
                .persistent()
                .get::<DataKey, i128>(&alloc_key)
                .unwrap_or(0);
            let next_alloc = cur_alloc + win;
            env.storage().persistent().set(&alloc_key, &next_alloc);
            Self::add_player_table(&env, &winner, &token, table_id);
            Self::add_table_player(&env, &token, table_id, &winner);

            let bal_key = DataKey::Balance(winner.clone(), token.clone());
            let mut bal = env
                .storage()
                .persistent()
                .get::<DataKey, PlayerBalance>(&bal_key)
                .unwrap_or(PlayerBalance {
                    total_deposited: 0,
                    available_balance: 0,
                    locked_balance: 0,
                });
            bal.locked_balance += win;
            bal.total_deposited += win;
            env.storage().persistent().set(&bal_key, &bal);
        }

        // Validate conservation: total_lost == total_won + rake
        if total_lost != total_won.checked_add(rake).ok_or(LiquidityPoolError::InvalidAmount)? {
            return Err(LiquidityPoolError::SettlementMismatch);
        }

        // Transfer rake if applicable
        if rake > 0 {
            if let Some(recipient) = rake_recipient {
                let token_client = token::Client::new(&env, &token);
                token_client.transfer(&env.current_contract_address(), &recipient, &rake);
            }
        }

        // Update table last activity
        env.storage()
            .persistent()
            .set(&DataKey::TableLastActivity(table_id), &env.ledger().sequence());

        env.events().publish(
            (Symbol::new(&env, "table_hand_settled"),),
            (token, table_id, total_won, rake),
        );
        Ok(())
    }

    /// Emergency unlock: if a table has been inactive for >= emergency_timeout_ledgers,
    /// the player can reclaim their locked allocation back to available balance.
    pub fn emergency_unlock(
        env: Env,
        player: Address,
        token: Address,
        table_id: u32,
    ) -> Result<(), LiquidityPoolError> {
        player.require_auth();

        let alloc_key = DataKey::TableAllocation(player.clone(), token.clone(), table_id);
        let cur_alloc = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&alloc_key)
            .unwrap_or(0);
        if cur_alloc == 0 {
            return Err(LiquidityPoolError::InsufficientTableAllocation);
        }

        let last_activity = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::TableLastActivity(table_id))
            .unwrap_or(0);
        let timeout = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::EmergencyTimeoutLedgers)
            .unwrap_or(DEFAULT_EMERGENCY_TIMEOUT_LEDGERS);

        let current_ledger = env.ledger().sequence();
        if current_ledger < last_activity.saturating_add(timeout) {
            return Err(LiquidityPoolError::EmergencyTimeoutNotElapsed);
        }

        // Clear sub-allocation
        env.storage().persistent().remove(&alloc_key);
        Self::remove_player_table(&env, &player, &token, table_id);
        Self::remove_table_player(&env, &token, table_id, &player);

        // Move locked to available
        let bal_key = DataKey::Balance(player.clone(), token.clone());
        let mut bal = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerBalance>(&bal_key)
            .ok_or(LiquidityPoolError::InsufficientAvailableBalance)?;

        bal.locked_balance = bal.locked_balance.saturating_sub(cur_alloc);
        bal.available_balance = bal.available_balance.saturating_add(cur_alloc);
        env.storage().persistent().set(&bal_key, &bal);

        env.events().publish(
            (Symbol::new(&env, "emergency_unlocked"),),
            (player, token, table_id, cur_alloc),
        );
        Ok(())
    }

    // ==========================================
    // Query Functions
    // ==========================================

    /// Get overall player balance
    pub fn get_player_balance(env: Env, player: Address, token: Address) -> PlayerBalance {
        env.storage()
            .persistent()
            .get::<DataKey, PlayerBalance>(&DataKey::Balance(player, token))
            .unwrap_or(PlayerBalance {
                total_deposited: 0,
                available_balance: 0,
                locked_balance: 0,
            })
    }

    /// Get table allocation for a player
    pub fn get_table_allocation(
        env: Env,
        player: Address,
        token: Address,
        table_id: u32,
    ) -> i128 {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TableAllocation(player, token, table_id))
            .unwrap_or(0)
    }

    /// Get list of tables where player has an active sub-allocation
    pub fn get_player_tables(env: Env, player: Address, token: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get::<DataKey, Vec<u32>>(&DataKey::PlayerTables(player, token))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Get total allocation across all players for a specific table
    pub fn get_table_total_allocation(env: Env, token: Address, table_id: u32) -> i128 {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TableTotalAllocation(token, table_id))
            .unwrap_or(0)
    }

    // ==========================================
    // Internal Indexing Helpers
    // ==========================================

    fn require_admin(env: &Env, admin: &Address) -> Result<(), LiquidityPoolError> {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LiquidityPoolError::NotInitialized)?;
        if stored_admin != *admin {
            return Err(LiquidityPoolError::Unauthorized);
        }
        Ok(())
    }

    fn require_authorized(env: &Env, caller: &Address) -> Result<(), LiquidityPoolError> {
        let is_auth = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::AuthorizedCaller(caller.clone()))
            .unwrap_or(false);
        if !is_auth {
            let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
            if let Some(adm) = admin {
                if adm == *caller {
                    return Ok(());
                }
            }
            return Err(LiquidityPoolError::Unauthorized);
        }
        Ok(())
    }

    fn require_player_or_authorized(
        env: &Env,
        caller: &Address,
        player: &Address,
    ) -> Result<(), LiquidityPoolError> {
        if caller == player {
            return Ok(());
        }
        Self::require_authorized(env, caller)
    }

    fn add_player_table(env: &Env, player: &Address, token: &Address, table_id: u32) {
        let key = DataKey::PlayerTables(player.clone(), token.clone());
        let mut tables: Vec<u32> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..tables.len() {
            if tables.get(i).unwrap() == table_id {
                return;
            }
        }
        tables.push_back(table_id);
        env.storage().persistent().set(&key, &tables);
    }

    fn remove_player_table(env: &Env, player: &Address, token: &Address, table_id: u32) {
        let key = DataKey::PlayerTables(player.clone(), token.clone());
        let tables: Vec<u32> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        let mut next: Vec<u32> = Vec::new(env);
        for i in 0..tables.len() {
            let t = tables.get(i).unwrap();
            if t != table_id {
                next.push_back(t);
            }
        }
        env.storage().persistent().set(&key, &next);
    }

    fn add_table_player(env: &Env, token: &Address, table_id: u32, player: &Address) {
        let key = DataKey::TablePlayers(token.clone(), table_id);
        let mut players: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..players.len() {
            if players.get(i).unwrap() == *player {
                return;
            }
        }
        players.push_back(player.clone());
        env.storage().persistent().set(&key, &players);
    }

    fn remove_table_player(env: &Env, token: &Address, table_id: u32, player: &Address) {
        let key = DataKey::TablePlayers(token.clone(), table_id);
        let players: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        let mut next: Vec<Address> = Vec::new(env);
        for i in 0..players.len() {
            let p = players.get(i).unwrap();
            if p != *player {
                next.push_back(p);
            }
        }
        env.storage().persistent().set(&key, &next);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, Vec};

    #[test]
    fn test_deposit_withdraw_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let player = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_client = token::Client::new(&env, &token_id);
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);

        token_admin_client.mint(&player, &5000);

        let contract_id = env.register(CrossTableLiquidityPool, ());
        let client = CrossTableLiquidityPoolClient::new(&env, &contract_id);
        client.initialize(&admin, &Some(100));

        // Deposit 3000 into pool
        client.deposit(&player, &token_id, &3000);
        let bal = client.get_player_balance(&player, &token_id);
        assert_eq!(bal.total_deposited, 3000);
        assert_eq!(bal.available_balance, 3000);
        assert_eq!(bal.locked_balance, 0);
        assert_eq!(token_client.balance(&contract_id), 3000);

        // Withdraw 1000
        client.withdraw(&player, &token_id, &1000);
        let bal2 = client.get_player_balance(&player, &token_id);
        assert_eq!(bal2.total_deposited, 2000);
        assert_eq!(bal2.available_balance, 2000);
        assert_eq!(token_client.balance(&player), 3000);
    }

    #[test]
    fn test_insufficient_withdrawal_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let player = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&player, &1000);

        let contract_id = env.register(CrossTableLiquidityPool, ());
        let client = CrossTableLiquidityPoolClient::new(&env, &contract_id);
        client.initialize(&admin, &Some(100));

        client.deposit(&player, &token_id, &500);
        assert!(client.try_withdraw(&player, &token_id, &600).is_err());
    }

    #[test]
    fn test_cross_table_allocations_and_settlement() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let table_contract = Address::generate(&env);
        let player1 = Address::generate(&env);
        let player2 = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&player1, &10_000);
        token_admin_client.mint(&player2, &10_000);

        let contract_id = env.register(CrossTableLiquidityPool, ());
        let client = CrossTableLiquidityPoolClient::new(&env, &contract_id);
        client.initialize(&admin, &Some(100));
        client.set_authorized_caller(&admin, &table_contract, &true);

        // Player 1 deposits 5000, Player 2 deposits 5000
        client.deposit(&player1, &token_id, &5000);
        client.deposit(&player2, &token_id, &5000);

        // Allocate to Table 1: P1 allocates 1000, P2 allocates 1000
        client.allocate_to_table(&player1, &player1, &token_id, &1, &1000);
        client.allocate_to_table(&player2, &player2, &token_id, &1, &1000);

        // Allocate to Table 2: P1 allocates 2000
        client.allocate_to_table(&player1, &player1, &token_id, &2, &2000);

        // Check P1 balance
        let p1_bal = client.get_player_balance(&player1, &token_id);
        assert_eq!(p1_bal.total_deposited, 5000);
        assert_eq!(p1_bal.available_balance, 2000); // 5000 - 1000 - 2000
        assert_eq!(p1_bal.locked_balance, 3000);

        let p1_tables = client.get_player_tables(&player1, &token_id);
        assert_eq!(p1_tables.len(), 2);

        // Settle a hand at Table 1: Player 1 wins 500 from Player 2 (rake 25)
        let mut winners = Vec::new(&env);
        winners.push_back((player1.clone(), 475));
        let mut losers = Vec::new(&env);
        losers.push_back((player2.clone(), 500));

        let rake_treasury = Address::generate(&env);
        client.settle_table_hand(&table_contract, &token_id, &1, &winners, &losers, &25, &Some(rake_treasury.clone()));

        assert_eq!(client.get_table_allocation(&player1, &token_id, &1), 1475);
        assert_eq!(client.get_table_allocation(&player2, &token_id, &1), 500);

        let token_client = token::Client::new(&env, &token_id);
        assert_eq!(token_client.balance(&rake_treasury), 25);

        // Deallocate remaining funds from Table 1 for Player 2
        client.deallocate_from_table(&player2, &player2, &token_id, &1, &500);
        assert_eq!(client.get_table_allocation(&player2, &token_id, &1), 0);
        let p2_bal = client.get_player_balance(&player2, &token_id);
        assert_eq!(p2_bal.available_balance, 4500);
        assert_eq!(p2_bal.locked_balance, 0);
    }

    #[test]
    fn test_emergency_unlock_after_timeout() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_sequence_number(100);

        let admin = Address::generate(&env);
        let player = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&player, &5000);

        let contract_id = env.register(CrossTableLiquidityPool, ());
        let client = CrossTableLiquidityPoolClient::new(&env, &contract_id);
        client.initialize(&admin, &Some(50)); // 50 ledgers timeout

        client.deposit(&player, &token_id, &2000);
        client.allocate_to_table(&player, &player, &token_id, &99, &1500);

        assert_eq!(client.get_table_allocation(&player, &token_id, &99), 1500);

        // Attempting emergency unlock before 50 ledgers fails
        env.ledger().set_sequence_number(120);
        assert!(client.try_emergency_unlock(&player, &token_id, &99).is_err());

        // Fast forward sequence past timeout
        env.ledger().set_sequence_number(160);
        client.emergency_unlock(&player, &token_id, &99);

        assert_eq!(client.get_table_allocation(&player, &token_id, &99), 0);
        let bal = client.get_player_balance(&player, &token_id);
        assert_eq!(bal.available_balance, 2000);
        assert_eq!(bal.locked_balance, 0);
    }
}
