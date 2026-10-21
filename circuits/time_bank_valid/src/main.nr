/// Privacy-preserving time-bank transition circuit (Issue #525).
///
/// Public inputs reveal only the player identifier, an action nonce, and the
/// commitments before and after the transition. Initial balance, time used,
/// and remaining balance stay private; public policy inputs prevent a prover
/// from inventing replenishment.

use stellar_poker_lib::commitments;

fn time_bank_commitment(player_hash: Field, balance: u32, nonce: Field) -> Field {
    commitments::poseidon2_hash_3(player_hash, balance as Field, nonce)
}

fn assert_valid_time_bank_transition(
    initial_balance: u32,
    time_used: u32,
    elapsed_ledgers: u32,
    replenishment_rate_ms: u32,
    previous_nonce: Field,
    next_nonce: Field,
    player_hash: Field,
    action_id: Field,
    previous_commitment: Field,
    next_commitment: Field,
) {
    assert(time_used > 0, "must use some time");
    assert(previous_nonce != next_nonce, "state nonce must rotate");

    let expected_previous = time_bank_commitment(player_hash, initial_balance, previous_nonce);
    assert(previous_commitment == expected_previous, "previous state mismatch");

    let replenished = (elapsed_ledgers * replenishment_rate_ms) / 1000;
    let available = initial_balance + replenished;
    assert(time_used <= available, "insufficient time-bank balance");
    let remaining = available - time_used;

    // Bind the new state to this public action so a proof cannot be replayed
    // for a different turn while keeping the exact remaining balance private.
    let action_bound_nonce = next_nonce + action_id;
    let expected_next = time_bank_commitment(player_hash, remaining, action_bound_nonce);
    assert(next_commitment == expected_next, "next state mismatch");
}

fn main(
    // Private witness: balances, usage, and commitment randomness.
    initial_balance: u32,
    time_used: u32,
    previous_nonce: Field,
    next_nonce: Field,
    // Public statement: policy, identity/action binding, and opaque states.
    elapsed_ledgers: pub u32,
    replenishment_rate_ms: pub u32,
    player_hash: pub Field,
    action_id: pub Field,
    previous_commitment: pub Field,
    next_commitment: pub Field,
) {
    assert_valid_time_bank_transition(
        initial_balance,
        time_used,
        elapsed_ledgers,
        replenishment_rate_ms,
        previous_nonce,
        next_nonce,
        player_hash,
        action_id,
        previous_commitment,
        next_commitment,
    );
}

#[test]
fn test_private_time_bank_transition() {
    let player = 42;
    let action = 9;
    let before = time_bank_commitment(player, 30, 111);
    // replenished = 10 * 1000 / 1000 = 10; remaining = 28
    let after = time_bank_commitment(player, 28, 222 + action);
    assert_valid_time_bank_transition(30, 12, 10, 1000, 111, 222, player, action, before, after);
}

#[test(should_fail_with = "insufficient time-bank balance")]
fn test_rejects_negative_remaining_balance() {
    let player = 42;
    let action = 10;
    let before = time_bank_commitment(player, 5, 111);
    let after = time_bank_commitment(player, 0, 222 + action);
    assert_valid_time_bank_transition(5, 6, 0, 0, 111, 222, player, action, before, after);
}

#[test(should_fail_with = "previous state mismatch")]
fn test_rejects_transition_for_different_player() {
    let before = time_bank_commitment(41, 30, 111);
    let after = time_bank_commitment(42, 20, 223);
    assert_valid_time_bank_transition(30, 10, 0, 0, 111, 222, 42, 1, before, after);
}
