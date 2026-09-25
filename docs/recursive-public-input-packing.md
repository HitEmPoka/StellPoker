# Recursive-Friendly Public Input Packing for Batched Hands

Status: design with a working prototype. The packing layer is implemented in
`circuits/lib/src/packing.nr`, exercised by `circuits/batched_hand_packing`,
and cross-checked against a Rust implementation through the shared test
vectors (`circuits/test-vectors`). The recursive verification of the inner
hand proofs, which this layer is shaped for, is the follow-on step.

## Problem

Every hand proof publishes its public inputs as individual field elements.
Measured from the compiled ABIs (`scripts/bench_public_inputs.py`):

| Circuit              | Public fields | Bytes on the wire |
| -------------------- | ------------: | ----------------: |
| `deal_valid`         |            20 |               640 |
| `reveal_board_valid` |            25 |               800 |
| `showdown_valid`     |            29 |               928 |

A tournament settlement that aggregates `B` hands into one proof would, done
naively, expose all of those fields for every hand: `B x 928` bytes for
showdowns alone, before the 16 256-byte proof. Each of those fields is also
one more public input the on-chain verifier has to fold into its public-input
evaluation, and one more field the recursive verifier inside the aggregation
circuit has to carry between layers.

## Goal

An aggregated proof whose on-chain public inputs do not grow with `B`, using
only constructions the protocol already relies on, so that the coordinator can
recompute every digest from the plain hand data it already stores.

## Scheme

All hashes are the four-wide BN254 Poseidon2 permutation, first output, the
same primitive as every commitment in `commitments.nr`. `H2(a, b)` and
`H3(a, b, c)` are `poseidon2_hash_2` and `poseidon2_hash_3`.

```
pack_fields(domain, xs)        = fold(H2(domain, |xs|), xs, H2)
pack_hand(hand_id, cid, xs)    = H2(H3(DOMAIN_HAND, hand_id, cid), pack_fields(DOMAIN_INPUTS, xs))
pack_batch(digests, n)         = fold(H2(DOMAIN_BATCH, n), digests[0..n], H2)
```

- `DOMAIN_INPUTS`, `DOMAIN_HAND`, `DOMAIN_BATCH` are distinct ASCII tags, so a
  packed digest can never be mistaken for a card or hand commitment.
- Lengths are absorbed first. `[a, b]` and `[a, b, 0]` digest differently, a
  batch of three hands and the same three hands plus padding digest
  differently, and the hand count is bound into the batch digest.
- `hand_id` and `circuit_id` are bound into each hand digest, so two hands
  with identical public inputs, or the same inputs proven by a different
  circuit, still pack to different values.
- Padding slots past `n` are ignored, so one batch circuit sized for `B` hands
  settles any smaller batch without a second circuit.

The aggregated proof then exposes two public values, `batch_digest` and
`num_hands`, 64 bytes in total, whatever `B` is.

## Why this shape is recursive-friendly

In the full design each slot of the batch circuit verifies one hand proof with
`std::verify_proof`. What that costs depends heavily on how many public inputs
the inner proof has: each is a field the recursive verifier must hash into its
transcript and carry as part of the aggregation object. With packing, a hand
circuit exposes `pack_hand(...)` as its single public input and keeps the
plain inputs private; the batch circuit recomputes the same digest from the
plain inputs it is given and asserts equality. Every recursion layer therefore
carries exactly one field element per inner proof, and a second layer that
aggregates batches can reuse `pack_batch` over batch digests without any new
construction.

The prototype pins the part of that pipeline which decides the on-chain
bytes: given the plain inputs for up to `BATCH` hands, prove they fold to a
claimed `batch_digest`. Wiring the inner `verify_proof` calls into the same
slots is additive.

## Cost

`circuits/batched_hand_packing` with `BATCH = 8` and 16 fields per hand:

| Metric                       | Value  |
| ---------------------------- | -----: |
| ACIR opcodes                 |    201 |
| UltraHonk gates (`bb gates`) | 14 021 |
| Poseidon2 permutations       |    161 |
| Public fields                |      2 |
| Public-input bytes           |     64 |

Per hand the packing costs 20 permutations (1 for the header, 17 for the
length-prefixed fold of 16 inputs, 1 combining the two, 1 into the batch
chain), plus 1 to start the batch chain: 8 x 20 + 1 = 161. The packing layer
therefore scales at roughly 1 750 gates per hand, small next to the inner
proof verification it will sit beside.

On-chain bytes for a batch of `B` showdown hands, proof included:

| `B` | Naive: `B` proofs + inputs | Naive: one proof, all inputs exposed | Packed: one proof + 2 fields |
| --: | -------------------------: | -----------------------------------: | ---------------------------: |
|   1 |                     17 184 |                               17 184 |                       16 320 |
|   8 |                    137 472 |                               23 680 |                       16 320 |
|  64 |                  1 099 776 |                               75 648 |                       16 320 |

Proof size is the measured 16 256 bytes and public inputs 32 bytes per field,
from `scripts/bench_public_inputs.py`.

## What the contract sees

The settlement contract receives `batch_digest` and `num_hands` with the proof
and needs the per-hand data to act on the result (winners, pots). Two options,
both compatible with this scheme:

1. The coordinator submits the plain hand public inputs alongside the proof as
   contract call data (not proof public inputs), and the contract recomputes
   `pack_batch(pack_hand(...))` with its Poseidon2 host function before
   trusting them. This keeps the proof small and moves the per-hand cost to a
   hash the contract controls.
2. The coordinator submits only the digest and emits the per-hand data as
   events; off-chain consumers rebuild and check the digest. Cheapest on-chain,
   weakest for contracts that must act on hand outcomes directly.

Option 1 is the recommended default for settlement; option 2 fits audit-only
aggregation such as tournament history anchoring.

## Cross-implementation check

The Rust mirror in `circuits/test-vectors/src/lib.rs` (`pack_fields`,
`pack_hand`, `pack_batch`) generates the packing vectors in `vectors.json` and
the `test_vectors_packing` Noir test, so the coordinator can compute digests
with the same `taceo_poseidon2` build it already uses for commitments and the
circuits are tested against those exact values.

## Open points for the recursive step

- Inner proofs must be produced with the recursive UltraHonk flavour so the
  aggregation circuit can verify them; the hand circuits' public-input
  signatures then change to the single digest, which is a coordinated change
  across circuits, coordinator and the verifier contract.
- `MAX_BATCH_HANDS` (64) bounds `pack_batch`; the practical batch size will be
  set by the recursive verification cost per slot, not by packing.
- `circuit_id` values need a registry (deal = 1, reveal = 2, showdown = 3 is
  used in the vectors) once more than one circuit family is aggregated.
