use soroban_sdk::{Env, Symbol};
use crate::types::*;

/// Hand cancellation mechanism for invalid states
/// Issue #194

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationReason {
    InvalidProof,
    MpcFailure,
    PlayerDisconnect,
    Timeout,
}

/// Cancel current hand and refund all bets
pub fn cancel_hand(
    env: &Env,
    table: &mut TableState,
    reason: CancellationReason,
) -> Result<i128, PokerTableError> {
    // Only allow cancellation during active gameplay
    if table.phase == GamePhase::Settlement || table.phase == GamePhase::WaitingForPlayers {
        return Err(PokerTableError::InvalidAction);
    }

    // Refund all active bets to players
    let refunded = crate::refund_table_players(env, table)?;

    // Reset game state
    table.phase = GamePhase::Settlement;
    table.pot = 0;
    table.current_bet = 0;
    table.last_raise_amount = 0;
    
    // Clear board cards
    table.board_card_indices = soroban_sdk::Vec::new(env);

    // Emit cancellation event
    let event_name = match reason {
        CancellationReason::InvalidProof => Symbol::new(env, "hand_cancelled_invalid_proof"),
        CancellationReason::MpcFailure => Symbol::new(env, "hand_cancelled_mpc_failure"),
        CancellationReason::PlayerDisconnect => Symbol::new(env, "hand_cancelled_disconnect"),
        CancellationReason::Timeout => Symbol::new(env, "hand_cancelled_timeout"),
    };
    
    env.events().publish((event_name,), refunded);

    Ok(refunded)
}

/// Check if hand should be auto-cancelled due to invalid state
pub fn should_cancel_hand(table: &TableState, current_ledger: u32) -> bool {
    // Cancel if stuck in same phase for too long (5 minutes = ~60 ledgers)
    if current_ledger > table.last_action_ledger + 60 {
        return true;
    }

    // Cancel if too few active players mid-hand
    let active_players = table
        .players
        .iter()
        .filter(|p| !p.folded && p.stack > 0)
        .count();
    
    if table.phase != GamePhase::Settlement && active_players < 2 {
        return true;
    }

    false
}
