# Commit-Reveal Scheme for Player Actions

## Overview
This document describes the commit-reveal cryptographic scheme implemented in the poker-table contract (issue #142) to prevent action ordering leakage — a vulnerability where the order in which actions are revealed can give information about hand strength.

## Problem
In a traditional poker contract, players submit actions immediately:
```
Player A: Bet 100
Player B: (sees A bet 100) Raises to 300
Player C: Folds
```

An attacker observing the order can infer:
- If B raises after seeing A's bet → likely strong hand
- The timing and order correlate with hand strength

## Solution: Two-Phase Action Submission

### Phase 1: Commit
Players submit only a **cryptographic commitment** (hash) to their action:

```
Player A: Hash(action=Bet, amount=100, nonce=rand1)
Player B: Hash(action=Raise, amount=300, nonce=rand2)
Player C: Hash(action=Fold, amount=0, nonce=rand3)
```

**Properties:**
- No one can see the actual action from the hash
- All commits are posted simultaneously (or collected over a time window)
- Nonce ensures two identical actions have different hashes

### Phase 2: Reveal
After all commits are received, players reveal their actual action + nonce:

```
Player A: Reveals Bet(100), nonce=rand1
         → Contract verifies Hash(Bet, 100, rand1) == committed hash
Player B: Reveals Raise(300), nonce=rand2
         → Contract verifies Hash(Raise, 300, rand2) == committed hash
Player C: Reveals Fold(0), nonce=rand3
         → Contract verifies Hash(Fold, 0, rand3) == committed hash
```

Only after verification does the action take effect.

## Contract Interface

### `commit_action`
**Purpose:** Submit an action commitment.

```rust
pub fn commit_action(
    env: Env,
    table_id: u32,
    player: Address,
    action_hash: Bytes,         // Keccak256(action || amount || nonce)
    nonce_hash: Bytes,          // Keccak256(nonce)
) -> Result<(), PokerTableError>
```

**Validation:**
- Player must be seated at the table
- Table must be in an active betting phase (Preflop, Flop, Turn, River)
- `action_hash` must be exactly 32 bytes
- `nonce_hash` must be exactly 32 bytes
- Player cannot commit twice in the same phase

**Events:**
- `action_committed(seat, hand_number)` — commitment recorded

### `reveal_action`
**Purpose:** Reveal and process a previously committed action.

```rust
pub fn reveal_action(
    env: Env,
    table_id: u32,
    player: Address,
    seq: u32,                   // Monotonic sequence number
    action: Action,             // The actual action (Fold, Check, Call, Bet, Raise, AllIn)
    amount: i128,               // Bet/raise amount (0 for Fold/Check/Call)
    nonce: Bytes,               // The random nonce used in commit
) -> Result<(), PokerTableError>
```

**Validation:**
- Commitment must exist for this player
- Computed hash of (action, amount, nonce) must match stored hash
- Sequence number must be exactly last_seq + 1
- Action is processed only if hash verification succeeds

**Side Effects:**
- Commitment is deleted after successful verification
- Action sequence counter is incremented
- Action is processed (pot updated, turn changes, etc.)
- Storage TTL is extended

**Events:**
- `action_revealed(seat, hand_number)` — action revealed and processed

## Hash Computation

The Keccak256 hash is computed as follows:

```rust
hash = Keccak256(
    action_byte ||           // 0=Fold, 1=Check, 2=Call, 3=Bet, 4=Raise, 5=AllIn
    amount_le_bytes (8) ||   // Little-endian i128 amount (only for Bet/Raise)
    nonce_bytes              // Full nonce
)
```

### Example (Rust)

```rust
use soroban_sdk::crypto;

let nonce = Bytes::from_slice(&env, &[42u8; 32]);
let hash = commit_reveal::compute_action_hash(
    &env,
    &Action::Raise(500),
    500,
    &nonce,
);
```

### Example (JavaScript/TypeScript)

```typescript
import keccak256 from 'js-sha3';

function computeActionHash(action, amount, nonce) {
  let preimage = new Uint8Array(256);
  let idx = 0;

  // Action code
  const actionCodes = {
    fold: 0, check: 1, call: 2, bet: 3, raise: 4, allIn: 5
  };
  preimage[idx++] = actionCodes[action];

  // Amount (for Bet/Raise)
  if (action === 'bet' || action === 'raise') {
    const amountBuf = new BigInt64Array([BigInt(amount)]);
    for (let byte of new Uint8Array(amountBuf.buffer)) {
      preimage[idx++] = byte;
    }
  }

  // Nonce
  for (let i = 0; i < nonce.length; i++) {
    preimage[idx++] = nonce[i];
  }

  return keccak256(preimage.slice(0, idx));
}
```

## Workflow Example

**Setup:** Preflop, 3-player table, Player A to act.

### Commit Phase (120 seconds)

```
Time T+0s:  A submits: commit_action(hash_bet_100)
Time T+30s: B submits: commit_action(hash_raise_300)
Time T+60s: C submits: commit_action(hash_fold)
```

At T+120s, all commits are collected. No one knows who did what.

### Reveal Phase (120 seconds)

```
Time T+120s: A reveals: reveal_action(action=Bet(100), nonce=rand_a)
             → Contract verifies hash matches, processes Bet(100)
Time T+130s: B reveals: reveal_action(action=Raise(300), nonce=rand_b)
             → Contract verifies hash matches, processes Raise(300)
Time T+140s: C reveals: reveal_action(action=Fold, nonce=rand_c)
             → Contract verifies hash matches, marks C folded
```

Action order is now **locked in the blockchain**, same for all observers.

## Security Properties

### What This Protects Against

1. **Order-dependent fairness:** An attacker cannot exploit action reveal order to gain information.
2. **Replay attacks:** Sequence numbers prevent old reveals from replaying.
3. **Hash collision:** Keccak256 is cryptographically secure; preimage/collision attacks are computationally infeasible.

### What This Does NOT Protect Against

1. **Off-chain ordering:** If a player commits online but reveals offline, they reveal last.
2. **Timeout exploitation:** If a player never reveals, timeout rules still apply.
3. **Colluding players:** Two committed players can collude offline to choose coordinated nonces.

## Configuration and Deployment

### Backward Compatibility

- The commit-reveal scheme is **optional**. Players can continue using `player_action` for direct submission.
- Both paths are supported in parallel.
- A table can migrate to commit-reveal by coordinator policy (out of scope for this contract).

### Migration Path

1. **Phase 1:** Both `player_action` and commit-reveal methods are enabled.
2. **Phase 2:** High-stakes tables opt into commit-reveal via table configuration.
3. **Phase 3:** Tournament structure may mandate commit-reveal for final tables.

## Gas and Storage Costs

- **Commit:** ~500-700 gas (storage write + event)
- **Reveal:** ~2000-3000 gas (hash verification + action processing)
- **Storage per commitment:** 32 bytes (hash) + 32 bytes (nonce hash) = 64 bytes per seat per phase

For a 6-player table, one hand = ~384 bytes of temporary storage.

## Testing

See `contracts/poker-table/src/commit_reveal.rs` for unit tests:
- `test_action_hash_fold` — hash computation for fold action
- `test_action_hash_consistency` — same inputs produce same hash
- `test_action_hash_different_actions` — different actions have different hashes

## References

- [Commit-Reveal Scheme (Wikipedia)](https://en.wikipedia.org/wiki/Commitment_scheme)
- [Keccak256 (FIPS 202)](https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-185.pdf)
- [Soroban Cryptography](https://developers.stellar.org/docs/learn/stellar-core/cryptography)
