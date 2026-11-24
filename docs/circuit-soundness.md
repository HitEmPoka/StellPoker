# Circuit Soundness Documentation

This document provides a formal soundness argument for each ZK circuit in the
StellPoker system. Each section covers the property being proved, the public and
private inputs, the adversarial model, key assumptions, and a completeness
argument.

All circuits are written in Noir and compiled to UltraHonk constraint systems.
The underlying proof system is assumed to be knowledge-sound and zero-knowledge
under standard cryptographic assumptions (discrete-log hardness over BN254,
collision-resistance of Poseidon2).

---

## 1. `deal_valid` — Deck Deal Validity

### What the circuit proves

A prover who knows three parties' private permutation and salt shares can
convince a verifier that:

1. The derived deck is a valid permutation of the 52-card canonical deck (each
   card value in `[0, 51]` appears exactly once).
2. Each active player's hole cards are bound to a Merkle commitment of the
   deck, preventing later substitution.
3. No two players received the same card index.

### Public inputs / outputs

| Signal | Direction | Meaning |
|---|---|---|
| `num_players` | public input | Number of active seats (2–6) |
| `deck_root` | public output | Merkle root over all 52 card commitments |
| `hand_commitments[p]` | public output | `H(commit(c1_p), commit(c2_p))` per player |
| `dealt_card1_indices[p]` | public output | Deck index of first hole card |
| `dealt_card2_indices[p]` | public output | Deck index of second hole card |

### Private inputs

Three parties each provide a permutation over `{0…51}` and 52 salt scalars.
These are secret-shared in MPC and never appear in plaintext outside the circuit.

### Adversarial model

A malicious prover controls all three party inputs. They cannot:

- Produce a valid proof for a deck that is not a permutation of `{0…51}`,
  because `assert_valid_deck` checks range and uniqueness for all 52 positions.
- Assign the same card to two players, because the circuit's explicit pairwise
  index-uniqueness check (`dealt_card1_indices[p1] != dealt_card1_indices[p2]`,
  etc.) constrains this.
- Swap a player's cards after the fact, because the `deck_root` is derived
  in-circuit from the same permutation, and `hand_commitments[p]` binds each
  player's pair to that root.

### Assumptions

- Poseidon2 is collision-resistant over BN254: two different `(card, salt)` pairs
  cannot produce the same commitment.
- The proof system (UltraHonk) is knowledge-sound: a prover cannot produce a
  valid proof without a satisfying witness.
- MPC secret sharing ensures that no single node sees all three permutation
  shares in plaintext.

### Completeness

An honest prover who holds valid permutation and salt shares for all three
parties can always satisfy every constraint:

- `assert_valid_permutation` passes because each party's share is a valid
  bijection over `{0…51}`, and their composition is also a bijection.
- `assert_valid_deck` passes because applying three bijections to the canonical
  deck yields another bijection.
- Index-uniqueness holds because the dealt indices are `0, 1, 2, 3, …` — a
  fixed deterministic assignment with no overlap.
- The Merkle root and all commitments are computed deterministically from the
  deck and salts, so they always match.

---

## 2. `reveal_board_valid` — Board Card Reveal

### What the circuit proves

Given the same deck root committed at deal time, the prover demonstrates that:

1. The deck is consistent with the one used at deal: the recomputed Merkle root
   matches `deck_root`.
2. The indices chosen for board cards do not overlap any previously used index
   (hole card indices).
3. The revealed card values are authentic (in range) and correspond to those
   deck positions.

### Public inputs / outputs

| Signal | Direction | Meaning |
|---|---|---|
| `deck_root` | public input | Root from the deal phase |
| `num_revealed` | public input | How many cards to reveal (1–3) |
| `num_previously_used` | public input | Count of already-used indices |
| `previously_used_indices` | public input | Array of used deck positions |
| `revealed_cards[i]` | public output | Plaintext card value for each position |
| `revealed_indices[i]` | public output | Deck position of each revealed card |

### Private inputs

Same three-party permutation and salt shares as the deal circuit.

### Adversarial model

A malicious prover cannot:

- Reveal a card that was already dealt to a player, because the `used[]` boolean
  array is populated from `previously_used_indices` and the selection loop
  skips any `used[idx] == true` position.
- Reveal a card with a different value than what was committed at deal time,
  because the Merkle root recomputation (`computed_root == deck_root`) ties the
  revealed `deck[idx]` value to the same commitment structure.
- Claim a different number of revealed cards, because `found == num_revealed`
  is asserted after the selection loop.

### Assumptions

Same as `deal_valid`. Additionally:

- The `previously_used_indices` provided as public input are correct — this is
  enforced by the contract, which tracks used indices on-chain.

### Completeness

An honest prover re-derives the deck identically to the deal phase. The Merkle
root check passes by construction. The `used[]` array correctly marks all
previously dealt positions, and the ascending index scan always finds
`num_revealed` free positions as long as fewer than 52 cards have been used
(guaranteed by the contract's game-state machine).

---

## 3. `showdown_valid` — Hand Showdown

### What the circuit proves

At showdown the prover demonstrates that:

1. The deck is consistent with all prior phases (Merkle root match).
2. Each active player's hole cards match the hand commitments published at deal
   time.
3. The hand evaluation is correct: `winner_index` holds the seat with the
   maximum 7-card hand score, and `tie_mask` marks every seat that tied.

### Public inputs / outputs

| Signal | Direction | Meaning |
|---|---|---|
| `num_active_players` | public input | Players remaining at showdown |
| `hand_commitments[p]` | public input | Commitments from deal phase |
| `board_indices[5]` | public input | Five board card positions |
| `deck_root` | public input | Root from deal phase |
| `hole_card1[p]`, `hole_card2[p]` | public output | Revealed hole cards |
| `winner_index` | public output | Winning seat index |
| `tie_mask` | public output | Bitmask of tied winners |

### Private inputs

Same three-party permutation and salt shares.

### Adversarial model

A malicious prover cannot:

- Assign different hole cards to a player than were committed at deal: the
  circuit recomputes `H(commit(deck[idx1], salt[idx1]), commit(deck[idx2], salt[idx2]))`
  and asserts it equals `hand_commitments[p]`.
- Declare a winner with a lower-ranked hand: the circuit iterates all active
  players and asserts `winner_score >= hand_scores[p]` for every seat.
- Use an inconsistent deck: the Merkle root recomputation gates all card
  resolution.
- Use duplicate card indices across hole cards and board cards: the `used_indices`
  boolean array prevents any index appearing twice.

### Assumptions

Same as the other circuits. Additionally:

- `evaluate_hand_rank` correctly implements standard 7-card poker hand ranking.
  This function is independently tested in `circuits/lib/src/cards.nr`. Any
  implementation error would be a completeness or correctness bug, not a
  soundness flaw (a cheating prover still cannot produce a false winner
  commitment without also breaking commitment binding).

### Completeness

An honest prover holds all party shares and can reconstruct the exact deck used
in prior phases. All commitment checks pass by construction. Hand evaluation is
deterministic over the derived card values, so the winner and tie mask are
uniquely determined and always satisfiable.

---

## 4. Chip numeric domain (Issue #889)

### Domain

All chip amounts are unsigned 64-bit integers:

```
0 <= amount <= 2^64 - 1 = 18_446_744_073_709_551_615
```

Noir stores witness values as field elements, so a bare `Field` amount has no
integer bound: `2^64`, `p - 1`, or any other field-wrapped value can satisfy an
equality that only holds modulo the BN254 scalar field. The shared module
`circuits/lib/src/chips.nr` performs a 64-bit big-endian decomposition and
reconstruction (`assert_chip_amount`), which is unsatisfiable outside the
domain because `2^64 << p`.

### Circuits covered

| Circuit | Amounts checked |
|---|---|
| `pot_calculation` | bets, side pots, rake, public pot total, aggregate bets |
| `chip_consistency` | stacks, pot, rake, buy-ins (private and public), aggregate |
| `side_pot_valid`, `split_pot_valid`, `odd_chip_valid` | `u32` amounts are already contained in the 64-bit domain |

`showdown_valid` does not consume chip amounts; its state is bound to the
committed deck and hand scores only. It is therefore unaffected by this
domain constraint.

### Boundary tests

`chips.nr`, `pot_calculation` and `chip_consistency` include explicit tests for
zero, `2^64 - 1`, `2^64`, field-wrapped values and aggregates that exceed the
domain. Values above the maximum must abort with
`chip amount exceeds 64-bit chip domain`.

### Constraint impact

Each `assert_chip_amount` adds a 64-bit witness decomposition. The
`pot_calculation` and `chip_consistency` circuits were added to
`scripts/circuit_constraints.py`'s default set and to
`circuits/constraint-budgets.json`; CI publishes the ACIR and backend gate
counts so the per-circuit overhead is measured on every circuit change. Keep
the published totals below `2^64` — a sum of several maximal amounts is a new
aggregate that is itself range-checked.

---

## 5. Deal witness fuzzing (Issue #888)

`deal_valid` accepts a private packed deck, three permutation shares and a
VRF output bound to its public inputs. Encode-layer corruption is rejected by
the ABI, but semantic corruption — duplicate permutations, out-of-range card
indices, and reordered salt shares — must be rejected by the circuit's own
constraints.

`circuits/lib/src/deal_fuzz.nr` provides a deterministic well-typed generator
and an O(N) validity predicate that mirrors those constraints. Seven mutation
classes are exercised:

| Seed class | Mutation | Expected |
|---|---|---|
| 0 | none | valid |
| 1 | duplicate entry in party 0 permutation | rejected |
| 2 | out-of-range canonical card index in party 1 | rejected |
| 3 | duplicate entry in party 2 permutation | rejected |
| 4 | player count above 6 | rejected |
| 5 | reordered salt assignment | valid |
| 6+ | repeat cycle with new PRNG state | deterministic |

`circuits/deal_valid_fuzz` executes 10,000 cases (`FUZZ_RUNS`). The job
`.github/workflows/circuit-benchmarks.yml#deal-valid-fuzz` runs on every
circuit change, on PRs and nightly via `scripts/fuzz-deal-valid.sh`. On any
mismatch the circuit prints `deal_valid fuzz mismatch at seed <N>` and returns
`N + 1`; CI fails and the seed can be replayed locally with the same generator.
The harness targets witness validation (permutations/deck/player count); the
VRF binding is covered by `deal_valid`'s dedicated tests because its equality
constraint is independent of witness shape.

The fuzz harness itself is not a proving circuit used in production — it is a
test circuit with a non-zero failure return value — so it is excluded from the
VK registry and constraint budgets.

---

## 6. Wallet action authorization (Issue #891)

`action_order` now requires a valid P-256 signature over a Blake2s message
binding `chain_id`, `table_id`, `hand_commitment`, `action`, `amount`, `nonce`
and `expiry`. `circuits/lib/src/wallet_auth.nr` exposes `action_message`,
`is_authorized_wallet` and `assert_authorized_wallet`; the calling circuit must
bind the supplied public key to the acting seat's wallet identity. Invalid
signatures abort with `invalid wallet authorization`, and negative tests cover
both bad signatures and expired/zero-nonce authorizations.

Replay protection: the nonce must be unique per hand/action, expiry is checked
against the proof's `now`, and every other field is part of the signed digest,
so a signature cannot be moved to another table, hand, action or amount. See
[the wallet-auth design](wallet-auth-circuit.md).

Constraint overhead is measured by adding `action_order` to
`scripts/circuit_constraints.py` and `circuits/constraint-budgets.json`; CI
publishes ACIR opcode and backend gate deltas on every circuit change.

---

## Peer Review Status

| Circuit | Internal review | External review |
|---|---|---|
| `deal_valid` | ✅ complete | Pending |
| `reveal_board_valid` | ✅ complete | Pending |
| `showdown_valid` | ✅ complete | Pending |

External peer review of this document and the circuit constraints is tracked in
issue [#312](https://github.com/HitEmPoka/StellPoker/issues/312). To submit a
review, open a PR editing this file under the reviewer's name and findings in
a new section, or leave a detailed comment on the issue.
