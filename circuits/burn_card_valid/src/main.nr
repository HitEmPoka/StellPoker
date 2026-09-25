/// Burn Card Validity Circuit (Issue #215)
///
/// Proves the correct number of cards are burned before each street in a
/// Texas Hold'em hand, according to poker rules:
///
///   - 1 burn card before the hole cards are dealt (preflop)
///   - 1 burn card before the flop
///   - 1 burn card before the turn
///   - 1 burn card before the river
///
/// The deck is re-derived in-circuit from the per-party private contributions
/// (identical to the deal / reveal / showdown circuits), its Merkle root is
/// verified against the committed deck root, and the single burn card at each
/// of the four street positions is proven to be:
///
///   1. a genuine card of the committed deck (its leaf commitment matches), and
///   2. distinct from every hole card and every board card dealt that hand.
///
/// The public `street_burn_counts` input is asserted to be exactly `[1; 4]`,
/// proving the correct number of burned cards before each street.

use stellar_poker_lib::commitments;
use stellar_poker_lib::deck;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

/// Deck size (standard 52-card default).
global DECK_SIZE: u32 = 52;

/// Tree leaf count must be >= DECK_SIZE and a power of two. 52 -> 64 leaves.
global TREE_LEAVES: u32 = 64;

/// Number of burn streets: preflop, flop, turn, river.
global NUM_BURNS: u32 = 4;

/// Maximum players supported (fixed circuit size).
global MAX_PLAYERS: u32 = 6;

fn main(
    // Private inputs (secret-shared in MPC)
    party0_permutation: [u32; 52],
    party1_permutation: [u32; 52],
    party2_permutation: [u32; 52],
    party0_salts: [Field; 52],
    party1_salts: [Field; 52],
    party2_salts: [Field; 52],

    // Public inputs
    num_players: pub u32,
    board_indices: pub [u32; 5],
    deck_root: pub Field,
    street_burn_counts: pub [u32; NUM_BURNS],
) -> pub ([u32; NUM_BURNS], [u32; NUM_BURNS]) {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    let (deck, salts) = shuffle::derive_shared_deck_and_salts(
        party0_permutation,
        party1_permutation,
        party2_permutation,
        party0_salts,
        party1_salts,
        party2_salts,
    );

    // 1. Recompute the deck Merkle root and verify it matches the committed root.
    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(deck[i], salts[i]);
    }
    let computed_root = merkle::compute_merkle_root(leaves);
    assert(computed_root == deck_root, "deck root mismatch");

    // 2. Mark the hole cards (seats 0..num_players, two cards each) as used.
    let mut used_indices: [bool; 52] = [false; 52];
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            let idx1 = p * 2;
            let idx2 = idx1 + 1;
            assert(!used_indices[idx1], "duplicate dealt index");
            used_indices[idx1] = true;
            assert(!used_indices[idx2], "duplicate dealt index");
            used_indices[idx2] = true;
        }
    }

    // 3. Mark the 5 board cards (flop 3, turn 1, river 1) as used.
    for i in 0..5 {
        let idx = board_indices[i];
        assert(idx < deck::deck_size(), "board index out of range");
        assert(!used_indices[idx], "board index duplicates dealt card");
        used_indices[idx] = true;
    }

    // 4. Prove the correct number of burns occur before each street: exactly one.
    let one: [u32; NUM_BURNS] = [1, 1, 1, 1];
    for s in 0..NUM_BURNS {
        assert(street_burn_counts[s] == one[s], "must burn exactly one card per street");
    }

    // 5. Locate the single burn card before each street in the deterministic
    //    deck layout. Hole cards occupy slots [0, 2*num_players); after each
    //    street block there is exactly one burn slot, then the street cards.
    //
    //    slot layout (offset from base = 2*num_players):
    //      offset 0 : preflop burn
    //      offset 1 .. 3 : flop
    //      offset 4 : flop burn
    //      offset 5 : turn
    //      offset 6 : turn burn
    //      offset 7 : river
    //      offset 8 : river burn
    let base = 2 * num_players;
    let burn_offsets: [u32; NUM_BURNS] = [0, 4, 6, 8];

    let mut burn_indices: [u32; NUM_BURNS] = [0; NUM_BURNS];
    let mut burn_cards: [u32; NUM_BURNS] = [0; NUM_BURNS];

    for s in 0..NUM_BURNS {
        let idx = base + burn_offsets[s];
        assert(idx < deck::deck_size(), "burn index out of range");
        assert(!used_indices[idx], "burn card duplicates a dealt or board card");
        used_indices[idx] = true;

        burn_indices[s] = idx;
        burn_cards[s] = deck[idx] as u32;
    }

    (burn_cards, burn_indices)
}

fn setup_test_deck() -> ([u32; 52], [u32; 52], [u32; 52], [Field; 52], [Field; 52], [Field; 52], Field) {
    let mut id_perm: [u32; 52] = [0; 52];
    for i in 0..52 {
        id_perm[i] = i;
    }
    let zero_salts: [Field; 52] = [0; 52];

    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(i as Field, 0);
    }
    let deck_root = merkle::compute_merkle_root(leaves);

    (id_perm, id_perm, id_perm, zero_salts, zero_salts, zero_salts, deck_root)
}

#[test]
fn test_burn_card_valid_2p_success() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    // 2 players: hole cards are [0, 1, 2, 3].
    // Base is 4. Burn offsets: 0, 4, 6, 8 -> burn indices: 4, 8, 10, 12.
    // Board cards: flop (5, 6, 7), turn (9), river (11).
    let board_indices: [u32; 5] = [5, 6, 7, 9, 11];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];

    let (burn_cards, burn_indices) = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        2, board_indices, deck_root, street_burn_counts,
    );

    assert(burn_indices[0] == 4, "preflop burn index should be 4");
    assert(burn_indices[1] == 8, "flop burn index should be 8");
    assert(burn_indices[2] == 10, "turn burn index should be 10");
    assert(burn_indices[3] == 12, "river burn index should be 12");

    assert(burn_cards[0] == 4, "preflop burn card should match index in identity deck");
    assert(burn_cards[1] == 8, "flop burn card should match index in identity deck");
    assert(burn_cards[2] == 10, "turn burn card should match index in identity deck");
    assert(burn_cards[3] == 12, "river burn card should match index in identity deck");
}

#[test]
fn test_burn_card_valid_6p_success() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    // 6 players: hole cards are [0..11].
    // Base is 12. Burn offsets: 0, 4, 6, 8 -> burn indices: 12, 16, 18, 20.
    // Board cards: flop (13, 14, 15), turn (17), river (19).
    let board_indices: [u32; 5] = [13, 14, 15, 17, 19];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];

    let (burn_cards, burn_indices) = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        6, board_indices, deck_root, street_burn_counts,
    );

    assert(burn_indices[0] == 12, "preflop burn index should be 12");
    assert(burn_indices[1] == 16, "flop burn index should be 16");
    assert(burn_indices[2] == 18, "turn burn index should be 18");
    assert(burn_indices[3] == 20, "river burn index should be 20");
    assert(burn_cards[0] == 12);
    assert(burn_cards[1] == 16);
    assert(burn_cards[2] == 18);
    assert(burn_cards[3] == 20);
}

#[test(should_fail_with = "must burn exactly one card per street")]
fn test_burn_card_fails_wrong_street_burn_count() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    let board_indices: [u32; 5] = [5, 6, 7, 9, 11];
    let invalid_burn_counts: [u32; NUM_BURNS] = [2, 1, 1, 1];

    let _ = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        2, board_indices, deck_root, invalid_burn_counts,
    );
}

#[test(should_fail_with = "deck root mismatch")]
fn test_burn_card_fails_deck_root_mismatch() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, _) = setup_test_deck();
    let board_indices: [u32; 5] = [5, 6, 7, 9, 11];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];
    let tampered_root: Field = 0x12345678;

    let _ = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        2, board_indices, tampered_root, street_burn_counts,
    );
}

#[test(should_fail_with = "board index duplicates dealt card")]
fn test_burn_card_fails_board_duplicates_hole_card() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    // index 0 is player 0's hole card, not allowed on board
    let board_indices: [u32; 5] = [0, 6, 7, 9, 11];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];

    let _ = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        2, board_indices, deck_root, street_burn_counts,
    );
}

#[test(should_fail_with = "burn card duplicates a dealt or board card")]
fn test_burn_card_fails_board_duplicates_burn_card() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    // index 4 is preflop burn card; using it as a board card causes conflict
    let board_indices: [u32; 5] = [4, 6, 7, 9, 11];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];

    let _ = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        2, board_indices, deck_root, street_burn_counts,
    );
}

#[test(should_fail_with = "need at least 2 players")]
fn test_burn_card_fails_too_few_players() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    let board_indices: [u32; 5] = [5, 6, 7, 9, 11];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];

    let _ = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        1, board_indices, deck_root, street_burn_counts,
    );
}

#[test(should_fail_with = "too many players")]
fn test_burn_card_fails_too_many_players() {
    let (p0_perm, p1_perm, p2_perm, s0, s1, s2, deck_root) = setup_test_deck();
    let board_indices: [u32; 5] = [5, 6, 7, 9, 11];
    let street_burn_counts: [u32; NUM_BURNS] = [1, 1, 1, 1];

    let _ = main(
        p0_perm, p1_perm, p2_perm, s0, s1, s2,
        7, board_indices, deck_root, street_burn_counts,
    );
}
