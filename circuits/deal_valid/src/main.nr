/// Deal Validity Circuit
///
/// The deck is derived in-circuit from per-party private contributions:
/// - party permutation shares
/// - party salt shares
/// - VRF output for public verifiability of randomness
///
/// This removes any single-node plaintext deck dependency during prepare/split.
/// The VRF shuffle seed enables public verifiability of the deck randomness.

use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::compression;
use stellar_poker_lib::deck;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;
use stellar_poker_lib::vrf;

/// Deck composition is parameterized by the shared deck module (Issue #216):
/// the compile-time global `deck::DECK_TYPE` selects between standard (52),
/// short (36, 6+) and joker (54) decks, and the deck-aware bounds below follow.

/// Deck size (cards in deck, default 52 for standard poker)
/// Can be overridden at compile time for variant poker games
global DECK_SIZE: u32 = 52;

/// Tree leaf count must be >= DECK_SIZE and power of 2
/// For 52 cards: 64 leaves (6 levels)
/// For 104 cards: 128 leaves (7 levels)
/// For 200+ cards: 256 leaves (8 levels)
global TREE_LEAVES: u32 = 64;
global TREE_DEPTH: u32 = 6;

/// Maximum players supported (for fixed circuit size).
/// Override at compile time via PLAYER_COUNT environment variable for
/// per-table-size parameterisation (Issue #7).
global MAX_PLAYERS: u32 = 6;

/// Enforce that every active seat received two distinct indices and that no
/// index appears in any other active seat. Keeping this check separate from
/// full-deck uniqueness makes the cross-seat invariant explicit and directly
/// testable if deal assignment stops being deterministic in a future circuit.
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

fn main(
    // Private inputs (secret-shared in MPC) — compressed permutations
    party0_permutation_packed: [u32; 10],
    party1_permutation_packed: [u32; 10],
    party2_permutation_packed: [u32; 10],
    party0_salts: [Field; 52],
    party1_salts: [Field; 52],
    party2_salts: [Field; 52],
    // VRF inputs for public verifiability of randomness
    vrf_sk: Field, // VRF private key share
    vrf_pk: Field, // VRF public key (public input)
    vrf_message: Field, // VRF message (deterministic label for deal)
    // Public inputs
    num_players: pub u32,
) -> pub (Field, [Field; MAX_PLAYERS], [u32; MAX_PLAYERS], [u32; MAX_PLAYERS]) {
    // Decompress permutations from packed form
    let party0_permutation = compression::unpack_permutation_52(party0_permutation_packed);
    let party1_permutation = compression::unpack_permutation_52(party1_permutation_packed);
    let party2_permutation = compression::unpack_permutation_52(party2_permutation_packed);
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // Derive shuffle seed from VRF output
    let shuffle_seed = vrf::derive_shuffle_seed(vrf_message, vrf_sk);

    // Verify VRF proof for public verifiability of randomness.
    // Anyone with the VRF public key can verify the shuffle seed was correctly derived.
    let (vrf_output, vrf_proof) = vrf::vrf_prove(vrf_sk, vrf_message);
    assert(
        vrf::vrf_verify(vrf_pk, vrf_message, shuffle_seed, vrf_proof),
        "VRF verification failed",
    );

    // Incorporate VRF shuffle seed into the salt contributions.
    // The VRF seed is added to party0's salts; the sum party0+party1+party2+seed
    // becomes the final salts used for deck derivation.
    // This ensures the derived deck is cryptographically bound to the VRF output.
    let mut modified_party0_salts: [Field; 52] = [0; 52];
    for i in 0..52 {
        modified_party0_salts[i] = party0_salts[i] + shuffle_seed;
    }

    // Derive the final secret deck and salts from all three party contributions,
    // with the VRF shuffle seed incorporated into party0's salts.
    let (deck, final_salts) = shuffle::derive_shared_deck_and_salts(
        party0_permutation,
        party1_permutation,
        party2_permutation,
        modified_party0_salts,
        party1_salts,
        party2_salts,
    );

    // 1. Verify all cards in derived deck are valid and unique.
    cards::assert_valid_deck(deck);

    // 2. Compute card commitments and Merkle root.
    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(deck[i], final_salts[i]);
    }
    // Remaining leaves are zero (padding).
    let deck_root = merkle::compute_merkle_root_generic::<64, 6>(leaves);

    // 3. Deterministically assign two cards per player by seat order.
    // Optimization: reuse card commitment leaves computed in step 2.
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

            // Use pre-computed leaf commitments from step 2 to reduce constraints.
            let c1_commit = leaves[idx1];
            let c2_commit = leaves[idx2];
            hand_commitments[p] = commitments::commit_hand(c1_commit, c2_commit);
        }
    }

    // 4. Enforce global injectivity independently of full-deck uniqueness.
    assert(2 * num_players <= deck::deck_size(), "not enough cards for players");
    assert_global_deal_injectivity(dealt_card1_indices, dealt_card2_indices, num_players);

    (deck_root, hand_commitments, dealt_card1_indices, dealt_card2_indices)
}

#[test]
fn test_global_deal_injectivity() {
    assert_global_deal_injectivity([0, 2, 4, 6, 8, 10], [1, 3, 5, 7, 9, 11], 6);
}

#[test(should_fail_with = "card dealt to two seats")]
fn test_global_deal_injectivity_rejects_cross_seat_duplicate() {
    assert_global_deal_injectivity([0, 2, 1, 6, 8, 10], [1, 3, 5, 7, 9, 11], 6);
}

#[test(should_fail_with = "seat received duplicate card")]
fn test_global_deal_injectivity_rejects_same_seat_duplicate() {
    assert_global_deal_injectivity([0, 2, 4, 6, 8, 10], [0, 3, 5, 7, 9, 11], 6);
}
