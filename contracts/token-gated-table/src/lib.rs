#![no_std]
// Soroban entrypoints include `Env` and the authenticated caller in addition
// to the policy fields, so the generated client methods exceed clippy's
// default seven-argument threshold without reducing contract clarity.
#![allow(clippy::too_many_arguments)]

//! Token-gated table admission contract.
//!
//! Intended semantics (issue #1008): a table may only admit a player when the
//! player (a) holds at least `min_balance` units of the configured Stellar
//! Asset Contract (SAC) buy-in token and, optionally, (b) owns the configured
//! NFT token id (for example a `table-nft` membership pass). Policies are
//! governed by an admin, can be time-bounded (`expires_at`) and can be revoked
//! at any time. No policy means no admission.
//!
//! `can_join` itself is a read-only view: a table calls it on-chain and does
//! not ask the player to sign a second time. All state-changing policy calls
//! require `admin.require_auth()`, and the authenticated `verify_join`
//! entrypoint lets a table ask the player to prove their identity explicitly.

use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, token, Address, Env, InvokeError, IntoVal,
    Symbol, Val, Vec,
};

#[contract]
pub struct TokenAllowlistContract;

/// A token admission policy. `expires_at == 0` means "no time limit".
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatePolicy {
    pub token_address: Address,
    pub min_balance: i128,
    pub nft_token: Option<Address>,
    pub nft_token_id: Option<u32>,
    pub expires_at: u64,
    pub enabled: bool,
}

/// Kept for backwards compatibility with existing allowlist consumers.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenInfo {
    pub token_address: Address,
    pub enabled: bool,
    pub min_decimals: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum AllowlistKey {
    Admin,
    Token(Address),
    Policy(Address),
    AllTokens,
}

#[contractevent]
pub struct AllowlistInitialized {
    pub admin: Address,
}

#[contractevent]
pub struct GatePolicySet {
    pub admin: Address,
    pub token_address: Address,
    pub min_balance: i128,
    pub expires_at: u64,
    pub enabled: bool,
}

#[contractevent]
pub struct GatePolicyRevoked {
    pub admin: Address,
    pub token_address: Address,
}

#[contractevent]
pub struct GatePolicyStatusChanged {
    pub admin: Address,
    pub token_address: Address,
    pub enabled: bool,
}

#[contractevent]
pub struct AdminTransferred {
    pub previous_admin: Address,
    pub new_admin: Address,
}

#[contractimpl]
impl TokenAllowlistContract {
    /// Initialize the policy administrator. Must be called once.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        assert!(
            !env.storage().instance().has(&AllowlistKey::Admin),
            "already initialized"
        );
        env.storage().instance().set(&AllowlistKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&AllowlistKey::AllTokens, &Vec::<Address>::new(&env));
        AllowlistInitialized { admin }.publish(&env);
    }

    /// Create or replace an admission policy for a SAC token.
    ///
    /// Set `nft_token`/`nft_token_id` to `Some` to additionally require NFT
    /// ownership. `expires_at` is a unix timestamp; `0` disables expiry.
    pub fn set_gate_policy(
        env: Env,
        admin: Address,
        token_address: Address,
        min_balance: i128,
        nft_token: Option<Address>,
        nft_token_id: Option<u32>,
        expires_at: u64,
        enabled: bool,
    ) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        assert!(min_balance > 0, "min_balance must be positive");
        assert!(
            nft_token.is_some() == nft_token_id.is_some(),
            "nft_token and nft_token_id must be set together"
        );

        // Verify the buy-in token is a real SAC before trusting its balance.
        let decimals = token::Client::new(&env, &token_address).decimals();

        let policy = GatePolicy {
            token_address: token_address.clone(),
            min_balance,
            nft_token,
            nft_token_id,
            expires_at,
            enabled,
        };
        env.storage()
            .instance()
            .set(&AllowlistKey::Policy(token_address.clone()), &policy);

        Self::track_token(&env, &token_address);
        env.storage().instance().set(
            &AllowlistKey::Token(token_address.clone()),
            &TokenInfo {
                token_address: token_address.clone(),
                enabled,
                min_decimals: decimals,
            },
        );

        GatePolicySet {
            admin,
            token_address,
            min_balance,
            expires_at,
            enabled,
        }
        .publish(&env);
    }

    /// Backwards-compatible helper: allowlist a SAC with a 1-unit threshold.
    pub fn add_token(env: Env, admin: Address, token_address: Address, min_decimals: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        let decimals = token::Client::new(&env, &token_address).decimals();
        assert!(decimals >= min_decimals, "token decimals below minimum");
        Self::set_gate_policy(env, admin, token_address, 1, None, None, 0, true);
    }

    /// Permanently revoke a policy. A revoked token can never be used to join
    /// until a new policy is explicitly configured.
    pub fn revoke_gate_policy(env: Env, admin: Address, token_address: Address) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        assert!(
            env.storage()
                .instance()
                .has(&AllowlistKey::Policy(token_address.clone())),
            "policy not found"
        );
        env.storage()
            .instance()
            .remove(&AllowlistKey::Policy(token_address.clone()));
        env.storage()
            .instance()
            .remove(&AllowlistKey::Token(token_address.clone()));
        Self::untrack_token(&env, &token_address);
        GatePolicyRevoked {
            admin,
            token_address,
        }
        .publish(&env);
    }

    /// Temporarily enable/disable a policy without deleting its parameters.
    pub fn set_gate_enabled(env: Env, admin: Address, token_address: Address, enabled: bool) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        let key = AllowlistKey::Policy(token_address.clone());
        let mut policy: GatePolicy = env
            .storage()
            .instance()
            .get(&key)
            .expect("policy not found");
        policy.enabled = enabled;
        env.storage().instance().set(&key, &policy);

        let token_key = AllowlistKey::Token(token_address.clone());
        if let Some(mut info) =
            env.storage().instance().get::<AllowlistKey, TokenInfo>(&token_key)
        {
            info.enabled = enabled;
            env.storage().instance().set(&token_key, &info);
        }

        GatePolicyStatusChanged {
            admin,
            token_address,
            enabled,
        }
        .publish(&env);
    }

    /// Transfer policy administration to another address.
    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();
        Self::require_admin(&env, &current_admin);
        env.storage()
            .instance()
            .set(&AllowlistKey::Admin, &new_admin);
        AdminTransferred {
            previous_admin: current_admin,
            new_admin,
        }
        .publish(&env);
    }

    /// Read-only admission check used by tables. Returns `false` (never an
    /// error) for unknown, disabled, expired, revoked or unmet policies.
    pub fn can_join(env: Env, player: Address, token_address: Address) -> bool {
        let policy: GatePolicy = match env
            .storage()
            .instance()
            .get(&AllowlistKey::Policy(token_address.clone()))
        {
            Some(policy) => policy,
            None => return false,
        };

        if !policy.enabled {
            return false;
        }
        if policy.expires_at != 0 && env.ledger().timestamp() >= policy.expires_at {
            return false;
        }
        if Self::sac_balance(&env, &token_address, &player) < policy.min_balance {
            return false;
        }
        match (policy.nft_token, policy.nft_token_id) {
            (Some(nft), Some(token_id)) => Self::owns_nft(&env, &nft, &player, token_id),
            _ => true,
        }
    }

    /// Authenticated variant for callers that want the player to prove their
    /// identity as part of the join request.
    pub fn verify_join(env: Env, player: Address, token_address: Address) -> bool {
        player.require_auth();
        Self::can_join(env, player, token_address)
    }

    pub fn get_gate_policy(env: Env, token_address: Address) -> Option<GatePolicy> {
        env.storage()
            .instance()
            .get(&AllowlistKey::Policy(token_address))
    }

    pub fn get_token_info(env: Env, token_address: Address) -> Option<TokenInfo> {
        env.storage()
            .instance()
            .get(&AllowlistKey::Token(token_address))
    }

    pub fn is_allowed(env: Env, token_address: Address) -> bool {
        env.storage()
            .instance()
            .get::<AllowlistKey, TokenInfo>(&AllowlistKey::Token(token_address))
            .is_some_and(|info| info.enabled)
    }

    pub fn list_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn list_enabled_tokens(env: Env) -> Vec<Address> {
        let all: Vec<Address> = Self::list_tokens(env.clone());
        let mut enabled = Vec::new(&env);
        for i in 0..all.len() {
            let addr = all.get(i).unwrap();
            if let Some(policy) = env
                .storage()
                .instance()
                .get::<AllowlistKey, GatePolicy>(&AllowlistKey::Policy(addr.clone()))
            {
                if policy.enabled
                    && (policy.expires_at == 0 || env.ledger().timestamp() < policy.expires_at)
                {
                    enabled.push_back(addr);
                }
            }
        }
        enabled
    }

    pub fn remove_token(env: Env, admin: Address, token_address: Address) {
        Self::revoke_gate_policy(env, admin, token_address)
    }

    pub fn set_token_enabled(env: Env, admin: Address, token_address: Address, enabled: bool) {
        Self::set_gate_enabled(env, admin, token_address, enabled)
    }

    fn sac_balance(env: &Env, token: &Address, player: &Address) -> i128 {
        let mut args = Vec::<Val>::new(env);
        args.push_back(player.into_val(env));
        match env.try_invoke_contract::<i128, InvokeError>(
            token,
            &Symbol::new(env, "balance"),
            args,
        ) {
            Ok(Ok(balance)) => balance,
            _ => 0,
        }
    }

    /// `table-nft` exposes `owner_of(token_id) -> Address`. Any contract using
    /// that shape works; missing/failed calls simply fail the gate.
    fn owns_nft(env: &Env, nft: &Address, player: &Address, token_id: u32) -> bool {
        let mut args = Vec::<Val>::new(env);
        args.push_back(token_id.into_val(env));
        match env.try_invoke_contract::<Address, InvokeError>(
            nft,
            &Symbol::new(env, "owner_of"),
            args,
        ) {
            Ok(Ok(owner)) => owner == *player,
            _ => false,
        }
    }

    fn require_admin(env: &Env, admin: &Address) {
        let stored: Address = env
            .storage()
            .instance()
            .get(&AllowlistKey::Admin)
            .expect("not initialized");
        assert!(admin == &stored, "not admin");
    }

    fn track_token(env: &Env, token: &Address) {
        let mut all: Vec<Address> = env
            .storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..all.len() {
            if all.get(i).unwrap() == *token {
                return;
            }
        }
        all.push_back(token.clone());
        env.storage()
            .instance()
            .set(&AllowlistKey::AllTokens, &all);
    }

    fn untrack_token(env: &Env, token: &Address) {
        let all: Vec<Address> = env
            .storage()
            .instance()
            .get(&AllowlistKey::AllTokens)
            .unwrap_or_else(|| Vec::new(env));
        let mut remaining = Vec::new(env);
        for i in 0..all.len() {
            let addr = all.get(i).unwrap();
            if addr != *token {
                remaining.push_back(addr);
            }
        }
        env.storage()
            .instance()
            .set(&AllowlistKey::AllTokens, &remaining);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl, contracttype,
        testutils::{Address as _, Events as _, Ledger},
        token::StellarAssetClient,
        Event,
    };

    #[contracttype]
    enum NftKey {
        Owner(u32),
    }

    /// Minimal membership NFT with the `owner_of(u32)` interface used by
    /// `table-nft`.
    #[contract]
    pub struct MockMembershipNft;

    #[contractimpl]
    impl MockMembershipNft {
        pub fn initialize(env: Env, owner: Address, token_id: u32) {
            env.storage()
                .persistent()
                .set(&NftKey::Owner(token_id), &owner);
        }

        pub fn owner_of(env: Env, token_id: u32) -> Address {
            env.storage()
                .persistent()
                .get(&NftKey::Owner(token_id))
                .expect("token not found")
        }

        pub fn transfer_owner(env: Env, from: Address, to: Address, token_id: u32) {
            from.require_auth();
            let owner: Address = env
                .storage()
                .persistent()
                .get(&NftKey::Owner(token_id))
                .expect("token not found");
            assert!(owner == from, "not owner");
            env.storage()
                .persistent()
                .set(&NftKey::Owner(token_id), &to);
        }
    }

    struct Setup<'a> {
        env: Env,
        client: TokenAllowlistContractClient<'a>,
        contract_id: Address,
        admin: Address,
        token: Address,
        sac: StellarAssetClient<'a>,
    }

    fn setup() -> Setup<'static> {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000_000);

        let contract_id = env.register(TokenAllowlistContract, ());
        let client = TokenAllowlistContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let token_admin = Address::generate(&env);
        let sac_address = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        let sac = StellarAssetClient::new(&env, &sac_address);

        Setup {
            env,
            client,
            contract_id,
            admin,
            token: sac_address,
            sac,
        }
    }

    #[test]
    fn pass_when_balance_and_nft_are_held() {
        let s = setup();
        let player = Address::generate(&s.env);
        s.sac.mint(&player, &100);

        let nft_id = s.env.register(MockMembershipNft, ());
        let nft = MockMembershipNftClient::new(&s.env, &nft_id);
        nft.initialize(&player, &7);

        s.client.set_gate_policy(
            &s.admin,
            &s.token,
            &100,
            &Some(nft_id.clone()),
            &Some(7),
            &0,
            &true,
        );

        assert!(s.client.can_join(&player, &s.token));
    }

    #[test]
    fn fail_when_balance_below_threshold() {
        let s = setup();
        let player = Address::generate(&s.env);
        s.sac.mint(&player, &99);

        s.client
            .set_gate_policy(&s.admin, &s.token, &100, &None, &None, &0, &true);

        assert!(!s.client.can_join(&player, &s.token));
    }

    #[test]
    fn fail_when_nft_not_owned() {
        let s = setup();
        let player = Address::generate(&s.env);
        let other = Address::generate(&s.env);
        s.sac.mint(&player, &100);

        let nft_id = s.env.register(MockMembershipNft, ());
        let nft = MockMembershipNftClient::new(&s.env, &nft_id);
        nft.initialize(&other, &7);

        s.client
            .set_gate_policy(&s.admin, &s.token, &100, &Some(nft_id), &Some(7), &0, &true);

        assert!(!s.client.can_join(&player, &s.token));
    }

    #[test]
    fn fail_when_policy_expired() {
        let s = setup();
        let player = Address::generate(&s.env);
        s.sac.mint(&player, &100);
        let expires_at = s.env.ledger().timestamp() + 60;

        s.client
            .set_gate_policy(&s.admin, &s.token, &100, &None, &None, &expires_at, &true);
        assert!(s.client.can_join(&player, &s.token));

        s.env.ledger().set_timestamp(expires_at + 1);
        assert!(!s.client.can_join(&player, &s.token));
    }

    #[test]
    fn fail_when_policy_revoked_or_disabled() {
        let s = setup();
        let player = Address::generate(&s.env);
        s.sac.mint(&player, &100);
        s.client
            .set_gate_policy(&s.admin, &s.token, &100, &None, &None, &0, &true);
        assert!(s.client.can_join(&player, &s.token));

        s.client.set_gate_enabled(&s.admin, &s.token, &false);
        assert!(!s.client.can_join(&player, &s.token));

        s.client.set_gate_enabled(&s.admin, &s.token, &true);
        assert!(s.client.can_join(&player, &s.token));

        s.client.revoke_gate_policy(&s.admin, &s.token);
        assert!(!s.client.can_join(&player, &s.token));
        assert!(s.client.get_gate_policy(&s.token).is_none());
    }

    #[test]
    fn nft_transfer_revokes_membership() {
        let s = setup();
        let player = Address::generate(&s.env);
        let other = Address::generate(&s.env);
        s.sac.mint(&player, &100);

        let nft_id = s.env.register(MockMembershipNft, ());
        let nft = MockMembershipNftClient::new(&s.env, &nft_id);
        nft.initialize(&player, &7);
        s.client.set_gate_policy(
            &s.admin,
            &s.token,
            &100,
            &Some(nft_id.clone()),
            &Some(7),
            &0,
            &true,
        );
        assert!(s.client.can_join(&player, &s.token));

        nft.transfer_owner(&player, &other, &7);
        assert!(!s.client.can_join(&player, &s.token));
        // `other` owns the NFT now but has no SAC balance, so still fails.
        assert!(!s.client.can_join(&other, &s.token));
    }

    #[test]
    fn unknown_token_fails_closed() {
        let s = setup();
        let player = Address::generate(&s.env);
        let unknown = Address::generate(&s.env);
        assert!(!s.client.can_join(&player, &unknown));
    }

    #[test]
    fn policy_changes_emit_events() {
        let s = setup();
        s.client
            .set_gate_policy(&s.admin, &s.token, &5, &None, &None, &0, &true);
        let set_event = GatePolicySet {
            admin: s.admin.clone(),
            token_address: s.token.clone(),
            min_balance: 5,
            expires_at: 0,
            enabled: true,
        }
        .to_xdr(&s.env, &s.contract_id);
        assert!(s.env.events().all().events().contains(&set_event));

        s.client.revoke_gate_policy(&s.admin, &s.token);
        let revoke_event = GatePolicyRevoked {
            admin: s.admin.clone(),
            token_address: s.token.clone(),
        }
        .to_xdr(&s.env, &s.contract_id);
        assert!(s.env.events().all().events().contains(&revoke_event));
    }

    #[test]
    fn non_admin_cannot_change_policy() {
        let s = setup();
        let stranger = Address::generate(&s.env);
        let result = s
            .client
            .try_set_gate_policy(&stranger, &s.token, &1, &None, &None, &0, &true);
        assert!(result.is_err());
    }

    #[test]
    fn verify_join_uses_player_auth() {
        let s = setup();
        let player = Address::generate(&s.env);
        s.sac.mint(&player, &1);
        s.client
            .set_gate_policy(&s.admin, &s.token, &1, &None, &None, &0, &true);
        assert!(s.client.verify_join(&player, &s.token));
    }
}
