use soroban_sdk::{contracttype, Address, Env, Map, Symbol, Vec};
use crate::types::PokerTableError;

/// Multi-currency support for buy-ins via Stellar anchors (Issue #193)

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrencyInfo {
    pub token_address: Address,
    pub enabled: bool,
    pub oracle_address: Address, // Price oracle for conversion
}

fn store_key(table_id: u32) -> (Symbol, u32) {
    (Symbol::short("curr"), table_id)
}

/// Whitelist a currency for a specific table
pub fn whitelist_currency(env: &Env, table_id: u32, token: Address, oracle: Address) {
    let key = store_key(table_id);
    let mut currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    currencies.set(
        token.clone(),
        CurrencyInfo {
            token_address: token.clone(),
            enabled: true,
            oracle_address: oracle,
        },
    );
    env.storage().persistent().set(&key, &currencies);
    env.storage()
        .persistent()
        .extend_ttl(&key, 17_280, 518_400);
}

/// Legacy overload without table_id (defaults to table 0 for backwards compat)
pub fn whitelist_currency_legacy(env: &Env, token: Address, oracle: Address) {
    whitelist_currency(env, 0, token, oracle)
}

/// Remove a currency from whitelist
pub fn remove_currency(env: &Env, table_id: u32, token: &Address) {
    let key = store_key(table_id);
    let mut currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    currencies.remove(token.clone());
    env.storage().persistent().set(&key, &currencies);
}

/// Check if currency is whitelisted for a table
pub fn is_whitelisted(env: &Env, table_id: u32, token: &Address) -> bool {
    let key = store_key(table_id);
    let currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    currencies
        .get(token.clone())
        .map(|info| info.enabled)
        .unwrap_or(false)
}

/// Legacy is_currency_whitelisted without table_id
pub fn is_currency_whitelisted(env: &Env, token: &Address) -> bool {
    is_whitelisted(env, 0, token)
}

pub fn get_currency_oracle(env: &Env, table_id: u32, token: &Address) -> Option<Address> {
    let key = store_key(table_id);
    let currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Map::new(env));
    currencies.get(token.clone()).map(|info| info.oracle_address)
}

/// Convert currency amount to base token amount using oracle price
pub fn convert_to_base_token(
    env: &Env,
    table_id: u32,
    token: &Address,
    amount: i128,
) -> Result<i128, PokerTableError> {
    if !is_whitelisted(env, table_id, token) {
        return Err(PokerTableError::InvalidBuyIn);
    }
    // Simplified 1:1 conversion for now; oracle lookup would be done off-chain or via
    // a proper Stellar oracle type conversion handling Address -> Val via env.invoke_contract
    let _ = get_currency_oracle(env, table_id, token);
    Ok(amount)
}

/// Legacy convert_to_xlm
pub fn convert_to_xlm(env: &Env, token: &Address, amount: i128) -> i128 {
    convert_to_base_token(env, 0, token, amount).unwrap_or(amount)
}
