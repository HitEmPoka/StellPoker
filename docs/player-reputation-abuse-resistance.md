# Player Reputation Score & Abuse Resistance Specification (Issue #690)

## 1. Overview

StellPoker provides an **opt-in, privacy-preserving reputation score** implemented on-chain in `contracts/player-rating/`. 

While competitive ELO ratings (Issue #70) track relative skill and matchmaking, the reputation score quantifies **reliability, attendance, sportsmanship, and table etiquette**.

### Key Principles:
1. **Strictly Opt-In**: Players must explicitly invoke `opt_in_reputation` to activate scoring and public visibility. Calling `opt_out_reputation` immediately revokes visibility (`Err(RatingError::NotOptedIn)`).
2. **Strict Privacy Preservation**: Zero private hand contents, cards, or betting amounts are processed, stored, or emitted. Only publicly verified table outcomes (`hands_completed`, `hands_won`, and `timeouts`) contribute to scoring.
3. **Abuse & Collusion Resistance**: Designed to deter farming, Sybil attacks, stalling, and griefing.

---

## 2. On-Chain Data Model & Scoring Formula

### Data Structure (`PlayerReputation`)

```rust
pub struct PlayerReputation {
    pub address: Address,
    pub opted_in: bool,
    pub score: u32,             // 0 to 1000 composite score
    pub hands_completed: u32,   // Hands completed without timeout
    pub hands_won: u32,         // Fair hand victories
    pub timeouts: u32,          // Stalling / timeout infractions
    pub clean_streak: u32,      // Consecutive clean hands
    pub last_update_ledger: u32,
}
```

### Scoring Formula

```text
Score = Clamp(500 + CompletionBonus + WinBonus + StreakBonus - TimeoutPenalty, 0, 1000)
```

- **Base Score**: 500 (neutral reputation upon opting in).
- **Attendance & Completion Bonus**: `min(hands_completed * 2, 300)` (up to +300 points for consistent, complete participation).
- **Fair Victory Bonus**: `min(hands_won * 3, 150)` (up to +150 points for legitimate table success).
- **Reliability Clean Streak**: `min(clean_streak / 5, 50)` (up to +50 points for long streaks without stalling).
- **Timeout & Stalling Penalty**: `timeouts * 50` (-50 points per timeout infraction).

---

## 3. Privacy Guarantees

In competitive poker, leaking hole cards, fold frequencies on specific streets, or exact bet sizing allows predatory opponents to construct Head-Up Display (HUD) profiles and exploit playing tendencies.

### Privacy Safeguards:
1. **Zero Hand Content Exposure**:
   - The contract entrypoint `record_hand_reputation(caller, participants, winner_index, has_timeout, timed_out_index)` takes only player addresses and result indices.
   - Hand cards, community runouts, mucks, and chip pot values are never accepted or stored.
2. **Opt-Out Erasure / Concealment**:
   - Calling `opt_out_reputation` sets `opted_in: false`.
   - Any external query to `get_reputation` immediately fails with `RatingError::NotOptedIn`.
3. **Restricted Recorder Authorization**:
   - Only audited smart contracts (such as `poker-table`) or authorized coordinator accounts can record match outcomes.

---

## 4. Abuse Resistance Notes

### 4.1 Collusion & Self-Play Farming
- **Threat**: A player spins up Sybil accounts at an empty table and folds repeatedly to farm completion and win points.
- **Defenses**:
  - **Sublinear Capping**: Completion points are strictly capped at +300, and win points at +150. Beyond this cap, reputation gains plateau regardless of hand volume.
  - **Table Minimums**: PokerTable requires minimum 2 active funded players and charges blinds/rake per hand. Farming incurs real token friction.
  - **Anti-Cheat Monitoring**: Interacts with `poker-table/src/anti_cheat.rs` which monitors chip dumping and anomalous player pair frequencies.

### 4.2 Rage-Quitting, Stalling & Denial-of-Service
- **Threat**: Players who are about to lose intentionally disconnect or stall until the timeout ledger elapses to grief opponents.
- **Defenses**:
  - **Heavy Asymmetric Penalty**: A single timeout deducts **50 points**, wiping out the reputation equivalent of 25 completed hands.
  - **Streak Invalidation**: A timeout instantly resets `clean_streak` to 0, destroying any accumulated reliability streak bonuses.
  - **Auto-Fold & Exclusion**: Repeated timeouts trigger automatic standing-down from the table.

### 4.3 Sybil Reputation Cycling
- **Threat**: A user creates fresh burner addresses when their reputation drops after toxic behavior.
- **Defenses**:
  - **Default Unverified State**: Fresh addresses start at 500 with 0 hands completed. Tables can set reputation gate policies (e.g. "Require >600 reputation or >50 hands completed").
  - Burner accounts have no established streak or completion bonuses and cannot enter high-reputation or token-gated tables.

---

## 5. Verification & Testing

Unit tests for reputation tracking and abuse resistance are located in:
`contracts/player-rating/src/lib.rs`

Run the test suite:
```bash
cargo test -p player-rating
```
