/// Fold Validity Circuit - Cryptographic Burn of Folded Cards
///
/// Proves that folded players' hole cards are correctly removed from the active
/// player set and are cryptographically "burned" so they cannot be reused in
/// later betting rounds.
///
/// The circuit takes the public fold flags for every seat and, for each folded
/// seat, proves that the player actually held the cards they folded (by
/// recomputing their hand commitment from the shared deck) and then folds those
/// card commitments into an accumulating burn register. The burn register is
/// chained from the previous round's burn commitment and bound to the current
/// round number and a nonce, which:
///
///   - removes folded cards from the active set (the folded/active partition is
///     proven: number of active players == total players minus folded seats),
///   - produces an immutable, publicly-verifiable record of exactly which cards
///     were folded (the cryptographic burn), and
///   - prevents replay / reuse of a burned card set in a later round, because
///     the deck is derived once per hand and a seat is either folded or active,
///     never both, and the chained burn commitment is unique per round.
///
/// The coordinator stores `burn_commitment` and feeds it back as
/// `prev_burn_commitment` on the next fold proof, forming a tamper-evident
/// chain of every burned card across the hand.

use stellar_poker_lib::commitments;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

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
    num_active_players: pub u32,
    folded_flags: pub [u32; MAX_PLAYERS],
    fold_hand_commitments: pub [Field; MAX_PLAYERS],
    deck_root: pub Field,
    round: pub u32,
    burn_nonce: pub Field,
    prev_burn_commitment: pub Field,
) -> pub (Field, u32) {
    assert(num_active_players <= MAX_PLAYERS, "too many active players");

    let (deck, salts) = shuffle::derive_shared_deck_and_salts(
        party0_permutation,
        party1_permutation,
        party2_permutation,
        party0_salts,
        party1_salts,
        party2_salts,
    );

    // 1. Recompute deck root and verify consistency with the deal phase.
    let mut leaves: [Field; 64] = [0; 64];
    for i in 0..52 {
        leaves[i] = commitments::commit_card(deck[i], salts[i]);
    }
    let computed_root = merkle::compute_merkle_root(leaves);
    assert(computed_root == deck_root, "deck root mismatch");

    // 2. Partition folded vs active players and cryptographically burn the
    //    folded cards. The burn register is seeded from the previous round's
    //    burn commitment to form a chain across rounds.
    let mut burn: Field = prev_burn_commitment;
    let mut folded_count: u32 = 0;
    let mut active_count: u32 = 0;

    for p in 0..MAX_PLAYERS {
        let is_folded = folded_flags[p] == 1;
        if is_folded {
            // Derive this folded player's actual hole cards from the shared deck.
            let idx1 = p * 2;
            let idx2 = idx1 + 1;
            assert(idx2 < 52, "folded hole card index out of range");

            let c1 = deck[idx1];
            let c2 = deck[idx2];

            let c1_commit = commitments::commit_card(c1, salts[idx1]);
            let c2_commit = commitments::commit_card(c2, salts[idx2]);
            let hand_commit = commitments::commit_hand(c1_commit, c2_commit);

            // Prove the player genuinely held these cards when they folded.
            assert(
                hand_commit == fold_hand_commitments[p],
                "folded hand commitment mismatch",
            );

            // Cryptographic burn: fold this player's card commitments into the
            // running burn register.
            burn = std::hash::poseidon2_permutation([burn, c1_commit, c2_commit, 0], 4)[0];

            folded_count += 1;
        } else {
            active_count += 1;
        }
    }

    // 3. Prove folded seats are exactly the complement of the active set.
    assert(active_count == num_active_players, "folded/active partition mismatch");

    // 4. Finalize the burn commitment for this round. Binding `round` and
    //    `burn_nonce` makes each burn unique, so a burned card set cannot be
    //    replayed or reused in a later round.
    let burn_commitment = std::hash::poseidon2_permutation(
        [burn, round as Field, burn_nonce, 0],
        4,
    )[0];

    (burn_commitment, folded_count)
}
