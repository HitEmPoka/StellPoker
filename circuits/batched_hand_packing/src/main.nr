/// Batched hand public-input packing (Issue #529 prototype).
///
/// Takes the plain public inputs of up to `BATCH` hand proofs and proves that
/// they fold to `batch_digest` under `stellar_poker_lib::packing`. In the full
/// recursive design each slot additionally verifies the hand proof itself
/// against `pack_hand(...)` as its single public input; this prototype pins
/// the packing layer, which is the part that decides the on-chain byte cost.
/// The design and byte accounting are in docs/recursive-public-input-packing.md.
use stellar_poker_lib::packing;

/// Hands per aggregated settlement.
global BATCH: u32 = 8;
/// Public field elements per hand. 16 covers showdown_valid's 15 inputs and
/// leaves one slot for the winner bitmap an aggregated hand carries.
global INPUTS_PER_HAND: u32 = 16;

fn main(
    hand_ids: [Field; BATCH],
    circuit_ids: [Field; BATCH],
    public_inputs: [[Field; INPUTS_PER_HAND]; BATCH],
    num_hands: pub u32,
    batch_digest: pub Field,
) {
    let mut digests: [Field; BATCH] = [0; BATCH];
    for i in 0..BATCH {
        digests[i] = packing::pack_hand(hand_ids[i], circuit_ids[i], public_inputs[i]);
    }
    assert(packing::pack_batch(digests, num_hands) == batch_digest, "batch digest mismatch");
}

#[test]
fn test_three_hands_pack_to_the_expected_digest() {
    let mut hand_ids: [Field; BATCH] = [0; BATCH];
    let mut circuit_ids: [Field; BATCH] = [0; BATCH];
    let mut inputs: [[Field; INPUTS_PER_HAND]; BATCH] = [[0; INPUTS_PER_HAND]; BATCH];
    let mut digests: [Field; BATCH] = [0; BATCH];
    for h in 0..3 {
        hand_ids[h] = (h as Field) + 100;
        circuit_ids[h] = 3; // showdown
        for j in 0..INPUTS_PER_HAND {
            inputs[h][j] = ((h * INPUTS_PER_HAND + j) as Field) + 1;
        }
        digests[h] = packing::pack_hand(hand_ids[h], circuit_ids[h], inputs[h]);
    }
    let expected = packing::pack_batch(digests, 3);
    main(hand_ids, circuit_ids, inputs, 3, expected);
}

#[test(should_fail_with = "batch digest mismatch")]
fn test_wrong_digest_rejected() {
    let hand_ids: [Field; BATCH] = [0; BATCH];
    let circuit_ids: [Field; BATCH] = [0; BATCH];
    let inputs: [[Field; INPUTS_PER_HAND]; BATCH] = [[0; INPUTS_PER_HAND]; BATCH];
    main(hand_ids, circuit_ids, inputs, 1, 12345);
}
