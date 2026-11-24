/// Derive the final secret deck and salts from all three party contributions,
/// starting from an arbitrary canonical deck arrangement.
///
/// - The final deck is obtained by composing three private permutations over
///   the canonical deck.
/// - The final salt for each position is the sum of three private salt shares.
///
/// For a standard 52-card deck the canonical arrangement is the identity
/// (position i holds card value i). For a multi-deck shoe the canonical
/// arrangement repeats every (rank, suit) identity `num_decks` times, so the
/// same permutation machinery produces a correctly-distributed shoe.
pub fn derive_shared_deck_and_salts_from<let N: u32>(
    canonical_deck: [Field; N],
    party0_permutation: [u32; N],
    party1_permutation: [u32; N],
    party2_permutation: [u32; N],
    party0_salts: [Field; N],
    party1_salts: [Field; N],
    party2_salts: [Field; N],
) -> ([Field; N], [Field; N]) {
    assert_valid_permutation(party0_permutation);
    assert_valid_permutation(party1_permutation);
    assert_valid_permutation(party2_permutation);

    let deck_after_party0 = apply_permutation(canonical_deck, party0_permutation);
    let deck_after_party1 = apply_permutation(deck_after_party0, party1_permutation);
    let final_deck = apply_permutation(deck_after_party1, party2_permutation);

    let mut final_salts: [Field; N] = [0; N];
    for i in 0..N {
        final_salts[i] = party0_salts[i] + party1_salts[i] + party2_salts[i];
    }

    (final_deck, final_salts)
}

/// Derive the final secret deck and salts from all three party contributions
/// for a standard single 52-card deck (identity canonical arrangement).
pub fn derive_shared_deck_and_salts<let N: u32>(
    party0_permutation: [u32; N],
    party1_permutation: [u32; N],
    party2_permutation: [u32; N],
    party0_salts: [Field; N],
    party1_salts: [Field; N],
    party2_salts: [Field; N],
) -> ([Field; N], [Field; N]) {
    let mut canonical_deck: [Field; N] = [0; N];
    for i in 0..N {
        canonical_deck[i] = i as Field;
    }

    derive_shared_deck_and_salts_from(
        canonical_deck,
        party0_permutation,
        party1_permutation,
        party2_permutation,
        party0_salts,
        party1_salts,
        party2_salts,
    )
}

/// Boolean form of the permutation check used by the circuit and by the deal
/// fuzz harness. A `seen` array makes this O(N) while remaining exactly
/// equivalent to range-checking every index and asserting all values differ.
pub fn is_valid_permutation<let N: u32>(permutation: [u32; N]) -> bool {
    let mut seen: [bool; N] = [false; N];
    for i in 0..N {
        let index = permutation[i];
        if index >= N {
            return false;
        }
        if seen[index] {
            return false;
        }
        seen[index] = true;
    }
    true
}

pub fn assert_valid_permutation<let N: u32>(permutation: [u32; N]) {
    assert(is_valid_permutation(permutation), "permutation index out of range or duplicate");
}

#[test]
fn test_is_valid_permutation() {
    let mut perm: [u32; 52] = [0; 52];
    for i in 0..52 {
        perm[i] = i as u32;
    }
    assert(is_valid_permutation(perm));
    let mut duplicated = perm;
    duplicated[1] = duplicated[0];
    assert(!is_valid_permutation(duplicated));
    let mut out_of_range = perm;
    out_of_range[1] = 52;
    assert(!is_valid_permutation(out_of_range));
}

fn apply_permutation<let N: u32>(deck: [Field; N], permutation: [u32; N]) -> [Field; N] {
    let mut out: [Field; N] = [0; N];
    for i in 0..N {
        out[i] = deck[permutation[i]];
    }
    out
}
