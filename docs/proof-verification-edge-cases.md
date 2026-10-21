# UltraHonk Verifier Edge Cases - Audit & Testing

## Overview
This document describes edge cases tested in the UltraHonk proof verifier (issue #141) to ensure robust handling of malformed, manipulated, and boundary-condition proofs.

## Edge Cases Tested

### 1. Empty or Missing Inputs
**Scenario**: Proof verification called with empty public inputs.
- **Expected Behavior**: Rejection with `InvalidInput("public inputs must be 32-byte aligned")`
- **Rationale**: Public inputs form the bridge between the circuit and the verifier; empty inputs break this contract.
- **Test**: `proof_with_empty_inputs_should_fail`

### 2. Misaligned Public Inputs (Non-32-Byte Boundaries)
**Scenario**: Public inputs whose byte length is not a multiple of 32.
- **Expected Behavior**: Rejection with `InvalidInput("public inputs must be 32-byte aligned")`
- **Rationale**: Field elements in the proof system must align to field size (256-bit/32-byte).
- **Validation Location**: `verifier.rs:58-61`
- **Test**: `proof_with_misaligned_inputs_should_fail`

### 3. Truncated Proof Data
**Scenario**: Proof data that is shorter than expected by the circuit.
- **Expected Behavior**: Parsing or verification failure when accessing required proof elements.
- **Rationale**: Incomplete proof cannot satisfy any verification equation.
- **Test**: `proof_with_truncated_data_should_fail`

### 4. Manipulated Proof Elements
**Scenario**: Proof bytes deliberately modified at critical positions (e.g., commitments, evaluation points).
- **Expected Behavior**: Verification fails at sum-check or Shplonk verification stage.
- **Rationale**: The verifier performs polynomial checks that immediately detect tampering.
- **Vulnerable Points**:
  - Commitment elements (first ~256 bytes)
  - Evaluation point fields
- **Test**: `proof_with_manipulated_elements_should_fail`

### 5. Boundary Value Inputs (Max Field Values)
**Scenario**: Public inputs set to maximum representable field values (all 0xFF bytes).
- **Expected Behavior**: Safe rejection or computation that doesn't overflow.
- **Rationale**: Field arithmetic must handle maximum values without wrapping incorrectly.
- **Test**: `proof_with_boundary_value_inputs`

### 6. Public Input Count Mismatch
**Scenario**: Number of public inputs doesn't match the verification key's expected count.
- **Expected Behavior**: Rejection with `InvalidInput("public inputs mismatch")`
- **Validation Location**: `verifier.rs:64-71`
- **Rationale**: The circuit compiled into the VK expects a specific input set.
- **Test**: `verifier_rejects_wrong_public_input_count`

### 7. Invalid VK Bytes (Malformed Verification Key)
**Scenario**: Verification key that cannot be parsed from provided bytes.
- **Expected Behavior**: Rejection at VK initialization with `InvalidInput("vk parse error")`
- **Validation Location**: `verifier.rs:37-41`
- **Rationale**: Invalid VK cannot guide verification; fail fast.
- **Test**: `proof_with_empty_inputs_should_fail`

## Verification Pipeline Architecture

```
Input Validation → Proof Parsing → Transcript Generation → Sum-Check → Shplonk
      ↓                                                        ↓          ↓
   Edge Cases                                         Polynomial        Pairing
   Tests 2, 6, 7                                      Verification      Checks
                                                       Tests 3, 4, 5    Tests 1, 2
```

## Constraints on Verifier Behavior

1. **All field arithmetic is modular** (mod 2^256 - 89)
2. **No panics on invalid input** — all errors are `VerifyError` variants
3. **Deterministic verification** — same proof + inputs + VK always produce same result
4. **No side-channel leaks** — verification time should not depend on which step fails

## Testing Guidelines

When adding new circuits:
1. Generate valid (proof, public_inputs, vk) triples from the circuit
2. Create mutant versions by flipping bytes at positions [0, 255] and [len/2, len/2+255]
3. Verify all mutations are rejected
4. Test input count = (expected - 1) and (expected + 1)
5. Benchmark proof size and verification time — flag regressions

## Performance Notes

- Verification time: **~50-200ms** per proof (CPU + Ledger limit dependent)
- Proof size: **~4-12 KB** (varies by circuit degree)
- Sum-check is O(log degree), Shplonk is O(1) in constraint count

## See Also

- [UltraHonk Paper](https://eprint.iacr.org/2023/1402)
- `contracts/poker-table/src/verifier.rs` — integration point
- `vendor/ultrahonk-rust-verifier/src/verifier.rs` — implementation
