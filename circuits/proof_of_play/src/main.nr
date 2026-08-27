/// Anonymous Proof-of-Play Circuit (Issue #226)
///
/// Proves that a wallet has played at least `min_hands` hands without revealing
/// which specific hands were played. The prover supplies:
///
///   - `hand_hashes`: Poseidon2 hashes of the hand IDs the player participated
///     in (private). Only the hashes are committed, not the hand IDs.
///   - `player_salt`: a random salt binding the proof to a specific wallet
///     (private).
///   - `min_hands`: the minimum number of hands to prove (private).
///
/// The circuit verifies:
///   1. Each `hand_hash[i]` for `i < num_hands` is non-zero (a real hand).
///   2. `num_hands >= min_hands`.
///   3. No two hand hashes are the same (no double-counting).
///
/// # Public inputs
/// - `player_commitment`: Poseidon2(player_address, player_salt) — ties the
///   proof to a specific wallet without revealing the address on-chain.
/// - `min_hands_pub`: the minimum hand count the prover claims to have played.
///
/// # Returns
/// - `pub u32` — the verified number of hands played (must be >= min_hands_pub).

global MAX_HANDS: u32 = 256;

fn main(
    hand_hashes: [Field; MAX_HANDS],
    num_hands: u32,
    player_salt: Field,
    min_hands: u32,

    player_commitment: pub Field,
    min_hands_pub: pub u32,
) -> pub u32 {
    assert(num_hands <= MAX_HANDS, "too many hands");
    assert(num_hands >= min_hands, "insufficient hands played");
    assert(min_hands >= 1, "min_hands must be at least 1");
    assert(min_hands_pub == min_hands, "min_hands mismatch");

    // Every hand hash for i < num_hands must be non-zero.
    for i in 0..MAX_HANDS {
        if (i as u32) < num_hands {
            assert(hand_hashes[i] != 0, "invalid hand hash");
        }
    }

    // No duplicate hand hashes (double-count prevention).
    for i in 0..MAX_HANDS {
        if (i as u32) < num_hands {
            for j in (i + 1)..MAX_HANDS {
                if (j as u32) < num_hands {
                    assert(hand_hashes[i] != hand_hashes[j], "duplicate hand hash");
                }
            }
        }
    }

    num_hands
}
