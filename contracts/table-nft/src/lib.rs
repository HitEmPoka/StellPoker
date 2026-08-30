#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, String, Symbol, Vec,
};

pub const MAX_RAKE_BPS: u32 = 500; // 5% maximum rake
pub const MAX_PLAYERS_LIMIT: u32 = 6;
pub const SECONDS_PER_DAY: u64 = 86_400;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRules {
    pub min_buy_in: i128,
    pub max_buy_in: i128,
    pub min_players: u32,
    pub max_players: u32,
    pub rake_bps: u32,
    pub jackpot_share_bps: u32,
    pub allowed_tokens: Vec<Address>,
    pub action_timeout_seconds: u32,
    pub is_private: bool,
    pub allow_straddle: bool,
    pub allow_run_it_twice: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableAesthetics {
    pub table_name: String,
    pub felt_color: String,
    pub card_back_uri: String,
    pub background_uri: String,
    pub avatar_frame_uri: String,
    pub soundtrack_theme: String,
    pub custom_metadata_uri: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RentalListing {
    pub is_listed: bool,
    pub price_per_day: i128,
    pub min_duration_seconds: u64,
    pub max_duration_seconds: u64,
    pub payment_token: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRental {
    pub renter: Address,
    pub rent_price: i128,
    pub payment_token: Address,
    pub start_time: u64,
    pub expires_at: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Name,
    Symbol,
    TotalSupply,
    Owner(u32),
    Balance(Address),
    Approved(u32),
    OperatorApproval(Address, Address), // (Owner, Operator)
    Rules(u32),
    Aesthetics(u32),
    RentalListing(u32),
    ActiveRental(u32),
    TableIds,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TableNftError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    TokenNotFound = 4,
    TokenAlreadyExists = 5,
    RakeBpsExceedsMax = 6,
    InvalidTableConfig = 7,
    TableAlreadyRented = 8,
    TableNotRented = 9,
    RentalNotListed = 10,
    InvalidRentalDuration = 11,
    RentalExpired = 12,
    RentalStillActive = 13,
    CannotTransferRentedTable = 14,
    InvalidPayment = 15,
    InvalidAmount = 16,
}

#[contract]
pub struct TableNftContract;

#[contractimpl]
impl TableNftContract {
    /// Initialize Table NFT contract with metadata and admin
    pub fn initialize(
        env: Env,
        admin: Address,
        name: String,
        symbol: String,
    ) -> Result<(), TableNftError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TableNftError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        env.storage().instance().set(&DataKey::TotalSupply, &0u32);
        env.storage()
            .persistent()
            .set(&DataKey::TableIds, &Vec::<u32>::new(&env));
        env.storage().instance().extend_ttl(100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "nft_initialized"),),
            (admin, name, symbol),
        );
        Ok(())
    }

    /// Mint a new poker table NFT with initial custom rules and aesthetics
    pub fn mint(
        env: Env,
        admin: Address,
        to: Address,
        token_id: u32,
        rules: TableRules,
        aesthetics: TableAesthetics,
    ) -> Result<(), TableNftError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        if env
            .storage()
            .persistent()
            .has(&DataKey::Owner(token_id))
        {
            return Err(TableNftError::TokenAlreadyExists);
        }

        Self::validate_rules(&rules)?;

        // Set ownership
        env.storage()
            .persistent()
            .set(&DataKey::Owner(token_id), &to);
        
        let prev_balance = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(prev_balance + 1));

        // Set rules & aesthetics
        env.storage()
            .persistent()
            .set(&DataKey::Rules(token_id), &rules);
        env.storage()
            .persistent()
            .set(&DataKey::Aesthetics(token_id), &aesthetics);

        // Update supply and IDs list
        let supply: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &(supply + 1));

        let mut ids: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::TableIds)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(token_id);
        env.storage().persistent().set(&DataKey::TableIds, &ids);

        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Owner(token_id), 100_000, 100_000);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Rules(token_id), 100_000, 100_000);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Aesthetics(token_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "table_minted"),),
            (to, token_id),
        );
        Ok(())
    }

    /// Transfer table NFT ownership to another address.
    /// Fails if table is currently actively leased.
    pub fn transfer(
        env: Env,
        caller: Address,
        from: Address,
        to: Address,
        token_id: u32,
    ) -> Result<(), TableNftError> {
        caller.require_auth();
        let owner = Self::owner_of(env.clone(), token_id)?;
        if owner != from {
            return Err(TableNftError::Unauthorized);
        }

        Self::require_approved_or_owner(&env, &caller, &owner, token_id)?;

        // Ensure table is not actively leased
        if Self::is_rented(env.clone(), token_id) {
            return Err(TableNftError::CannotTransferRentedTable);
        }

        // Clear approval
        env.storage().persistent().remove(&DataKey::Approved(token_id));

        // Update balances
        let from_bal = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::Balance(from.clone()))
            .unwrap_or(1);
        let to_bal = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::Balance(to.clone()))
            .unwrap_or(0);

        if from_bal > 0 {
            env.storage()
                .persistent()
                .set(&DataKey::Balance(from.clone()), &(from_bal - 1));
        }
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_bal + 1));

        // Set new owner
        env.storage()
            .persistent()
            .set(&DataKey::Owner(token_id), &to);

        // Remove any open rental listings upon transfer
        env.storage()
            .persistent()
            .remove(&DataKey::RentalListing(token_id));

        env.events().publish(
            (Symbol::new(&env, "table_transferred"),),
            (from, to, token_id),
        );
        Ok(())
    }

    /// Approve an address to manage this table token
    pub fn approve(
        env: Env,
        caller: Address,
        approved: Option<Address>,
        token_id: u32,
    ) -> Result<(), TableNftError> {
        caller.require_auth();
        let owner = Self::owner_of(env.clone(), token_id)?;
        if caller != owner && !Self::is_approved_for_all(env.clone(), owner.clone(), caller.clone()) {
            return Err(TableNftError::Unauthorized);
        }

        if let Some(ref addr) = approved {
            env.storage()
                .persistent()
                .set(&DataKey::Approved(token_id), addr);
        } else {
            env.storage().persistent().remove(&DataKey::Approved(token_id));
        }

        env.events().publish(
            (Symbol::new(&env, "table_approved"),),
            (owner, approved, token_id),
        );
        Ok(())
    }

    /// Set or unset operator approval for all tokens owned by caller
    pub fn set_approval_for_all(
        env: Env,
        owner: Address,
        operator: Address,
        approved: bool,
    ) -> Result<(), TableNftError> {
        owner.require_auth();
        let key = DataKey::OperatorApproval(owner.clone(), operator.clone());
        if approved {
            env.storage().persistent().set(&key, &true);
        } else {
            env.storage().persistent().remove(&key);
        }
        env.events().publish(
            (Symbol::new(&env, "approval_for_all"),),
            (owner, operator, approved),
        );
        Ok(())
    }

    /// Get owner of token
    pub fn owner_of(env: Env, token_id: u32) -> Result<Address, TableNftError> {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Owner(token_id))
            .ok_or(TableNftError::TokenNotFound)
    }

    /// Get balance of owner
    pub fn balance_of(env: Env, owner: Address) -> u32 {
        env.storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::Balance(owner))
            .unwrap_or(0)
    }

    /// Get approved address for a token
    pub fn get_approved(env: Env, token_id: u32) -> Option<Address> {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::Approved(token_id))
    }

    /// Check if operator is approved for all tokens of owner
    pub fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::OperatorApproval(owner, operator))
            .unwrap_or(false)
    }

    /// Total supply of minted tables
    pub fn total_supply(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::TotalSupply)
            .unwrap_or(0)
    }

    /// Get list of all table token IDs
    pub fn get_table_ids(env: Env) -> Vec<u32> {
        env.storage()
            .persistent()
            .get::<DataKey, Vec<u32>>(&DataKey::TableIds)
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ==========================================
    // Table Customization: Rules & Aesthetics
    // ==========================================

    /// Set table rules. Can be called by current effective operator or table owner.
    pub fn set_table_rules(
        env: Env,
        caller: Address,
        token_id: u32,
        rules: TableRules,
    ) -> Result<(), TableNftError> {
        caller.require_auth();
        Self::require_effective_operator_or_owner(&env, &caller, token_id)?;
        Self::validate_rules(&rules)?;

        env.storage()
            .persistent()
            .set(&DataKey::Rules(token_id), &rules);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Rules(token_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "table_rules_updated"),),
            (caller, token_id, rules.rake_bps),
        );
        Ok(())
    }

    /// Get table rules
    pub fn get_table_rules(env: Env, token_id: u32) -> Result<TableRules, TableNftError> {
        env.storage()
            .persistent()
            .get::<DataKey, TableRules>(&DataKey::Rules(token_id))
            .ok_or(TableNftError::TokenNotFound)
    }

    /// Set table aesthetics (felt color, cards, theme, music). Can be called by effective operator or owner.
    pub fn set_table_aesthetics(
        env: Env,
        caller: Address,
        token_id: u32,
        aesthetics: TableAesthetics,
    ) -> Result<(), TableNftError> {
        caller.require_auth();
        Self::require_effective_operator_or_owner(&env, &caller, token_id)?;

        env.storage()
            .persistent()
            .set(&DataKey::Aesthetics(token_id), &aesthetics);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Aesthetics(token_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "table_aesthetics_updated"),),
            (caller, token_id, aesthetics.table_name.clone()),
        );
        Ok(())
    }

    /// Get table aesthetics
    pub fn get_table_aesthetics(env: Env, token_id: u32) -> Result<TableAesthetics, TableNftError> {
        env.storage()
            .persistent()
            .get::<DataKey, TableAesthetics>(&DataKey::Aesthetics(token_id))
            .ok_or(TableNftError::TokenNotFound)
    }

    /// Returns the currently active operator (renter if leased and active; otherwise owner).
    pub fn get_effective_operator(env: Env, token_id: u32) -> Result<Address, TableNftError> {
        let owner = Self::owner_of(env.clone(), token_id)?;
        if let Some(rental) = env
            .storage()
            .persistent()
            .get::<DataKey, TableRental>(&DataKey::ActiveRental(token_id))
        {
            let current_time = env.ledger().timestamp();
            if rental.is_active && current_time < rental.expires_at {
                return Ok(rental.renter);
            }
        }
        Ok(owner)
    }

    // ==========================================
    // Table Rental / Operator Leasing System
    // ==========================================

    /// Table owner lists table for rent with daily rate, duration bounds, and payment token
    pub fn list_for_rent(
        env: Env,
        caller: Address,
        token_id: u32,
        price_per_day: i128,
        min_duration_seconds: u64,
        max_duration_seconds: u64,
        payment_token: Address,
    ) -> Result<(), TableNftError> {
        caller.require_auth();
        let owner = Self::owner_of(env.clone(), token_id)?;
        if caller != owner {
            return Err(TableNftError::Unauthorized);
        }
        if price_per_day <= 0 || min_duration_seconds == 0 || min_duration_seconds > max_duration_seconds {
            return Err(TableNftError::InvalidTableConfig);
        }
        if Self::is_rented(env.clone(), token_id) {
            return Err(TableNftError::TableAlreadyRented);
        }

        let listing = RentalListing {
            is_listed: true,
            price_per_day,
            min_duration_seconds,
            max_duration_seconds,
            payment_token: payment_token.clone(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::RentalListing(token_id), &listing);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::RentalListing(token_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "table_listed_for_rent"),),
            (owner, token_id, price_per_day, payment_token),
        );
        Ok(())
    }

    /// Delist a table from rental marketplace
    pub fn delist_rental(env: Env, caller: Address, token_id: u32) -> Result<(), TableNftError> {
        caller.require_auth();
        let owner = Self::owner_of(env.clone(), token_id)?;
        if caller != owner {
            return Err(TableNftError::Unauthorized);
        }
        env.storage()
            .persistent()
            .remove(&DataKey::RentalListing(token_id));
        env.events().publish(
            (Symbol::new(&env, "table_delisted_rental"),),
            (owner, token_id),
        );
        Ok(())
    }

    /// Operator rents table from marketplace for time-limited duration
    pub fn rent_table(
        env: Env,
        renter: Address,
        token_id: u32,
        duration_seconds: u64,
    ) -> Result<(), TableNftError> {
        renter.require_auth();
        let owner = Self::owner_of(env.clone(), token_id)?;
        if renter == owner {
            return Err(TableNftError::Unauthorized);
        }

        let listing: RentalListing = env
            .storage()
            .persistent()
            .get(&DataKey::RentalListing(token_id))
            .ok_or(TableNftError::RentalNotListed)?;

        if !listing.is_listed {
            return Err(TableNftError::RentalNotListed);
        }

        if duration_seconds < listing.min_duration_seconds
            || duration_seconds > listing.max_duration_seconds
        {
            return Err(TableNftError::InvalidRentalDuration);
        }

        if Self::is_rented(env.clone(), token_id) {
            return Err(TableNftError::TableAlreadyRented);
        }

        // Calculate rent payment: price_per_day * duration / 86400
        let total_rent = (listing.price_per_day * (duration_seconds as i128)) / (SECONDS_PER_DAY as i128);
        let rent_amount = if total_rent < 1 { 1 } else { total_rent };

        // Transfer payment from renter to table owner
        let token_client = token::Client::new(&env, &listing.payment_token);
        token_client.transfer(&renter, &owner, &rent_amount);

        let now = env.ledger().timestamp();
        let expires_at = now.saturating_add(duration_seconds);

        let rental = TableRental {
            renter: renter.clone(),
            rent_price: rent_amount,
            payment_token: listing.payment_token.clone(),
            start_time: now,
            expires_at,
            is_active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::ActiveRental(token_id), &rental);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ActiveRental(token_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "table_rented"),),
            (owner, renter, token_id, rent_amount, expires_at),
        );
        Ok(())
    }

    /// Direct private lease from table owner to a specific operator
    pub fn direct_lease(
        env: Env,
        owner: Address,
        token_id: u32,
        renter: Address,
        duration_seconds: u64,
        rent_fee: i128,
        payment_token: Address,
    ) -> Result<(), TableNftError> {
        owner.require_auth();
        let stored_owner = Self::owner_of(env.clone(), token_id)?;
        if owner != stored_owner {
            return Err(TableNftError::Unauthorized);
        }
        if duration_seconds == 0 {
            return Err(TableNftError::InvalidRentalDuration);
        }
        if Self::is_rented(env.clone(), token_id) {
            return Err(TableNftError::TableAlreadyRented);
        }

        if rent_fee > 0 {
            renter.require_auth();
            let token_client = token::Client::new(&env, &payment_token);
            token_client.transfer(&renter, &owner, &rent_fee);
        }

        let now = env.ledger().timestamp();
        let expires_at = now.saturating_add(duration_seconds);

        let rental = TableRental {
            renter: renter.clone(),
            rent_price: rent_fee,
            payment_token: payment_token.clone(),
            start_time: now,
            expires_at,
            is_active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::ActiveRental(token_id), &rental);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ActiveRental(token_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "table_direct_leased"),),
            (owner, renter, token_id, expires_at),
        );
        Ok(())
    }

    /// Terminate an expired rental, releasing the table back to the owner
    pub fn terminate_expired_rental(env: Env, token_id: u32) -> Result<(), TableNftError> {
        let rental: TableRental = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveRental(token_id))
            .ok_or(TableNftError::TableNotRented)?;

        let now = env.ledger().timestamp();
        if now < rental.expires_at && rental.is_active {
            return Err(TableNftError::RentalStillActive);
        }

        env.storage()
            .persistent()
            .remove(&DataKey::ActiveRental(token_id));

        env.events().publish(
            (Symbol::new(&env, "rental_terminated"),),
            (token_id, rental.renter),
        );
        Ok(())
    }

    /// Get active rental details if present
    pub fn get_rental_info(env: Env, token_id: u32) -> Option<TableRental> {
        env.storage()
            .persistent()
            .get::<DataKey, TableRental>(&DataKey::ActiveRental(token_id))
    }

    /// Get current rental listing if listed
    pub fn get_rental_listing(env: Env, token_id: u32) -> Option<RentalListing> {
        env.storage()
            .persistent()
            .get::<DataKey, RentalListing>(&DataKey::RentalListing(token_id))
    }

    /// Check if table is currently actively leased and unexpired
    pub fn is_rented(env: Env, token_id: u32) -> bool {
        if let Some(rental) = env
            .storage()
            .persistent()
            .get::<DataKey, TableRental>(&DataKey::ActiveRental(token_id))
        {
            let now = env.ledger().timestamp();
            rental.is_active && now < rental.expires_at
        } else {
            false
        }
    }

    // ==========================================
    // Internal Helper Functions
    // ==========================================

    fn require_admin(env: &Env, admin: &Address) -> Result<(), TableNftError> {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(TableNftError::NotInitialized)?;
        if stored_admin != *admin {
            return Err(TableNftError::Unauthorized);
        }
        Ok(())
    }

    fn require_approved_or_owner(
        env: &Env,
        caller: &Address,
        owner: &Address,
        token_id: u32,
    ) -> Result<(), TableNftError> {
        if caller == owner {
            return Ok(());
        }
        if Self::is_approved_for_all(env.clone(), owner.clone(), caller.clone()) {
            return Ok(());
        }
        if let Some(approved) = Self::get_approved(env.clone(), token_id) {
            if approved == *caller {
                return Ok(());
            }
        }
        Err(TableNftError::Unauthorized)
    }

    fn require_effective_operator_or_owner(
        env: &Env,
        caller: &Address,
        token_id: u32,
    ) -> Result<(), TableNftError> {
        let effective_operator = Self::get_effective_operator(env.clone(), token_id)?;
        if *caller == effective_operator {
            return Ok(());
        }
        Err(TableNftError::Unauthorized)
    }

    fn validate_rules(rules: &TableRules) -> Result<(), TableNftError> {
        if rules.rake_bps > MAX_RAKE_BPS {
            return Err(TableNftError::RakeBpsExceedsMax);
        }
        if rules.min_players < 2
            || rules.max_players > MAX_PLAYERS_LIMIT
            || rules.min_players > rules.max_players
        {
            return Err(TableNftError::InvalidTableConfig);
        }
        if rules.min_buy_in <= 0 || rules.max_buy_in < rules.min_buy_in {
            return Err(TableNftError::InvalidTableConfig);
        }
        if rules.jackpot_share_bps > 10_000 {
            return Err(TableNftError::InvalidTableConfig);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, String, Vec};

    fn default_rules(env: &Env, token: &Address) -> TableRules {
        let mut allowed = Vec::new(env);
        allowed.push_back(token.clone());
        TableRules {
            min_buy_in: 100,
            max_buy_in: 1000,
            min_players: 2,
            max_players: 6,
            rake_bps: 250, // 2.5%
            jackpot_share_bps: 500, // 5% of rake
            allowed_tokens: allowed,
            action_timeout_seconds: 30,
            is_private: false,
            allow_straddle: true,
            allow_run_it_twice: true,
        }
    }

    fn default_aesthetics(env: &Env) -> TableAesthetics {
        TableAesthetics {
            table_name: String::from_str(env, "High Stakes Cyber Lounge"),
            felt_color: String::from_str(env, "#1A2B3C"),
            card_back_uri: String::from_str(env, "ipfs://QmCyberBack"),
            background_uri: String::from_str(env, "ipfs://QmCyberBg"),
            avatar_frame_uri: String::from_str(env, "ipfs://QmFrame"),
            soundtrack_theme: String::from_str(env, "synthwave_night"),
            custom_metadata_uri: String::from_str(env, "https://stellpoker.io/meta/1"),
        }
    }

    #[test]
    fn test_initialize_and_mint() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(TableNftContract, ());
        let client = TableNftContractClient::new(&env, &contract_id);

        client.initialize(
            &admin,
            &String::from_str(&env, "StellPoker Tables"),
            &String::from_str(&env, "SPTAB"),
        );

        let rules = default_rules(&env, &token);
        let aesthetics = default_aesthetics(&env);

        client.mint(&admin, &owner, &1, &rules, &aesthetics);

        assert_eq!(client.owner_of(&1), owner);
        assert_eq!(client.balance_of(&owner), 1);
        assert_eq!(client.total_supply(), 1);

        let saved_rules = client.get_table_rules(&1);
        assert_eq!(saved_rules.rake_bps, 250);
        assert_eq!(saved_rules.min_players, 2);

        let saved_aesthetics = client.get_table_aesthetics(&1);
        assert_eq!(saved_aesthetics.table_name, String::from_str(&env, "High Stakes Cyber Lounge"));
    }

    #[test]
    fn test_max_rake_rejection() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(TableNftContract, ());
        let client = TableNftContractClient::new(&env, &contract_id);
        client.initialize(&admin, &String::from_str(&env, "SP"), &String::from_str(&env, "SP"));

        let mut invalid_rules = default_rules(&env, &token);
        invalid_rules.rake_bps = 501; // > 5%

        assert!(client.try_mint(&admin, &owner, &1, &invalid_rules, &default_aesthetics(&env)).is_err());
    }

    #[test]
    fn test_approvals_and_transfers() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let approved_user = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(TableNftContract, ());
        let client = TableNftContractClient::new(&env, &contract_id);
        client.initialize(&admin, &String::from_str(&env, "SP"), &String::from_str(&env, "SP"));

        client.mint(&admin, &owner, &1, &default_rules(&env, &token), &default_aesthetics(&env));

        client.approve(&owner, &Some(approved_user.clone()), &1);
        assert_eq!(client.get_approved(&1), Some(approved_user.clone()));

        // Approved user executes transfer
        client.transfer(&approved_user, &owner, &recipient, &1);
        assert_eq!(client.owner_of(&1), recipient);
        assert_eq!(client.get_approved(&1), None);
    }

    #[test]
    fn test_custom_rules_and_aesthetics_update() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(TableNftContract, ());
        let client = TableNftContractClient::new(&env, &contract_id);
        client.initialize(&admin, &String::from_str(&env, "SP"), &String::from_str(&env, "SP"));

        client.mint(&admin, &owner, &1, &default_rules(&env, &token), &default_aesthetics(&env));

        let mut updated_rules = default_rules(&env, &token);
        updated_rules.rake_bps = 400;
        updated_rules.min_buy_in = 500;
        client.set_table_rules(&owner, &1, &updated_rules);

        let r = client.get_table_rules(&1);
        assert_eq!(r.rake_bps, 400);
        assert_eq!(r.min_buy_in, 500);

        let mut updated_aesthetics = default_aesthetics(&env);
        updated_aesthetics.felt_color = String::from_str(&env, "#00FF66");
        client.set_table_aesthetics(&owner, &1, &updated_aesthetics);

        let a = client.get_table_aesthetics(&1);
        assert_eq!(a.felt_color, String::from_str(&env, "#00FF66"));
    }

    #[test]
    fn test_rental_flow() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000_000);

        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let renter = Address::generate(&env);
        let payment_token_admin = Address::generate(&env);

        let payment_token_id = env.register_stellar_asset_contract_v2(payment_token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &payment_token_id);
        token_admin_client.mint(&renter, &10_000);

        let contract_id = env.register(TableNftContract, ());
        let client = TableNftContractClient::new(&env, &contract_id);
        client.initialize(&admin, &String::from_str(&env, "SP"), &String::from_str(&env, "SP"));

        client.mint(&admin, &owner, &10, &default_rules(&env, &payment_token_id), &default_aesthetics(&env));

        // List for rent: 500 tokens per day, duration 1 to 7 days
        let one_day = 86_400u64;
        let seven_days = 7 * one_day;
        client.list_for_rent(&owner, &10, &500, &one_day, &seven_days, &payment_token_id);

        assert!(client.get_rental_listing(&10).is_some());
        assert!(!client.is_rented(&10));
        assert_eq!(client.get_effective_operator(&10), owner);

        // Renter leases table for 2 days (172,800 seconds)
        let lease_duration = 2 * one_day;
        client.rent_table(&renter, &10, &lease_duration);

        assert!(client.is_rented(&10));
        assert_eq!(client.get_effective_operator(&10), renter);

        let token_client = token::Client::new(&env, &payment_token_id);
        assert_eq!(token_client.balance(&owner), 1000); // 500 * 2 days = 1000
        assert_eq!(token_client.balance(&renter), 9000);

        // Operator can update table aesthetics during lease
        let mut operator_aesthetics = default_aesthetics(&env);
        operator_aesthetics.table_name = String::from_str(&env, "Operator's VIP Room");
        client.set_table_aesthetics(&renter, &10, &operator_aesthetics);
        assert_eq!(client.get_table_aesthetics(&10).table_name, String::from_str(&env, "Operator's VIP Room"));

        // Owner cannot transfer while rented
        let stranger = Address::generate(&env);
        assert!(client.try_transfer(&owner, &owner, &stranger, &10).is_err());

        // Fast forward time past expiration
        env.ledger().set_timestamp(1_000_000 + lease_duration + 10);
        assert!(!client.is_rented(&10));
        assert_eq!(client.get_effective_operator(&10), owner);

        // Terminate expired rental
        client.terminate_expired_rental(&10);
        assert!(client.get_rental_info(&10).is_none());

        // Now owner can transfer table
        client.transfer(&owner, &owner, &stranger, &10);
        assert_eq!(client.owner_of(&10), stranger);
    }

    #[test]
    fn test_direct_lease() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(2_000_000);

        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let operator = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&operator, &2000);

        let contract_id = env.register(TableNftContract, ());
        let client = TableNftContractClient::new(&env, &contract_id);
        client.initialize(&admin, &String::from_str(&env, "SP"), &String::from_str(&env, "SP"));

        client.mint(&admin, &owner, &5, &default_rules(&env, &token_id), &default_aesthetics(&env));

        client.direct_lease(&owner, &5, &operator, &86400, &500, &token_id);
        assert!(client.is_rented(&5));
        assert_eq!(client.get_effective_operator(&5), operator);

        let token_client = token::Client::new(&env, &token_id);
        assert_eq!(token_client.balance(&owner), 500);
        assert_eq!(token_client.balance(&operator), 1500);
    }
}
