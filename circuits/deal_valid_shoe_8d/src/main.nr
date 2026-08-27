/// Deal Validity Circuit - Multi-deck Shoe variant (8 decks)
///
/// See deal_valid_shoe for the full description. This variant compiles the
/// same logic for an 8-deck shoe (416 cards) instead of 6.

use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

/// Number of 52-card decks in the shoe (8 for this variant).
global NUM_DECKS: u32 = 8;

/// Total cards in the shoe: 8 * 52 = 416.
global DECK_SIZE: u32 = 416;

/// Tree leaf count must be >= DECK_SIZE and a power of 2. 416 fits in 512.
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
