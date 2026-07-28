use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;

/// Proves the category of two committed hole cards without revealing either
/// card or its salt. Categories use the shared evaluator's 0..8 encoding.
fn main(
    hole_card1: u32,
    hole_card2: u32,
    salt1: Field,
    salt2: Field,
    board: pub [u32; 5],
    expected_hand_commitment: pub Field,
    claimed_category: pub u32,
) -> pub u32 {
    cards::assert_valid_card(hole_card1 as Field);
    cards::assert_valid_card(hole_card2 as Field);
    let mut seven_cards: [u32; 7] = [0; 7];
    seven_cards[0] = hole_card1;
    seven_cards[1] = hole_card2;
    for i in 0..5 {
        cards::assert_valid_card(board[i] as Field);
        seven_cards[i + 2] = board[i];
    }
    for i in 0..7 {
        for j in (i + 1)..7 {
            assert(seven_cards[i] != seven_cards[j], "duplicate card");
        }
    }

    let c1 = commitments::commit_card(hole_card1 as Field, salt1);
    let c2 = commitments::commit_card(hole_card2 as Field, salt2);
    assert(
        commitments::commit_hand(c1, c2) == expected_hand_commitment,
        "hand commitment mismatch",
    );

    let score = cards::evaluate_hand_rank(seven_cards);
    let category = score / 1048576;
    assert(category == claimed_category, "claimed hand rank is incorrect");
    category
}
