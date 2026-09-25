#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec};

/// Token Allowlist Governance contract.
///
/// Manages a governance-controlled allowlist of SAC (Stellar Asset Contract)
/// tokens that are accepted for buy-ins. Tables reference this contract to
/// gate which assets players may use.
#[contract]
pub struct TokenAllowlistContract;

#[contracttype]
#[derive(Clone, Debug)]
pub struct TokenInfo {
    pub token_address: Address,
    pub enabled: bool,
    /// Minimum decimals the token must report. Used for metadata validation.
    pub min_decimals: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum AllowlistKey {
    Admin,
    Token(Address),
    AllTokens,
}

#[contractimpl]
impl TokenAllowlistContract {
    /// Initialize the allowlist with an admin.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        assert!(
            !env.storage().instance().has(&AllowlistKey::Admin),
            "already initialized"
        );
        env.storage().instance().set(&AllowlistKey::Admin, &admin);
    }

    /// Add a token to the allowlist. Validates that the token contract exists
    /// by querying its decimals. Admin only.
    pub fn add_token(env: Env, admin: Address, token_address: Address, min_decimals: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let key = AllowlistKey::Token(token_address.clone());
        assert!(
            !env.storage().instance().has(&key),
            "token already allowlisted"
        );

        // Validate the token contract exists by calling decimals().
        let token_client = token::Client::new(&env, &token_address);
        let decimals = token_client.decimals();
        assert!(
            decimals >= min_decimals,
            "token decimals below minimum"
        );

        let info = TokenInfo {
            token_address: token_address.clone(),
            enabled: true,
            min_decimals,
        };
        env.storage().instance().set(&key, &info);

        // Track in the all-tokens list.
        let mut all: Vec<Address> = env
            .storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(&env));
        all.push_back(token_address.clone());
        env.storage().instance().set(&AllowlistKey::AllTokens, &all);

        env.events().publish(
            (Symbol::new(&env, "token_added"),),
            (token_address, min_decimals),
        );
    }

    /// Remove a token from the allowlist. Admin only.
    pub fn remove_token(env: Env, admin: Address, token_address: Address) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let key = AllowlistKey::Token(token_address.clone());
        assert!(
            env.storage().instance().has(&key),
            "token not allowlisted"
        );

        env.storage().instance().remove(&key);

        // Remove from the all-tokens list.
        let all: Vec<Address> = env
            .storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_all = Vec::new(&env);
        for i in 0..all.len() {
            let addr = all.get(i).unwrap();
            if addr != token_address {
                new_all.push_back(addr);
            }
        }
        env.storage()
            .instance()
            .set(&AllowlistKey::AllTokens, &new_all);

        env.events().publish(
            (Symbol::new(&env, "token_removed"),),
            token_address,
        );
    }

    /// Enable or disable a token without removing it. Admin only.
    pub fn set_token_enabled(env: Env, admin: Address, token_address: Address, enabled: bool) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let key = AllowlistKey::Token(token_address.clone());
        let mut info: TokenInfo = env
            .storage()
            .instance()
            .get(&key)
            .expect("token not allowlisted");
        info.enabled = enabled;
        env.storage().instance().set(&key, &info);

        env.events().publish(
            (Symbol::new(&env, "token_status_changed"),),
            (token_address, enabled),
        );
    }

    /// Check if a token is on the allowlist and enabled.
    pub fn is_allowed(env: Env, token_address: Address) -> bool {
        let key = AllowlistKey::Token(token_address);
        env.storage()
            .instance()
            .get::<AllowlistKey, TokenInfo>(&key)
            .map(|info| info.enabled)
            .unwrap_or(false)
    }

    /// Check if a player can join a table with a given buy-in token.
    /// Rejects non-allowlisted assets.
    pub fn can_join(env: Env, _player: Address, token_address: Address) -> bool {
        Self::is_allowed(env, token_address)
    }

    /// Get token info for an allowlisted token.
    pub fn get_token_info(env: Env, token_address: Address) -> Option<TokenInfo> {
        env.storage()
            .instance()
            .get(&AllowlistKey::Token(token_address))
    }

    /// List all allowlisted token addresses.
    pub fn list_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// List all enabled token addresses.
    pub fn list_enabled_tokens(env: Env) -> Vec<Address> {
        let all: Vec<Address> = env
            .storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(&env));
        let mut enabled = Vec::new(&env);
        for i in 0..all.len() {
            let addr = all.get(i).unwrap();
            let key = AllowlistKey::Token(addr.clone());
            if let Some(info) = env
                .storage()
                .instance()
                .get::<AllowlistKey, TokenInfo>(&key)
            {
                if info.enabled {
                    enabled.push_back(addr);
                }
            }
        }
        enabled
    }

    /// Transfer admin to a new address. Current admin only.
    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();
        Self::require_admin(&env, &current_admin);
        env.storage()
            .instance()
            .set(&AllowlistKey::Admin, &new_admin);
        env.events().publish(
            (Symbol::new(&env, "admin_transferred"),),
            (current_admin, new_admin),
        );
    }

    fn require_admin(env: &Env, admin: &Address) {
        let stored: Address = env
            .storage()
            .instance()
            .get(&AllowlistKey::Admin)
            .expect("not initialized");
        assert!(admin == &stored, "not admin");
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Address, Env,
    };

    struct Setup<'a> {
        env: Env,
        client: TokenAllowlistContractClient<'a>,
        admin: Address,
        token_addr: Address,
        _token_client: TokenClient<'a>,
    }

    fn setup() -> Setup<'static> {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(TokenAllowlistContract, ());
        let client = TokenAllowlistContractClient::new(&env, &contract_id);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token_client = TokenClient::new(&env, &sac.address());

        let admin = Address::generate(&env);
        client.initialize(&admin);

        Setup {
            env,
            client,
            admin,
            token_addr: sac.address(),
            _token_client: token_client,
        }
    }

    #[test]
    fn add_token_and_check_allowed() {
        let s = setup();
        assert!(!s.client.is_allowed(&s.token_addr));

        s.client.add_token(&s.admin, &s.token_addr, &0);
        assert!(s.client.is_allowed(&s.token_addr));
    }

    #[test]
    fn can_join_rejects_non_allowlisted() {
        let s = setup();
        let player = Address::generate(&s.env);
        assert!(!s.client.can_join(&player, &s.token_addr));
    }

    #[test]
    fn can_join_accepts_allowlisted() {
        let s = setup();
        let player = Address::generate(&s.env);
        s.client.add_token(&s.admin, &s.token_addr, &0);
        assert!(s.client.can_join(&player, &s.token_addr));
    }

    #[test]
    fn remove_token_disallows() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);
        assert!(s.client.is_allowed(&s.token_addr));

        s.client.remove_token(&s.admin, &s.token_addr);
        assert!(!s.client.is_allowed(&s.token_addr));
    }

    #[test]
    fn disable_then_enable_token() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);

        s.client.set_token_enabled(&s.admin, &s.token_addr, &false);
        assert!(!s.client.is_allowed(&s.token_addr));

        s.client.set_token_enabled(&s.admin, &s.token_addr, &true);
        assert!(s.client.is_allowed(&s.token_addr));
    }

    #[test]
    fn list_tokens_returns_all() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);

        let token_admin2 = Address::generate(&s.env);
        let sac2 = s.env.register_stellar_asset_contract_v2(token_admin2);
        s.client.add_token(&s.admin, &sac2.address(), &0);

        let all = s.client.list_tokens();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn list_enabled_tokens_filters_disabled() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);

        let token_admin2 = Address::generate(&s.env);
        let sac2 = s.env.register_stellar_asset_contract_v2(token_admin2);
        s.client.add_token(&s.admin, &sac2.address(), &0);

        s.client.set_token_enabled(&s.admin, &s.token_addr, &false);

        let enabled = s.client.list_enabled_tokens();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled.get(0).unwrap(), sac2.address());
    }

    #[test]
    #[should_panic(expected = "token already allowlisted")]
    fn add_token_twice_reverts() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);
        s.client.add_token(&s.admin, &s.token_addr, &0);
    }

    #[test]
    #[should_panic(expected = "token not allowlisted")]
    fn remove_nonexistent_token_reverts() {
        let s = setup();
        s.client.remove_token(&s.admin, &s.token_addr);
    }

    #[test]
    #[should_panic(expected = "not admin")]
    fn non_admin_cannot_add_token() {
        let s = setup();
        let stranger = Address::generate(&s.env);
        s.client.add_token(&stranger, &s.token_addr, &0);
    }

    #[test]
    #[should_panic(expected = "not admin")]
    fn non_admin_cannot_remove_token() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);
        let stranger = Address::generate(&s.env);
        s.client.remove_token(&stranger, &s.token_addr);
    }

    #[test]
    fn get_token_info_returns_details() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &7);
        let info = s.client.get_token_info(&s.token_addr).unwrap();
        assert!(info.enabled);
        assert_eq!(info.min_decimals, 7);
    }

    #[test]
    fn get_token_info_none_for_unknown() {
        let s = setup();
        assert!(s.client.get_token_info(&s.token_addr).is_none());
    }

    #[test]
    fn transfer_admin() {
        let s = setup();
        let new_admin = Address::generate(&s.env);
        s.client.transfer_admin(&s.admin, &new_admin);

        // Old admin can no longer add tokens.
        let token_admin3 = Address::generate(&s.env);
        let sac3 = s.env.register_stellar_asset_contract_v2(token_admin3);
        // New admin can.
        s.client.add_token(&new_admin, &sac3.address(), &0);
        assert!(s.client.is_allowed(&sac3.address()));
    }

    #[test]
    fn remove_token_cleans_up_list() {
        let s = setup();
        s.client.add_token(&s.admin, &s.token_addr, &0);

        let token_admin2 = Address::generate(&s.env);
        let sac2 = s.env.register_stellar_asset_contract_v2(token_admin2);
        s.client.add_token(&s.admin, &sac2.address(), &0);

        s.client.remove_token(&s.admin, &s.token_addr);
        let all = s.client.list_tokens();
        assert_eq!(all.len(), 1);
        assert_eq!(all.get(0).unwrap(), sac2.address());
    }
}
