#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol,
};

/// Maximum protocol fee, in basis points (5%). Enforced on every
/// `set_fee_bps` call so the switch can never be cranked above the cap.
pub const MAX_FEE_BPS: u32 = 500;

/// Protocol fee switch and treasury accounting.
///
/// Separates protocol-level revenue from the existing table rake and
/// committee node fees: the fee switch is a configurable basis-point rate,
/// and accrued fees sit in an on-chain treasury with transparent balances,
/// admin-only withdrawals, and conservation-accountable in/out totals.
#[contract]
pub struct ProtocolTreasury;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TreasuryError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    FeeBpsExceedsMax = 4,
    NonPositiveAmount = 5,
    ExceedsTreasuryBalance = 6,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Token,
    /// Protocol fee switch in basis points.
    FeeBps,
    /// Current treasury balance.
    Balance,
    /// Lifetime total deposited.
    TotalIn,
    /// Lifetime total withdrawn.
    TotalOut,
}

#[contractimpl]
impl ProtocolTreasury {
    /// Initialize the treasury with an admin and the fee token.
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), TreasuryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TreasuryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::FeeBps, &0u32);
        env.storage().instance().set(&DataKey::Balance, &0i128);
        env.storage().instance().set(&DataKey::TotalIn, &0i128);
        env.storage().instance().set(&DataKey::TotalOut, &0i128);
        Ok(())
    }

    /// Set the protocol fee switch (admin only), in basis points.
    /// Capped at `MAX_FEE_BPS` (5%). `0` disables the protocol fee.
    /// Emits `fee_bps_updated`.
    pub fn set_fee_bps(env: Env, admin: Address, fee_bps: u32) -> Result<(), TreasuryError> {
        Self::require_admin(&env, &admin)?;
        if fee_bps > MAX_FEE_BPS {
            return Err(TreasuryError::FeeBpsExceedsMax);
        }
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        env.events()
            .publish((Symbol::new(&env, "fee_bps_updated"),), fee_bps);
        Ok(())
    }

    /// Read the current protocol fee switch in basis points.
    pub fn get_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::FeeBps)
            .unwrap_or(0)
    }

    /// Apply the current fee switch to a gross amount:
    /// `floor(gross * fee_bps / 10_000)`. Pure view helper so revenue
    /// splitters stay consistent with the on-chain switch.
    pub fn compute_fee(env: Env, gross: i128) -> i128 {
        let fee_bps: u32 = Self::get_fee_bps(env);
        if gross <= 0 || fee_bps == 0 {
            return 0;
        }
        gross * fee_bps as i128 / 10_000
    }

    /// Deposit tokens into the protocol treasury. Emits `fee_deposited`.
    pub fn deposit(env: Env, from: Address, amount: i128) -> Result<(), TreasuryError> {
        Self::require_initialized(&env)?;
        from.require_auth();
        if amount <= 0 {
            return Err(TreasuryError::NonPositiveAmount);
        }

        let token = Self::token_client(&env);
        let contract = env.current_contract_address();
        token.transfer(&from, &contract, &amount);

        let balance = Self::balance(&env) + amount;
        env.storage().instance().set(&DataKey::Balance, &balance);
        let total_in: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIn)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalIn, &(total_in + amount));

        env.events().publish(
            (Symbol::new(&env, "fee_deposited"),),
            (from, amount, balance),
        );
        Ok(())
    }

    /// Withdraw from the treasury to `to` (admin only). Returns the amount
    /// withdrawn. Cannot exceed the current balance. Emits `fee_withdrawn`.
    pub fn withdraw(env: Env, admin: Address, to: Address, amount: i128) -> Result<i128, TreasuryError> {
        Self::require_admin(&env, &admin)?;
        if amount <= 0 {
            return Err(TreasuryError::NonPositiveAmount);
        }

        let balance = Self::balance(&env);
        if amount > balance {
            return Err(TreasuryError::ExceedsTreasuryBalance);
        }

        env.storage()
            .instance()
            .set(&DataKey::Balance, &(balance - amount));
        let total_out: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalOut)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalOut, &(total_out + amount));

        let token = Self::token_client(&env);
        let contract = env.current_contract_address();
        token.transfer(&contract, &to, &amount);

        env.events().publish(
            (Symbol::new(&env, "fee_withdrawn"),),
            (to, amount, balance - amount),
        );
        Ok(amount)
    }

    /// Read the current treasury balance.
    pub fn get_balance(env: Env) -> i128 {
        Self::balance(&env)
    }

    /// Treasury accounting in one read: `(balance, lifetime_in, lifetime_out)`.
    /// Conservation invariant: `balance == lifetime_in - lifetime_out`.
    pub fn get_state(env: Env) -> (i128, i128, i128) {
        (
            Self::balance(&env),
            env.storage().instance().get(&DataKey::TotalIn).unwrap_or(0),
            env.storage().instance().get(&DataKey::TotalOut).unwrap_or(0),
        )
    }

    fn require_initialized(env: &Env) -> Result<(), TreasuryError> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(TreasuryError::NotInitialized);
        }
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), TreasuryError> {
        Self::require_initialized(env)?;
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(TreasuryError::NotInitialized)?;
        admin.require_auth();
        if *admin != stored {
            return Err(TreasuryError::NotAdmin);
        }
        Ok(())
    }

    fn balance(env: &Env) -> i128 {
        env.storage().instance().get(&DataKey::Balance).unwrap_or(0)
    }

    fn token_client(env: &Env) -> token::Client<'_> {
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .expect("not initialized");
        token::Client::new(env, &token_addr)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Env,
    };

    struct Setup {
        env: Env,
        client: ProtocolTreasuryClient<'static>,
        token: TokenClient<'static>,
        admin: Address,
    }

    fn setup() -> Setup {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ProtocolTreasury, ());
        let client = ProtocolTreasuryClient::new(&env, &contract_id);

        let token_admin_addr = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin_addr);
        let token = TokenClient::new(&env, &sac.address());

        let admin = Address::generate(&env);
        client.initialize(&admin, &sac.address());

        Setup {
            env,
            client,
            token,
            admin,
        }
    }

    fn mint(s: &Setup, to: &Address, amount: i128) {
        StellarAssetClient::new(&s.env, &s.token.address).mint(to, &amount);
    }

    #[test]
    fn initialize_sets_admin_and_zero_state() {
        let s = setup();
        assert_eq!(s.client.get_fee_bps(), 0);
        assert_eq!(s.client.get_balance(), 0);
        let (balance, total_in, total_out) = s.client.get_state();
        assert_eq!((balance, total_in, total_out), (0, 0, 0));
    }

    #[test]
    fn initialize_twice_fails() {
        let s = setup();
        let result = s.client.try_initialize(&s.admin, &s.token.address);
        assert_eq!(result, Err(Ok(TreasuryError::AlreadyInitialized)));
    }

    #[test]
    fn set_fee_bps_updates_switch_and_caps() {
        let s = setup();
        s.client.set_fee_bps(&s.admin, &250);
        assert_eq!(s.client.get_fee_bps(), 250);

        s.client.set_fee_bps(&s.admin, &0);
        assert_eq!(s.client.get_fee_bps(), 0);

        let result = s.client.try_set_fee_bps(&s.admin, &501);
        assert_eq!(result, Err(Ok(TreasuryError::FeeBpsExceedsMax)));
    }

    #[test]
    fn non_admin_cannot_set_fee_bps() {
        let s = setup();
        let stranger = Address::generate(&s.env);
        let result = s.client.try_set_fee_bps(&stranger, &100);
        assert_eq!(result, Err(Ok(TreasuryError::NotAdmin)));
    }

    #[test]
    fn compute_fee_applies_current_switch() {
        let s = setup();
        assert_eq!(s.client.compute_fee(&10_000), 0); // switch off
        s.client.set_fee_bps(&s.admin, &200); // 2%
        assert_eq!(s.client.compute_fee(&10_000), 200);
        assert_eq!(s.client.compute_fee(&0), 0);
        assert_eq!(s.client.compute_fee(&-5), 0);
    }

    #[test]
    fn deposit_accumulates_balance() {
        let s = setup();
        let payer = Address::generate(&s.env);
        mint(&s, &payer, 1_000);

        s.client.deposit(&payer, &400);
        s.client.deposit(&payer, &600);

        assert_eq!(s.client.get_balance(), 1_000);
        let (balance, total_in, total_out) = s.client.get_state();
        assert_eq!((balance, total_in, total_out), (1_000, 1_000, 0));
        assert_eq!(s.token.balance(&s.client.address), 1_000);
    }

    #[test]
    fn deposit_rejects_non_positive() {
        let s = setup();
        let payer = Address::generate(&s.env);
        let result = s.client.try_deposit(&payer, &0);
        assert_eq!(result, Err(Ok(TreasuryError::NonPositiveAmount)));
        let result = s.client.try_deposit(&payer, &-1);
        assert_eq!(result, Err(Ok(TreasuryError::NonPositiveAmount)));
    }

    #[test]
    fn withdraw_transfers_and_conserves() {
        let s = setup();
        let payer = Address::generate(&s.env);
        mint(&s, &payer, 1_500);
        s.client.deposit(&payer, &1_000);
        s.client.deposit(&payer, &500);

        let recipient = Address::generate(&s.env);
        let withdrawn = s.client.withdraw(&s.admin, &recipient, &700);
        assert_eq!(withdrawn, 700);
        assert_eq!(s.token.balance(&recipient), 700);

        // Conservation: balance == lifetime_in - lifetime_out, and the
        // contract still holds exactly the remaining balance.
        let (balance, total_in, total_out) = s.client.get_state();
        assert_eq!(balance, total_in - total_out);
        assert_eq!(balance, 800);
        assert_eq!(total_in, 1_500);
        assert_eq!(total_out, 700);
        assert_eq!(s.token.balance(&s.client.address), balance);
    }

    #[test]
    fn withdraw_cannot_exceed_balance() {
        let s = setup();
        let payer = Address::generate(&s.env);
        mint(&s, &payer, 100);
        s.client.deposit(&payer, &100);

        let recipient = Address::generate(&s.env);
        let result = s.client.try_withdraw(&s.admin, &recipient, &101);
        assert_eq!(result, Err(Ok(TreasuryError::ExceedsTreasuryBalance)));
    }

    #[test]
    fn non_admin_cannot_withdraw() {
        let s = setup();
        let payer = Address::generate(&s.env);
        mint(&s, &payer, 100);
        s.client.deposit(&payer, &100);

        let stranger = Address::generate(&s.env);
        let result = s.client.try_withdraw(&stranger, &stranger, &50);
        assert_eq!(result, Err(Ok(TreasuryError::NotAdmin)));
    }

    #[test]
    fn withdraw_rejects_non_positive() {
        let s = setup();
        let recipient = Address::generate(&s.env);
        let result = s.client.try_withdraw(&s.admin, &recipient, &0);
        assert_eq!(result, Err(Ok(TreasuryError::NonPositiveAmount)));
    }
}
