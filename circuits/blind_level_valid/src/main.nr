/// Blind Level Transition Circuit (Issue #224)
///
/// Proves that the blind level applied to a hand is the one the tournament
/// clock actually called for at the time the hand was dealt.
///
/// The manipulation this closes runs in both directions, and both are
/// profitable to whoever controls the clock:
///
///   - **Understating the level** keeps blinds small, which favours a deep
///     stack that wants to see cheap flops.
///   - **Overstating the level** inflates blinds, which busts short stacks
///     faster and can be aimed at a specific player about to be blinded out.
///
/// Because the level is derived off-chain from wall-clock time, neither is
/// visible from the hand history alone: a hand played at level 4 looks
/// identical to a hand played at level 6 except for the numbers, and those are
/// the numbers under dispute.
///
/// The circuit verifies:
///
///   1. The hand was dealt at or after the tournament start.
///   2. Time spent paused is excluded from the clock — a break must not
///      advance the blinds.
///   3. `level_index = effective_elapsed / level_duration`, clamped to the
///      final level so a long tournament does not index past the structure.
///   4. The small and big blind for that level match the published structure,
///      committed to as a Merkle root so the whole schedule need not be a
///      public input.
///   5. The big blind is exactly twice the small blind, and both are non-zero.
///
/// # Why the structure is a commitment rather than an input
///
/// A tournament structure can run to 30+ levels. Passing all of them as public
/// inputs makes the proof size grow with the structure and makes every
/// verification pay for levels it does not use. Instead the structure is
/// committed once (`structure_root`), and the circuit is handed the one level
/// it needs plus a Merkle path proving that level belongs to the committed
/// schedule.
///
/// All arithmetic is u64. Noir's u64 subtraction wraps rather than trapping,
/// so every subtraction is guarded by a comparison first.
///
/// # Public inputs
/// - `tournament_id`: which tournament this hand belongs to
/// - `hand_commitment`: commitment to the hand being validated
/// - `tournament_start`: tournament start time (seconds)
/// - `level_duration`: seconds per blind level
/// - `total_levels`: number of levels in the structure
/// - `structure_root`: Merkle root committing to the blind schedule
/// - `claimed_level`: the level index the coordinator applied
/// - `claimed_small_blind` / `claimed_big_blind`: the blinds actually charged
///
/// # Returns
/// - `pub u64` — the verified level index, for the table contract to record.
fn main(
    // Private: the exact deal time and the accumulated pause duration. Private
    // because publishing precise deal timestamps leaks table pace, which is
    // itself readable information at a live table.
    hand_dealt_at: u64,
    paused_duration: u64,
    // Merkle path proving (claimed_level, small, big) is in the committed
    // structure. Depth 5 supports up to 32 levels.
    merkle_path: [Field; 5],
    merkle_index: u64,

    tournament_id: pub Field,
    hand_commitment: pub Field,
    tournament_start: pub u64,
    level_duration: pub u64,
    total_levels: pub u64,
    structure_root: pub Field,
    claimed_level: pub u64,
    claimed_small_blind: pub u64,
    claimed_big_blind: pub u64,
) -> pub u64 {
    // ── Structure sanity ────────────────────────────────────────────────────
    assert(level_duration > 0, "level duration must be positive");
    assert(total_levels > 0, "structure must have at least one level");
    assert(total_levels <= 32, "structure exceeds the supported depth");
    assert(structure_root != 0, "structure root must be set");
    assert(tournament_id != 0, "tournament id must be set");
    assert(hand_commitment != 0, "hand commitment must be set");

    // ── The clock ───────────────────────────────────────────────────────────
    assert(
        hand_dealt_at >= tournament_start,
        "hand dealt before the tournament started",
    );

    let wall_elapsed = hand_dealt_at - tournament_start;

    // Paused time does not advance the blinds. Without this, a long break
    // would silently push the tournament forward several levels — the players
    // return to blinds they never played through.
    assert(
        paused_duration <= wall_elapsed,
        "paused longer than the tournament has run",
    );
    let effective_elapsed = wall_elapsed - paused_duration;

    // ── Level derivation ────────────────────────────────────────────────────
    let raw_level = effective_elapsed / level_duration;

    // Clamp to the final level. Structures end with a level that runs until
    // the tournament does; indexing past it would read outside the committed
    // schedule.
    let expected_level = if raw_level >= total_levels {
        total_levels - 1
    } else {
        raw_level
    };

    assert(
        claimed_level == expected_level,
        "claimed blind level does not match the tournament clock",
    );

    // ── Blind values ────────────────────────────────────────────────────────
    assert(claimed_small_blind > 0, "small blind must be positive");
    // Standard Hold'em: the big blind is one full bet, the small blind half.
    // Checked explicitly so a structure entry that violates it cannot be
    // smuggled in behind a valid Merkle path.
    assert(
        claimed_big_blind == claimed_small_blind * 2,
        "big blind must be twice the small blind",
    );

    // The leaf binds the level index to its blinds, so a valid path for level 3
    // cannot be replayed to justify level 7's blinds.
    let leaf = hash_3(
        claimed_level as Field,
        claimed_small_blind as Field,
        claimed_big_blind as Field,
    );

    let computed_root = compute_merkle_root(leaf, merkle_index, merkle_path);
    assert(
        computed_root == structure_root,
        "blind level is not part of the committed structure",
    );

    // The path index must be the level being proven, otherwise a valid leaf
    // could be authenticated at the wrong position in the tree.
    assert(merkle_index == claimed_level, "merkle index does not match the claimed level");

    expected_level
}

/// Poseidon2 over two field elements.
///
/// Uses `poseidon2_permutation` over a zero-padded array, mirroring
/// `circuits/lib/src/commitments.nr`. That module still passes the older
/// two-argument form; this uses the single-argument signature the current
/// compiler expects.
fn hash_2(a: Field, b: Field) -> Field {
    std::hash::poseidon2_permutation([a, b, 0, 0])[0]
}

/// Poseidon2 over three field elements.
fn hash_3(a: Field, b: Field, c: Field) -> Field {
    std::hash::poseidon2_permutation([a, b, c, 0])[0]
}

/// Recompute a Merkle root from a leaf, its index, and the sibling path.
///
/// Sibling ordering is derived from the index bit at each depth, so the same
/// leaf at a different index produces a different root — which is what stops a
/// level's blinds being authenticated at another level's position.
fn compute_merkle_root(leaf: Field, index: u64, path: [Field; 5]) -> Field {
    let mut current = leaf;
    let mut idx = index;

    for i in 0..5 {
        let sibling = path[i];
        let is_right = (idx % 2) == 1;

        let (left, right) = if is_right {
            (sibling, current)
        } else {
            (current, sibling)
        };

        current = hash_2(left, right);
        idx = idx / 2;
    }

    current
}

// ── Tests ───────────────────────────────────────────────────────────────────

global START: u64 = 2_000_000;
global LEVEL: u64 = 900; // 15-minute levels
global LEVELS: u64 = 20;
global TID: Field = 0x7;
global HAND: Field = 0xbeef;

/// Build a leaf for a level, matching the circuit's own leaf construction.
fn leaf_for(level: u64, small: u64, big: u64) -> Field {
    hash_3(level as Field, small as Field, big as Field)
}

/// Recompute the root for a leaf so tests can supply a consistent commitment
/// without hard-coding hashes that would break if the hash changes.
fn root_for(leaf: Field, index: u64, path: [Field; 5]) -> Field {
    compute_merkle_root(leaf, index, path)
}

/// A fixed sibling path. The values are arbitrary — what matters is that the
/// test derives the root from the same path it passes in, so the check is on
/// the circuit's path logic rather than on a memorised digest.
global PATH: [Field; 5] = [11, 22, 33, 44, 55];

#[test]
fn level_zero_applies_during_the_first_level() {
    let (small, big) = (25, 50);
    let leaf = leaf_for(0, small, big);
    let root = root_for(leaf, 0, PATH);

    let level = main(
        START + 100, 0, PATH, 0, TID, HAND, START, LEVEL, LEVELS, root, 0, small, big,
    );
    assert(level == 0);
}

#[test]
fn the_level_advances_exactly_on_the_boundary() {
    let (small, big) = (50, 100);
    let leaf = leaf_for(1, small, big);
    let root = root_for(leaf, 1, PATH);

    // Exactly one level of elapsed time means level 1 has begun.
    let level = main(
        START + LEVEL, 0, PATH, 1, TID, HAND, START, LEVEL, LEVELS, root, 1, small, big,
    );
    assert(level == 1);
}

#[test]
fn one_second_before_the_boundary_is_still_the_previous_level() {
    let (small, big) = (25, 50);
    let leaf = leaf_for(0, small, big);
    let root = root_for(leaf, 0, PATH);

    let level = main(
        START + LEVEL - 1, 0, PATH, 0, TID, HAND, START, LEVEL, LEVELS, root, 0, small, big,
    );
    assert(level == 0);
}

#[test(should_fail_with = "claimed blind level does not match")]
fn understating_the_level_is_rejected() {
    // Four levels have elapsed but the coordinator claims level 2, keeping
    // blinds cheap for a deep stack.
    let (small, big) = (50, 100);
    let leaf = leaf_for(2, small, big);
    let root = root_for(leaf, 2, PATH);

    let _ = main(
        START + 4 * LEVEL, 0, PATH, 2, TID, HAND, START, LEVEL, LEVELS, root, 2, small, big,
    );
}

#[test(should_fail_with = "claimed blind level does not match")]
fn overstating_the_level_is_rejected() {
    // Two levels have elapsed but level 6 is claimed, inflating blinds to bust
    // a short stack early.
    let (small, big) = (400, 800);
    let leaf = leaf_for(6, small, big);
    let root = root_for(leaf, 6, PATH);

    let _ = main(
        START + 2 * LEVEL, 0, PATH, 6, TID, HAND, START, LEVEL, LEVELS, root, 6, small, big,
    );
}

#[test]
fn a_pause_holds_the_level_where_it_was() {
    // Wall clock says three levels; one level of that was a break, so the
    // tournament is only two levels in.
    let (small, big) = (100, 200);
    let leaf = leaf_for(2, small, big);
    let root = root_for(leaf, 2, PATH);

    let level = main(
        START + 3 * LEVEL, LEVEL, PATH, 2, TID, HAND, START, LEVEL, LEVELS, root, 2, small, big,
    );
    assert(level == 2);
}

#[test(should_fail_with = "claimed blind level does not match")]
fn ignoring_a_pause_advances_the_blinds_and_is_rejected() {
    // The same hand as above, but the coordinator forgets (or declines) to
    // deduct the break — the players would return to blinds they never played
    // through.
    let (small, big) = (200, 400);
    let leaf = leaf_for(3, small, big);
    let root = root_for(leaf, 3, PATH);

    let _ = main(
        START + 3 * LEVEL, LEVEL, PATH, 3, TID, HAND, START, LEVEL, LEVELS, root, 3, small, big,
    );
}

#[test(should_fail_with = "paused longer than the tournament has run")]
fn a_pause_longer_than_the_tournament_is_rejected() {
    // Would otherwise underflow the effective clock and wrap to a huge value.
    let (small, big) = (25, 50);
    let leaf = leaf_for(0, small, big);
    let root = root_for(leaf, 0, PATH);

    let _ = main(
        START + LEVEL, 5 * LEVEL, PATH, 0, TID, HAND, START, LEVEL, LEVELS, root, 0, small, big,
    );
}

#[test]
fn the_final_level_is_held_rather_than_indexing_past_the_structure() {
    // 50 levels of elapsed time against a 20-level structure clamps to 19.
    let last = LEVELS - 1;
    let (small, big) = (10_000, 20_000);
    let leaf = leaf_for(last, small, big);
    let root = root_for(leaf, last, PATH);

    let level = main(
        START + 50 * LEVEL, 0, PATH, last, TID, HAND, START, LEVEL, LEVELS, root, last, small, big,
    );
    assert(level == last);
}

#[test(should_fail_with = "hand dealt before the tournament started")]
fn a_hand_dealt_before_the_start_is_rejected() {
    let (small, big) = (25, 50);
    let leaf = leaf_for(0, small, big);
    let root = root_for(leaf, 0, PATH);

    let _ = main(
        START - 1, 0, PATH, 0, TID, HAND, START, LEVEL, LEVELS, root, 0, small, big,
    );
}

#[test(should_fail_with = "big blind must be twice the small blind")]
fn a_malformed_blind_pair_is_rejected() {
    // Checked independently of the Merkle path so a bad structure entry cannot
    // be smuggled in behind a valid proof.
    let leaf = leaf_for(1, 50, 150);
    let root = root_for(leaf, 1, PATH);

    let _ = main(
        START + LEVEL, 0, PATH, 1, TID, HAND, START, LEVEL, LEVELS, root, 1, 50, 150,
    );
}

#[test(should_fail_with = "small blind must be positive")]
fn a_zero_small_blind_is_rejected() {
    let leaf = leaf_for(1, 0, 0);
    let root = root_for(leaf, 1, PATH);

    let _ = main(START + LEVEL, 0, PATH, 1, TID, HAND, START, LEVEL, LEVELS, root, 1, 0, 0);
}

#[test(should_fail_with = "blind level is not part of the committed structure")]
fn blinds_outside_the_committed_structure_are_rejected() {
    // Correct level and a well-formed blind pair, but not the pair the
    // tournament published — the substitution this commitment exists to catch.
    let honest_root = root_for(leaf_for(1, 50, 100), 1, PATH);

    let _ = main(
        START + LEVEL, 0, PATH, 1, TID, HAND, START, LEVEL, LEVELS, honest_root, 1, 500, 1000,
    );
}

#[test(should_fail_with = "merkle index does not match the claimed level")]
fn authenticating_a_level_at_the_wrong_position_is_rejected() {
    // A valid leaf for level 3 replayed while claiming level 1.
    let leaf = leaf_for(1, 50, 100);
    let root = root_for(leaf, 3, PATH);

    let _ = main(
        START + LEVEL, 0, PATH, 3, TID, HAND, START, LEVEL, LEVELS, root, 1, 50, 100,
    );
}

#[test(should_fail_with = "level duration must be positive")]
fn a_zero_level_duration_is_rejected() {
    let leaf = leaf_for(0, 25, 50);
    let root = root_for(leaf, 0, PATH);

    let _ = main(START + 10, 0, PATH, 0, TID, HAND, START, 0, LEVELS, root, 0, 25, 50);
}

#[test(should_fail_with = "structure must have at least one level")]
fn an_empty_structure_is_rejected() {
    let leaf = leaf_for(0, 25, 50);
    let root = root_for(leaf, 0, PATH);

    let _ = main(START + 10, 0, PATH, 0, TID, HAND, START, LEVEL, 0, root, 0, 25, 50);
}

#[test]
fn a_pause_exactly_equal_to_elapsed_time_holds_level_zero() {
    // Boundary of the pause guard: the tournament has run, but every second of
    // it was a break.
    let (small, big) = (25, 50);
    let leaf = leaf_for(0, small, big);
    let root = root_for(leaf, 0, PATH);

    let level = main(
        START + 3 * LEVEL, 3 * LEVEL, PATH, 0, TID, HAND, START, LEVEL, LEVELS, root, 0, small,
        big,
    );
    assert(level == 0);
}
