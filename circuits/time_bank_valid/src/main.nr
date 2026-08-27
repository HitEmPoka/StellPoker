/// Time-Bank Usage Limit Circuit (Issue #225)
///
/// Proves that a player had sufficient time-bank balance when requesting a
/// time extension. The circuit verifies:
///
///   1. `initial_balance >= 0`
///   2. `time_used >= 0` and `time_used <= initial_balance + replenished`
///   3. `replenished = elapsed_time * replenishment_rate` (rate is in
///      milliseconds per ledger, rounded down)
///   4. Final balance after usage is non-negative
///
/// All arithmetic is done in u32 to avoid overflow in Noir. The prover
/// supplies `initial_balance`, `time_used`, `elapsed_time`, and
/// `replenishment_rate` as private inputs; the contract checks the
/// public outputs against on-chain state.
///
/// # Public inputs
/// - `player_hash`: Poseidon2 hash of the player address (commitment)
/// - `final_balance`: the claimed remaining time-bank balance after extension
///
/// # Returns
/// - `pub u32` — the verified final balance (must match `final_balance` input)

fn main(
    initial_balance: u32,
    time_used: u32,
    elapsed_time: u32,
    replenishment_rate: u32,

    player_hash: pub Field,
    final_balance: pub u32,
) -> pub u32 {
    // Basic sanity: cannot use negative time (u32 is always >= 0).
    assert(time_used > 0, "must use some time");

    // Compute replenished amount (floor division).
    let replenished = (elapsed_time * replenishment_rate) / 1000;

    // Total available time = initial balance + replenishment.
    let total_available = initial_balance + replenished;

    // Cannot use more than what is available.
    assert(time_used <= total_available, "insufficient time-bank balance");

    // Compute the actual final balance.
    let actual_final = total_available - time_used;

    // The claimed final balance must match.
    assert(final_balance == actual_final, "final balance mismatch");

    actual_final
}
