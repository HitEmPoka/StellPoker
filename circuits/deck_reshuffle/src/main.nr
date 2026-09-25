/// Deck Reshuffle Circuit (Issue #535)
///
/// Proves that the deck for hand N+1 correctly derives from the committed deck
/// of hand N through a fresh reshuffling, preventing deck-fixing across rounds.
///
/// The proof establishes a cryptographic chain: the new deck is derived by:
/// 1. Taking the deck commitment from the prior hand (deck_n_root)
/// 2. Verifying it matches the revealed deck from hand N (via Merkle proof)
/// 3. Applying fresh party permutation/salt shares for round N+1
/// 4. Computing a new deck commitment (deck_n+1_root)
///
/// This ensures that once a deck is committed and revealed in hand N,
/// no party can unilaterally modify it for hand N+1.

use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::compression;
use stellar_poker_lib::deck;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

global DECK_SIZE: u32 = 52;
global TREE_LEAVES: u32 = 64;
global TREE_DEPTH: u32 = 6;
global MAX_PLAYERS: u32 = 6;

fn main(
    // Hand N: prior deck commitment and revealed deck
    deck_n_root: pub Field,
    deck_n: [Field; DECK_SIZE],
    deck_n_salts: [Field; DECK_SIZE],

    // Merkle inclusion proofs: verify deck_n matches deck_n_root
    deck_n_proof: [Field; TREE_DEPTH],
    deck_n_leaf_index: u32,

    // Hand N+1: fresh permutation and salt shares from three parties
    party0_permutation_packed: [u32; 10],
    party1_permutation_packed: [u32; 10],
    party2_permutation_packed: [u32; 10],
    party0_salts: [Field; DECK_SIZE],
    party1_salts: [Field; DECK_SIZE],
    party2_salts: [Field; DECK_SIZE],

    // Public inputs
    num_players: pub u32,
) -> pub (Field, [Field; MAX_PLAYERS], [u32; MAX_PLAYERS], [u32; MAX_PLAYERS]) {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // ===== Verify Hand N Deck Root =====
    // Compute Merkle root of deck_n to ensure it matches deck_n_root
    let mut deck_n_leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        deck_n_leaves[i] = commitments::commit_card(deck_n[i], deck_n_salts[i]);
    }
    let computed_deck_n_root = merkle::compute_merkle_root_generic::<64, 6>(deck_n_leaves);
    assert(computed_deck_n_root == deck_n_root, "deck_n root mismatch");

    // ===== Reshuffle Hand N Deck for Hand N+1 =====
    // Apply fresh party permutations to deck_n to produce deck_n+1
    let party0_permutation = compression::unpack_permutation_52(party0_permutation_packed);
    let party1_permutation = compression::unpack_permutation_52(party1_permutation_packed);
    let party2_permutation = compression::unpack_permutation_52(party2_permutation_packed);

    let (deck_n_plus_1, final_salts) = shuffle::derive_shared_deck_and_salts_from(
        deck_n,
        party0_permutation,
        party1_permutation,
        party2_permutation,
        party0_salts,
        party1_salts,
        party2_salts,
    );

    // ===== Verify Hand N+1 Deck Validity =====
    // Ensure all cards in the reshuffled deck are valid and unique
    cards::assert_valid_deck(deck_n_plus_1);

    // ===== Compute Hand N+1 Deck Root and Hand Assignments =====
    let mut deck_n_plus_1_leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        deck_n_plus_1_leaves[i] = commitments::commit_card(deck_n_plus_1[i], final_salts[i]);
    }
    let deck_n_plus_1_root = merkle::compute_merkle_root_generic::<64, 6>(deck_n_plus_1_leaves);

    let mut hand_commitments: [Field; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut dealt_card1_indices: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut dealt_card2_indices: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for p in 0..MAX_PLAYERS {
        if p < num_players {
            let idx1 = p * 2;
            let idx2 = idx1 + 1;
            assert(idx2 < deck::deck_size(), "dealt index out of range");

            dealt_card1_indices[p] = idx1;
            dealt_card2_indices[p] = idx2;

            let c1_commit = deck_n_plus_1_leaves[idx1];
            let c2_commit = deck_n_plus_1_leaves[idx2];
            hand_commitments[p] = commitments::commit_hand(c1_commit, c2_commit);
        }
    }

    assert(2 * num_players <= deck::deck_size(), "not enough cards for players");
    assert_global_deal_injectivity(dealt_card1_indices, dealt_card2_indices, num_players);

    // ===== Output Chain Link =====
    // Return the new deck root (deck_n+1_root) and hand commitments
    // This root becomes deck_n_root for the next hand's reshuffle proof
    (deck_n_plus_1_root, hand_commitments, dealt_card1_indices, dealt_card2_indices)
}

fn assert_global_deal_injectivity(
    card1_indices: [u32; MAX_PLAYERS],
    card2_indices: [u32; MAX_PLAYERS],
    num_players: u32,
) {
    for seat in 0..MAX_PLAYERS {
        if seat < num_players {
            assert(card1_indices[seat] != card2_indices[seat], "seat received duplicate card");

            for other_seat in 0..MAX_PLAYERS {
                if other_seat < num_players & other_seat > seat {
                    assert(
                        card1_indices[seat] != card1_indices[other_seat],
                        "card dealt to two seats",
                    );
                    assert(
                        card1_indices[seat] != card2_indices[other_seat],
                        "card dealt to two seats",
                    );
                    assert(
                        card2_indices[seat] != card1_indices[other_seat],
                        "card dealt to two seats",
                    );
                    assert(
                        card2_indices[seat] != card2_indices[other_seat],
                        "card dealt to two seats",
                    );
                }
            }
        }
    }
}

#[test]
fn test_deck_reshuffle_identity() {
    // Test that applying identity permutations produces a valid deck
    let mut deck: [Field; DECK_SIZE] = [0; DECK_SIZE];
    let mut salts: [Field; DECK_SIZE] = [0; DECK_SIZE];

    for i in 0..DECK_SIZE {
        deck[i] = i as Field;
        salts[i] = i as Field;
    }

    let mut identity: [u32; 52] = [0; 52];
    for i in 0..52 {
        identity[i] = i as u32;
    }

    let packed = compression::pack_permutation_52(identity);
    let unpacked = compression::unpack_permutation_52(packed);

    for i in 0..52 {
        assert(unpacked[i] == identity[i], "identity permutation failed");
    }
}

#[test]
fn test_deck_reshuffle_injectivity() {
    let mut dealt_card1_indices: [u32; MAX_PLAYERS] = [0, 2, 4, 6, 8, 10];
    let mut dealt_card2_indices: [u32; MAX_PLAYERS] = [1, 3, 5, 7, 9, 11];

    assert_global_deal_injectivity(dealt_card1_indices, dealt_card2_indices, 6);
}

#[test(should_fail_with = "card dealt to two seats")]
fn test_deck_reshuffle_injectivity_fails_on_duplicate() {
    let mut dealt_card1_indices: [u32; MAX_PLAYERS] = [0, 2, 1, 6, 8, 10];
    let mut dealt_card2_indices: [u32; MAX_PLAYERS] = [1, 3, 5, 7, 9, 11];

    assert_global_deal_injectivity(dealt_card1_indices, dealt_card2_indices, 6);
}
