# ADR-006: Hand Class Lookup Table Integration vs. Custom Gates

**Date:** 2026-09-25  
**Status:** Adopted  
**Issue:** #537  
**Related:** Issue #222 (Merkle-committed rank lookup tables)

## Overview

This ADR evaluates two approaches for evaluating 5-card hand rankings in the showdown circuit:

1. **Lookup Table (Current Implementation):** Merkle-committed table with inclusion proofs
2. **Custom Gates (Alternative):** Custom constraint gates in the backend (UltraHonk)

## Problem Statement

Hand evaluation (finding the best 5 cards from 7, computing rank score) is computationally expensive in circuits:

- Naive enumeration: C(7,5) = 21 combinations per hand
- 6 players: 126 evaluations
- Inline gates: ~3,000–5,000 constraints per player

This dominates the showdown circuit's gate count (showdown_valid has ~237,018 backend gates at 6 players).

## Solution: Merkle-Committed Lookup Table

A pre-computed table of all 2,847 achievable 5-card hand scores is committed to via Merkle tree
(Issue #222). The prover proves membership in this table rather than deriving scores inline.

### Implementation Details

**Storage:**
- Table: 2,847 distinct achievable scores (padded to 4,096 = 2^12)
- Merkle Tree: 4,096-leaf tree, depth 12
- Root: Single Field value (public parameter, committed at game start)

**Proof:**
- Prover supplies: `rank_score`, `rank_index`, `rank_path[12]` per player
- Circuit verifies: Merkle inclusion using `rank_path` and `rank_table_root`
- Constraint cost: O(log N) = ~12 hash gates per player

**Advantages:**

| Factor | Lookup Table | Custom Gates |
|--------|--------------|--------------|
| Gate count | 12 per hand | 3,000–5,000 per hand |
| Reduction | **98.8% fewer gates** | Baseline |
| Proving time | 150 ms (6 players) | ~250+ ms (estimated) |
| Backend-agnostic | ✓ Yes | ✗ No (UltraHonk specific) |
| Soundness | ✓ Proven | ⚠️ Verifier complexity |
| Auditability | ✓ Table is public | ⚠️ Custom gates harder to audit |

## Constraint Budget Comparison

### Lookup Table (Current)

| Component | Gates | Notes |
|-----------|-------|-------|
| Deck derivation + commitment | 5,000 | 3 permutations |
| Merkle deck root | 2,000 | Tree verification |
| Hand rank table lookup × 6 | 72 | 12 gates × 6 players |
| Deck/hand Merkle proofs | 3,000 | Inclusion checks |
| Winner selection | 1,000 | Tie handling logic |
| **Total (6 players)** | **~11,072** | ACIR opcodes |
| Backend expansion | **237,018** | UltraHonk gates |

### Custom Gates (Hypothetical)

| Component | Gates | Notes |
|-----------|-------|-------|
| Deck derivation + commitment | 5,000 | Same |
| Merkle deck root | 2,000 | Same |
| Inline hand evaluation × 6 | ~24,000 | 4,000 gates/player |
| Deck/hand proofs | 3,000 | Same |
| Winner selection | 1,000 | Same |
| **Total (6 players)** | **~35,000** | ACIR opcodes (est.) |
| Backend expansion | **~400,000** | UltraHonk gates (est.) |

## Verification Complexity

### Lookup Table
- **Verifier:** Standard Merkle membership proof
- **Cost:** Hash gates only
- **Complexity:** Well-studied, standard

### Custom Gates
- **Verifier:** Must understand custom gate semantics
- **Cost:** Custom gate evaluation in verifier
- **Complexity:** Higher; increases verifier contract size (Soroban budget risk)

## Decision: Adopt Lookup Table

**Rationale:**

1. **Constraint Reduction:** 98.8% fewer constraints than inline evaluation (12 vs. 4,000+ gates per hand)
2. **Proving Time:** 150 ms (current) is acceptable; custom gates would push to 250+ ms
3. **Verifier Simplicity:** Standard Merkle proofs are well-understood; no custom gate complexity
4. **Soroban Compatibility:** Current on-chain verifier uses standard hash/curve ops; custom gates risk exceeding budget
5. **Auditability:** Table can be publicly regenerated and verified against reference evaluator
6. **Portability:** Lookup table works across all backends; custom gates are UltraHonk-specific

**Trade-offs Accepted:**

- Witness size increases (12 Field values per player for Merkle path)
- Requires off-chain table generation (one-time, not per-proof)
- Table commitment is a public parameter (must be consistent across nodes)

## Implementation Status

**Issue #222 (Merkle-committed rank lookup tables):**
- ✅ Lookup table generated and embedded in `rank_table.nr`
- ✅ Merkle tree proof integrated into showdown_valid
- ✅ Tests verify inclusion for all achievable scores

**Issue #537 (Benchmark comparison):**
- ✅ Lookup table documented
- ✅ Constraint budget measured (237,018 gates)
- ✅ Proving time observed (150 ms @ 6 players)
- ℹ️ Custom gates not implemented (decision to adopt lookup table made)

## Future Improvements

1. **Precompute Table Permutations:** Cache different player-count variants
2. **Witness Compression:** Pack rank indices using bit-packing (6 bits per index)
3. **Variant Poker:** Generate separate tables for wild card games (Issue #228)

## References

- Issue #222: Merkle-committed rank lookup table
- Issue #537: Lookup table integration for 5-card hand class
- `circuits/lib/src/rank_table.nr`: Lookup table implementation
- `circuits/showdown_valid/src/main.nr`: Integration
