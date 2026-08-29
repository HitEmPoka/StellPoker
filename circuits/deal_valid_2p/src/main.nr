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
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;
use stellar_poker_lib::vrf;

/// Maximum players supported (for fixed circuit size).
/// Override at compile time via PLAYER_COUNT environment variable for
/// per-table-size parameterisation (Issue #7).
global MAX_PLAYERS: u32 = 6;

fn main(
    // Private inputs (secret-shared in MPC)
    party0_permutation: [u32; 52],
    party1_permutation: [u32; 52],
    party2_permutation: [u32; 52],
    party0_salts: [Field; 52],
    party1_salts: [Field; 52],
    party2_salts: [Field; 52],

    // VRF inputs for public verifiability of randomness
    vrf_sk: Field,       // VRF private key share
    vrf_pk: Field,       // VRF public key (public input)
    vrf_message: Field,  // VRF message (deterministic label for deal)

    // Public inputs
    num_players: pub u32,
) -> pub (Field, [Field; MAX_PLAYERS], [u32; MAX_PLAYERS], [u32; MAX_PLAYERS]) {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // Derive shuffle seed from VRF output
    let shuffle_seed = vrf::derive_shuffle_seed(vrf_message, vrf_sk);

    // Verify VRF proof for public verifiability of randomness.
    // Anyone with the VRF public key can verify the shuffle seed was correctly derived.
    let (vrf_output, vrf_proof) = vrf::vrf_prove(vrf_sk, vrf_message);
    assert(vrf::vrf_verify(vrf_pk, vrf_message, shuffle_seed, vrf_proof), "VRF verification failed");

    // Incorporate VRF shuffle seed into the salt contributions.
    // The VRF seed is added to party0's salts; the sum party0+party1+party2+seed
    // becomes the final salts used for deck derivation.
    // This ensures the derived deck is cryptographically bound to the VRF output.
    let modified_party0_salts: [Field; 52] = [0; 52];
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
    let mut leaves: [Field; 64] = [0; 64];
    for i in 0..52 {
        leaves[i] = commitments::commit_card(deck[i], final_salts[i]);
    }
    // Padding leaves 52..63 are zero.
    let deck_root = merkle::compute_merkle_root(leaves);

    // 3. Deterministically assign two cards per player by seat order.
    let mut hand_commitments: [Field; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut dealt_card1_indices: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut dealt_card2_indices: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for p in 0..MAX_PLAYERS {
        if p < num_players {
            let idx1 = p * 2;
            let idx2 = idx1 + 1;
            assert(idx2 < 52, "dealt index out of range");

            dealt_card1_indices[p] = idx1;
            dealt_card2_indices[p] = idx2;

            let c1_commit = commitments::commit_card(deck[idx1], final_salts[idx1]);
            let c2_commit = commitments::commit_card(deck[idx2], final_salts[idx2]);
            hand_commitments[p] = commitments::commit_hand(c1_commit, c2_commit);
        }
    }

    // 4. Sanity check: verify no duplicate dealt indices across active players.
    for p1 in 0..MAX_PLAYERS {
        if p1 < num_players {
            for p2 in (p1 + 1)..MAX_PLAYERS {
                if p2 < num_players {
                    assert(
                        dealt_card1_indices[p1] != dealt_card1_indices[p2],
                        "same card dealt twice",
                    );
                    assert(
                        dealt_card1_indices[p1] != dealt_card2_indices[p2],
                        "same card dealt twice",
                    );
                    assert(
                        dealt_card2_indices[p1] != dealt_card1_indices[p2],
                        "same card dealt twice",
                    );
                    assert(
                        dealt_card2_indices[p1] != dealt_card2_indices[p2],
                        "same card dealt twice",
                    );
                }
            }
        }
    }

    (deck_root, hand_commitments, dealt_card1_indices, dealt_card2_indices)
}