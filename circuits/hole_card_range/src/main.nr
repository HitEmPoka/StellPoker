use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::game;

/// Hole-Card Range Circuit (Issue #211).
///
/// Proves, without revealing the cards, that a player's private hole cards lie
/// within a claimed "poker range", i.e. every hole card has rank >= the
/// public threshold. This lets a player credibly claim e.g. "queen or better"
/// (threshold 10) while the verifier learns neither the exact cards nor the
/// salts.
///
/// Range semantics:
///   hole card is in range  <=>  rank(card) >= threshold
/// where rank(card) = card % 13 (0 = deuce ... 12 = ace).
///
/// The hand is bound to a Poseidon2 commitment so the proof cannot be
/// replayed against a different hand, and the blinding term prevents dictionary
/// attacks on the committed values.
///
/// Private: hole_card1, hole_card2, blinding.
/// Public:  threshold, hand_commitment.
/// Output:  in_range == true (satisfiable exactly when both cards are in range).
fn main(
    hole_card1: u32,
    hole_card2: u32,
    blinding: Field,
    threshold: pub u32,
    expected_hand_commitment: pub Field,
) -> pub bool {
    cards::assert_valid_card(hole_card1 as Field);
    cards::assert_valid_card(hole_card2 as Field);
    assert(hole_card1 != hole_card2, "duplicate hole card");

    // A range is a prefix of hole-card combos ordered by minimum rank; prove
    // both cards satisfy it without revealing which exact combo is held.
    let in_range = game::hole_range_is_satisfied(hole_card1, hole_card2, threshold);

    // Commit to the (blinded) hole cards so the range claim is bound to this
    // specific hand and stays zero-knowledge.
    let hand_commitment = commitments::commit_range_hand(
        hole_card1 as Field,
        hole_card2 as Field,
        blinding,
    );
    assert(hand_commitment == expected_hand_commitment, "hand commitment mismatch");

    game::assert_hole_in_range(hole_card1, hole_card2, threshold);
    in_range
}
