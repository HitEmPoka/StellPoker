#![no_std]
#![allow(deprecated)]

//! On-chain player avatar registry (Issue #153).
//!
//! Allows players to set a profile picture either as:
//! - An on-chain SVG avatar via a compact template-id + parameters scheme
//! - A reference to an external NFT (contract address + token ID)
//!
//! Avatar data is keyed by Stellar address and cached off-chain by the
//! coordinator/frontend for performance. Falls back to the frontend's
//! deterministic Identicon when no custom avatar is set.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env, String, Symbol, Vec,
};

/// Maximum length of SVG template params string (avoids excessive storage).
const MAX_PARAMS_LEN: u32 = 512;
/// Maximum number of pre-approved NFT contracts (admin-only list).
const MAX_APPROVED_NFTS: u32 = 32;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvatarKind {
    /// No custom avatar set — frontend falls back to Identicon.
    None,
    /// On-chain SVG avatar: template ID + compact parameter string.
    /// `template_id` references a curated set of SVG templates shipped
    /// with the frontend; `params` is a comma-separated k=v list
    /// (e.g. "bg=#1a1b2e,fg=#f1c40f,hat=3,body=5") rendered client-side.
    Svg {
        template_id: u32,
        params: String,
    },
    /// Reference to an external NFT held by the player.
    /// Frontend verifies ownership via the NFT contract's `owner_of`
    /// before displaying, and caches the resolved image URL.
    Nft {
        nft_contract: Address,
        token_id: u32,
    },
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerAvatar {
    pub kind: AvatarKind,
    /// Ledger timestamp of last update — used by the coordinator's
    /// off-chain cache for TTL invalidation.
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Paused,
    /// Avatar state per player address.
    Avatar(Address),
    /// Admin-curated list of approved NFT contracts.
    /// Players may only link NFTs from contracts on this list (safety
    /// guard against arbitrary NFT metadata URIs).
    ApprovedNfts,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AvatarError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    Paused = 4,
    ParamsTooLong = 5,
    NftNotApproved = 6,
    TemplateIdInvalid = 7,
    ApprovedListFull = 8,
}

#[contract]
pub struct PlayerAvatarContract;

#[contractimpl]
impl PlayerAvatarContract {
    /// Initialize the avatar registry.
    pub fn initialize(env: Env, admin: Address) -> Result<(), AvatarError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(AvatarError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage()
            .persistent()
            .set(&DataKey::ApprovedNfts, &Vec::<Address>::new(&env));
        env.storage().instance().extend_ttl(100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "avatar_initialized"),),
            admin,
        );
        Ok(())
    }

    /// Admin: pause / unpause avatar mutations.
    pub fn set_paused(env: Env, admin: Address, paused: bool) -> Result<(), AvatarError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &paused);
        env.events().publish(
            (Symbol::new(&env, "avatar_paused_changed"),),
            paused,
        );
        Ok(())
    }

    /// Admin: add an NFT contract to the approved list.
    pub fn approve_nft_contract(
        env: Env,
        admin: Address,
        nft_contract: Address,
    ) -> Result<(), AvatarError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;

        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::ApprovedNfts)
            .unwrap_or_else(|| Vec::new(&env));

        // Dedupe
        for i in 0..list.len() {
            if list.get(i).unwrap() == nft_contract {
                return Ok(());
            }
        }
        if list.len() >= MAX_APPROVED_NFTS {
            return Err(AvatarError::ApprovedListFull);
        }
        list.push_back(nft_contract.clone());
        env.storage().persistent().set(&DataKey::ApprovedNfts, &list);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ApprovedNfts, 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "nft_contract_approved"),),
            nft_contract,
        );
        Ok(())
    }

    /// Admin: remove an NFT contract from the approved list.
    pub fn revoke_nft_contract(
        env: Env,
        admin: Address,
        nft_contract: Address,
    ) -> Result<(), AvatarError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::ApprovedNfts)
            .unwrap_or_else(|| Vec::new(&env));
        let mut next = Vec::new(&env);
        for i in 0..list.len() {
            let a = list.get(i).unwrap();
            if a != nft_contract {
                next.push_back(a);
            }
        }
        env.storage().persistent().set(&DataKey::ApprovedNfts, &next);

        env.events().publish(
            (Symbol::new(&env, "nft_contract_revoked"),),
            nft_contract,
        );
        Ok(())
    }

    /// Get the current list of approved NFT contracts.
    pub fn get_approved_nfts(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::ApprovedNfts)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Player sets their avatar to an on-chain SVG template.
    ///
    /// `template_id` is a reference to a frontend SVG template (0-based
    /// index into the shipped library). `params` is a compact k=v string
    /// of template variables (color picks, feature toggles, etc.).
    /// Passing `template_id == u32::MAX` and empty `params` clears the
    /// custom avatar back to None.
    pub fn set_svg_avatar(
        env: Env,
        player: Address,
        template_id: u32,
        params: String,
    ) -> Result<(), AvatarError> {
        player.require_auth();
        Self::require_not_paused(&env)?;

        // Clear sentinel: u32::MAX + empty params -> reset to None
        let kind = if template_id == u32::MAX && params.is_empty() {
            AvatarKind::None
        } else {
            if params.len() > MAX_PARAMS_LEN {
                return Err(AvatarError::ParamsTooLong);
            }
            // Template IDs 0..=255 reserved for the core SVG library shipped
            // with the frontend. IDs >= 256 are reserved for future use
            // (community templates, premium packs, etc.).
            if template_id > 65535 {
                return Err(AvatarError::TemplateIdInvalid);
            }
            AvatarKind::Svg {
                template_id,
                params,
            }
        };

        let avatar = PlayerAvatar {
            kind,
            updated_at: env.ledger().timestamp(),
        };
        let key = DataKey::Avatar(player.clone());
        env.storage().persistent().set(&key, &avatar);
        env.storage()
            .persistent()
            .extend_ttl(&key, 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "avatar_updated"),),
            (player.clone(), template_id),
        );
        Ok(())
    }

    /// Player links an external NFT as their avatar.
    ///
    /// The contract only records the reference; ownership verification
    /// and image URL resolution happen client-side (caller reads the
    /// NFT contract's `token_uri` / `owner_of`). The NFT contract must
    /// be on the admin-approved list.
    ///
    /// Pass `token_id == u32::MAX` to clear back to None (equivalent to
    /// calling `set_svg_avatar` with the clear sentinel).
    pub fn set_nft_avatar(
        env: Env,
        player: Address,
        nft_contract: Address,
        token_id: u32,
    ) -> Result<(), AvatarError> {
        player.require_auth();
        Self::require_not_paused(&env)?;

        // Clear sentinel: token_id == u32::MAX -> reset
        let kind = if token_id == u32::MAX {
            AvatarKind::None
        } else {
            // Verify contract is approved
            let approved = Self::get_approved_nfts(env.clone());
            let mut found = false;
            for i in 0..approved.len() {
                if approved.get(i).unwrap() == nft_contract {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(AvatarError::NftNotApproved);
            }
            AvatarKind::Nft {
                nft_contract,
                token_id,
            }
        };

        let avatar = PlayerAvatar {
            kind,
            updated_at: env.ledger().timestamp(),
        };
        let key = DataKey::Avatar(player.clone());
        env.storage().persistent().set(&key, &avatar);
        env.storage()
            .persistent()
            .extend_ttl(&key, 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "avatar_updated_nft"),),
            player.clone(),
        );
        Ok(())
    }

    /// Clear custom avatar — alias for `set_svg_avatar` with clear sentinel.
    pub fn clear_avatar(env: Env, player: Address) -> Result<(), AvatarError> {
        Self::set_svg_avatar(env, player, u32::MAX, String::new(&Env::default()))
    }

    /// Fetch a player's current avatar, or `None` if not set.
    pub fn get_avatar(env: Env, player: Address) -> Option<PlayerAvatar> {
        let key = DataKey::Avatar(player);
        env.storage().persistent().get(&key)
    }

    /// Batch-fetch avatars for a list of addresses. Used by the
    /// coordinator when assembling a table view so it can warm the
    /// off-chain cache in a single call.
    pub fn get_avatars(env: Env, players: Vec<Address>) -> Vec<Option<PlayerAvatar>> {
        let mut out = Vec::new(&env);
        for i in 0..players.len() {
            let p = players.get(i).unwrap();
            out.push_back(Self::get_avatar(env.clone(), p));
        }
        out
    }

    // ---------- internal helpers ----------

    fn require_admin(env: &Env, admin: &Address) -> Result<(), AvatarError> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(AvatarError::NotInitialized)?;
        if stored != *admin {
            return Err(AvatarError::Unauthorized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), AvatarError> {
        if env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
        {
            return Err(AvatarError::Paused);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env};

    fn setup(env: &Env) -> (Address, Address, Address) {
        let admin = Address::generate(env);
        let alice = Address::generate(env);
        let nft_contract = Address::generate(env);
        env.mock_all_auths();

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(env, &contract_id);
        client.initialize(&admin);
        (admin, alice, nft_contract)
    }

    #[test]
    fn test_initialize_and_default_none() {
        let env = Env::default();
        let (_, alice, _) = setup(&env);
        let contract_id = env
            .register(PlayerAvatarContract, ())
            .clone(); // no-op placeholder to avoid unused warnings in setup logic re-check below — actual contract uses initialized one
        let client = PlayerAvatarContractClient::new(&env, &env.register(PlayerAvatarContract, ()));
        // Note: setup() already initializes a separate client; re-init here for standalone assertions.
        let admin2 = Address::generate(&env);
        let alice2 = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin2);
        assert!(client.get_avatar(&alice2).is_none());
    }

    #[test]
    fn test_set_svg_avatar_and_read_back() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let params = String::from_str(&env, "bg=#1a1b2e,fg=#f1c40f,hat=3");
        client.set_svg_avatar(&alice, &5, &params);

        let avatar = client.get_avatar(&alice).expect("avatar set");
        match avatar.kind {
            AvatarKind::Svg {
                template_id,
                params: p,
            } => {
                assert_eq!(template_id, 5);
                assert_eq!(p, params);
            }
            _ => panic!("expected Svg kind"),
        }
        assert!(avatar.updated_at > 0);
    }

    #[test]
    fn test_svg_params_too_long_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let long = String::from_str(&env, &"x".repeat(513));
        let result = client.try_set_svg_avatar(&alice, &1, &long);
        assert!(result.is_err());
    }

    #[test]
    fn test_clear_via_svg_sentinel() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        client.set_svg_avatar(&alice, &2, &String::from_str(&env, "a=b"));
        assert!(client.get_avatar(&alice).is_some());

        client.set_svg_avatar(&alice, &u32::MAX, &String::new(&env));
        let avatar = client.get_avatar(&alice).expect("clear still stores record");
        assert!(matches!(avatar.kind, AvatarKind::None));
    }

    #[test]
    fn test_nft_avatar_requires_approved_contract() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let nft_a = Address::generate(&env);
        let nft_b = Address::generate(&env);

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        // Unapproved NFT rejected
        let result = client.try_set_nft_avatar(&alice, &nft_b, &42);
        assert!(result.is_err());

        // Approve, then it works
        client.approve_nft_contract(&admin, &nft_b);
        client.set_nft_avatar(&alice, &nft_b, &42);
        let avatar = client.get_avatar(&alice).unwrap();
        match avatar.kind {
            AvatarKind::Nft {
                nft_contract,
                token_id,
            } => {
                assert_eq!(nft_contract, nft_b);
                assert_eq!(token_id, 42);
            }
            _ => panic!("expected Nft kind"),
        }

        // Revoke, then setting again fails
        client.revoke_nft_contract(&admin, &nft_b);
        let result2 = client.try_set_nft_avatar(&alice, &nft_b, &43);
        assert!(result2.is_err());
    }

    #[test]
    fn test_batch_get_avatars() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        client.set_svg_avatar(&alice, &1, &String::from_str(&env, "a=b"));
        // Bob: no avatar set
        client.set_svg_avatar(&carol, &2, &String::from_str(&env, "c=d"));

        let mut list = Vec::new(&env);
        list.push_back(alice.clone());
        list.push_back(bob.clone());
        list.push_back(carol.clone());

        let batch = client.get_avatars(&list);
        assert_eq!(batch.len(), 3);
        assert!(batch.get(0).unwrap().is_some());
        assert!(batch.get(1).unwrap().is_none());
        assert!(batch.get(2).unwrap().is_some());
    }

    #[test]
    fn test_pause_blocks_mutations() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let contract_id = env.register(PlayerAvatarContract, ());
        let client = PlayerAvatarContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        client.set_paused(&admin, &true);
        let result = client.try_set_svg_avatar(&alice, &1, &String::from_str(&env, "a=b"));
        assert!(result.is_err());

        client.set_paused(&admin, &false);
        client.set_svg_avatar(&alice, &1, &String::from_str(&env, "a=b"));
        assert!(client.get_avatar(&alice).is_some());
    }
}
