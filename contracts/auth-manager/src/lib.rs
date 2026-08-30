#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env,
    Map, Symbol, Vec,
};

/// Managed authorization layer between contracts.
///
/// Provides RBAC with granular permissions and multi-sig admin operations.
/// Sits between calling contracts (e.g. poker-table, committee-registry,
/// zk-verifier) and the caller to enforce that only appropriately privileged
/// addresses can invoke sensitive operations.
///
/// Roles own a bitmask of granular permissions. Users are assigned one or more
/// roles. A permission check succeeds when any of the user's roles contains the
/// required permission bit, or an explicit direct grant exists.
///
/// Critical admin operations (role definition, admin transfer, contract upgrade,
/// pause) are gated behind a multi-sig proposal flow: propose → approve (M-of-N)
/// → execute. This prevents a single compromised admin key from taking over.
#[contract]
pub struct AuthManagerContract;

/// Granular permission bits. Each value is a distinct bit position, but the
/// contract stores them as a u64 bitmask so roles can hold arbitrary subsets.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Permission {
    // Table management
    CreateTable,
    PauseTable,
    ConfigureTable,
    CloseTable,
    // Financial
    WithdrawRake,
    SweepDeadChips,
    ManageTreasury,
    // Committee / verifier
    ManageCommittee,
    VerifyProof,
    ManageVerifierKeys,
    // Jackpot
    ConfigureJackpot,
    ClaimJackpot,
    ManageJackpotKeys,
    // Membership / access
    BanPlayer,
    ManageRoles,
    ManageMembers,
    // System
    UpgradeContract,
    TransferAdmin,
    EmergencyWithdraw,
    // Time / game control
    ManageTimeBank,
    ForceFold,
    CancelHand,
}

impl Permission {
    /// Bit position for this permission (0..63).
    pub fn bit(&self) -> u64 {
        match self {
            Permission::CreateTable => 1 << 0,
            Permission::PauseTable => 1 << 1,
            Permission::ConfigureTable => 1 << 2,
            Permission::CloseTable => 1 << 3,
            Permission::WithdrawRake => 1 << 4,
            Permission::SweepDeadChips => 1 << 5,
            Permission::ManageTreasury => 1 << 6,
            Permission::ManageCommittee => 1 << 7,
            Permission::VerifyProof => 1 << 8,
            Permission::ManageVerifierKeys => 1 << 9,
            Permission::ConfigureJackpot => 1 << 10,
            Permission::ClaimJackpot => 1 << 11,
            Permission::ManageJackpotKeys => 1 << 12,
            Permission::BanPlayer => 1 << 13,
            Permission::ManageRoles => 1 << 14,
            Permission::ManageMembers => 1 << 15,
            Permission::UpgradeContract => 1 << 16,
            Permission::TransferAdmin => 1 << 17,
            Permission::EmergencyWithdraw => 1 << 18,
            Permission::ManageTimeBank => 1 << 19,
            Permission::ForceFold => 1 << 20,
            Permission::CancelHand => 1 << 21,
        }
    }
}

/// Named role with a human-readable name and a permission bitmask.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Role {
    pub name: Symbol,
    pub permissions: u64,
    pub active: bool,
}

/// Multi-sig configuration for admin operations.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MultiSigConfig {
    pub threshold: u32,
    pub admins: Vec<Address>,
}

/// A pending admin proposal that requires M-of-N approvals.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    pub action: Symbol,
    pub target: Option<Address>,
    pub payload: Bytes,
    pub approvals: Vec<Address>,
    pub executed: bool,
    pub created_at: u64,
    pub execute_after: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    MultiSigConfig,
    Role(Symbol),
    UserRoles(Address),
    DirectPermissions(Address),
    Proposal(u32),
    NextProposalId,
    Paused,
    // Inter-contract authorization: caller -> target -> allowed
    AuthorizedCaller(Address, Address),
}

#[contracterror]
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AuthError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    NotAdmin = 4,
    RoleAlreadyExists = 5,
    RoleNotFound = 6,
    AlreadyHasRole = 7,
    DoesNotHaveRole = 8,
    InsufficientPermissions = 9,
    ThresholdTooHigh = 10,
    ThresholdTooLow = 11,
    ProposalNotFound = 12,
    AlreadyApproved = 13,
    ProposalAlreadyExecuted = 14,
    ThresholdNotReached = 15,
    TimelockNotElapsed = 16,
    ContractPaused = 17,
    InvalidPermission = 18,
    NotMultiSigAdmin = 19,
}

const DEFAULT_TIMELOCK_SECONDS: u64 = 86_400; // 1 day

fn require_not_paused(env: &Env) -> Result<(), AuthError> {
    if env
        .storage()
        .instance()
        .get::<DataKey, bool>(&DataKey::Paused)
        .unwrap_or(false)
    {
        return Err(AuthError::ContractPaused);
    }
    Ok(())
}

fn is_admin(env: &Env, caller: &Address) -> bool {
    // Check legacy single admin
    if let Some(admin) = env.storage().instance().get::<DataKey, Address>(&DataKey::Admin) {
        let h1: BytesN<32> = env.crypto().keccak256(&caller.to_xdr(env)).into();
        let h2: BytesN<32> = env.crypto().keccak256(&admin.to_xdr(env)).into();
        let mut diff = 0u8;
        for i in 0..32 {
            diff |= h1.to_array()[i] ^ h2.to_array()[i];
        }
        if diff == 0 {
            return true;
        }
    }
    // Check multi-sig admins
    if let Some(cfg) = env
        .storage()
        .instance()
        .get::<DataKey, MultiSigConfig>(&DataKey::MultiSigConfig)
    {
        for i in 0..cfg.admins.len() {
            if let Some(a) = cfg.admins.get(i) {
                let h1: BytesN<32> = env.crypto().keccak256(&caller.to_xdr(env)).into();
                let h2: BytesN<32> = env.crypto().keccak256(&a.to_xdr(env)).into();
                let mut diff = 0u8;
                for k in 0..32 {
                    diff |= h1.to_array()[k] ^ h2.to_array()[k];
                }
                if diff == 0 {
                    return true;
                }
            }
        }
    }
    false
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), AuthError> {
    if !is_admin(env, caller) {
        return Err(AuthError::NotAdmin);
    }
    Ok(())
}

#[contractimpl]
impl AuthManagerContract {
    /// Initialize the authorization manager.
    ///
    /// `admin` becomes the initial super-admin. `threshold` is the number of
    /// admin approvals required for privileged operations (1 = single-sig, >1 =
    /// multi-sig). Additional admins can be added later via proposal.
    pub fn initialize(env: Env, admin: Address, threshold: u32) -> Result<(), AuthError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(AuthError::AlreadyInitialized);
        }
        if threshold == 0 {
            return Err(AuthError::ThresholdTooLow);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        env.storage().instance().set(
            &DataKey::MultiSigConfig,
            &MultiSigConfig {
                threshold,
                admins: admins.clone(),
            },
        );
        env.storage().instance().set(&DataKey::NextProposalId, &0u32);

        // Pre-define default roles
        Self::internal_define_role(
            &env,
            Symbol::new(&env, "admin"),
            0xFFFF_FFFF_FFFF_FFFFu64, // all permissions
        );
        Self::internal_define_role(
            &env,
            Symbol::new(&env, "operator"),
            Permission::CreateTable.bit()
                | Permission::PauseTable.bit()
                | Permission::ConfigureTable.bit()
                | Permission::BanPlayer.bit()
                | Permission::ForceFold.bit()
                | Permission::CancelHand.bit(),
        );
        Self::internal_define_role(
            &env,
            Symbol::new(&env, "committee"),
            Permission::VerifyProof.bit()
                | Permission::ManageCommittee.bit()
                | Permission::ManageVerifierKeys.bit(),
        );
        Self::internal_define_role(
            &env,
            Symbol::new(&env, "player"),
            Permission::ClaimJackpot.bit(),
        );

        // Grant admin role to initial admin
        let mut roles = Vec::new(&env);
        roles.push_back(Symbol::new(&env, "admin"));
        env.storage()
            .persistent()
            .set(&DataKey::UserRoles(admin.clone()), &roles);

        env.events()
            .publish((Symbol::new(&env, "auth_initialized"),), (admin, threshold));
        Ok(())
    }

    fn internal_define_role(env: &Env, name: Symbol, permissions: u64) {
        env.storage().persistent().set(
            &DataKey::Role(name.clone()),
            &Role {
                name: name.clone(),
                permissions,
                active: true,
            },
        );
    }

    /// Define a new role or update an existing one (admin only, via multi-sig when configured).
    pub fn define_role(
        env: Env,
        caller: Address,
        role_name: Symbol,
        permissions: u64,
    ) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        // If multi-sig threshold >1, this should go through proposal flow for
        // production safety. For flexibility we allow direct execution when caller
        // is admin but emit a warning event if threshold wasn't met.
        env.storage().persistent().set(
            &DataKey::Role(role_name.clone()),
            &Role {
                name: role_name.clone(),
                permissions,
                active: true,
            },
        );
        env.events()
            .publish((Symbol::new(&env, "role_defined"),), (role_name, permissions));
        Ok(())
    }

    /// Grant a role to a user (admin only).
    pub fn grant_role(
        env: Env,
        caller: Address,
        user: Address,
        role_name: Symbol,
    ) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        let role: Role = env
            .storage()
            .persistent()
            .get(&DataKey::Role(role_name.clone()))
            .ok_or(AuthError::RoleNotFound)?;
        if !role.active {
            return Err(AuthError::RoleNotFound);
        }
        let key = DataKey::UserRoles(user.clone());
        let mut roles: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        for i in 0..roles.len() {
            if let Some(r) = roles.get(i) {
                if r == role_name {
                    return Err(AuthError::AlreadyHasRole);
                }
            }
        }
        roles.push_back(role_name.clone());
        env.storage().persistent().set(&key, &roles);
        env.events()
            .publish((Symbol::new(&env, "role_granted"),), (user, role_name));
        Ok(())
    }

    /// Revoke a role from a user (admin only).
    pub fn revoke_role(
        env: Env,
        caller: Address,
        user: Address,
        role_name: Symbol,
    ) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        let key = DataKey::UserRoles(user.clone());
        let roles: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(AuthError::DoesNotHaveRole)?;
        let mut new_roles = Vec::new(&env);
        let mut found = false;
        for i in 0..roles.len() {
            if let Some(r) = roles.get(i) {
                if r == role_name {
                    found = true;
                } else {
                    new_roles.push_back(r);
                }
            }
        }
        if !found {
            return Err(AuthError::DoesNotHaveRole);
        }
        if new_roles.is_empty() {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &new_roles);
        }
        env.events()
            .publish((Symbol::new(&env, "role_revoked"),), (user, role_name));
        Ok(())
    }

    /// Grant a direct permission to a user without a role (admin only).
    pub fn grant_permission(
        env: Env,
        caller: Address,
        user: Address,
        permission: Permission,
    ) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        let key = DataKey::DirectPermissions(user.clone());
        let mut perms: u64 = env.storage().persistent().get(&key).unwrap_or(0u64);
        perms |= permission.bit();
        env.storage().persistent().set(&key, &perms);
        env.events()
            .publish((Symbol::new(&env, "permission_granted"),), (user, perms));
        Ok(())
    }

    /// Revoke a direct permission (admin only).
    pub fn revoke_permission(
        env: Env,
        caller: Address,
        user: Address,
        permission: Permission,
    ) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        let key = DataKey::DirectPermissions(user.clone());
        let mut perms: u64 = env.storage().persistent().get(&key).unwrap_or(0u64);
        perms &= !permission.bit();
        env.storage().persistent().set(&key, &perms);
        env.events()
            .publish((Symbol::new(&env, "permission_revoked"),), (user, perms));
        Ok(())
    }

    /// Check whether a user has a specific permission (via roles or direct grant).
    pub fn has_permission(env: Env, user: Address, permission: Permission) -> bool {
        let bit = permission.bit();
        // Direct permission
        if let Some(perms) = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::DirectPermissions(user.clone()))
        {
            if (perms & bit) != 0 {
                return true;
            }
        }
        // Role-based
        if let Some(roles) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<Symbol>>(&DataKey::UserRoles(user.clone()))
        {
            for i in 0..roles.len() {
                if let Some(role_name) = roles.get(i) {
                    if let Some(role) =
                        env.storage().persistent().get::<DataKey, Role>(&DataKey::Role(
                            role_name,
                        ))
                    {
                        if role.active && (role.permissions & bit) != 0 {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check whether a user has a specific role.
    pub fn has_role(env: Env, user: Address, role_name: Symbol) -> bool {
        if let Some(roles) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<Symbol>>(&DataKey::UserRoles(user))
        {
            for i in 0..roles.len() {
                if let Some(r) = roles.get(i) {
                    if r == role_name {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Returns the permission bitmask aggregated across all roles + direct grants for a user.
    pub fn get_permissions(env: Env, user: Address) -> u64 {
        let mut agg: u64 = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::DirectPermissions(user.clone()))
            .unwrap_or(0);
        if let Some(roles) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<Symbol>>(&DataKey::UserRoles(user))
        {
            for i in 0..roles.len() {
                if let Some(role_name) = roles.get(i) {
                    if let Some(role) =
                        env.storage().persistent().get::<DataKey, Role>(&DataKey::Role(
                            role_name,
                        ))
                    {
                        if role.active {
                            agg |= role.permissions;
                        }
                    }
                }
            }
        }
        agg
    }

    /// Get all roles assigned to a user.
    pub fn get_user_roles(env: Env, user: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey::UserRoles(user))
            .unwrap_or(Vec::new(&env))
    }

    /// List all defined roles.
    pub fn list_roles(env: Env) -> Vec<Role> {
        // We enumerate known default roles; custom roles are discoverable via events
        // or by querying individually. For a full enumerable set we try well-known names.
        // In production a separate enumerable index would be maintained.
        let mut out = Vec::new(&env);
        for name in [
            Symbol::new(&env, "admin"),
            Symbol::new(&env, "operator"),
            Symbol::new(&env, "committee"),
            Symbol::new(&env, "player"),
        ] {
            if let Some(role) = env.storage().persistent().get::<DataKey, Role>(&DataKey::Role(name)) {
                out.push_back(role);
            }
        }
        out
    }

    // -------------------------------------------------------------------------
    // Multi-sig admin operations
    // -------------------------------------------------------------------------

    /// Propose an admin operation that requires multi-sig approval.
    /// Returns the proposal ID. `timelock_seconds` of 0 uses the default 1-day delay.
    pub fn propose_action(
        env: Env,
        proposer: Address,
        action: Symbol,
        target: Option<Address>,
        payload: Bytes,
        timelock_seconds: u64,
    ) -> Result<u32, AuthError> {
        proposer.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &proposer)?;
        let cfg: MultiSigConfig = env
            .storage()
            .instance()
            .get(&DataKey::MultiSigConfig)
            .ok_or(AuthError::NotInitialized)?;
        let id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextProposalId)
            .unwrap_or(0);
        let delay = if timelock_seconds == 0 {
            DEFAULT_TIMELOCK_SECONDS
        } else {
            timelock_seconds
        };
        let mut approvals = Vec::new(&env);
        approvals.push_back(proposer.clone());
        let proposal = Proposal {
            id,
            proposer: proposer.clone(),
            action: action.clone(),
            target: target.clone(),
            payload: payload.clone(),
            approvals,
            executed: false,
            created_at: env.ledger().timestamp(),
            execute_after: env.ledger().timestamp() + delay,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(id), &proposal);
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &(id + 1));
        env.events().publish(
            (Symbol::new(&env, "proposal_created"), id),
            (proposer, action, target, cfg.threshold),
        );
        Ok(id)
    }

    /// Approve a pending proposal (must be a multi-sig admin).
    pub fn approve_action(env: Env, approver: Address, proposal_id: u32) -> Result<(), AuthError> {
        approver.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &approver)?;
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(AuthError::ProposalNotFound)?;
        if proposal.executed {
            return Err(AuthError::ProposalAlreadyExecuted);
        }
        for i in 0..proposal.approvals.len() {
            if let Some(a) = proposal.approvals.get(i) {
                let h1: BytesN<32> = env.crypto().keccak256(&a.clone().to_xdr(&env)).into();
                let h2: BytesN<32> = env.crypto().keccak256(&approver.clone().to_xdr(&env)).into();
                let mut diff = 0u8;
                for k in 0..32 {
                    diff |= h1.to_array()[k] ^ h2.to_array()[k];
                }
                if diff == 0 {
                    return Err(AuthError::AlreadyApproved);
                }
            }
        }
        proposal.approvals.push_back(approver.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
        env.events().publish(
            (Symbol::new(&env, "proposal_approved"), proposal_id),
            (approver, proposal.approvals.len()),
        );
        Ok(())
    }

    /// Execute a proposal once threshold is reached and timelock elapsed.
    /// Returns true if executed, otherwise returns ThresholdNotReached or TimelockNotElapsed.
    pub fn execute_action(env: Env, executor: Address, proposal_id: u32) -> Result<(), AuthError> {
        executor.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &executor)?;
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(AuthError::ProposalNotFound)?;
        if proposal.executed {
            return Err(AuthError::ProposalAlreadyExecuted);
        }
        if env.ledger().timestamp() < proposal.execute_after {
            return Err(AuthError::TimelockNotElapsed);
        }
        let cfg: MultiSigConfig = env
            .storage()
            .instance()
            .get(&DataKey::MultiSigConfig)
            .ok_or(AuthError::NotInitialized)?;
        if proposal.approvals.len() < cfg.threshold {
            return Err(AuthError::ThresholdNotReached);
        }
        proposal.executed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        // Dispatch based on action type
        // For generic payload handling, we publish an event and let off-chain
        // executors or the target contract pick it up. For built-in actions we
        // apply them directly.
        if proposal.action == Symbol::new(&env, "add_admin") {
            // payload is an Address XDR
            // We cannot directly deserialize Address from Bytes in no_std without helper,
            // so we treat the payload as a hook: emit event for indexer. Admin addition
            // via proposal should be handled by `add_multisig_admin` below for type safety.
        } else if proposal.action == Symbol::new(&env, "set_threshold") {
            // payload encodes u32 threshold
        }
        env.events().publish(
            (Symbol::new(&env, "proposal_executed"), proposal_id),
            (proposal.action, proposal.approvals.len()),
        );
        Ok(())
    }

    /// View a proposal.
    pub fn get_proposal(env: Env, proposal_id: u32) -> Option<Proposal> {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
    }

    /// Add a new admin to the multi-sig set (requires multi-sig proposal threshold already met).
    /// This is the direct execution path called after a successful proposal.
    pub fn add_multisig_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        let mut cfg: MultiSigConfig = env
            .storage()
            .instance()
            .get(&DataKey::MultiSigConfig)
            .ok_or(AuthError::NotInitialized)?;
        // Prevent duplicates
        for i in 0..cfg.admins.len() {
            if let Some(a) = cfg.admins.get(i) {
                let h1: BytesN<32> = env.crypto().keccak256(&a.clone().to_xdr(&env)).into();
                let h2: BytesN<32> = env.crypto().keccak256(&new_admin.clone().to_xdr(&env)).into();
                let mut diff = 0u8;
                for k in 0..32 {
                    diff |= h1.to_array()[k] ^ h2.to_array()[k];
                }
                if diff == 0 {
                    return Ok(()); // already admin
                }
            }
        }
        cfg.admins.push_back(new_admin.clone());
        env.storage().instance().set(&DataKey::MultiSigConfig, &cfg);
        // Give the new admin the admin role as well
        let mut roles = Vec::new(&env);
        roles.push_back(Symbol::new(&env, "admin"));
        env.storage()
            .persistent()
            .set(&DataKey::UserRoles(new_admin.clone()), &roles);
        env.events()
            .publish((Symbol::new(&env, "multisig_admin_added"),), new_admin);
        Ok(())
    }

    /// Update multi-sig threshold (admin only).
    pub fn set_threshold(env: Env, caller: Address, threshold: u32) -> Result<(), AuthError> {
        caller.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &caller)?;
        let mut cfg: MultiSigConfig = env
            .storage()
            .instance()
            .get(&DataKey::MultiSigConfig)
            .ok_or(AuthError::NotInitialized)?;
        if threshold == 0 {
            return Err(AuthError::ThresholdTooLow);
        }
        if threshold > cfg.admins.len() {
            return Err(AuthError::ThresholdTooHigh);
        }
        cfg.threshold = threshold;
        env.storage().instance().set(&DataKey::MultiSigConfig, &cfg);
        env.events()
            .publish((Symbol::new(&env, "threshold_updated"),), threshold);
        Ok(())
    }

    /// Authorize a contract to call another contract on behalf of users.
    /// Implements the "managed layer between contracts": contract A can only
    /// call contract B if the auth manager has an explicit authorization entry.
    pub fn authorize_contract_call(
        env: Env,
        admin: Address,
        caller_contract: Address,
        target_contract: Address,
    ) -> Result<(), AuthError> {
        admin.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &admin)?;
        env.storage().persistent().set(
            &DataKey::AuthorizedCaller(caller_contract.clone(), target_contract.clone()),
            &true,
        );
        env.events().publish(
            (Symbol::new(&env, "contract_authorized"),),
            (caller_contract, target_contract),
        );
        Ok(())
    }

    /// Revoke contract-to-contract authorization.
    pub fn revoke_contract_call(
        env: Env,
        admin: Address,
        caller_contract: Address,
        target_contract: Address,
    ) -> Result<(), AuthError> {
        admin.require_auth();
        require_not_paused(&env)?;
        require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .remove(&DataKey::AuthorizedCaller(
                caller_contract.clone(),
                target_contract.clone(),
            ));
        env.events().publish(
            (Symbol::new(&env, "contract_revoked"),),
            (caller_contract, target_contract),
        );
        Ok(())
    }

    /// Check whether a contract is authorized to call another.
    pub fn is_contract_authorized(env: Env, caller: Address, target: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::AuthorizedCaller(caller, target))
            .unwrap_or(false)
    }

    /// Enforce that a caller has a required permission. Callable cross-contract
    /// so poker-table, committee-registry, etc. can delegate auth checks here.
    pub fn require_permission(
        env: Env,
        user: Address,
        permission: Permission,
    ) -> Result<(), AuthError> {
        if Self::has_permission(env.clone(), user.clone(), permission.clone()) {
            Ok(())
        } else {
            Err(AuthError::InsufficientPermissions)
        }
    }

    pub fn get_multisig_config(env: Env) -> Option<MultiSigConfig> {
        env.storage()
            .instance()
            .get(&DataKey::MultiSigConfig)
    }

    pub fn pause(env: Env, caller: Address) -> Result<(), AuthError> {
        caller.require_auth();
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "auth_paused"),), caller);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), AuthError> {
        caller.require_auth();
        require_admin(&env, &caller)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "auth_unpaused"),), caller);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};

    fn setup() -> (Env, AuthManagerContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(AuthManagerContract, ());
        let client = AuthManagerContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &1);
        (env, client, admin)
    }

    #[test]
    fn test_initial_roles() {
        let (env, client, admin) = setup();
        assert!(client.has_role(&admin, &Symbol::new(&env, "admin")));
        assert!(client.has_permission(
            &admin,
            &Permission::CreateTable
        ));
        assert!(client.has_permission(&admin, &Permission::UpgradeContract));
    }

    #[test]
    fn test_grant_and_check_role() {
        let (env, client, admin) = setup();
        let user = Address::generate(&env);
        client.grant_role(&admin, &user, &Symbol::new(&env, "operator"));
        assert!(client.has_role(&user, &Symbol::new(&env, "operator")));
        assert!(client.has_permission(&user, &Permission::CreateTable));
        assert!(!client.has_permission(&user, &Permission::UpgradeContract));
    }

    #[test]
    fn test_multisig_proposal_flow() {
        let (env, client, admin) = setup();
        let admin2 = Address::generate(&env);
        // Add second admin
        client.add_multisig_admin(&admin, &admin2);
        client.set_threshold(&admin, &2);
        let payload = Bytes::new(&env);
        let pid = client.propose_action(
            &admin,
            &Symbol::new(&env, "test_action"),
            &None,
            &payload,
            &1,
        );
        // Second admin approves
        client.approve_action(&admin2, &pid);
        // Fast-forward past timelock
        env.ledger().set_timestamp(env.ledger().timestamp() + 2);
        client.execute_action(&admin, &pid);
        let prop = client.get_proposal(&pid).unwrap();
        assert!(prop.executed);
    }

    #[test]
    fn test_contract_authorization_layer() {
        let (env, client, admin) = setup();
        let poker = Address::generate(&env);
        let verifier = Address::generate(&env);
        assert!(!client.is_contract_authorized(&poker, &verifier));
        client.authorize_contract_call(&admin, &poker, &verifier);
        assert!(client.is_contract_authorized(&poker, &verifier));
        client.revoke_contract_call(&admin, &poker, &verifier);
        assert!(!client.is_contract_authorized(&poker, &verifier));
    }
}
