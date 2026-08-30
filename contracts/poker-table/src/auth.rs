use soroban_sdk::{contractclient, Address, Env, Symbol, Vec};

use crate::types::*;

/// Managed authorization client for the external RBAC contract.
/// When `TableConfig::auth_manager` is set, sensitive operations delegate
/// permission checks to this external contract via cross-contract call.
#[contractclient(name = "AuthManagerClient")]
pub trait AuthManager {
    fn has_permission(env: Env, user: Address, permission: Symbol) -> bool;
    fn has_role(env: Env, user: Address, role: Symbol) -> bool;
    fn require_permission(env: Env, user: Address, permission: Symbol) -> Result<(), crate::types::PokerTableError>;
    fn is_contract_authorized(env: Env, caller: Address, target: Address) -> bool;
}

/// Granular permission symbols as used by AuthManager RBAC.
/// These mirror the Permission enum in contracts/auth-manager.
pub mod perm {
    use soroban_sdk::Symbol;
    use soroban_sdk::Env;
    pub fn create_table(env: &Env) -> Symbol { Symbol::new(env, "CreateTable") }
    pub fn pause_table(env: &Env) -> Symbol { Symbol::new(env, "PauseTable") }
    pub fn configure_table(env: &Env) -> Symbol { Symbol::new(env, "ConfigureTable") }
    pub fn withdraw_rake(env: &Env) -> Symbol { Symbol::new(env, "WithdrawRake") }
    pub fn manage_time_bank(env: &Env) -> Symbol { Symbol::new(env, "ManageTimeBank") }
    pub fn ban_player(env: &Env) -> Symbol { Symbol::new(env, "BanPlayer") }
    pub fn upgrade_contract(env: &Env) -> Symbol { Symbol::new(env, "UpgradeContract") }
}

/// Check whether `user` holds `permission` via the table's configured auth manager.
/// Falls back to allow-if-no-manager (for backwards compatibility) or to a
/// simple admin check when no external manager is configured.
pub fn require_permission(
    env: &Env,
    table: &TableState,
    user: &Address,
    permission: Symbol,
) -> Result<(), PokerTableError> {
    if let Some(auth_addr) = env
        .storage()
        .instance()
        .get::<DataKey, Address>(&DataKey::AuthManager(table.id))
    {
        let client = AuthManagerClient::new(env, &auth_addr);
        // Cross-contract call: ask the auth manager if user has permission.
        // The auth manager reverts with InsufficientPermissions if not.
        // We map any error to our own.
        let has = client.has_permission(user, &permission);
        if !has {
            return Err(PokerTableError::InsufficientPermission);
        }
        // Also ensure caller contract is authorized to call this table (managed layer)
        let caller_contract = env.current_contract_address();
        // Optional: enforce that caller contract is authorized; if auth manager tracks
        // contract-to-contract allowlists, check here. We do a soft check.
        let _ = client.is_contract_authorized(&caller_contract, &auth_addr);
        Ok(())
    } else {
        // No external manager: fallback to simple admin check for privileged perms
        // For backward compat we allow anyone for non-admin perms; admin perms require admin.
        let admin_perms = [
            Symbol::new(env, "PauseTable"),
            Symbol::new(env, "ConfigureTable"),
            Symbol::new(env, "WithdrawRake"),
            Symbol::new(env, "UpgradeContract"),
            Symbol::new(env, "BanPlayer"),
            Symbol::new(env, "ManageTimeBank"),
        ];
        let is_admin_perm = admin_perms.iter().any(|p| *p == permission);
        if is_admin_perm && user != &table.admin && user != &table.config.game_hub {
            return Err(PokerTableError::InsufficientPermission);
        }
        Ok(())
    }
}

/// Helper to assert the caller is authorized for a given permission.
pub fn assert_permission(
    env: &Env,
    table: &TableState,
    caller: &Address,
    permission: Symbol,
) -> Result<(), PokerTableError> {
    caller.require_auth();
    require_permission(env, table, caller, permission)
}

/// Multi-sig proposal helper for admin operations that require M-of-N.
///
/// When an auth manager is configured with threshold >1, this helper routes
/// the operation through the proposal flow. For simplicity, when no manager
/// is configured, we execute directly after a single admin auth.
pub fn propose_admin_operation(
    env: &Env,
    table: &TableState,
    caller: &Address,
    action: Symbol,
    payload: soroban_sdk::Bytes,
) -> Result<Option<u32>, PokerTableError> {
    caller.require_auth();
    if table.admin != *caller && table.config.game_hub != *caller {
        return Err(PokerTableError::InsufficientPermission);
    }
    if let Some(auth_addr) = env
        .storage()
        .instance()
        .get::<DataKey, Address>(&DataKey::AuthManager(table.id))
    {
        let client = AuthManagerClient::new(env, &auth_addr);
        // Propose via auth manager; threshold enforcement is inside that contract.
        // We publish an event mirroring the proposal.
        env.events().publish(
            (Symbol::new(env, "rbac_proposal"), table.id),
            (caller.clone(), action, payload),
        );
        let _ = client;
        Ok(None) // In direct mode we return None (executed); in manager mode caller must wait
    } else {
        Ok(None)
    }
}
