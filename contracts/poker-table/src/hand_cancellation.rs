use soroban_sdk::{contracttype, Env, Symbol};
use crate::types::*;

/// Hand cancellation mechanism for invalid states
/// Issue #194

#[contracttype]
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
    if table.phase == GamePhase::Settlement || table.phase == GamePhase::Waiting || table.phase == GamePhase::WaitingForPlayers {
        return Err(PokerTableError::InvalidAction);
    }

    // Refund all active bets to players
    let refunded = crate::refund_table_players(env, table)?;

    // Reset game state to Settlement; preserve pot=0 and clear board/commitments
    table.phase = GamePhase::Settlement;
    table.pot = 0;
    // Reset per-round bet tracking via existing field
    table.last_raise_size = 0;
    // Clear board cards correctly
    table.board_cards = soroban_sdk::Vec::new(env);
    table.dealt_indices = soroban_sdk::Vec::new(env);
    table.hand_commitments = soroban_sdk::Vec::new(env);
    table.side_pots = soroban_sdk::Vec::new(env);
    table.rit_state = None;

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
    let mut active = 0u32;
    for i in 0..table.players.len() {
        if let Some(p) = table.players.get(i) {
            if !p.folded && p.stack > 0 {
                active += 1;
            }
        }
    }
    
    if table.phase != GamePhase::Settlement && active < 2 {
        return true;
    }

    false
}
