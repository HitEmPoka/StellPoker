/// Deck Completeness Circuit
///
/// Proves a shuffled deck contains exactly 52 unique, valid cards with no duplicates.
/// Each card is verified to be within valid rank/suit ranges (0-51) and all cards are distinct.

use stellar_poker_lib::cards;

fn main(
    // Private input: the shuffled deck (52 cards)
    deck: [Field; 52],
) -> pub Field {
    // Verify all cards in the deck are valid (0-51) and unique.
    // This checks:
    //   - Each card value < 52 (valid rank/suit range)
    //   - No duplicate cards (all 52 cards are distinct)
    cards::assert_valid_deck(deck);

    // Compute a commitment to the valid deck as public output.
    // This allows the verifier to confirm which deck was proven valid.
    let mut deck_hash = 0 as Field;
    for i in 0..52 {
        deck_hash = deck_hash + deck[i] * (i as Field + 1);
    }

    deck_hash
}

#[test]
fn test_valid_deck() {
    let deck = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51];
    let result = main(deck);
    // Expected hash for ordered deck 0..51
    // sum_{i=0..51} i * (i+1) = sum i^2 + i = 45526 + 1326 = 46852
    assert(result == 46852);
}

#[test]
fn test_valid_shuffled_deck() {
    // A valid shuffled deck (different order, same cards)
    let deck = [51, 50, 49, 48, 47, 46, 45, 44, 43, 42, 41, 40, 39, 38, 37, 36, 35, 34, 33, 32, 31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
    let result = main(deck);
    // The hash should be different from ordered deck
    assert(result != 46852);
}

#[test(should_fail)]
fn test_invalid_deck_duplicate() {
    // Invalid deck: duplicate card (0 appears twice, 1 is missing)
    let deck = [0, 0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51];
    main(deck);
}

#[test(should_fail)]
fn test_invalid_deck_out_of_range() {
    // Invalid deck: card value 52 is out of range
    let deck = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 52];
    main(deck);
}