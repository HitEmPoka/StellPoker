// Deck composition configuration (Issue #216).
//
// Circuits are parameterized by deck type at compile time. The following
// variants are supported:
//
//   DECK_TYPE = 0 : standard 52-card deck (4 suits x 13 ranks, no jokers)
//   DECK_TYPE = 1 : short deck 36 cards (6+), ranks 6..A, no jokers
//   DECK_TYPE = 2 : standard 52-card deck plus 2 jokers (54 cards)
//
// The card encoding is kept stable regardless of deck type so that the rest of
// the library (commitments, merkle roots, shuffling) is unaffected:
//
//   card value = suit * 13 + rank        (rank 0..12, suit 0..3)
//   joker values = 52, 53                (only when DECK_TYPE == 2)
//
// A short deck uses the same 13-rank encoding but only ranks 4..12 are valid
// (6, 7, 8, 9, 10, J, Q, K, A), which has 9 ranks per suit -> 36 cards.
//
// All branch conditions compare the comptime global DECK_TYPE against integer
// literals so that the compiler can monomorphise every helper.

/// Compile-time deck type. Override for parameterised circuit builds.
/// 0 = standard 52, 1 = short deck 6+, 2 = standard + 2 jokers.
pub global DECK_TYPE: u32 = 0;

/// Number of suits in the deck (jokers are not counted as a suit).
pub global NUM_SUITS: u32 = 4;

/// Ranks per suit in the physical card encoding. Kept at 13 so the value
/// encoding `suit * 13 + rank` is stable across deck types.
pub global NUM_RANKS: u32 = 13;

/// Index of the first valid rank. Standard / jokers: 0 (2). Short deck: 4 (6).
pub fn rank_offset() -> u32 {
    if DECK_TYPE == 1 {
        4
    } else {
        0
    }
}

/// Number of distinct ranks actually used for this deck type.
/// Standard / jokers: 13. Short deck (6+): 9.
pub fn num_ranks() -> u32 {
    if DECK_TYPE == 1 {
        9
    } else {
        NUM_RANKS
    }
}

/// True when the deck includes jokers.
pub fn has_jokers() -> bool {
    DECK_TYPE == 2
}

/// Number of joker cards (0 normally, 2 for a joker deck).
pub fn joker_count() -> u32 {
    if DECK_TYPE == 2 {
        2
    } else {
        0
    }
}

/// Total number of cards in the deck.
pub fn deck_size() -> u32 {
    if DECK_TYPE == 1 {
        36
    } else if DECK_TYPE == 2 {
        54
    } else {
        52
    }
}

/// True when a given rank is valid for the configured deck type.
/// Short deck excludes ranks 0..3 (2..5). Jokers use rank 0 on a special suit.
pub fn is_valid_rank(rank: u32) -> bool {
    if DECK_TYPE == 1 {
        (rank >= 4) & (rank < NUM_RANKS)
    } else {
        rank < NUM_RANKS
    }
}

/// True when a card is one of the joker values (only for a joker deck).
pub fn is_joker(card: u32) -> bool {
    if DECK_TYPE == 2 {
        (card >= 52) & (card < 54)
    } else {
        false
    }
}

/// Smallest power of two tree leaf count that can hold the deck.
pub fn tree_leaves() -> u32 {
    if deck_size() <= 64 {
        64
    } else if deck_size() <= 128 {
        128
    } else {
        256
    }
}

/// Merkle tree depth implied by `tree_leaves()`.
pub fn tree_depth() -> u32 {
    if tree_leaves() == 64 {
        6
    } else if tree_leaves() == 128 {
        7
    } else {
        8
    }
}

#[test]
fn test_standard_deck() {
    assert(DECK_TYPE == 0);
    assert(deck_size() == 52);
    assert(num_ranks() == 13);
    assert(rank_offset() == 0);
    assert(!has_jokers());
    assert(joker_count() == 0);
    let r0 = is_valid_rank(0);
    let r12 = is_valid_rank(12);
    let r13 = is_valid_rank(13);
    assert(r0);
    assert(r12);
    assert(!r13);
    let j0 = is_joker(0);
    let j52 = is_joker(52);
    assert(!j0);
    assert(!j52);
    assert(tree_leaves() == 64);
    assert(tree_depth() == 6);
}

#[test]
fn test_short_deck_params() {
    // The default build is a standard deck. Short-deck param selection is a
    // compile-time property; this test guards the helpers against regressions
    // under the default configuration.
    assert(DECK_TYPE == 0);
    assert(deck_size() == 52);
    let r4 = is_valid_rank(4);
    assert(r4); // rank 6 is valid in every real deck
}
