use crate::cards;

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

fn assert_valid_permutation<let N: u32>(permutation: [u32; N]) {
    let mut as_fields: [Field; N] = [0; N];
    for i in 0..N {
        assert(permutation[i] < N, "permutation index out of range");
        as_fields[i] = permutation[i] as Field;
    }
    cards::assert_all_unique(as_fields);
}

fn apply_permutation<let N: u32>(deck: [Field; N], permutation: [u32; N]) -> [Field; N] {
    let mut out: [Field; N] = [0; N];
    for i in 0..N {
        out[i] = deck[permutation[i]];
    }
    out
}
