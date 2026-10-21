# UltraHonk Verifier Public Input Layout (Issue #533)

**Status**: Stable ABI (v1.0)
**Last Updated**: 2026-09-25

## Overview

This document specifies the stable Application Binary Interface (ABI) for the UltraHonk verifier's public input layout used by all Stellar Poker circuits. The verifier consumes ACIR-compiled Noir circuit artifacts and produces proofs whose verification requires exact adherence to this layout.

## Motivation

The UltraHonk verifier is the backend prover/verifier for all Noir circuits in the Stellar Poker protocol. Public inputs (the circuit interface) must remain stable across protocol versions to ensure:

1. **Interoperability**: Contract verification gates recognize the same public input format
2. **Backward Compatibility**: Old proofs remain verifiable even after circuit updates
3. **Determinism**: Encoding is unambiguous and reproducible across implementations
4. **Auditability**: Stakeholders can verify the exact shape of each proof

This ABI freezes the serialization format for all circuits effective immediately.

## Public Input Encoding

### General Format

UltraHonk public inputs are a flat array of Bn254 field elements (each < 2^254).

Each circuit's public inputs appear in its `main` function signature, marked with the `pub` keyword:

```noir
fn main(
    // Private inputs (witness)
    private_input_1: Type1,
    ...

    // Public inputs
    num_players: pub u32,
    commitments: pub [u32; 6],
    deck_root: pub Field,
    ...
) -> pub OutputType { ... }
```

### Serialization Rules

1. **Field Elements** (`Field`): Encoded directly as a single field element.
2. **Unsigned Integers** (`u32`, `u16`, `u8`): Encoded as field elements (value < 2^32 for u32).
3. **Booleans** (`bool`): Encoded as field elements (1 for true, 0 for false).
4. **Arrays** (`[T; N]`): Flattened; each element follows its type's encoding rule.
5. **Tuples** (`(T1, T2, ...)`): Elements appear in order.

### Field Preservation

Field elements are NOT reduced modulo the circuit's characteristic during encoding. The verifier expects:
- Each field element is a valid Bn254 element (< 21888242871839275222246405745257275088548364400416034343698204186575808495617)
- No modular reduction occurs in the public input layer

## Circuit-Specific Layouts

### deal_valid

```
Public Inputs:
  num_players: u32 (1 element)
  deck_root: Field (1 element)
  committed_hash: Field (1 element)
```

**Total**: 3 field elements

### reveal_board_valid

```
Public Inputs:
  num_players: u32 (1 element)
  board_indices: [u32; 5] (5 elements)
  deck_root: Field (1 element)
```

**Total**: 7 field elements

### showdown_valid

```
Public Inputs:
  num_active_players: u32 (1 element)
  hand_commitments: [Field; 6] (6 elements)
  board_indices: [u32; 5] (5 elements)
  deck_root: Field (1 element)
  game_type: u32 (1 element)
  rank_table_root: Field (1 element)

Output: (hole_card1: [u32; 6], hole_card2: [u32; 6], winner_index: u32, tie_mask: u32)
        → 6 + 6 + 1 + 1 = 14 field elements (public output)
```

**Total Public Inputs**: 15 field elements

### fold_valid

```
Public Inputs:
  num_active_players: u32 (1 element)
  folded_flags: [u32; 6] (6 elements)
  fold_hand_commitments: [Field; 6] (6 elements)
  deck_root: Field (1 element)
  round: u32 (1 element)
  burn_nonce: Field (1 element)
  prev_burn_commitment: Field (1 element)

Output: (burn_commitment: Field, folded_count: u32)
        → 1 + 1 = 2 field elements (public output)
```

**Total Public Inputs**: 17 field elements

### muck_valid

```
Public Inputs:
  num_players: u32 (1 element)
  commitments: [u32; 6] (6 elements)
  all_in: [bool; 6] (6 elements, 0 or 1 each)
  folded: [bool; 6] (6 elements, 0 or 1 each)
  muck_commitment: Field (1 element)
```

**Total Public Inputs**: 20 field elements

### side_pot_valid

```
Public Inputs:
  num_players: u32 (1 element)
  commitments: [u32; 6] (6 elements)
  all_in: [bool; 6] (6 elements)
  folded: [bool; 6] (6 elements)
  num_pots: u32 (1 element)
  declared_amounts: [u32; 6] (6 elements)
  declared_eligible: [[bool; 6]; 6] (36 elements)
```

**Total Public Inputs**: 68 field elements

### split_pot_valid (New - Issue #532)

```
Public Inputs:
  num_players: u32 (1 element)
  commitments: [u32; 6] (6 elements)
  all_in: [bool; 6] (6 elements)
  folded: [bool; 6] (6 elements)
  num_pots: u32 (1 element)
  declared_amounts: [u32; 6] (6 elements)
  declared_eligible: [[bool; 6]; 6] (36 elements)

Output: (derived_count: u32, amounts: [u32; 6])
        → 1 + 6 = 7 field elements (public output)
```

**Total Public Inputs**: 68 field elements

## Bit-Width Guarantees

The circuit backend (UltraPlonk/UltraHonk) does NOT impose bit-width constraints on public inputs. Validators MUST enforce the following at the contract level:

| Field | Bit Width | Max Value |
|-------|-----------|-----------|
| `u32` | 32 | 4,294,967,295 |
| `u16` | 16 | 65,535 |
| `u8`  | 8  | 255 |
| `bool` | 1  | 1 |
| `Field` | 254 | 21,888,242,871,839,275,222,246,405,745,257,275,088,548,364,400,416,034,343,698,204,186,575,808,495,616 |

## Backward Compatibility

This ABI is frozen as of merge of Issue #533. Future changes MUST:

1. Be versioned (e.g., `ABI_VERSION = 2`)
2. Include a migration strategy (e.g., dual-path verifier)
3. Be approved via governance (not unilateral deployment)

## Implementation Notes

### In Smart Contracts (Solidity/Rust)

```solidity
// Verify the public inputs match the expected circuit output
function verifyShowdown(
    bytes calldata proof,
    uint256[15] calldata publicInputs  // 15 field elements per spec
) public {
    // Extract from publicInputs[0..14]:
    // [0]: num_active_players (u32)
    // [1..6]: hand_commitments (6× Field)
    // [7..11]: board_indices (5× u32)
    // [12]: deck_root (Field)
    // [13]: game_type (u32)
    // [14]: rank_table_root (Field)
    // + 14 public output elements (winner, tie mask, hole cards)
    
    require(publicInputs.length == 15, "wrong input count");
    
    bytes32 vk = SHOWDOWN_VERIFICATION_KEY;
    UltraHonkVerifier.verify(vk, proof, publicInputs);
}
```

### In Noir

```noir
use stellar_poker_lib::commitments;
use stellar_poker_lib::shuffle;

fn main(
    // Private: MPC deck shares
    party0_permutation: [u32; 52],
    party1_permutation: [u32; 52],
    party2_permutation: [u32; 52],
    party0_salts: [Field; 52],
    party1_salts: [Field; 52],
    party2_salts: [Field; 52],

    // PUBLIC: These are the verifiable commitment
    num_active_players: pub u32,
    hand_commitments: pub [Field; 6],
    board_indices: pub [u32; 5],
    deck_root: pub Field,
    game_type: pub u32,
    rank_table_root: pub Field,
) -> pub ([u32; 6], [u32; 6], u32, u32) {
    // Circuit logic
    let (hole_card1, hole_card2, winner_index, tie_mask) = showdown_eval(...);
    (hole_card1, hole_card2, winner_index, tie_mask)
}
```

The return type is automatically appended to the public inputs by the Noir compiler.

## Verification Checklist

Before deploying a new circuit version:

- [ ] All `pub` inputs/outputs listed in circuit layout above?
- [ ] Field widths match the spec (u32 < 2^32, etc.)?
- [ ] Array lengths match (e.g., `[bool; 6]` is 6 elements, not compressed)?
- [ ] Tuples flattened in source order?
- [ ] No custom serialization in circuit (rely on Noir's default)?
- [ ] Verifier gate count and constraint budgets stable under `nargo info`?
- [ ] Public input layout frozen in code review before merge?

## References

- [Noir Language Spec](https://docs.noir-lang.org) — public input encoding
- [UltraPlonk Spec](https://github.com/AztecProtocol/barretenberg) — backend verifier
- [Stellar Poker Circuits](../circuits/) — all circuit implementations
