# Standardized Hand History Format v1.0.0 (Issue #691)

## 1. Motivation

Third-party poker tools, hand tracking databases, equity calculators, and compliance auditors require a **stable, versioned, portable hand history standard**. 

While StellPoker preserves an authoritative on-chain circular buffer (`docs/poker-table-hand-history.md`), the **Standard Hand History JSON Schema (v1.0.0)** defines the universal interchange schema for exporting hands to external tools.

## 2. Schema Location & Identification

- **JSON Schema (Draft 2020-12)**: `schemas/hand-history/v1.json`
- **Schema ID**: `https://stellpoker.xyz/schemas/hand-history/v1.json`
- **Current Version**: `1.0.0`

## 3. Core Specification

### 3.1 Card Notation
Cards are represented as standard 2-character strings `[2-9TJQKA][cdhs]`:
- **Ranks**: `2`, `3`, `4`, `5`, `6`, `7`, `8`, `9`, `T` (10), `J`, `Q`, `K`, `A`.
- **Suits**: `c` (Clubs), `d` (Diamonds), `h` (Hearts), `s` (Spades).
- **Masked Cards**: `??` represents opponent hole cards that were not revealed at showdown or folded before showdown (preserving privacy).

### 3.2 Key Data Sections

1. **Table Metadata (`table`)**:
   - `id`: On-chain table sequence ID.
   - `variant`: Game type (`texas_holdem`, `omaha`, `short_deck`).
   - `betting_structure`: `no_limit`, `pot_limit`, or `fixed_limit`.
   - `stakes`: Small blind, big blind, ante, currency (USDC, XLM, or token contract).

2. **Hand Metadata (`hand`)**:
   - `number`: Monotonic hand number on the table.
   - `session_id`: Deterministic session identifier linked with Game Studio or table lifecycle.
   - `timestamp`: ISO-8601 UTC timestamp.
   - `dealer_seat`: Seat number of the dealer button.

3. **Players (`players`)**:
   - `seat`: Player position (0-8).
   - `address`: Stellar G-address.
   - `starting_stack` & `final_stack`.
   - `net_payout`: Net chips won or lost.
   - `hole_cards`: Revealed cards or masked `["??", "??"]`.

4. **Street Actions (`streets`)**:
   - Chronological breakdown by street (`preflop`, `flop`, `turn`, `river`).
   - Actions: `post_sb`, `post_bb`, `post_ante`, `fold`, `check`, `call`, `bet`, `raise`, `all_in`, `timeout`.

5. **Pots & Payouts (`pots`)**:
   - Main pot and side pots (`is_side_pot: true`).
   - Rake deducted.
   - Seat-resolved winners with respective chip amounts.

6. **Cryptographic Verifiability (`verifiability`)**:
   - `deck_root`: 32-byte Poseidon hash of shuffled deck.
   - `zk_proof_verified`: True if Noir UltraHonk proof verified on-chain.
   - `showdown_proof_hash`: Transaction or proof hash verifying winner correctness.
   - `settled_ledger`: Ledger sequence when hand was finalized.

---

## 4. Fixture Files

Reference example files matching various hand scenarios are located in `fixtures/hand-history/v1/`:

| Fixture File | Scenario |
|--------------|----------|
| `standard-heads-up-showdown.json` | 2 players, flop/turn/river runout, showdown two-pair vs pair |
| `multiway-all-in-side-pot.json` | 3 players, short-stack all-in preflop with main and side pot |
| `preflop-fold.json` | Small blind folds preflop; no community cards dealt |
| `timeout-fold.json` | Player action times out on the flop resulting in an auto-fold |

---

## 5. Exporter Usage (TypeScript)

The exporter library is located at `app/src/lib/standard-hand-history.ts`:

```typescript
import {
  createStandardHandHistory,
  validateStandardHandHistory,
  exportStandardHandHistoryJson,
} from "@/lib/standard-hand-history";

// Convert internal browser hand entry to standardized schema
const standardHand = createStandardHandHistory(rawEntry, "stellar-testnet");

// Validate against schema constraints
const { valid, errors } = validateStandardHandHistory(standardHand);
if (valid) {
  const jsonExport = exportStandardHandHistoryJson(standardHand);
  // Download or send to third-party tool
}
```
