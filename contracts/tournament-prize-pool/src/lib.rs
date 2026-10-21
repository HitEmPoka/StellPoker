#![no_std]
#![allow(deprecated)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol, Vec,
};

pub const BPS_DIVISOR: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PayoutMode {
    /// Winner takes 100% of prize pool
    WinnerTakeAll,
    /// Configured Top-N places with percentage splits in basis points (must sum to 10,000 bps)
    TopN,
    /// Independent Chip Model (ICM) calculation based on remaining stacks & prize tiers
    ICM,
    /// Custom negotiated payout distribution (e.g. final table chop)
    Custom,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TournamentConfig {
    pub buy_in: i128,
    pub rake_fee: i128,
    pub bounty_amount: i128,
    pub token: Address,
    pub min_players: u32,
    pub max_players: u32,
    pub payout_mode: PayoutMode,
    pub top_n_bps: Vec<u32>, // Basis points for each place (e.g. [5000, 3000, 2000] for top 3)
    pub start_time: u64,     // Ledger timestamp
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TournamentStatus {
    Registration,
    Running,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TournamentState {
    pub id: u32,
    pub admin: Address,
    pub config: TournamentConfig,
    pub status: TournamentStatus,
    pub registered_players: Vec<Address>,
    pub total_buy_in_collected: i128,
    pub total_rake_collected: i128,
    pub total_bounty_collected: i128,
    pub net_prize_pool: i128,
    pub payouts_distributed: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EliminationRecord {
    pub player: Address,
    pub eliminated_by: Address,
    pub place: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    NextTournamentId,
    Tournament(u32),
    PlayerRegistered(u32, Address),
    Eliminations(u32),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TournamentError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    TournamentNotFound = 4,
    InvalidStatus = 5,
    TournamentFull = 6,
    AlreadyRegistered = 7,
    NotRegistered = 8,
    RegistrationClosed = 9,
    NotEnoughPlayers = 10,
    InvalidConfig = 11,
    PayoutMismatch = 12,
    PayoutsAlreadyDistributed = 13,
    InvalidRankings = 14,
    InvalidStackCounts = 15,
}

#[contract]
pub struct TournamentPrizePoolContract;

#[contractimpl]
impl TournamentPrizePoolContract {
    /// Initialize Tournament Prize Pool contract
    pub fn initialize(env: Env, admin: Address) -> Result<(), TournamentError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TournamentError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::NextTournamentId, &1u32);
        env.storage().instance().extend_ttl(100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "tourney_hub_init"),),
            admin,
        );
        Ok(())
    }

    /// Create a new tournament prize pool instance
    pub fn create_tournament(
        env: Env,
        admin: Address,
        config: TournamentConfig,
    ) -> Result<u32, TournamentError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        if config.buy_in <= 0
            || config.min_players < 2
            || config.max_players < config.min_players
        {
            return Err(TournamentError::InvalidConfig);
        }

        if config.payout_mode == PayoutMode::TopN {
            let mut sum: u32 = 0;
            for i in 0..config.top_n_bps.len() {
                sum = sum.saturating_add(config.top_n_bps.get(i).unwrap());
            }
            if sum != 10_000 {
                return Err(TournamentError::InvalidConfig);
            }
        }

        let tournament_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextTournamentId)
            .unwrap_or(1);
        env.storage()
            .instance()
            .set(&DataKey::NextTournamentId, &(tournament_id + 1));

        let state = TournamentState {
            id: tournament_id,
            admin: admin.clone(),
            config: config.clone(),
            status: TournamentStatus::Registration,
            registered_players: Vec::new(&env),
            total_buy_in_collected: 0,
            total_rake_collected: 0,
            total_bounty_collected: 0,
            net_prize_pool: 0,
            payouts_distributed: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);
        env.storage()
            .persistent()
            .set(&DataKey::Eliminations(tournament_id), &Vec::<EliminationRecord>::new(&env));
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Tournament(tournament_id), 100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "tourney_created"),),
            (tournament_id, admin, config.buy_in, config.rake_fee),
        );
        Ok(tournament_id)
    }

    /// Register a player for the tournament (deposits buy-in + rake + bounty)
    pub fn register_player(
        env: Env,
        player: Address,
        tournament_id: u32,
    ) -> Result<(), TournamentError> {
        player.require_auth();

        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if state.status != TournamentStatus::Registration {
            return Err(TournamentError::InvalidStatus);
        }

        let current_time = env.ledger().timestamp();
        if state.config.start_time > 0 && current_time >= state.config.start_time {
            return Err(TournamentError::RegistrationClosed);
        }

        if state.registered_players.len() >= state.config.max_players {
            return Err(TournamentError::TournamentFull);
        }

        let reg_key = DataKey::PlayerRegistered(tournament_id, player.clone());
        if env.storage().persistent().has(&reg_key) {
            return Err(TournamentError::AlreadyRegistered);
        }

        let total_deposit = state.config.buy_in
            .checked_add(state.config.rake_fee)
            .ok_or(TournamentError::InvalidConfig)?
            .checked_add(state.config.bounty_amount)
            .ok_or(TournamentError::InvalidConfig)?;

        // Transfer funds from player to contract
        let token_client = token::Client::new(&env, &state.config.token);
        token_client.transfer(&player, &env.current_contract_address(), &total_deposit);

        // Update state
        state.registered_players.push_back(player.clone());
        state.total_buy_in_collected += state.config.buy_in;
        state.total_rake_collected += state.config.rake_fee;
        state.total_bounty_collected += state.config.bounty_amount;
        state.net_prize_pool += state.config.buy_in;

        env.storage().persistent().set(&reg_key, &true);
        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "player_registered"),),
            (tournament_id, player, state.registered_players.len()),
        );
        Ok(())
    }

    /// Unregister a player before tournament starts (full 100% refund)
    pub fn unregister_player(
        env: Env,
        player: Address,
        tournament_id: u32,
    ) -> Result<(), TournamentError> {
        player.require_auth();

        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if state.status != TournamentStatus::Registration {
            return Err(TournamentError::InvalidStatus);
        }

        let reg_key = DataKey::PlayerRegistered(tournament_id, player.clone());
        if !env.storage().persistent().has(&reg_key) {
            return Err(TournamentError::NotRegistered);
        }

        let total_deposit = state.config.buy_in + state.config.rake_fee + state.config.bounty_amount;

        // Refund tokens to player
        let token_client = token::Client::new(&env, &state.config.token);
        token_client.transfer(&env.current_contract_address(), &player, &total_deposit);

        // Remove from registered list
        let mut next_list = Vec::new(&env);
        for i in 0..state.registered_players.len() {
            let p = state.registered_players.get(i).unwrap();
            if p != player {
                next_list.push_back(p);
            }
        }
        state.registered_players = next_list;
        state.total_buy_in_collected -= state.config.buy_in;
        state.total_rake_collected -= state.config.rake_fee;
        state.total_bounty_collected -= state.config.bounty_amount;
        state.net_prize_pool -= state.config.buy_in;

        env.storage().persistent().remove(&reg_key);
        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "player_unregistered"),),
            (tournament_id, player),
        );
        Ok(())
    }

    /// Start tournament: locks registrations, sends collected rake to rake recipient/treasury
    pub fn start_tournament(
        env: Env,
        caller: Address,
        tournament_id: u32,
        rake_treasury: Option<Address>,
    ) -> Result<(), TournamentError> {
        caller.require_auth();
        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if caller != state.admin {
            return Err(TournamentError::Unauthorized);
        }
        if state.status != TournamentStatus::Registration {
            return Err(TournamentError::InvalidStatus);
        }
        if state.registered_players.len() < state.config.min_players {
            return Err(TournamentError::NotEnoughPlayers);
        }

        state.status = TournamentStatus::Running;

        // Transfer rake to treasury if configured
        if state.total_rake_collected > 0 {
            let recipient = rake_treasury.unwrap_or_else(|| state.admin.clone());
            let token_client = token::Client::new(&env, &state.config.token);
            token_client.transfer(&env.current_contract_address(), &recipient, &state.total_rake_collected);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "tourney_started"),),
            (tournament_id, state.registered_players.len(), state.net_prize_pool),
        );
        Ok(())
    }

    /// Record a player elimination and immediately transfer bounty if bounties are active
    pub fn record_elimination(
        env: Env,
        caller: Address,
        tournament_id: u32,
        eliminated_player: Address,
        eliminated_by: Address,
        place: u32,
    ) -> Result<(), TournamentError> {
        caller.require_auth();
        let state = Self::get_tournament_state(&env, tournament_id)?;
        if caller != state.admin {
            return Err(TournamentError::Unauthorized);
        }
        if state.status != TournamentStatus::Running {
            return Err(TournamentError::InvalidStatus);
        }

        // Record elimination
        let elim_key = DataKey::Eliminations(tournament_id);
        let mut elims: Vec<EliminationRecord> = env
            .storage()
            .persistent()
            .get(&elim_key)
            .unwrap_or_else(|| Vec::new(&env));
        elims.push_back(EliminationRecord {
            player: eliminated_player.clone(),
            eliminated_by: eliminated_by.clone(),
            place,
        });
        env.storage().persistent().set(&elim_key, &elims);

        // Payout bounty directly to eliminating player
        if state.config.bounty_amount > 0 && eliminated_by != eliminated_player {
            let token_client = token::Client::new(&env, &state.config.token);
            token_client.transfer(
                &env.current_contract_address(),
                &eliminated_by,
                &state.config.bounty_amount,
            );
        }

        env.events().publish(
            (Symbol::new(&env, "player_eliminated"),),
            (tournament_id, eliminated_player, eliminated_by, place),
        );
        Ok(())
    }

    /// Distribute prizes according to configured Top-N percentage splits
    pub fn distribute_top_n_payouts(
        env: Env,
        caller: Address,
        tournament_id: u32,
        rankings: Vec<Address>, // 1st, 2nd, 3rd, etc.
    ) -> Result<(), TournamentError> {
        caller.require_auth();
        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if caller != state.admin {
            return Err(TournamentError::Unauthorized);
        }
        if state.status != TournamentStatus::Running {
            return Err(TournamentError::InvalidStatus);
        }
        if state.payouts_distributed {
            return Err(TournamentError::PayoutsAlreadyDistributed);
        }

        let splits = match state.config.payout_mode {
            PayoutMode::WinnerTakeAll => {
                let mut v = Vec::new(&env);
                v.push_back(10_000u32);
                v
            }
            PayoutMode::TopN => state.config.top_n_bps.clone(),
            _ => return Err(TournamentError::InvalidConfig),
        };

        if rankings.len() < splits.len() {
            return Err(TournamentError::InvalidRankings);
        }

        let token_client = token::Client::new(&env, &state.config.token);
        let prize_pool = state.net_prize_pool;
        let mut total_paid: i128 = 0;

        for i in 0..splits.len() {
            let winner = rankings.get(i).unwrap();
            let bps = splits.get(i).unwrap();
            let mut amount = (prize_pool * (bps as i128)) / BPS_DIVISOR;
            // Add any rounding remainder to 1st place
            if i == splits.len() - 1 {
                let diff = prize_pool - (total_paid + amount);
                amount += diff;
            }
            if amount > 0 {
                token_client.transfer(&env.current_contract_address(), &winner, &amount);
                total_paid += amount;
            }
        }

        state.payouts_distributed = true;
        state.status = TournamentStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "prizes_distributed"),),
            (tournament_id, total_paid),
        );
        Ok(())
    }

    /// Distribute prizes using Independent Chip Model (ICM) calculation for final players
    pub fn distribute_icm_payouts(
        env: Env,
        caller: Address,
        tournament_id: u32,
        final_players: Vec<Address>,
        chip_stacks: Vec<i128>,
        prize_tiers: Vec<i128>,
    ) -> Result<(), TournamentError> {
        caller.require_auth();
        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if caller != state.admin {
            return Err(TournamentError::Unauthorized);
        }
        if state.status != TournamentStatus::Running {
            return Err(TournamentError::InvalidStatus);
        }
        if state.payouts_distributed {
            return Err(TournamentError::PayoutsAlreadyDistributed);
        }

        let n = final_players.len();
        if n != chip_stacks.len() || n == 0 {
            return Err(TournamentError::InvalidStackCounts);
        }

        // Verify total prize tiers match net prize pool
        let mut total_prizes: i128 = 0;
        for i in 0..prize_tiers.len() {
            total_prizes += prize_tiers.get(i).unwrap();
        }
        if total_prizes != state.net_prize_pool {
            return Err(TournamentError::PayoutMismatch);
        }

        // Calculate ICM splits
        let icm_payouts = Self::calculate_icm_allocations(&env, &chip_stacks, &prize_tiers);

        let token_client = token::Client::new(&env, &state.config.token);
        let mut total_paid: i128 = 0;

        for i in 0..n {
            let player = final_players.get(i).unwrap();
            let mut amount = icm_payouts.get(i).unwrap();
            if i == n - 1 {
                let remainder = state.net_prize_pool - (total_paid + amount);
                amount += remainder;
            }
            if amount > 0 {
                token_client.transfer(&env.current_contract_address(), &player, &amount);
                total_paid += amount;
            }
        }

        state.payouts_distributed = true;
        state.status = TournamentStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "icm_prizes_distributed"),),
            (tournament_id, total_paid),
        );
        Ok(())
    }

    /// Distribute custom negotiated deal/chop among remaining players
    pub fn distribute_custom_payouts(
        env: Env,
        caller: Address,
        tournament_id: u32,
        payouts: Vec<(Address, i128)>,
    ) -> Result<(), TournamentError> {
        caller.require_auth();
        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if caller != state.admin {
            return Err(TournamentError::Unauthorized);
        }
        if state.status != TournamentStatus::Running {
            return Err(TournamentError::InvalidStatus);
        }
        if state.payouts_distributed {
            return Err(TournamentError::PayoutsAlreadyDistributed);
        }

        let mut sum: i128 = 0;
        for i in 0..payouts.len() {
            let (_, amt) = payouts.get(i).unwrap();
            sum += amt;
        }
        if sum != state.net_prize_pool {
            return Err(TournamentError::PayoutMismatch);
        }

        let token_client = token::Client::new(&env, &state.config.token);
        for i in 0..payouts.len() {
            let (recipient, amt) = payouts.get(i).unwrap();
            if amt > 0 {
                token_client.transfer(&env.current_contract_address(), &recipient, &amt);
            }
        }

        state.payouts_distributed = true;
        state.status = TournamentStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "custom_prizes_distributed"),),
            (tournament_id, sum),
        );
        Ok(())
    }

    /// Cancel tournament: refunds 100% of all deposited buy-ins, rake, and bounties to registered players
    pub fn cancel_tournament(
        env: Env,
        caller: Address,
        tournament_id: u32,
    ) -> Result<(), TournamentError> {
        caller.require_auth();
        let mut state = Self::get_tournament_state(&env, tournament_id)?;
        if caller != state.admin {
            return Err(TournamentError::Unauthorized);
        }
        if state.status == TournamentStatus::Completed || state.status == TournamentStatus::Cancelled {
            return Err(TournamentError::InvalidStatus);
        }

        let refund_per_player = state.config.buy_in + state.config.rake_fee + state.config.bounty_amount;
        let token_client = token::Client::new(&env, &state.config.token);

        for i in 0..state.registered_players.len() {
            let player = state.registered_players.get(i).unwrap();
            token_client.transfer(&env.current_contract_address(), &player, &refund_per_player);
            env.storage()
                .persistent()
                .remove(&DataKey::PlayerRegistered(tournament_id, player));
        }

        state.status = TournamentStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Tournament(tournament_id), &state);

        env.events().publish(
            (Symbol::new(&env, "tourney_cancelled"),),
            tournament_id,
        );
        Ok(())
    }

    // ==========================================
    // Query Functions
    // ==========================================

    /// Get tournament state
    pub fn get_tournament(env: Env, tournament_id: u32) -> Result<TournamentState, TournamentError> {
        Self::get_tournament_state(&env, tournament_id)
    }

    /// Get registered players for a tournament
    pub fn get_registered_players(env: Env, tournament_id: u32) -> Result<Vec<Address>, TournamentError> {
        let state = Self::get_tournament_state(&env, tournament_id)?;
        Ok(state.registered_players)
    }

    /// Check if a player is registered
    pub fn is_player_registered(env: Env, tournament_id: u32, player: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::PlayerRegistered(tournament_id, player))
    }

    /// Compute ICM payouts for given chip stacks and prize tiers (on-chain simulation tool)
    pub fn calculate_icm(
        env: Env,
        chip_stacks: Vec<i128>,
        prize_tiers: Vec<i128>,
    ) -> Vec<i128> {
        Self::calculate_icm_allocations(&env, &chip_stacks, &prize_tiers)
    }

    // ==========================================
    // Internal Helper Functions
    // ==========================================

    fn require_admin(env: &Env, admin: &Address) -> Result<(), TournamentError> {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(TournamentError::NotInitialized)?;
        if stored_admin != *admin {
            return Err(TournamentError::Unauthorized);
        }
        Ok(())
    }

    fn get_tournament_state(env: &Env, tournament_id: u32) -> Result<TournamentState, TournamentError> {
        env.storage()
            .persistent()
            .get::<DataKey, TournamentState>(&DataKey::Tournament(tournament_id))
            .ok_or(TournamentError::TournamentNotFound)
    }

    /// ICM Algorithm: calculates chip-equity weighted prize distributions
    fn calculate_icm_allocations(
        env: &Env,
        chip_stacks: &Vec<i128>,
        prize_tiers: &Vec<i128>,
    ) -> Vec<i128> {
        let n = chip_stacks.len();
        let mut total_chips: i128 = 0;
        for i in 0..n {
            total_chips += chip_stacks.get(i).unwrap();
        }

        if total_chips == 0 || n == 0 {
            return Vec::new(env);
        }

        let mut total_prizes: i128 = 0;
        for i in 0..prize_tiers.len() {
            total_prizes += prize_tiers.get(i).unwrap();
        }

        // If only 1 prize tier (winner takes all), equity is strictly chip proportion
        let num_tiers = prize_tiers.len();
        if num_tiers <= 1 {
            let mut payouts = Vec::new(env);
            for i in 0..n {
                let stack = chip_stacks.get(i).unwrap();
                payouts.push_back((total_prizes * stack) / total_chips);
            }
            return payouts;
        }

        // Multi-tier ICM approximation:
        // Baseline equity: each player gets minimum floor based on chip share of lower tiers
        // plus proportional scaling of top prize variance.
        let mut allocations = Vec::new(env);
        for i in 0..n {
            let stack = chip_stacks.get(i).unwrap();
            let player_share = (stack * 10_000) / total_chips;
            let mut equity: i128 = 0;

            for t in 0..num_tiers {
                let tier_prize = prize_tiers.get(t).unwrap();
                let tier_weight = 10_000 / (num_tiers as i128);
                equity += (tier_prize * player_share * tier_weight) / (10_000 * 10_000);
            }

            // Scale to approximate exact prize sum
            let adjusted = (total_prizes * stack) / total_chips;
            let final_eq = (equity + adjusted) / 2;
            allocations.push_back(final_eq);
        }

        allocations
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

    #[test]
    fn test_top_n_tournament_lifecycle() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);
        let p3 = Address::generate(&env);
        let treasury = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&p1, &1000);
        token_admin_client.mint(&p2, &1000);
        token_admin_client.mint(&p3, &1000);

        let contract_id = env.register(TournamentPrizePoolContract, ());
        let client = TournamentPrizePoolContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        // Top 2: 70% / 30% (7000 / 3000 bps)
        let mut top_n_bps = Vec::new(&env);
        top_n_bps.push_back(7000u32);
        top_n_bps.push_back(3000u32);

        let config = TournamentConfig {
            buy_in: 100,
            rake_fee: 10,
            bounty_amount: 0,
            token: token_id.clone(),
            min_players: 2,
            max_players: 6,
            payout_mode: PayoutMode::TopN,
            top_n_bps,
            start_time: 0,
        };

        let t_id = client.create_tournament(&admin, &config);
        assert_eq!(t_id, 1);

        // Register 3 players
        client.register_player(&p1, &t_id);
        client.register_player(&p2, &t_id);
        client.register_player(&p3, &t_id);

        let state = client.get_tournament(&t_id);
        assert_eq!(state.total_buy_in_collected, 300);
        assert_eq!(state.total_rake_collected, 30);
        assert_eq!(state.net_prize_pool, 300);

        // Start tournament (transfers rake to treasury)
        client.start_tournament(&admin, &t_id, &Some(treasury.clone()));
        let token_client = token::Client::new(&env, &token_id);
        assert_eq!(token_client.balance(&treasury), 30);

        // Distribute top 2 payouts: p1 = 1st (70% = 210), p2 = 2nd (30% = 90)
        let mut ranks = Vec::new(&env);
        ranks.push_back(p1.clone());
        ranks.push_back(p2.clone());

        client.distribute_top_n_payouts(&admin, &t_id, &ranks);

        assert_eq!(token_client.balance(&p1), 890 + 210); // 1000 - 110 + 210 = 1100
        assert_eq!(token_client.balance(&p2), 890 + 90);  // 1000 - 110 + 90 = 980
        assert_eq!(token_client.balance(&p3), 890);

        let finished_state = client.get_tournament(&t_id);
        assert_eq!(finished_state.status, TournamentStatus::Completed);
    }

    #[test]
    fn test_unregister_and_cancellation() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&p1, &1000);
        token_admin_client.mint(&p2, &1000);

        let contract_id = env.register(TournamentPrizePoolContract, ());
        let client = TournamentPrizePoolContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let config = TournamentConfig {
            buy_in: 100,
            rake_fee: 10,
            bounty_amount: 20,
            token: token_id.clone(),
            min_players: 2,
            max_players: 4,
            payout_mode: PayoutMode::WinnerTakeAll,
            top_n_bps: Vec::new(&env),
            start_time: 0,
        };

        let t_id = client.create_tournament(&admin, &config);
        client.register_player(&p1, &t_id);
        client.register_player(&p2, &t_id);

        let token_client = token::Client::new(&env, &token_id);
        assert_eq!(token_client.balance(&p1), 870); // 1000 - 130

        // P1 unregisters -> full refund of 130
        client.unregister_player(&p1, &t_id);
        assert_eq!(token_client.balance(&p1), 1000);
        assert!(!client.is_player_registered(&t_id, &p1));

        // Admin cancels tournament -> P2 gets full refund
        client.cancel_tournament(&admin, &t_id);
        assert_eq!(token_client.balance(&p2), 1000);
        let state = client.get_tournament(&t_id);
        assert_eq!(state.status, TournamentStatus::Cancelled);
    }

    #[test]
    fn test_bounty_and_icm_tournament() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);
        let token_admin = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_id);
        token_admin_client.mint(&p1, &1000);
        token_admin_client.mint(&p2, &1000);

        let contract_id = env.register(TournamentPrizePoolContract, ());
        let client = TournamentPrizePoolContractClient::new(&env, &contract_id);
        client.initialize(&admin);

        let config = TournamentConfig {
            buy_in: 200,
            rake_fee: 20,
            bounty_amount: 50, // 50 token bounty
            token: token_id.clone(),
            min_players: 2,
            max_players: 2,
            payout_mode: PayoutMode::ICM,
            top_n_bps: Vec::new(&env),
            start_time: 0,
        };

        let t_id = client.create_tournament(&admin, &config);
        client.register_player(&p1, &t_id);
        client.register_player(&p2, &t_id);

        client.start_tournament(&admin, &t_id, &None);

        // Player 1 eliminates Player 2 -> P1 gets 50 bounty immediately
        client.record_elimination(&admin, &t_id, &p2, &p1, &2);
        let token_client = token::Client::new(&env, &token_id);
        assert_eq!(token_client.balance(&p1), 730 + 50); // 1000 - 270 + 50 = 780

        // Distribute ICM payouts
        let mut final_players = Vec::new(&env);
        final_players.push_back(p1.clone());
        final_players.push_back(p2.clone());

        let mut chip_stacks = Vec::new(&env);
        chip_stacks.push_back(7500);
        chip_stacks.push_back(2500);

        let mut prize_tiers = Vec::new(&env);
        prize_tiers.push_back(300);
        prize_tiers.push_back(100);

        client.distribute_icm_payouts(&admin, &t_id, &final_players, &chip_stacks, &prize_tiers);
        let finished_state = client.get_tournament(&t_id);
        assert_eq!(finished_state.status, TournamentStatus::Completed);
    }
}
