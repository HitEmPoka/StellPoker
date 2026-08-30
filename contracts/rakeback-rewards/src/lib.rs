#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol,
};

/// Loyalty tier based on accumulated lifetime points / rake volume.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Tier {
    Bronze = 0,   // 5% baseline rakeback (500 bps)
    Silver = 1,   // 10% rakeback (1000 bps)
    Gold = 2,     // 15% rakeback (1500 bps)
    Platinum = 3, // 20% rakeback (2000 bps)
    Diamond = 4,  // 30% rakeback (3000 bps)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerRewards {
    pub player: Address,
    pub points_balance: u64,
    pub lifetime_points: u64,
    pub total_rake_paid: i128,
    pub tier: Tier,
    pub active_discount_bps: u32,
    pub lifetime_points_redeemed: u64,
    pub last_activity_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RakebackConfig {
    /// Points earned per 1 rake unit (e.g. 10 points per 1 unit).
    pub points_per_rake_unit: u32,
    /// Points needed to redeem 1 reward token (e.g. 100 points = 1 token).
    pub token_redemption_rate: u32,
    /// Points needed per 100 bps (1%) rake discount for next hands.
    pub discount_points_cost: u64,
    /// Maximum discount allowed to be redeemed (e.g. 3000 = 30%).
    pub max_discount_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TierConfig {
    pub silver_threshold: u64,
    pub gold_threshold: u64,
    pub platinum_threshold: u64,
    pub diamond_threshold: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    RewardToken,
    Config,
    TierThresholds,
    Player(Address),
    Recorder(Address),
    Paused,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RakebackError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    InsufficientPoints = 5,
    InvalidAmount = 6,
    NoRewardTokenConfigured = 7,
    DiscountExceedsMax = 8,
    InsufficientTokenBalance = 9,
}

const DEFAULT_POINTS_PER_RAKE: u32 = 10;
const DEFAULT_TOKEN_REDEMPTION_RATE: u32 = 100;
const DEFAULT_DISCOUNT_POINTS_COST: u64 = 500;
const DEFAULT_MAX_DISCOUNT_BPS: u32 = 3000;

const DEFAULT_SILVER_THRESHOLD: u64 = 1_000;
const DEFAULT_GOLD_THRESHOLD: u64 = 5_000;
const DEFAULT_PLATINUM_THRESHOLD: u64 = 20_000;
const DEFAULT_DIAMOND_THRESHOLD: u64 = 50_000;

#[contract]
pub struct RakebackRewardsContract;

#[contractimpl]
impl RakebackRewardsContract {
    /// Initialize the rakeback rewards loyalty program contract.
    pub fn initialize(
        env: Env,
        admin: Address,
        reward_token: Option<Address>,
        config: Option<RakebackConfig>,
        tier_thresholds: Option<TierConfig>,
    ) -> Result<(), RakebackError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RakebackError::AlreadyInitialized);
        }

        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);

        if let Some(token_addr) = reward_token {
            env.storage().instance().set(&DataKey::RewardToken, &token_addr);
        }

        let cfg = config.unwrap_or(RakebackConfig {
            points_per_rake_unit: DEFAULT_POINTS_PER_RAKE,
            token_redemption_rate: DEFAULT_TOKEN_REDEMPTION_RATE,
            discount_points_cost: DEFAULT_DISCOUNT_POINTS_COST,
            max_discount_bps: DEFAULT_MAX_DISCOUNT_BPS,
        });
        env.storage().instance().set(&DataKey::Config, &cfg);

        let tiers = tier_thresholds.unwrap_or(TierConfig {
            silver_threshold: DEFAULT_SILVER_THRESHOLD,
            gold_threshold: DEFAULT_GOLD_THRESHOLD,
            platinum_threshold: DEFAULT_PLATINUM_THRESHOLD,
            diamond_threshold: DEFAULT_DIAMOND_THRESHOLD,
        });
        env.storage().instance().set(&DataKey::TierThresholds, &tiers);

        // Admin is authorized recorder by default
        env.storage().instance().set(&DataKey::Recorder(admin.clone()), &true);
        env.storage().instance().set(&DataKey::Paused, &false);

        Ok(())
    }

    /// Record rake paid by a player and credit loyalty points.
    /// Authorized callers: Admin or authorized poker table / coordinator contracts.
    pub fn record_rake(
        env: Env,
        caller: Address,
        player: Address,
        rake_paid: i128,
    ) -> Result<u64, RakebackError> {
        caller.require_auth();
        Self::ensure_not_paused(&env)?;

        if !Self::is_authorized_recorder(&env, &caller) {
            return Err(RakebackError::Unauthorized);
        }

        if rake_paid <= 0 {
            return Err(RakebackError::InvalidAmount);
        }

        let config = Self::get_config(&env);
        let tier_config = Self::get_tier_thresholds(&env);

        let earned_points = ((rake_paid as u64)
            .checked_mul(config.points_per_rake_unit as u64))
            .ok_or(RakebackError::InvalidAmount)?;

        let mut record = Self::get_or_create_player(&env, &player);
        record.points_balance = record
            .points_balance
            .checked_add(earned_points)
            .ok_or(RakebackError::InvalidAmount)?;
        record.lifetime_points = record
            .lifetime_points
            .checked_add(earned_points)
            .ok_or(RakebackError::InvalidAmount)?;
        record.total_rake_paid = record
            .total_rake_paid
            .checked_add(rake_paid)
            .ok_or(RakebackError::InvalidAmount)?;
        record.tier = Self::calculate_tier(record.lifetime_points, &tier_config);
        record.last_activity_ledger = env.ledger().sequence();

        env.storage().persistent().set(&DataKey::Player(player.clone()), &record);

        env.events().publish(
            (Symbol::new(&env, "rake_recorded"), player),
            (rake_paid, earned_points, record.points_balance, record.tier as u32),
        );

        Ok(earned_points)
    }

    /// Get current player rewards profile.
    pub fn get_player_rewards(env: Env, player: Address) -> PlayerRewards {
        Self::get_or_create_player(&env, &player)
    }

    /// Get tier rakeback percentage in basis points (500 = 5%, 3000 = 30%).
    pub fn get_tier_rakeback_bps(_env: Env, tier: Tier) -> u32 {
        match tier {
            Tier::Bronze => 500,
            Tier::Silver => 1000,
            Tier::Gold => 1500,
            Tier::Platinum => 2000,
            Tier::Diamond => 3000,
        }
    }

    /// Redeem loyalty points for a rake discount (in basis points) on future hands.
    pub fn redeem_for_discount(
        env: Env,
        player: Address,
        points_to_spend: u64,
    ) -> Result<u32, RakebackError> {
        player.require_auth();
        Self::ensure_not_paused(&env)?;

        if points_to_spend == 0 {
            return Err(RakebackError::InvalidAmount);
        }

        let config = Self::get_config(&env);
        let mut record = Self::get_or_create_player(&env, &player);

        if record.points_balance < points_to_spend {
            return Err(RakebackError::InsufficientPoints);
        }

        // Each `discount_points_cost` points gives 100 bps discount
        let discount_increments = points_to_spend / config.discount_points_cost;
        if discount_increments == 0 {
            return Err(RakebackError::InsufficientPoints);
        }

        let added_discount_bps = (discount_increments as u32) * 100;
        let new_discount_bps = record.active_discount_bps.saturating_add(added_discount_bps);

        if new_discount_bps > config.max_discount_bps {
            return Err(RakebackError::DiscountExceedsMax);
        }

        let actual_points_spent = discount_increments * config.discount_points_cost;
        record.points_balance -= actual_points_spent;
        record.lifetime_points_redeemed = record
            .lifetime_points_redeemed
            .saturating_add(actual_points_spent);
        record.active_discount_bps = new_discount_bps;
        record.last_activity_ledger = env.ledger().sequence();

        env.storage().persistent().set(&DataKey::Player(player.clone()), &record);

        env.events().publish(
            (Symbol::new(&env, "discount_redeemed"), player),
            (actual_points_spent, new_discount_bps),
        );

        Ok(new_discount_bps)
    }

    /// Redeem loyalty points for reward tokens.
    pub fn redeem_for_tokens(
        env: Env,
        player: Address,
        points_to_spend: u64,
    ) -> Result<i128, RakebackError> {
        player.require_auth();
        Self::ensure_not_paused(&env)?;

        let token_addr = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::RewardToken)
            .ok_or(RakebackError::NoRewardTokenConfigured)?;

        if points_to_spend == 0 {
            return Err(RakebackError::InvalidAmount);
        }

        let config = Self::get_config(&env);
        let mut record = Self::get_or_create_player(&env, &player);

        if record.points_balance < points_to_spend {
            return Err(RakebackError::InsufficientPoints);
        }

        let token_units = points_to_spend / (config.token_redemption_rate as u64);
        if token_units == 0 {
            return Err(RakebackError::InsufficientPoints);
        }

        let tokens_to_payout = token_units as i128;
        let actual_points_spent = token_units * (config.token_redemption_rate as u64);

        let token_client = token::Client::new(&env, &token_addr);
        let contract_balance = token_client.balance(&env.current_contract_address());
        if contract_balance < tokens_to_payout {
            return Err(RakebackError::InsufficientTokenBalance);
        }

        record.points_balance -= actual_points_spent;
        record.lifetime_points_redeemed = record
            .lifetime_points_redeemed
            .saturating_add(actual_points_spent);
        record.last_activity_ledger = env.ledger().sequence();

        env.storage().persistent().set(&DataKey::Player(player.clone()), &record);

        // Transfer reward tokens from contract to player
        token_client.transfer(&env.current_contract_address(), &player, &tokens_to_payout);

        env.events().publish(
            (Symbol::new(&env, "tokens_redeemed"), player),
            (actual_points_spent, tokens_to_payout),
        );

        Ok(tokens_to_payout)
    }

    /// Consume active discount when a hand is settled (returns active discount bps and resets it).
    pub fn consume_discount(
        env: Env,
        caller: Address,
        player: Address,
    ) -> Result<u32, RakebackError> {
        caller.require_auth();
        Self::ensure_not_paused(&env)?;

        if !Self::is_authorized_recorder(&env, &caller) {
            return Err(RakebackError::Unauthorized);
        }

        let mut record = Self::get_or_create_player(&env, &player);
        let applied_discount = record.active_discount_bps;
        if applied_discount > 0 {
            record.active_discount_bps = 0;
            env.storage().persistent().set(&DataKey::Player(player), &record);
        }

        Ok(applied_discount)
    }

    // ── Admin Management Functions ─────────────────────────────────────────

    pub fn set_config(env: Env, config: RakebackConfig) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Config, &config);
        Ok(())
    }

    pub fn set_tier_thresholds(env: Env, tiers: TierConfig) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::TierThresholds, &tiers);
        Ok(())
    }

    pub fn set_reward_token(env: Env, reward_token: Option<Address>) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        if let Some(token_addr) = reward_token {
            env.storage().instance().set(&DataKey::RewardToken, &token_addr);
        } else {
            env.storage().instance().remove(&DataKey::RewardToken);
        }
        Ok(())
    }

    pub fn set_authorized_recorder(
        env: Env,
        recorder: Address,
        authorized: bool,
    ) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Recorder(recorder), &authorized);
        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), RakebackError> {
        Self::require_admin(&env)?;
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    // ── Internal Helpers ───────────────────────────────────────────────────

    fn require_admin(env: &Env) -> Result<Address, RakebackError> {
        let admin = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::Admin)
            .ok_or(RakebackError::NotInitialized)?;
        admin.require_auth();
        Ok(admin)
    }

    fn ensure_not_paused(env: &Env) -> Result<(), RakebackError> {
        if env.storage().instance().get::<_, bool>(&DataKey::Paused).unwrap_or(false) {
            return Err(RakebackError::ContractPaused);
        }
        Ok(())
    }

    fn is_authorized_recorder(env: &Env, recorder: &Address) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&DataKey::Recorder(recorder.clone()))
            .unwrap_or(false)
    }

    fn get_config(env: &Env) -> RakebackConfig {
        env.storage().instance().get(&DataKey::Config).unwrap_or(RakebackConfig {
            points_per_rake_unit: DEFAULT_POINTS_PER_RAKE,
            token_redemption_rate: DEFAULT_TOKEN_REDEMPTION_RATE,
            discount_points_cost: DEFAULT_DISCOUNT_POINTS_COST,
            max_discount_bps: DEFAULT_MAX_DISCOUNT_BPS,
        })
    }

    fn get_tier_thresholds(env: &Env) -> TierConfig {
        env.storage().instance().get(&DataKey::TierThresholds).unwrap_or(TierConfig {
            silver_threshold: DEFAULT_SILVER_THRESHOLD,
            gold_threshold: DEFAULT_GOLD_THRESHOLD,
            platinum_threshold: DEFAULT_PLATINUM_THRESHOLD,
            diamond_threshold: DEFAULT_DIAMOND_THRESHOLD,
        })
    }

    fn calculate_tier(lifetime_points: u64, config: &TierConfig) -> Tier {
        if lifetime_points >= config.diamond_threshold {
            Tier::Diamond
        } else if lifetime_points >= config.platinum_threshold {
            Tier::Platinum
        } else if lifetime_points >= config.gold_threshold {
            Tier::Gold
        } else if lifetime_points >= config.silver_threshold {
            Tier::Silver
        } else {
            Tier::Bronze
        }
    }

    fn get_or_create_player(env: &Env, player: &Address) -> PlayerRewards {
        env.storage()
            .persistent()
            .get(&DataKey::Player(player.clone()))
            .unwrap_or(PlayerRewards {
                player: player.clone(),
                points_balance: 0,
                lifetime_points: 0,
                total_rake_paid: 0,
                tier: Tier::Bronze,
                active_discount_bps: 0,
                lifetime_points_redeemed: 0,
                last_activity_ledger: env.ledger().sequence(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, token::StellarAssetClient};

    fn setup_contract(env: &Env) -> (Address, Address, RakebackRewardsContractClient<'_>) {
        let admin = Address::generate(env);
        let contract_id = env.register(RakebackRewardsContract, ());
        let client = RakebackRewardsContractClient::new(env, &contract_id);
        (admin, contract_id, client)
    }

    #[test]
    fn test_initialize_and_record_rake_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &None, &None);

        let player = Address::generate(&env);
        let earned = client.record_rake(&admin, &player, &100);
        assert_eq!(earned, 1000); // 100 * 10 = 1000 points

        let rewards = client.get_player_rewards(&player);
        assert_eq!(rewards.points_balance, 1000);
        assert_eq!(rewards.lifetime_points, 1000);
        assert_eq!(rewards.total_rake_paid, 100);
        assert_eq!(rewards.tier, Tier::Silver); // 1000 pts reaches Silver
    }

    #[test]
    fn test_tier_progression() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &None, &None);

        let player = Address::generate(&env);

        // Bronze
        client.record_rake(&admin, &player, &50); // 500 pts
        assert_eq!(client.get_player_rewards(&player).tier, Tier::Bronze);

        // Silver (1,000 pts)
        client.record_rake(&admin, &player, &50); // +500 = 1000 pts
        assert_eq!(client.get_player_rewards(&player).tier, Tier::Silver);

        // Gold (5,000 pts)
        client.record_rake(&admin, &player, &400); // +4000 = 5000 pts
        assert_eq!(client.get_player_rewards(&player).tier, Tier::Gold);

        // Platinum (20,000 pts)
        client.record_rake(&admin, &player, &1500); // +15000 = 20000 pts
        assert_eq!(client.get_player_rewards(&player).tier, Tier::Platinum);

        // Diamond (50,000 pts)
        client.record_rake(&admin, &player, &3000); // +30000 = 50000 pts
        assert_eq!(client.get_player_rewards(&player).tier, Tier::Diamond);
    }

    #[test]
    fn test_redeem_for_discount_and_consume() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &None, &None);

        let player = Address::generate(&env);
        client.record_rake(&admin, &player, &200); // 2000 pts

        // Redeem 1000 pts (2 * 500) -> 200 bps discount
        let discount = client.redeem_for_discount(&player, &1000);
        assert_eq!(discount, 200);

        let info = client.get_player_rewards(&player);
        assert_eq!(info.points_balance, 1000);
        assert_eq!(info.active_discount_bps, 200);
        assert_eq!(info.lifetime_points_redeemed, 1000);

        // Table consumes discount
        let consumed = client.consume_discount(&admin, &player);
        assert_eq!(consumed, 200);

        let info_after = client.get_player_rewards(&player);
        assert_eq!(info_after.active_discount_bps, 0);
    }

    #[test]
    fn test_redeem_for_tokens() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, contract_id, client) = setup_contract(&env);
        let token_admin = Address::generate(&env);
        let token_contract = env.register_stellar_asset_contract_v2(token_admin);
        let token_client = StellarAssetClient::new(&env, &token_contract.address());

        // Mint reward tokens to the rakeback contract
        token_client.mint(&contract_id, &10000);

        client.initialize(&admin, &Some(token_contract.address()), &None, &None);

        let player = Address::generate(&env);
        client.record_rake(&admin, &player, &100); // 1000 pts

        // 100 pts = 1 token -> 500 pts = 5 tokens
        let paid = client.redeem_for_tokens(&player, &500);
        assert_eq!(paid, 5);

        let player_token_balance = token::Client::new(&env, &token_contract.address()).balance(&player);
        assert_eq!(player_token_balance, 5);

        let info = client.get_player_rewards(&player);
        assert_eq!(info.points_balance, 500);
        assert_eq!(info.lifetime_points_redeemed, 500);
    }

    #[test]
    fn test_pause_and_unpause() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, _, client) = setup_contract(&env);
        client.initialize(&admin, &None, &None, &None);

        client.pause();

        let player = Address::generate(&env);
        let res = client.try_record_rake(&admin, &player, &100);
        assert!(res.is_err());

        client.unpause();
        let res2 = client.try_record_rake(&admin, &player, &100);
        assert!(res2.is_ok());
    }
}
