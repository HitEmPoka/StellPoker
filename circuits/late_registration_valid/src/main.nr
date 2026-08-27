/// Late Registration Circuit (Issue #223)
///
/// Proves that a player entered a tournament inside the late-registration
/// window and was seated with the correct reduced stack for their entry time.
///
/// The manipulation this closes is a late entrant claiming a full starting
/// stack. Registration is off-chain — the coordinator seats the player — so
/// without a proof the only record that a player entered at level 6 rather
/// than level 1 is the coordinator's own word. A player colluding with (or
/// compromising) the coordinator could buy in an hour late and still receive
/// the chips of someone who has been paying blinds since the start.
///
/// The circuit verifies:
///
///   1. `registration_time > tournament_start` — otherwise this is a normal
///      entry, not a late one, and belongs in the standard registration path.
///   2. `registration_time <= late_reg_close` — the window is genuinely open.
///   3. The awarded stack matches the schedule for the number of blind levels
///      elapsed at `registration_time`.
///   4. The awarded stack is never larger than the starting stack, and never
///      zero — a stack of zero would seat a player who is already busted.
///
/// # Stack reduction model
///
/// A late entrant receives the starting stack less the blinds a player seated
/// from the start would have posted over the same period. That is the fair
/// comparison: it leaves the late entrant with the chips a punctual player
/// would have left, rather than an arbitrary discount.
///
/// The deduction is `levels_elapsed * reduction_per_level`, floored at
/// `min_stack` so a very late entry still receives a playable stack rather
/// than underflowing. `reduction_per_level` and `min_stack` come from the
/// tournament structure and are public, so anyone can recompute the result.
///
/// All arithmetic is u64 over seconds and chip counts. Noir's u64 subtraction
/// wraps on underflow rather than trapping, so every subtraction below is
/// guarded by an explicit comparison first.
///
/// # Public inputs
/// - `tournament_id`: identifies which tournament's structure applies
/// - `player_commitment`: Poseidon2 commitment to the player's identity
/// - `tournament_start`: tournament start time (seconds)
/// - `late_reg_close`: last instant a late entry is accepted (seconds)
/// - `level_duration`: seconds per blind level
/// - `starting_stack`: chips a player seated at the start received
/// - `reduction_per_level`: chips deducted per elapsed blind level
/// - `min_stack`: floor below which the stack is not reduced further
/// - `awarded_stack`: the stack the coordinator claims to have seated with
///
/// # Returns
/// - `pub u64` — the verified number of blind levels elapsed at entry, which
///   the table contract uses to place the player at the correct level.
fn main(
    // Private: the exact entry time. Kept private so the schedule can be
    // verified without publishing when each player bought in — that ordering
    // is itself exploitable information at a live table.
    registration_time: u64,

    tournament_id: pub Field,
    player_commitment: pub Field,
    tournament_start: pub u64,
    late_reg_close: pub u64,
    level_duration: pub u64,
    starting_stack: pub u64,
    reduction_per_level: pub u64,
    min_stack: pub u64,
    awarded_stack: pub u64,
) -> pub u64 {
    // ── Structure sanity ────────────────────────────────────────────────────
    // Rejected here rather than assumed, because a zero level_duration would
    // divide by zero and a close before the start would make the window empty
    // while still admitting entries under a naive range check.
    assert(level_duration > 0, "level duration must be positive");
    assert(late_reg_close > tournament_start, "late reg window is empty");
    assert(starting_stack > 0, "starting stack must be positive");
    assert(min_stack > 0, "min stack must be positive");
    assert(min_stack <= starting_stack, "min stack exceeds starting stack");

    // Identity must be committed, not zero — a zero commitment would let one
    // proof be replayed for any player.
    assert(player_commitment != 0, "player commitment must be set");
    assert(tournament_id != 0, "tournament id must be set");

    // ── The late-registration window ────────────────────────────────────────
    // Strictly after the start: an entry at exactly `tournament_start` is a
    // normal registration and must not claim a reduced-stack seat, which
    // would otherwise be a way to pay less for a full-value entry.
    assert(
        registration_time > tournament_start,
        "not a late registration: entry at or before tournament start",
    );
    assert(
        registration_time <= late_reg_close,
        "late registration window has closed",
    );

    // ── Elapsed blind levels ────────────────────────────────────────────────
    // Safe: the assertion above establishes registration_time > start.
    let elapsed = registration_time - tournament_start;

    // Floor division. A player entering part-way through level 3 is treated as
    // having missed levels 1 and 2 in full — they are entering *during* level
    // 3, so only completed levels are deducted.
    let levels_elapsed = elapsed / level_duration;

    // ── Stack schedule ──────────────────────────────────────────────────────
    let total_reduction = levels_elapsed * reduction_per_level;

    // Guarded subtraction: u64 wraps on underflow in Noir, so a reduction
    // larger than the starting stack would silently produce an enormous stack
    // rather than a small one — the exact inflation this circuit exists to
    // prevent.
    let expected_stack = if total_reduction >= starting_stack {
        min_stack
    } else {
        let reduced = starting_stack - total_reduction;
        if reduced < min_stack {
            min_stack
        } else {
            reduced
        }
    };

    assert(awarded_stack == expected_stack, "awarded stack does not match the late-entry schedule");

    // ── Post-conditions ─────────────────────────────────────────────────────
    // Restated as explicit assertions rather than left implicit in the
    // arithmetic above, so a future change to the schedule cannot quietly
    // violate them.
    assert(awarded_stack <= starting_stack, "late entrant received more than the starting stack");
    assert(awarded_stack >= min_stack, "awarded stack below the configured floor");

    levels_elapsed
}

// ── Tests ───────────────────────────────────────────────────────────────────
//
// Times are seconds from an arbitrary epoch; the circuit only ever uses
// differences, so the absolute value is irrelevant.

global START: u64 = 1_000_000;
global LEVEL: u64 = 600; // 10-minute levels
global CLOSE: u64 = START + 6 * LEVEL; // late reg closes after 6 levels
global STACK: u64 = 20_000;
global PER_LEVEL: u64 = 1_500;
global FLOOR: u64 = 5_000;
global PLAYER: Field = 0x1234;
global TID: Field = 0x99;

#[test]
fn entry_during_the_first_late_level_loses_one_level() {
    // 1.5 levels in: one level completed, so one level of chips deducted.
    let entry = START + LEVEL + LEVEL / 2;
    let levels = main(
        entry, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR,
        STACK - PER_LEVEL,
    );
    assert(levels == 1);
}

#[test]
fn entry_one_second_after_the_start_loses_nothing_yet() {
    // Still inside level 1, so no level has completed. The entrant is late but
    // has not yet missed a full level of blinds.
    let levels = main(
        START + 1, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR, STACK,
    );
    assert(levels == 0);
}

#[test]
fn entry_exactly_on_a_level_boundary_counts_that_level_as_complete() {
    let entry = START + 2 * LEVEL;
    let levels = main(
        entry, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR,
        STACK - 2 * PER_LEVEL,
    );
    assert(levels == 2);
}

#[test]
fn entry_at_the_close_instant_is_still_accepted() {
    // The window is inclusive of its final second; a player who registers on
    // the deadline has registered.
    let levels = main(
        CLOSE, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR,
        STACK - 6 * PER_LEVEL,
    );
    assert(levels == 6);
}

#[test(should_fail_with = "late registration window has closed")]
fn entry_one_second_after_the_close_is_rejected() {
    let _ = main(
        CLOSE + 1, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR, STACK,
    );
}

#[test(should_fail_with = "not a late registration")]
fn entry_exactly_at_the_start_is_not_a_late_registration() {
    // Would otherwise be a way to claim a late-entry seat at full value.
    let _ = main(
        START, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR, STACK,
    );
}

#[test(should_fail_with = "not a late registration")]
fn entry_before_the_start_is_rejected() {
    let _ = main(
        START - 100, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR, STACK,
    );
}

#[test(should_fail_with = "awarded stack does not match")]
fn claiming_a_full_stack_after_four_levels_is_rejected() {
    // The headline attack: enter late, claim the punctual player's stack.
    let entry = START + 4 * LEVEL;
    let _ = main(
        entry, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR, STACK,
    );
}

#[test(should_fail_with = "awarded stack does not match")]
fn claiming_more_than_the_schedule_allows_is_rejected() {
    let entry = START + 3 * LEVEL;
    let _ = main(
        entry, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR,
        STACK - 3 * PER_LEVEL + 1,
    );
}

#[test]
fn a_very_late_entry_is_floored_rather_than_underflowing() {
    // 10 levels x 1_500 = 15_000 against a 20_000 stack leaves 5_000, which is
    // exactly the floor. Pushing further must clamp, not wrap around to a
    // near-u64::MAX stack.
    let late_close = START + 20 * LEVEL;
    let entry = START + 18 * LEVEL; // 18 * 1_500 = 27_000 > 20_000 stack
    let levels = main(
        entry, TID, PLAYER, START, late_close, LEVEL, STACK, PER_LEVEL, FLOOR, FLOOR,
    );
    assert(levels == 18);
}

#[test]
fn the_floor_applies_before_the_stack_reaches_zero() {
    let late_close = START + 20 * LEVEL;
    // 11 levels x 1_500 = 16_500; 20_000 - 16_500 = 3_500, below the 5_000
    // floor, so the floor wins.
    let entry = START + 11 * LEVEL;
    let levels = main(
        entry, TID, PLAYER, START, late_close, LEVEL, STACK, PER_LEVEL, FLOOR, FLOOR,
    );
    assert(levels == 11);
}

#[test(should_fail_with = "level duration must be positive")]
fn a_zero_level_duration_is_rejected() {
    // Would divide by zero when computing elapsed levels.
    let _ = main(
        START + 100, TID, PLAYER, START, CLOSE, 0, STACK, PER_LEVEL, FLOOR, STACK,
    );
}

#[test(should_fail_with = "late reg window is empty")]
fn a_close_before_the_start_is_rejected() {
    let _ = main(
        START + 1, TID, PLAYER, START, START - 1, LEVEL, STACK, PER_LEVEL, FLOOR, STACK,
    );
}

#[test(should_fail_with = "player commitment must be set")]
fn a_zero_player_commitment_is_rejected() {
    // A zero commitment would make one proof valid for every player.
    let _ = main(
        START + LEVEL, TID, 0, START, CLOSE, LEVEL, STACK, PER_LEVEL, FLOOR,
        STACK - PER_LEVEL,
    );
}

#[test(should_fail_with = "min stack exceeds starting stack")]
fn a_floor_above_the_starting_stack_is_rejected() {
    let _ = main(
        START + LEVEL, TID, PLAYER, START, CLOSE, LEVEL, STACK, PER_LEVEL, STACK + 1,
        STACK,
    );
}

#[test]
fn a_zero_reduction_schedule_awards_the_full_stack() {
    // A tournament may choose not to penalise late entry at all. The circuit
    // must permit that rather than assuming a reduction is always applied.
    let entry = START + 3 * LEVEL;
    let levels = main(entry, TID, PLAYER, START, CLOSE, LEVEL, STACK, 0, FLOOR, STACK);
    assert(levels == 3);
}
