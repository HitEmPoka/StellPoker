use soroban_sdk::{contracttype, Address, Env, Map, Symbol};

/// Multi-currency support for buy-ins via Stellar anchors
/// Issue #193

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrencyInfo {
    pub token_address: Address,
    pub enabled: bool,
    pub oracle_address: Address, // Price oracle for conversion
}

const CURRENCIES: Symbol = Symbol::short("CURR");

pub fn whitelist_currency(env: &Env, token: Address, oracle: Address) {
    let mut currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&CURRENCIES)
        .unwrap_or(Map::new(env));

    currencies.set(
        token.clone(),
        CurrencyInfo {
            token_address: token,
            enabled: true,
            oracle_address: oracle,
        },
    );

    env.storage().persistent().set(&CURRENCIES, &currencies);
}

pub fn is_currency_whitelisted(env: &Env, token: &Address) -> bool {
    let currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&CURRENCIES)
        .unwrap_or(Map::new(env));

    currencies
        .get(token.clone())
        .map(|info| info.enabled)
        .unwrap_or(false)
}

pub fn get_currency_oracle(env: &Env, token: &Address) -> Option<Address> {
    let currencies: Map<Address, CurrencyInfo> = env
        .storage()
        .persistent()
        .get(&CURRENCIES)
        .unwrap_or(Map::new(env));

    currencies
        .get(token.clone())
        .map(|info| info.oracle_address)
}

/// Convert anchor asset amount to XLM using oracle price
/// Returns equivalent XLM amount
pub fn convert_to_xlm(env: &Env, token: &Address, amount: i128) -> i128 {
    if let Some(oracle) = get_currency_oracle(env, token) {
        // Call oracle contract to get conversion rate
        // Simplified: oracle returns rate as XLM per token unit (with 7 decimals)
        let rate: i128 = env
            .invoke_contract(&oracle, &Symbol::new(env, "get_price"), (token,).into())
            .unwrap_or(10_000_000); // Default 1:1 if oracle fails

        (amount * rate) / 10_000_000
    } else {
        amount // 1:1 if no oracle configured
    }
}
