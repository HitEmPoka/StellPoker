# ADR-004: Poseidon2 parameters for BN254 circuits

## Status

Accepted

## Decision

All circuit commitments use Noir's built-in BN254 Poseidon2 permutation with a
four-field state (`t = 4`) and the round constants supplied by the pinned Noir
toolchain. Two- and three-value commitments are domain-padded to four fields.
Circuits must call `poseidon2_hash_2` or `poseidon2_hash_3` from
`circuits/lib/src/commitments.nr`; they must not carry custom round constants.

This is the cheapest sound option that is also compatible with the Soroban
Protocol 25 host function used by the UltraHonk verifier. Choosing custom
widths or reducing rounds would require in-circuit emulation, increase the
constraint count, and lose the audited built-in parameter set.

## Parameter and verification-cost comparison

The table records one permutation used to absorb up to three application
fields. Constraint figures are the backend behavior of the pinned built-in or
the conservative cost of emulating a non-native alternative. Verification
budget is relative because UltraHonk verification is primarily a function of
the final circuit domain size, not the hash API call alone.

| Candidate | State / capacity | Round policy | Constraints per hash | Soroban verification budget | Decision |
| --- | --- | --- | ---: | --- | --- |
| Built-in Poseidon2 BN254 | `t=4`, one padded field | Noir audited constants | ~30 | Baseline; native-compatible | Selected |
| Two `t=3` permutations | one capacity field each | custom constants | >=60 | Higher circuit domain | Rejected |
| Custom reduced-round `t=4` | one capacity field | fewer than built-in | <30 | Lower, but unsound/unreviewed | Rejected |
| Pedersen commitment | curve point output | backend default | >100 | Larger public data/domain | Rejected |

`~30` is the repository's measured source-to-backend estimate; exact totals
must be refreshed with `nargo info` and `bb gates` after toolchain upgrades.
The full-circuit baseline and Soroban transaction budget are tracked in
`circuits/BENCHMARKS.md`.

## Consequences

- The state width and constants cannot silently diverge between circuits.
- A toolchain upgrade that changes the built-in parameters requires new VK
  content hashes and a new registry version.
- Adding a fourth application value requires a second permutation or a
  documented domain-separation construction; it may not consume the capacity
  slot.
