use soroban_sdk::{
    contracttype, token, Address, Env, InvokeError, IntoVal, Map, Symbol, Val, Vec as SorobanVec,
};
use crate::types::PokerTableError;

/// Multi-currency support for buy-ins via Stellar anchors (Issue #193)

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrencyInfo {
    pub token_address: Address,
    pub enabled: bool,
    pub oracle_address: Address, // Price oracle for conversion
    /// SAC decimal scale. Poker amounts are normalized to the 7-decimal Stellar scale.
    pub decimals: u32,
}

fn store_key(table_id: u32) -> (Symbol, u32) {
    (Symbol::short("curr"), table_id)
}

/// Whitelist a currency for a specific table
pub fn whitelist_currency(env: &Env, table_id: u32, token: Address, oracle: Address) -> Result<(), PokerTableError> {
    let decimals = token::Client::new(env, &token).decimals();
    if decimals != 7 {
        return Err(PokerTableError::UnsupportedTokenScale);
    }
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
            decimals,
        },
    );
    env.storage().persistent().set(&key, &currencies);
    crate::ttl::bump_persistent(env, &key, crate::ttl::TABLE);
    Ok(())
}

/// Legacy overload without table_id (defaults to table 0 for backwards compat)
pub fn whitelist_currency_legacy(env: &Env, token: Address, oracle: Address) {
    let _ = whitelist_currency(env, 0, token, oracle);
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

/// Get currency oracle.
///
/// # Parameters
/// - `table_id`: parameter
/// - `token`: parameter
///
/// # Returns
/// - `Option<Address>`
///
/// # Errors
/// Returns an error if the operation fails.
///
/// # Authorization
/// Requires appropriate authorization.
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
    let info = currencies_for(env, table_id).get(token.clone()).ok_or(PokerTableError::InvalidBuyIn)?;
    if info.decimals != 7 {
        return Err(PokerTableError::UnsupportedTokenScale);
    }
    let _ = get_currency_oracle(env, table_id, token);
    Ok(amount)
}

fn currencies_for(env: &Env, table_id: u32) -> Map<Address, CurrencyInfo> {
    env.storage().persistent().get(&store_key(table_id)).unwrap_or(Map::new(env))
}

/// Legacy convert_to_xlm
pub fn convert_to_xlm(env: &Env, token: &Address, amount: i128) -> i128 {
    convert_to_base_token(env, 0, token, amount).unwrap_or(amount)
}

/// Try an SAC transfer. On freeze, clawback, or any transfer failure, keep the
/// payout as an internal claimable balance and record the visible table deficit.
pub fn safe_transfer(
    env: &Env,
    table_id: u32,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) -> bool {
    if amount <= 0 {
        return true;
    }
    let mut args: SorobanVec<Val> = SorobanVec::new(env);
    args.push_back(from.into_val(env));
    args.push_back(to.into_val(env));
    args.push_back(amount.into_val(env));
    let transferred = env
        .try_invoke_contract::<(), InvokeError>(
            token,
            &Symbol::new(env, "transfer"),
            args,
        )
        .ok()
        .and_then(|r| r.ok())
        .is_some();

    if transferred {
        return true;
    }

    let claim_key = (Symbol::short("claim"), table_id, to.clone());
    let prior: i128 = env.storage().persistent().get(&claim_key).unwrap_or(0);
    env.storage()
        .persistent()
        .set(&claim_key, &(prior.saturating_add(amount)));
    crate::ttl::bump_persistent(env, &claim_key, crate::ttl::TABLE);

    let deficit_key = (Symbol::short("def"), table_id);
    let prior_deficit: i128 = env.storage().instance().get(&deficit_key).unwrap_or(0);
    env.storage()
        .instance()
        .set(&deficit_key, &prior_deficit.saturating_add(amount));
    false
}

pub fn claimable_balance(env: &Env, table_id: u32, player: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&(Symbol::short("claim"), table_id, player.clone()))
        .unwrap_or(0)
}

pub fn settlement_deficit(env: &Env, table_id: u32) -> i128 {
    env.storage()
        .instance()
        .get(&(Symbol::short("def"), table_id))
        .unwrap_or(0)
}
