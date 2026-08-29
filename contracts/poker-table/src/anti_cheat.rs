//! Anti-cheat chip dumping detection module.
//!
//! Detects suspicious patterns indicating chip dumping:
//! - Repeated small losses from one player to another
//! - Abnormal fold rates against specific opponents
//! - Short-stack targeting behavior

use soroban_sdk::{Address, Env, Vec};

/// Threshold for repeated losses to trigger flagging (number of hands)
const REPEATED_LOSS_THRESHOLD: u32 = 5;
/// Minimum fold rate against an opponent to be considered suspicious (percentage)
const ABNORMAL_FOLD_RATE_THRESHOLD: u32 = 80;
/// Number of hands to track for pattern detection
const TRACKING_WINDOW: u32 = 20;

/// Pattern detection data for a player pair
#[derive(Clone, Debug)]
pub struct PlayerInteractionStats {
    /// Number of hands where player A lost to player B
    pub losses_to_opponent: u32,
    /// Number of hands where player A folded when facing player B
    pub folds_against_opponent: u32,
    /// Total hands where player A and player B were both active
    pub total_interactions: u32,
    /// Average amount lost per hand to opponent
    pub avg_loss_amount: i128,
}

impl PlayerInteractionStats {
    pub fn new() -> Self {
        Self {
            losses_to_opponent: 0,
            folds_against_opponent: 0,
            total_interactions: 0,
            avg_loss_amount: 0,
        }
    }

    /// Calculate fold rate as percentage
    pub fn fold_rate(&self) -> u32 {
        if self.total_interactions == 0 {
            return 0;
        }
        (self.folds_against_opponent * 100) / self.total_interactions
    }

    /// Check if pattern indicates chip dumping
    pub fn is_suspicious(&self) -> bool {
        // Check for repeated losses
        if self.losses_to_opponent >= REPEATED_LOSS_THRESHOLD {
            return true;
        }

        // Check for abnormally high fold rate
        if self.fold_rate() >= ABNORMAL_FOLD_RATE_THRESHOLD {
            return true;
        }

        // Check for consistent small losses (potential intentional dumping)
        if self.losses_to_opponent > 3
            && self.avg_loss_amount > 0
            && self.avg_loss_amount < 500
        {
            // Consistent small losses might indicate controlled dumping
            return true;
        }

        false
    }
}

/// Chip dumping detection result
#[derive(Clone, Debug)]
pub struct ChipDumpingFlag {
    pub suspected_dumper: Address,
    pub suspected_receiver: Address,
    pub reason: ChipDumpingReason,
    pub confidence: u32, // 0-100 percentage
}

#[derive(Clone, Debug)]
pub enum ChipDumpingReason {
    RepeatedLosses,
    AbnormalFoldRate,
    ShortStackTargeting,
    SuspiciousLossPattern,
}

/// Analyze player interaction history for chip dumping patterns
pub fn detect_chip_dumping(
    _env: &Env,
    player_a: &Address,
    player_b: &Address,
    stats: &PlayerInteractionStats,
) -> Option<ChipDumpingFlag> {
    if !stats.is_suspicious() {
        return None;
    }

    let mut confidence: u32 = 0;
    let mut reason = ChipDumpingReason::SuspiciousLossPattern;

    // Calculate confidence based on multiple factors
    if stats.losses_to_opponent >= REPEATED_LOSS_THRESHOLD {
        confidence += 40;
        reason = ChipDumpingReason::RepeatedLosses;
    }

    let fold_rate = stats.fold_rate();
    if fold_rate >= ABNORMAL_FOLD_RATE_THRESHOLD {
        confidence += 35;
        if confidence == 35 {
            reason = ChipDumpingReason::AbnormalFoldRate;
        }
    }

    // Small consistent losses pattern
    if stats.losses_to_opponent > 3
        && stats.avg_loss_amount > 0
        && stats.avg_loss_amount < 500
    {
        confidence += 25;
    }

    // Cap confidence at 100
    confidence = confidence.min(100);

    if confidence >= 50 {
        Some(ChipDumpingFlag {
            suspected_dumper: player_a.clone(),
            suspected_receiver: player_b.clone(),
            reason,
            confidence,
        })
    } else {
        None
    }
}

/// Track a hand outcome for chip dumping analysis
pub fn record_hand_outcome(
    stats: &mut PlayerInteractionStats,
    player_a_won: bool,
    player_a_folded: bool,
    pot_amount: i128,
) {
    stats.total_interactions += 1;

    if player_a_folded {
        stats.folds_against_opponent += 1;
    }

    if !player_a_won && !player_a_folded {
        stats.losses_to_opponent += 1;

        // Update average loss amount
        let total_losses = stats.losses_to_opponent as i128;
        if total_losses > 0 {
            let prev_total = stats.avg_loss_amount * (total_losses - 1);
            stats.avg_loss_amount = (prev_total + pot_amount) / total_losses;
        }
    }

    // Keep window size limited
    if stats.total_interactions > TRACKING_WINDOW {
        // Simple decay: reduce all counters proportionally
        let decay_factor = TRACKING_WINDOW as f64 / stats.total_interactions as f64;
        stats.losses_to_opponent =
            (stats.losses_to_opponent as f64 * decay_factor) as u32;
        stats.folds_against_opponent =
            (stats.folds_against_opponent as f64 * decay_factor) as u32;
        stats.total_interactions = TRACKING_WINDOW;
    }
}

/// Get all flagged player pairs for admin review
pub fn get_flagged_interactions(
    env: &Env,
    all_stats: &Vec<(Address, Address, PlayerInteractionStats)>,
) -> Vec<ChipDumpingFlag> {
    let mut flags: Vec<ChipDumpingFlag> = Vec::new(env);

    for i in 0..all_stats.len() {
        if let Some((player_a, player_b, stats)) = all_stats.get(i) {
            if let Some(flag) = detect_chip_dumping(env, &player_a, &player_b, &stats) {
                flags.push_back(flag);
            }
        }
    }

    flags
}

/// Short-stack targeting detection: check if a player consistently
/// targets opponents with low chip counts
pub fn detect_short_stack_targeting(
    _env: &Env,
    aggressive_player_wins_vs_short_stacks: u32,
    total_wins: u32,
) -> bool {
    if total_wins < 5 {
        return false; // Not enough data
    }

    // If more than 70% of wins are against short stacks, flag it
    let short_stack_win_rate = (aggressive_player_wins_vs_short_stacks * 100) / total_wins;
    short_stack_win_rate >= 70
}
