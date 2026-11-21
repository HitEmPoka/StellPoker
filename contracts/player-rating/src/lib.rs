#![no_std]

//! On-chain player ranking / rating system (Issue #70).
//!
//! Stores ELO ratings per player address. Each completed hand can update
//! ratings via `record_hand`. Leaderboard queries are exposed on-chain.
//! Players must reach `min_hands` before appearing on the public leaderboard
//! (anti-manipulation).

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, Symbol, Vec,
};

/// Default starting ELO rating.
const DEFAULT_RATING: u32 = 1500;
/// Default K-factor for ELO updates.
const DEFAULT_K_FACTOR: u32 = 32;
/// Default minimum hands before leaderboard eligibility.
const DEFAULT_MIN_HANDS: u32 = 10;
/// Maximum leaderboard page size.
const MAX_LEADERBOARD_LIMIT: u32 = 50;

pub const INITIAL_REPUTATION: u32 = 500;
pub const MAX_REPUTATION: u32 = 1000;
pub const MIN_REPUTATION: u32 = 0;

#[contract]
pub struct PlayerRatingContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerRating {
    pub address: Address,
    /// Current ELO rating (integer, starts at 1500).
    pub rating: u32,
    pub hands_played: u32,
    pub hands_won: u32,
    pub last_update_ledger: u32,
}

/// Opt-in privacy-preserving player reputation record (Issue #690).
/// Stores high-level attendance and reliability statistics without revealing
/// any private hand contents, cards, or betting actions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerReputation {
    pub address: Address,
    pub opted_in: bool,
    /// Composite reputation score (0 - 1000, starts at 500).
    pub score: u32,
    /// Total hands attended and completed cleanly without timing out.
    pub hands_completed: u32,
    /// Total hands won.
    pub hands_won: u32,
    /// Total timeouts / stalling incidents recorded.
    pub timeouts: u32,
    /// Consecutive clean hands without timeouts.
    pub clean_streak: u32,
    pub last_update_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingConfig {
    pub min_hands: u32,
    pub k_factor: u32,
    pub initial_rating: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Config,
    Player(Address),
    /// Sorted leaderboard addresses (eligible players only), highest rating first.
    Leaderboard,
    /// Addresses authorized to call `record_hand` (e.g. poker-table / coordinator).
    Recorder(Address),
    Paused,
    Reputation(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RatingError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    Paused = 4,
    EmptyResults = 5,
    InvalidConfig = 6,
    PlayerNotFound = 7,
    LimitTooLarge = 8,
    NotOptedIn = 9,
}

#[contractevent]
pub struct ReputationOptInToggled {
    pub player: Address,
    pub opted_in: bool,
}

#[contractevent]
pub struct ReputationUpdated {
    pub player: Address,
    pub new_score: u32,
    pub hands_completed: u32,
    pub timeouts: u32,
}

#[contractimpl]
impl PlayerRatingContract {
    /// Initialize the rating contract.
    pub fn initialize(
        env: Env,
        admin: Address,
        min_hands: u32,
        k_factor: u32,
        initial_rating: u32,
    ) -> Result<(), RatingError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RatingError::AlreadyInitialized);
        }
        if k_factor == 0 || initial_rating == 0 {
            return Err(RatingError::InvalidConfig);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(
            &DataKey::Config,
            &RatingConfig {
                min_hands,
                k_factor,
                initial_rating,
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::Leaderboard, &Vec::<Address>::new(&env));
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().extend_ttl(100_000, 100_000);

        env.events().publish(
            (Symbol::new(&env, "rating_initialized"),),
            (admin, min_hands, k_factor, initial_rating),
        );
        Ok(())
    }

    /// Admin grants an address permission to record match results.
    pub fn add_recorder(env: Env, admin: Address, recorder: Address) -> Result<(), RatingError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Recorder(recorder.clone()), &true);
        env.events()
            .publish((Symbol::new(&env, "recorder_added"),), recorder);
        Ok(())
    }

    /// Admin revokes recorder permission.
    pub fn remove_recorder(env: Env, admin: Address, recorder: Address) -> Result<(), RatingError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .remove(&DataKey::Recorder(recorder.clone()));
        env.events()
            .publish((Symbol::new(&env, "recorder_removed"),), recorder);
        Ok(())
    }

    /// Update config (min hands, K-factor). Does not reset existing ratings.
    pub fn set_config(
        env: Env,
        admin: Address,
        min_hands: u32,
        k_factor: u32,
    ) -> Result<(), RatingError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        if k_factor == 0 {
            return Err(RatingError::InvalidConfig);
        }
        let mut cfg = Self::config(&env)?;
        cfg.min_hands = min_hands;
        cfg.k_factor = k_factor;
        env.storage().instance().set(&DataKey::Config, &cfg);
        env.events().publish(
            (Symbol::new(&env, "rating_config_updated"),),
            (min_hands, k_factor),
        );
        Ok(())
    }

    /// Pause.
    ///
    /// # Parameters
    /// - `admin`: parameter
    ///
    /// # Returns
    /// - `Result<(), RatingError>`
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    ///
    /// # Authorization
    /// Requires appropriate authorization.
    pub fn pause(env: Env, admin: Address) -> Result<(), RatingError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    /// Unpause.
    ///
    /// # Parameters
    /// - `admin`: parameter
    ///
    /// # Returns
    /// - `Result<(), RatingError>`
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    ///
    /// # Authorization
    /// Requires appropriate authorization.
    pub fn unpause(env: Env, admin: Address) -> Result<(), RatingError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Record a completed hand result and update ELO for all participants.
    ///
    /// `participants` are all players in the hand. `winner_index` is the index
    /// of the winner within `participants` (draws: pass the sole winner; multi-way
    /// pots should call once per distinct side-pot winner with only involved players).
    ///
    /// Auth: admin or an authorized recorder.
    pub fn record_hand(
        env: Env,
        caller: Address,
        participants: Vec<Address>,
        winner_index: u32,
    ) -> Result<(), RatingError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::require_recorder_or_admin(&env, &caller)?;

        let n = participants.len();
        if n < 2 || winner_index >= n {
            return Err(RatingError::EmptyResults);
        }

        let cfg = Self::config(&env)?;
        let ledger = env.ledger().sequence();

        // Average opponent rating for multi-player ELO (winner vs field, field vs winner).
        let mut ratings: Vec<u32> = Vec::new(&env);
        let mut hands: Vec<u32> = Vec::new(&env);
        let mut wins: Vec<u32> = Vec::new(&env);

        for i in 0..n {
            let addr = participants.get(i).unwrap();
            let pr = Self::get_or_create(&env, &addr, &cfg, ledger);
            ratings.push_back(pr.rating);
            hands.push_back(pr.hands_played);
            wins.push_back(pr.hands_won);
        }

        let winner_rating = ratings.get(winner_index).unwrap();
        let mut opp_sum: u64 = 0;
        for i in 0..n {
            if i != winner_index {
                opp_sum += ratings.get(i).unwrap() as u64;
            }
        }
        let opp_avg = (opp_sum / (n as u64 - 1)) as u32;

        // Winner gains vs average field; each loser loses vs winner.
        let winner_delta = elo_delta(winner_rating, opp_avg, true, cfg.k_factor);
        let new_winner = apply_delta(winner_rating, winner_delta);

        for i in 0..n {
            let addr = participants.get(i).unwrap();
            let old_r = ratings.get(i).unwrap();
            let hp = hands.get(i).unwrap() + 1;
            let hw = if i == winner_index {
                wins.get(i).unwrap() + 1
            } else {
                wins.get(i).unwrap()
            };

            let new_r = if i == winner_index {
                new_winner
            } else {
                let d = elo_delta(old_r, winner_rating, false, cfg.k_factor);
                apply_delta(old_r, d)
            };

            let updated = PlayerRating {
                address: addr.clone(),
                rating: new_r,
                hands_played: hp,
                hands_won: hw,
                last_update_ledger: ledger,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Player(addr.clone()), &updated);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Player(addr.clone()), 100_000, 100_000);

            Self::refresh_leaderboard_entry(&env, &updated, cfg.min_hands);
        }

        env.events().publish(
            (Symbol::new(&env, "hand_rated"),),
            (winner_index, n, new_winner),
        );
        Ok(())
    }

    /// Get a player's rating record (creates default view if never played).
    pub fn get_rating(env: Env, player: Address) -> Result<PlayerRating, RatingError> {
        Self::require_initialized(&env)?;
        let cfg = Self::config(&env)?;
        if let Some(pr) = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerRating>(&DataKey::Player(player.clone()))
        {
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Player(player), 100_000, 100_000);
            return Ok(pr);
        }
        Ok(PlayerRating {
            address: player,
            rating: cfg.initial_rating,
            hands_played: 0,
            hands_won: 0,
            last_update_ledger: 0,
        })
    }

    /// Leaderboard page: eligible players only (hands_played >= min_hands).
    /// Ordered by rating descending.
    pub fn get_leaderboard(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<PlayerRating>, RatingError> {
        Self::require_initialized(&env)?;
        if limit == 0 || limit > MAX_LEADERBOARD_LIMIT {
            return Err(RatingError::LimitTooLarge);
        }

        let board: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Leaderboard)
            .unwrap_or_else(|| Vec::new(&env));

        let mut out: Vec<PlayerRating> = Vec::new(&env);
        let end = core::cmp::min(offset.saturating_add(limit), board.len());
        let mut i = offset;
        while i < end {
            let addr = board.get(i).unwrap();
            if let Some(pr) = env
                .storage()
                .persistent()
                .get::<DataKey, PlayerRating>(&DataKey::Player(addr))
            {
                out.push_back(pr);
            }
            i += 1;
        }
        Ok(out)
    }

    /// Get config.
    ///
    /// # Parameters
    ///
    /// # Returns
    /// - `Result<RatingConfig, RatingError>`
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    ///
    /// # Authorization
    /// Requires appropriate authorization.
    pub fn get_config(env: Env) -> Result<RatingConfig, RatingError> {
        Self::config(&env)
    }

    /// Leaderboard size.
    ///
    /// # Parameters
    ///
    /// # Returns
    /// - `Result<u32, RatingError>`
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    ///
    /// # Authorization
    /// Requires appropriate authorization.
    pub fn leaderboard_size(env: Env) -> Result<u32, RatingError> {
        Self::require_initialized(&env)?;
        let board: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Leaderboard)
            .unwrap_or_else(|| Vec::new(&env));
        Ok(board.len())
    }

    // ========================================================================
    // Player Reputation (Issue #690: Opt-in, Privacy-Preserving)
    // ========================================================================

    /// Opt in to public reputation scoring (Issue #690).
    pub fn opt_in_reputation(env: Env, player: Address) -> Result<(), RatingError> {
        player.require_auth();
        Self::require_initialized(&env)?;
        let ledger = env.ledger().sequence();
        let mut rep = env
            .storage()
            .persistent()
            .get(&DataKey::Reputation(player.clone()))
            .unwrap_or_else(|| PlayerReputation {
                address: player.clone(),
                opted_in: true,
                score: INITIAL_REPUTATION,
                hands_completed: 0,
                hands_won: 0,
                timeouts: 0,
                clean_streak: 0,
                last_update_ledger: ledger,
            });

        rep.opted_in = true;
        rep.last_update_ledger = ledger;

        env.storage()
            .persistent()
            .set(&DataKey::Reputation(player.clone()), &rep);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Reputation(player.clone()), 100_000, 100_000);

        ReputationOptInToggled {
            player,
            opted_in: true,
        }
        .publish(&env);
        Ok(())
    }

    /// Opt out of reputation scoring. Hides reputation record from public view.
    pub fn opt_out_reputation(env: Env, player: Address) -> Result<(), RatingError> {
        player.require_auth();
        Self::require_initialized(&env)?;
        let ledger = env.ledger().sequence();
        if let Some(mut rep) = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerReputation>(&DataKey::Reputation(player.clone()))
        {
            rep.opted_in = false;
            rep.last_update_ledger = ledger;
            env.storage()
                .persistent()
                .set(&DataKey::Reputation(player.clone()), &rep);
        }

        ReputationOptInToggled {
            player,
            opted_in: false,
        }
        .publish(&env);
        Ok(())
    }

    /// Check if a player has opted into reputation tracking.
    pub fn is_opted_in(env: Env, player: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, PlayerReputation>(&DataKey::Reputation(player))
            .map(|r| r.opted_in)
            .unwrap_or(false)
    }

    /// Read a player's reputation score. Returns Err(NotOptedIn) if not opted in.
    pub fn get_reputation(env: Env, player: Address) -> Result<PlayerReputation, RatingError> {
        Self::require_initialized(&env)?;
        let rep = env
            .storage()
            .persistent()
            .get::<DataKey, PlayerReputation>(&DataKey::Reputation(player.clone()))
            .ok_or(RatingError::NotOptedIn)?;

        if !rep.opted_in {
            return Err(RatingError::NotOptedIn);
        }

        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Reputation(player), 100_000, 100_000);
        Ok(rep)
    }

    /// Record hand completion results for reputation (wins, attendance, timeouts).
    ///
    /// Auth: authorized recorder (poker-table / coordinator) or admin.
    /// Preserves privacy: strictly receives table outcome indexes, zero hand content.
    pub fn record_hand_reputation(
        env: Env,
        caller: Address,
        participants: Vec<Address>,
        winner_index: u32,
        has_timeout: bool,
        timed_out_index: u32,
    ) -> Result<(), RatingError> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        Self::require_recorder_or_admin(&env, &caller)?;

        let n = participants.len();
        if n < 2 || winner_index >= n {
            return Err(RatingError::EmptyResults);
        }

        let ledger = env.ledger().sequence();

        for i in 0..n {
            let addr = participants.get(i).unwrap();
            if let Some(mut rep) = env
                .storage()
                .persistent()
                .get::<DataKey, PlayerReputation>(&DataKey::Reputation(addr.clone()))
            {
                if !rep.opted_in {
                    continue;
                }

                let is_timeout = has_timeout && (timed_out_index == i);
                let is_winner = i == winner_index;

                if is_timeout {
                    rep.timeouts = rep.timeouts.saturating_add(1);
                    rep.clean_streak = 0;
                } else {
                    rep.hands_completed = rep.hands_completed.saturating_add(1);
                    rep.clean_streak = rep.clean_streak.saturating_add(1);
                }

                if is_winner && !is_timeout {
                    rep.hands_won = rep.hands_won.saturating_add(1);
                }

                rep.score = calculate_reputation(
                    rep.hands_completed,
                    rep.hands_won,
                    rep.timeouts,
                    rep.clean_streak,
                );
                rep.last_update_ledger = ledger;

                env.storage()
                    .persistent()
                    .set(&DataKey::Reputation(addr.clone()), &rep);
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Reputation(addr.clone()), 100_000, 100_000);

                ReputationUpdated {
                    player: addr,
                    new_score: rep.score,
                    hands_completed: rep.hands_completed,
                    timeouts: rep.timeouts,
                }
                .publish(&env);
            }
        }

        Ok(())
    }

    // ── internals ────────────────────────────────────────────────────────────

    fn require_initialized(env: &Env) -> Result<(), RatingError> {
        if env.storage().instance().has(&DataKey::Admin) {
            Ok(())
        } else {
            Err(RatingError::NotInitialized)
        }
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), RatingError> {
        Self::require_initialized(env)?;
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != *admin {
            return Err(RatingError::Unauthorized);
        }
        Ok(())
    }

    fn require_recorder_or_admin(env: &Env, caller: &Address) -> Result<(), RatingError> {
        Self::require_initialized(env)?;
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if *caller == admin {
            return Ok(());
        }
        let is_recorder: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Recorder(caller.clone()))
            .unwrap_or(false);
        if is_recorder {
            Ok(())
        } else {
            Err(RatingError::Unauthorized)
        }
    }

    fn require_not_paused(env: &Env) -> Result<(), RatingError> {
        Self::require_initialized(env)?;
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            Err(RatingError::Paused)
        } else {
            Ok(())
        }
    }

    fn config(env: &Env) -> Result<RatingConfig, RatingError> {
        Self::require_initialized(env)?;
        Ok(env
            .storage()
            .instance()
            .get(&DataKey::Config)
            .unwrap_or(RatingConfig {
                min_hands: DEFAULT_MIN_HANDS,
                k_factor: DEFAULT_K_FACTOR,
                initial_rating: DEFAULT_RATING,
            }))
    }

    fn get_or_create(env: &Env, addr: &Address, cfg: &RatingConfig, ledger: u32) -> PlayerRating {
        env.storage()
            .persistent()
            .get(&DataKey::Player(addr.clone()))
            .unwrap_or_else(|| PlayerRating {
                address: addr.clone(),
                rating: cfg.initial_rating,
                hands_played: 0,
                hands_won: 0,
                last_update_ledger: ledger,
            })
    }

    /// Insert or re-sort player into the leaderboard if eligible.
    fn refresh_leaderboard_entry(env: &Env, pr: &PlayerRating, min_hands: u32) {
        let mut board: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Leaderboard)
            .unwrap_or_else(|| Vec::new(env));

        // Remove existing entry.
        let mut next: Vec<Address> = Vec::new(env);
        for i in 0..board.len() {
            let a = board.get(i).unwrap();
            if a != pr.address {
                next.push_back(a);
            }
        }
        board = next;

        if pr.hands_played < min_hands {
            env.storage()
                .persistent()
                .set(&DataKey::Leaderboard, &board);
            return;
        }

        // Insert by rating descending.
        let mut inserted = false;
        let mut out: Vec<Address> = Vec::new(env);
        for i in 0..board.len() {
            let a = board.get(i).unwrap();
            if !inserted {
                let other: PlayerRating = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Player(a.clone()))
                    .unwrap_or(PlayerRating {
                        address: a.clone(),
                        rating: 0,
                        hands_played: 0,
                        hands_won: 0,
                        last_update_ledger: 0,
                    });
                if pr.rating > other.rating {
                    out.push_back(pr.address.clone());
                    inserted = true;
                }
            }
            out.push_back(a);
        }
        if !inserted {
            out.push_back(pr.address.clone());
        }

        env.storage().persistent().set(&DataKey::Leaderboard, &out);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Leaderboard, 100_000, 100_000);
    }
}

/// Integer ELO delta. `won` = true if `player` beat `opponent`.
/// Uses a piecewise expected-score approximation (no floats on Soroban).
fn elo_delta(player: u32, opponent: u32, won: bool, k: u32) -> i32 {
    // expected score in basis points (0..10000) via logistic approx on rating diff.
    let diff = opponent as i64 - player as i64;
    // Clamp diff for stability.
    let diff = diff.clamp(-800, 800);
    // expected ≈ 1 / (1 + 10^(diff/400))
    // Approximate with linear segments in 1/10000 units.
    let expected_bp: i64 = if diff <= -400 {
        9000
    } else if diff <= -200 {
        7500
    } else if diff <= -100 {
        6400
    } else if diff <= 0 {
        5000 + (-diff) * 14 / 10 // 5000..6400
    } else if diff <= 100 {
        5000 - diff * 14 / 10
    } else if diff <= 200 {
        3600
    } else if diff <= 400 {
        2500
    } else {
        1000
    };

    let score_bp: i64 = if won { 10_000 } else { 0 };
    let delta = (k as i64) * (score_bp - expected_bp) / 10_000;
    delta as i32
}

fn apply_delta(rating: u32, delta: i32) -> u32 {
    let next = rating as i64 + delta as i64;
    // Floor at 100 to avoid underflow / zero ratings.
    if next < 100 {
        100
    } else if next > 4000 {
        4000
    } else {
        next as u32
    }
}

/// Scaled composite reputation calculator (Issue #690).
/// Strictly uses high-level results: attendance/completion, fair wins, and penalties for timeouts.
pub fn calculate_reputation(
    hands_completed: u32,
    hands_won: u32,
    timeouts: u32,
    clean_streak: u32,
) -> u32 {
    let mut score = INITIAL_REPUTATION as i32;

    // Attendance & completion: +2 points per hand completed (capped at +300)
    let completion_bonus = core::cmp::min(hands_completed * 2, 300) as i32;
    score += completion_bonus;

    // Fair victory bonus: +3 points per win (capped at +150)
    let win_bonus = core::cmp::min(hands_won * 3, 150) as i32;
    score += win_bonus;

    // Clean streak reliability bonus: +1 point per 5 consecutive hands without timeout (capped at +50)
    let streak_bonus = core::cmp::min(clean_streak / 5, 50) as i32;
    score += streak_bonus;

    // Timeout penalty: -50 points per timeout/stalling incident
    let timeout_penalty = (timeouts * 50) as i32;
    score -= timeout_penalty;

    if score < MIN_REPUTATION as i32 {
        MIN_REPUTATION
    } else if score > MAX_REPUTATION as i32 {
        MAX_REPUTATION
    } else {
        score as u32
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env, Vec};

    fn setup() -> (Env, Address, PlayerRatingContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(PlayerRatingContract, ());
        let client = PlayerRatingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &10, &32, &1500);
        (env, admin, client)
    }

    #[test]
    fn initialize_and_default_rating() {
        let (env, _admin, client) = setup();
        let p = Address::generate(&env);
        let r = client.get_rating(&p);
        assert_eq!(r.rating, 1500);
        assert_eq!(r.hands_played, 0);
    }

    #[test]
    fn record_hand_updates_elo_and_counts() {
        let (env, admin, client) = setup();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut parts = Vec::new(&env);
        parts.push_back(a.clone());
        parts.push_back(b.clone());

        client.record_hand(&admin, &parts, &0);

        let ra = client.get_rating(&a);
        let rb = client.get_rating(&b);
        assert_eq!(ra.hands_played, 1);
        assert_eq!(ra.hands_won, 1);
        assert_eq!(rb.hands_played, 1);
        assert_eq!(rb.hands_won, 0);
        assert!(ra.rating > 1500);
        assert!(rb.rating < 1500);
    }

    #[test]
    fn min_hands_gates_leaderboard() {
        let (env, admin, client) = setup();
        let a = Address::generate(&env);
        let b = Address::generate(&env);

        // Play 9 hands — still under min_hands=10.
        for _ in 0..9 {
            let mut parts = Vec::new(&env);
            parts.push_back(a.clone());
            parts.push_back(b.clone());
            client.record_hand(&admin, &parts, &0);
        }
        assert_eq!(client.leaderboard_size(), 0);

        // 10th hand — both become eligible.
        let mut parts = Vec::new(&env);
        parts.push_back(a.clone());
        parts.push_back(b.clone());
        client.record_hand(&admin, &parts, &0);
        assert_eq!(client.leaderboard_size(), 2);

        let board = client.get_leaderboard(&0, &10);
        assert_eq!(board.len(), 2);
        // Winner a should rank first.
        assert_eq!(board.get(0).unwrap().address, a);
        assert!(board.get(0).unwrap().rating >= board.get(1).unwrap().rating);
    }

    #[test]
    fn unauthorized_recorder_rejected() {
        let (env, _admin, client) = setup();
        let stranger = Address::generate(&env);
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut parts = Vec::new(&env);
        parts.push_back(a);
        parts.push_back(b);
        let result = client.try_record_hand(&stranger, &parts, &0);
        assert!(result.is_err());
    }

    #[test]
    fn authorized_recorder_can_update() {
        let (env, admin, client) = setup();
        let recorder = Address::generate(&env);
        client.add_recorder(&admin, &recorder);
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut parts = Vec::new(&env);
        parts.push_back(a.clone());
        parts.push_back(b);
        client.record_hand(&recorder, &parts, &0);
        assert_eq!(client.get_rating(&a).hands_played, 1);
    }

    #[test]
    fn set_config_and_pause() {
        let (env, admin, client) = setup();
        client.set_config(&admin, &5, &24);
        let cfg = client.get_config();
        assert_eq!(cfg.min_hands, 5);
        assert_eq!(cfg.k_factor, 24);

        client.pause(&admin);
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let mut parts = Vec::new(&env);
        parts.push_back(a);
        parts.push_back(b);
        assert!(client.try_record_hand(&admin, &parts, &0).is_err());

        client.unpause(&admin);
        assert!(client.try_record_hand(&admin, &parts, &0).is_ok());
    }

    #[test]
    fn test_reputation_opt_in_out_and_privacy() {
        let (env, _admin, client) = setup();
        let player = Address::generate(&env);

        // Initially not opted in
        assert!(!client.is_opted_in(&player));
        assert_eq!(client.try_get_reputation(&player), Err(Ok(RatingError::NotOptedIn)));

        // Opt in
        client.opt_in_reputation(&player);
        assert!(client.is_opted_in(&player));

        let rep = client.get_reputation(&player);
        assert_eq!(rep.score, INITIAL_REPUTATION);
        assert_eq!(rep.hands_completed, 0);
        assert_eq!(rep.hands_won, 0);
        assert_eq!(rep.timeouts, 0);
        assert!(rep.opted_in);

        // Opt out -> privacy restored, querying reputation returns Err(NotOptedIn)
        client.opt_out_reputation(&player);
        assert!(!client.is_opted_in(&player));
        assert_eq!(client.try_get_reputation(&player), Err(Ok(RatingError::NotOptedIn)));
    }

    #[test]
    fn test_reputation_attendance_wins_and_penalties() {
        let (env, admin, client) = setup();
        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);
        let p3_non_opted = Address::generate(&env);

        client.opt_in_reputation(&p1);
        client.opt_in_reputation(&p2);

        let mut parts = Vec::new(&env);
        parts.push_back(p1.clone());
        parts.push_back(p2.clone());
        parts.push_back(p3_non_opted.clone());

        // Hand 1: p1 wins cleanly (no timeout)
        client.record_hand_reputation(&admin, &parts, &0, &false, &0);

        let r1 = client.get_reputation(&p1);
        // p1: 500 (base) + 2 (completion) + 3 (win) = 505
        assert_eq!(r1.hands_completed, 1);
        assert_eq!(r1.hands_won, 1);
        assert_eq!(r1.clean_streak, 1);
        assert_eq!(r1.score, 505);

        let r2 = client.get_reputation(&p2);
        // p2: 500 (base) + 2 (completion) + 0 (win) = 502
        assert_eq!(r2.hands_completed, 1);
        assert_eq!(r2.hands_won, 0);
        assert_eq!(r2.clean_streak, 1);
        assert_eq!(r2.score, 502);

        // p3 was not opted in, still not opted in
        assert!(!client.is_opted_in(&p3_non_opted));

        // Hand 2: p2 causes a timeout, p1 wins
        client.record_hand_reputation(&admin, &parts, &0, &true, &1);

        let r1_after = client.get_reputation(&p1);
        // p1: 500 + 4 (2 completions) + 6 (2 wins) = 510
        assert_eq!(r1_after.hands_completed, 2);
        assert_eq!(r1_after.hands_won, 2);
        assert_eq!(r1_after.clean_streak, 2);
        assert_eq!(r1_after.score, 510);

        let r2_after = client.get_reputation(&p2);
        // p2: 500 + 2 (1 completion) - 50 (1 timeout) = 452
        assert_eq!(r2_after.hands_completed, 1); // did not increment on timeout
        assert_eq!(r2_after.timeouts, 1);
        assert_eq!(r2_after.clean_streak, 0); // streak reset
        assert_eq!(r2_after.score, 452);
    }

    #[test]
    fn test_calculate_reputation_clamping() {
        // Test lower bound clamp at 0
        assert_eq!(calculate_reputation(0, 0, 20, 0), MIN_REPUTATION);

        // Test upper bound clamp at 1000
        assert_eq!(calculate_reputation(1000, 500, 0, 500), MAX_REPUTATION);

        // Test base
        assert_eq!(calculate_reputation(0, 0, 0, 0), INITIAL_REPUTATION);
    }
}
