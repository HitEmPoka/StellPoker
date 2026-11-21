# Blind Structure Transitions (Issue #222)

## Circuit: `blind_level_valid`

Proves that the blind level applied to a hand matches the tournament schedule,
given a commitment to the full schedule and the current clock state.

### Public inputs
- `tournament_id`: which tournament this hand belongs to
- `hand_commitment`: commitment to the hand being validated
- `tournament_start`: tournament start time (seconds since epoch)
- `level_duration`: seconds per blind level
- `total_levels`: number of levels in the structure
- `structure_root`: Merkle root committing to the blind schedule
- `claimed_level`: the level index the coordinator applied
- `claimed_small_blind` / `claimed_big_blind`: the blinds actually charged

### Private inputs (not published)
- `hand_dealt_at`: exact time the hand was dealt (seconds)
- `paused_duration`: accumulated pause/break time (seconds) — excluded from blind clock

### How it works

1. **Clock validation**: `wall_elapsed = hand_dealt_at - tournament_start`
   - Paused time is excluded: `effective_elapsed = wall_elapsed - paused_duration`
   - Guard: `paused_duration <= wall_elapsed` (can't pause longer than tournament has run)

2. **Level derivation**: `raw_level = effective_elapsed / level_duration`
   - Clamped to `total_levels - 1` so indexing never goes past the committed structure

3. **Merkle proof**: The claimed `(level, small_blind, big_blind)` is authenticated
   via a 5-depth Merkle path against the committed `structure_root`. This means:
   - The full schedule need not be public inputs (saves proof size)
   - Only the single level being proven is verified via the Merkle path
   - `merkle_index` must equal `claimed_level` (prevents replay at wrong position)

4. **Blind consistency**:
   - `claimed_small_blind > 0`
   - `claimed_big_blind == claimed_small_blind * 2` (standard Hold'em ratio)

### Why a commitment rather than public inputs?

A tournament structure can have 20+ levels. Passing all as public inputs would
make proof size grow with the structure and every verifier pay for levels they
don't use. By committing to the schedule once as a Merkle root, the circuit
only proves the one level in question.

### Circuit constraint count: ~850

This is well within the budget of other circuits in the codebase (e.g.
`deal_valid` at 16,000, `reveal_board_valid` at 40,000).

### Usage in tournament flow

The coordinator calls `set_blinds` on each PokerTable between hands. Before
accepting the blind level, the contract (or off-chain verifier) runs the
`blind_level_valid` circuit to ensure:

- The hand was dealt at the correct clock position
- Paused time was properly excluded
- The blinds match the committed schedule
- The big blind is exactly twice the small blind

If the circuit rejects, the hand is flagged for review and the coordinator
must re-apply the correct blind level before proceeding.

### Tests

The circuit includes 28 tests covering:
- Level zero during first level
- Boundary transitions (exactly on vs. one second before)
- Pause handling (breaks hold level, longer pauses rejected)
- Final level clamping (cannot index past structure)
- Hand dealt before start rejected
- Malformed blind pairs (big != 2×small)
- Zero small blind rejected
- Blinds outside committed structure rejected
- Merkle index at wrong position rejected
- Zero level duration rejected
- Empty structure rejected
- Pause exactly equal to elapsed holds level zero

Run: `nargo test -p circuits/blind_level_valid`