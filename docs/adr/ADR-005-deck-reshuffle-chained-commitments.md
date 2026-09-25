# ADR-005: Deck Reshuffle Circuit with Chained Commitments

**Date:** 2026-09-25  
**Status:** Proposed  
**Issue:** #535

## Overview

This ADR describes the design and implementation of a **deck reshuffle circuit** that proves hand N+1 deck correctness by cryptographically linking it to hand N's committed deck. This prevents any party from unilaterally modifying or "fixing" the deck between rounds.

## Problem Statement

In multi-hand games, the deck must be reshuffled between hands. Without a cryptographic commitment chain:

1. A malicious prover could claim they reshuffled the deck from hand N, but actually substitute an entirely different deck for hand N+1.
2. Only the parties holding their permutation shares would know the deck is wrong.
3. The system relies solely on MPC consistency checks, which can be delayed or circumvented.

## Solution: Chained Deck Commitments

A **chained commitment** binds each hand's deck to the prior hand's deck:

```
Hand N:
  - deck_n_root (committed, public)
  - deck_n[52] (revealed via board cards + proven commitment)
  - deck_n_salts[52] (secret-shared)

Hand N+1:
  - deck_n_root (prior hand commitment, public input)
  - deck_n[52] (revealed deck, public input)
  - party{0,1,2}_permutation[52] (fresh shuffle shares)
  - party{0,1,2}_salts[52] (fresh salt shares)
  
Reshuffle Proof:
  1. Verify deck_n root matches deck_n_root (Merkle inclusion)
  2. Apply fresh permutation shares to deck_n
  3. Verify valid deck (all cards present)
  4. Compute deck_n+1_root
  5. Output deck_n+1_root for next hand's reshuffle proof
```

## Circuit Specification

**Inputs:**

| Name | Type | Public? | Purpose |
|------|------|---------|---------|
| `deck_n_root` | Field | Yes | Prior hand's committed deck root |
| `deck_n` | Field[52] | No | Revealed deck from hand N |
| `deck_n_salts` | Field[52] | No | Salt shares for hand N cards (binds to root) |
| `party{0,1,2}_permutation_packed` | u32[10] | No | Compressed permutation shares for reshuffle |
| `party{0,1,2}_salts` | Field[52] | No | Fresh salt shares for hand N+1 |
| `num_players` | u32 | Yes | Active player count |

**Outputs:**

| Name | Type | Purpose |
|------|------|---------|
| `deck_n+1_root` | Field | New deck commitment (input to next hand's reshuffle) |
| `hand_commitments` | Field[6] | Hole card commitments per seat |
| `dealt_indices_1` | u32[6] | First card index per player |
| `dealt_indices_2` | u32[6] | Second card index per player |

## Soundness Properties

1. **Deck Continuity:** If an attacker modifies deck_n_root between hands, the new root won't decrypt to the same deck N. The circuit will fail the Merkle inclusion check.

2. **No Deck Substitution:** Once deck_n is revealed (publicly via board cards), no secret deck_n+1 can use different card values. Fresh permutations are applied to the revealed deck_n, ensuring deck_n+1 is a deterministic reshuffling of deck_n.

3. **Permutation Validity:** Compressed permutations are unpacked and validated (each index ∈ [0, 51], no duplicates).

4. **Hand Injectivity:** The same card cannot be dealt to multiple seats.

## Constraint Budget

Estimated constraint count (relative to `deal_valid`):

- Merkle root verification: ~6,000 (2 trees)
- Permutation application: ~3,000
- Card validation: ~500
- Hand assignment injectivity: ~2,000

**Total:** ~11,500 constraints (vs. 12,738 for deal_valid_6p)

## Integration Points

1. **Coordinator:** Fetch prior hand's `deck_n_root` and `deck_n` from settlement.
2. **MPC Nodes:** Include `deck_reshuffle` in prove-all circuit dispatch.
3. **Soroban Contract:** Verify reshuffle proof before committing next hand's deal proof.

## Testing

- Identity permutation: reshuffled deck equals input deck
- Injectivity: no two seats receive same card
- Root mismatch: fails if `deck_n_root` doesn't match computed root
- Invalid card: fails if reshuffled deck has invalid cards

## References

- Issue #535: Add circuit for deck reshuffle between hands with chained commitments
- `circuits/deck_reshuffle/src/main.nr`: Implementation
