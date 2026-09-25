/// On-chain dispute window for settlement challenges (Issue #199).
///
/// After a hand settles, there is a configurable dispute window during which
/// any participant can challenge the settlement by committing an evidence hash
/// (e.g., a hash of offchain replay data). If the challenge is not resolved
/// before the window expires, the original settlement is finalized.
///
/// State machine:
///   Settled → Challenged → Resolved
///   Settled → (timeout) → Finalized
///   Challenged → (timeout) → Finalized (original settlement stands)

use soroban_sdk::{Address, BytesN, Env};

use crate::types::*;

/// Default dispute window in ledger sequences (~5 sec each).
/// 120 ledgers ≈ 10 minutes.
pub const DEFAULT_DISPUTE_WINDOW_LEDGERS: u32 = 120;

/// Dispute challenge state.
#[derive(Clone, Debug)]
pub enum ChallengeState {
    /// No challenge filed; settlement accepted.
    Settled,
    /// Challenge filed; awaiting resolution.
    Challenged,
    /// Challenge resolved (either upheld or dismissed).
    Resolved,
}

/// A settlement challenge record.
pub struct SettlementChallenge {
    pub table_id: u32,
    pub hand_number: u32,
    pub challenger: Address,
    /// Hash of the offchain evidence (e.g., replay buffer data).
    pub evidence_hash: BytesN<32>,
    /// Ledger at which the challenge was filed.
    pub challenged_at: u32,
    /// Ledger at which the dispute window expires.
    pub expires_at: u32,
    /// Whether the challenge has been resolved.
    pub resolved: bool,
    /// If resolved, was the challenge upheld (true = settlement reversed).
    pub upheld: bool,
}

/// Check if a table's hand is within the dispute window.
pub fn is_in_dispute_window(
    env: &Env,
    table: &TableState,
    hand_number: u32,
) -> bool {
    if table.phase != GamePhase::Settlement {
        return false;
    }
    if table.hand_number != hand_number {
        return false;
    }
    let window = DEFAULT_DISPUTE_WINDOW_LEDGERS;
    let deadline = table.settlement_entered_ledger + window;
    env.ledger().sequence() <= deadline
}

/// Check if the dispute window has expired (settlement finalized).
pub fn is_dispute_window_expired(
    env: &Env,
    table: &TableState,
) -> bool {
    if table.settlement_entered_ledger == 0 {
        return false;
    }
    let window = DEFAULT_DISPUTE_WINDOW_LEDGERS;
    let deadline = table.settlement_entered_ledger + window;
    env.ledger().sequence() > deadline
}

/// Validate an evidence hash (must be non-zero).
pub fn validate_evidence_hash(hash: &BytesN<32>) -> bool {
    let arr = hash.to_array();
    arr.iter().any(|&b| b != 0)
}
