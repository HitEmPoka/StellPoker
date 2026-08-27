/// Deal Validity Circuit - Multi-deck Shoe variant
///
/// Extends the standard deal circuit to support a multi-deck shoe: 6 (or 8)
/// standard 52-card decks shuffled together into a single shoe of 312 (or 416)
/// cards.
///
/// The circuit:
///   1. Builds the canonical shoe ordering so that every (rank, suit) identity
///      appears exactly `num_decks` times.
///   2. Derives the shuffled shoe from the three party contributions.
///   3. Verifies the shoe distribution with `assert_valid_shoe`, which tracks
///      the per-card, per-rank, and per-suit counts and proves the correct
///      distribution was drawn from the shoe.
///   4. Computes the Merkle root and assigns two cards per player as usual.
///
/// Compile with NUM_DECKS=6 (this file) or NUM_DECKS=8 (deal_valid_shoe_8d).

use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

/// Number of 52-card decks in the shoe (6 for this variant).
global NUM_DECKS: u32 = 6;

/// Total cards in the shoe: 6 * 52 = 312.
global DECK_SIZE: u32 = 312;

/// Tree leaf count must be >= DECK_SIZE and a power of 2.
/// 312 -> 512 leaves (depth 9); 416 also fits in 512.
global TREE_LEAVES: u32 = 512;
global TREE_DEPTH: u32 = 9;

/// Maximum players supported (for fixed circuit size).
global MAX_PLAYERS: u32 = 6;

fn main(
    // Private inputs (secret-shared in MPC)
    party0_permutation: [u32; DECK_SIZE],
    party1_permutation: [u32; DECK_SIZE],
    party2_permutation: [u32; DECK_SIZE],
    party0_salts: [Field; DECK_SIZE],
    party1_salts: [Field; DECK_SIZE],
    party2_salts: [Field; DECK_SIZE],

    // Public inputs
    num_players: pub u32,
) -> pub (Field, [Field; MAX_PLAYERS], [u32; MAX_PLAYERS], [u32; MAX_PLAYERS]) {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // 1. Build the canonical shoe: each (rank, suit) identity appears exactly
    //    NUM_DECKS times, once per physical deck in the shoe.
    let mut canonical_deck: [Field; DECK_SIZE] = [0; DECK_SIZE];
    for k in 0..NUM_DECKS {
        for c in 0..52 {
            canonical_deck[k * 52 + c] = c as Field;
        }
    }

    let (deck, salts) = shuffle::derive_shared_deck_and_salts_from(
        canonical_deck,
        party0_permutation,
        party1_permutation,
        party2_permutation,
        party0_salts,
        party1_salts,
        party2_salts,
    );

    // 2. Verify the shoe is a correct multi-deck composition.
    cards::assert_valid_shoe(deck, NUM_DECKS);

    // 3. Compute card commitments and Merkle root over TREE_LEAVES.
    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(deck[i], salts[i]);
    }
    // Remaining leaves are zero (padding).
    let deck_root = merkle::compute_merkle_root_generic::<512, 9>(leaves);

    // 4. Deterministically assign two cards per player by seat order.
    let mut hand_commitments: [Field; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut dealt_card1_indices: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut dealt_card2_indices: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for p in 0..MAX_PLAYERS {
        if p < num_players {
            let idx1 = p * 2;
            let idx2 = idx1 + 1;
            assert(idx2 < DECK_SIZE, "dealt index out of range");

            dealt_card1_indices[p] = idx1;
            dealt_card2_indices[p] = idx2;

            let c1_commit = leaves[idx1];
            let c2_commit = leaves[idx2];
            hand_commitments[p] = commitments::commit_hand(c1_commit, c2_commit);
        }
    }

    // 5. Sanity check: enough cards for all players.
    assert(2 * num_players <= DECK_SIZE, "not enough cards for players");

    (deck_root, hand_commitments, dealt_card1_indices, dealt_card2_indices)
}
