/// Deterministic, well-typed witness fuzzer for `deal_valid` (Issue #888).
///
/// The generator produces complete deal witnesses using only legal Noir types
/// (no malformed byte encodings), then applies semantic mutations: duplicate
/// permutation entries, out-of-range card indices, duplicated cards in a
/// second party's permutation, out-of-range player counts and reordered salt
/// shares. Each generated case carries the expected validity. The boolean
/// validator below is the same predicate that `deal_valid` asserts through
/// `cards::is_valid_deck` / `shuffle::is_valid_permutation`, so a mismatch means
/// either the generator's expectation is wrong or a constraint has regressed.
///
/// `run_fuzz` is compiled into `circuits/deal_valid_fuzz` for nightly and
/// circuit-change jobs and is also exercised with a small deterministic batch
/// by the normal `nargo test` suite. Every case is derived from an integer
/// seed; a failing seed is printed and returned so CI logs are reproducible.

use crate::cards;
use crate::shuffle;

pub global DECK_SIZE: u32 = 52;
pub global MAX_PLAYERS: u32 = 6;

/// A complete, type-correct deal witness. `salt_order` models the per-position
/// salt assignment; reordering it changes commitments but not deck validity.
pub struct DealWitness {
    pub party0: [u32; DECK_SIZE],
    pub party1: [u32; DECK_SIZE],
    pub party2: [u32; DECK_SIZE],
    pub salt_order: [u32; DECK_SIZE],
    pub num_players: u32,
}

/// Mutation classes. 0 and `SALT_REORDER` are expected to remain valid; all
/// other classes must be rejected by the circuit's constraints.
pub global VALID_DEAL: u32 = 0;
pub global DUPLICATE_CARD: u32 = 1;
pub global OUT_OF_RANGE_RANK: u32 = 2;
pub global DUPLICATE_CARD_SECOND_PARTY: u32 = 3;
pub global INVALID_PLAYER_COUNT: u32 = 4;
pub global SALT_REORDER: u32 = 5;
pub global MUTATION_KINDS: u32 = 6;

/// xorshift32 state advance. Returns `(new_state, random_value)`.
pub fn next_u32(state: u32) -> (u32, u32) {
    let mut x = state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    (x, x)
}

/// Generate a valid permutation of `0..52` with Fisher-Yates.
pub fn generate_permutation(seed: u32) -> ([u32; DECK_SIZE], u32) {
    let mut perm: [u32; DECK_SIZE] = [0; DECK_SIZE];
    for i in 0..DECK_SIZE {
        perm[i] = i as u32;
    }
    let mut state = seed | 1;
    for i in 0..DECK_SIZE {
        let j = DECK_SIZE - 1 - i;
        let (next_state, value) = next_u32(state);
        state = next_state;
        let k = value % (j + 1);
        let tmp = perm[j];
        perm[j] = perm[k];
        perm[k] = tmp;
    }
    (perm, state)
}

/// Derive the composed deck exactly as `shuffle::derive_shared_deck_and_salts`
/// does for a canonical identity deck.
pub fn derive_deck(witness: DealWitness) -> [Field; DECK_SIZE] {
    let mut deck: [Field; DECK_SIZE] = [0; DECK_SIZE];
    for i in 0..DECK_SIZE {
        deck[i] = witness.party0[witness.party1[witness.party2[i]]] as Field;
    }
    deck
}

/// The exact validity predicate consumed by the deal circuit. It accepts only
/// well-formed permutations, a valid composed deck, and a player count whose
/// deterministic `2*p, 2*p+1` card assignment fits in the deck.
pub fn validate_deal(witness: DealWitness) -> bool {
    if (witness.num_players < 2) || (witness.num_players > MAX_PLAYERS) {
        return false;
    }
    if !shuffle::is_valid_permutation(witness.party0) {
        return false;
    }
    if !shuffle::is_valid_permutation(witness.party1) {
        return false;
    }
    if !shuffle::is_valid_permutation(witness.party2) {
        return false;
    }
    if !cards::is_valid_deck(derive_deck(witness)) {
        return false;
    }
    // Deterministic seat assignment is injective iff enough cards exist.
    if 2 * witness.num_players > DECK_SIZE {
        return false;
    }
    true
}

/// Build one deterministic case and its expected validity from `seed`.
pub fn generate_case(seed: u32) -> (DealWitness, bool) {
    let (party0, state0) = generate_permutation(seed);
    let (party1, state1) = generate_permutation(state0 ^ 0x9e3779b9);
    let (party2, state2) = generate_permutation(state1 ^ 0x85ebca6b);

    let mut salt_order: [u32; DECK_SIZE] = [0; DECK_SIZE];
    for i in 0..DECK_SIZE {
        salt_order[i] = i as u32;
    }

    let witness = DealWitness {
        party0: party0,
        party1: party1,
        party2: party2,
        salt_order: salt_order,
        num_players: 2 + (state2 % (MAX_PLAYERS - 1)),
    };

    let kind = seed % MUTATION_KINDS;
    apply_mutation(witness, kind, state2, kind == VALID_DEAL || kind == SALT_REORDER)
}

/// Apply a semantic mutation. `i` and `j` are always distinct so each invalid
/// class is guaranteed to produce an unsatisfiable witness.
fn apply_mutation(
    witness: DealWitness,
    kind: u32,
    state: u32,
    expected_valid: bool,
) -> (DealWitness, bool) {
    let mut w = witness;
    let i = state % DECK_SIZE;
    let j = (i + 1 + ((state >> 8) % (DECK_SIZE - 1))) % DECK_SIZE;

    if kind == DUPLICATE_CARD {
        w.party0[i] = w.party0[j];
    } else if kind == OUT_OF_RANGE_RANK {
        // Still a well-typed u32, but outside the 52-card canonical deck.
        w.party1[i] = DECK_SIZE + ((state >> 8) % 12);
    } else if kind == DUPLICATE_CARD_SECOND_PARTY {
        w.party2[i] = w.party2[j];
    } else if kind == INVALID_PLAYER_COUNT {
        w.num_players = MAX_PLAYERS + 1 + (state % 3);
    } else if kind == SALT_REORDER {
        let tmp = w.salt_order[i];
        w.salt_order[i] = w.salt_order[j];
        w.salt_order[j] = tmp;
    }

    (w, expected_valid)
}

/// Run `num_runs` deterministic cases. Returns `0` when every case matched its
/// expectation, otherwise `failing_seed + 1` (a non-zero, reproducible seed).
/// The bound is a normal runtime value: this compiles to an ACIR loop instead
/// of unrolling 10k copies of the generator.
pub fn run_fuzz(start_seed: u32, num_runs: u32) -> u32 {
    for i in 0..num_runs {
        let seed = start_seed + i;
        let (witness, expected_valid) = generate_case(seed);
        let actual = validate_deal(witness);
        if actual != expected_valid {
            std::println(f"deal_valid fuzz mismatch at seed {seed}");
            return seed + 1;
        }
    }
    0
}

#[test]
fn test_generated_valid_deals_are_accepted() {
    let (witness, expected) = generate_case(0);
    assert(expected);
    assert(validate_deal(witness));
}

#[test]
fn test_duplicate_card_is_rejected() {
    let (witness, expected) = generate_case(1);
    assert(!expected);
    assert(!validate_deal(witness));
}

#[test]
fn test_out_of_range_rank_is_rejected() {
    let (witness, expected) = generate_case(2);
    assert(!expected);
    assert(!validate_deal(witness));
}

#[test]
fn test_reordered_salts_remain_valid() {
    let (witness, expected) = generate_case(5);
    assert(expected);
    assert(validate_deal(witness));
}

#[test]
fn test_invalid_player_count_is_rejected() {
    let (witness, expected) = generate_case(4);
    assert(!expected);
    assert(!validate_deal(witness));
}

#[test]
fn test_deterministic_fuzz_batch() {
    assert(run_fuzz(0, 256) == 0);
}
