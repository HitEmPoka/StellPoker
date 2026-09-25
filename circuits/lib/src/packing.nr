// Public-input packing for batched hand proofs (Issue #529).
//
// A hand proof exposes its public inputs one field element at a time, and a
// settlement that aggregates B hands would otherwise carry B copies of that
// list on-chain. This module folds each hand's public inputs into one digest
// and folds the digests into one batch digest, so an aggregated proof exposes
// two public values (the batch digest and the hand count) whatever B is.
//
// The scheme is a length-prefixed Poseidon2 chain, chosen so that:
//   - every layer of a recursive aggregation carries exactly one field
//     element forward, which keeps the inner proofs' public-input arrays at
//     the minimum the recursive verifier has to handle;
//   - it is replayable off-chain from the plain public inputs with the same
//     Poseidon2 the rest of the protocol already uses (the coordinator's
//     `taceo_poseidon2` build reproduces it; see circuits/test-vectors);
//   - the hand count and every list length are absorbed first, so a padded
//     batch, a truncated input list and a re-ordered batch all digest
//     differently.
//
// Layout (all hashes are the 4-wide Poseidon2 permutation, first output):
//   pack_fields(domain, xs)  = fold(H2(domain, |xs|), xs, H2)
//   pack_hand(id, cid, xs)   = H2(H3(DOMAIN_HAND, id, cid), pack_fields(DOMAIN_INPUTS, xs))
//   pack_batch(ds, n)        = fold(H2(DOMAIN_BATCH, n), ds[0..n], H2)
// Design notes and the on-chain byte accounting are in
// docs/recursive-public-input-packing.md.

use crate::commitments::{poseidon2_hash_2, poseidon2_hash_3};

// ASCII tags, so a digest cannot be confused with a card or hand commitment.
pub global DOMAIN_INPUTS: Field = 0x5350494e; // "SPIN"
pub global DOMAIN_HAND: Field = 0x53504844; // "SPHD"
pub global DOMAIN_BATCH: Field = 0x53504254; // "SPBT"

// Upper bound on hands per batch; one Poseidon2 permutation per slot.
pub global MAX_BATCH_HANDS: u32 = 64;

// Digest a fixed-length list under a domain tag. The length is absorbed first
// so `[a, b]` and `[a, b, 0]` never collide.
pub fn pack_fields<let N: u32>(domain: Field, fields: [Field; N]) -> Field {
    let mut acc = poseidon2_hash_2(domain, N as Field);
    for i in 0..N {
        acc = poseidon2_hash_2(acc, fields[i]);
    }
    acc
}

// Digest one hand's public inputs together with the hand id and the circuit
// that produced them, so two hands with identical inputs (or the same inputs
// proven by different circuits) still pack differently.
pub fn pack_hand<let N: u32>(hand_id: Field, circuit_id: Field, public_inputs: [Field; N]) -> Field {
    let header = poseidon2_hash_3(DOMAIN_HAND, hand_id, circuit_id);
    poseidon2_hash_2(header, pack_fields(DOMAIN_INPUTS, public_inputs))
}

// Fold the first `num_hands` hand digests into the batch digest. Slots past
// `num_hands` are ignored, so a batch circuit sized for B hands can settle
// fewer without changing the digest of the hands it does carry.
pub fn pack_batch<let B: u32>(hand_digests: [Field; B], num_hands: u32) -> Field {
    std::static_assert(B <= MAX_BATCH_HANDS, "batch exceeds MAX_BATCH_HANDS");
    assert(num_hands <= B, "num_hands exceeds batch capacity");
    let mut acc = poseidon2_hash_2(DOMAIN_BATCH, num_hands as Field);
    for i in 0..B {
        let next = poseidon2_hash_2(acc, hand_digests[i]);
        acc = if i < num_hands { next } else { acc };
    }
    acc
}

#[test]
fn test_pack_fields_is_length_prefixed() {
    let two = pack_fields(DOMAIN_INPUTS, [1, 2]);
    let three = pack_fields(DOMAIN_INPUTS, [1, 2, 0]);
    assert(two != three, "a trailing zero must change the digest");
    assert(two == pack_fields(DOMAIN_INPUTS, [1, 2]), "digest must be deterministic");
    assert(two != pack_fields(DOMAIN_HAND, [1, 2]), "domain must separate digests");
}

#[test]
fn test_pack_hand_binds_id_and_circuit() {
    let inputs = [5, 6, 7, 8];
    let base = pack_hand(1, 10, inputs);
    assert(base == pack_hand(1, 10, inputs));
    assert(base != pack_hand(2, 10, inputs), "hand id must be bound");
    assert(base != pack_hand(1, 11, inputs), "circuit id must be bound");
    assert(base != pack_hand(1, 10, [5, 6, 7, 9]), "inputs must be bound");
}

#[test]
fn test_pack_batch_ignores_padding_and_binds_order_and_count() {
    let a = pack_hand(1, 10, [1, 2]);
    let b = pack_hand(2, 10, [3, 4]);
    let full = pack_batch([a, b, 0, 0], 2);
    assert(full == pack_batch([a, b, 99, 98], 2), "slots past num_hands must not matter");
    assert(full != pack_batch([b, a, 0, 0], 2), "order must be bound");
    assert(full != pack_batch([a, b, 0, 0], 3), "hand count must be bound");
    assert(full != pack_batch([a, 0, 0, 0], 1), "dropping a hand must change the digest");
}

#[test(should_fail_with = "num_hands exceeds batch capacity")]
fn test_pack_batch_rejects_overfull_count() {
    let _ = pack_batch([1, 2], 3);
}
