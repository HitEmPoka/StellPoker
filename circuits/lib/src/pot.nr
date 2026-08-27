/// Side-pot allocation logic (Issue #219).
///
/// Pure Noir reimplementation of `contracts/poker-table/src/pot.rs`
/// `calculate_side_pots`. It derives the main pot and side pots from a table of
/// player commitments, all-in flags and folded flags, and is used by the
/// `side_pot_valid` circuit to prove that a claimed allocation is correct.
///
/// Rules (Texas Hold'em):
///  - Each pot is a "layer" of contributions between two consecutive distinct
///    all-in (or final) commitment levels.
///  - A player is eligible to win a pot only if they contributed the full layer,
///    i.e. their total commitment reaches the layer's upper bound, and they are
///    not folded. Folded players' chips are still swept in (dead money), but they
///    are never eligible.
///  - Pots are returned main-pot first, then side pots from lowest all-in level
///    upward. The sum of all pot amounts equals the sum of all commitments (chips
///    are conserved - nothing is orphaned or created).
///
/// Reference: <https://en.wikipedia.org/wiki/Side_pot>

global MAX_PLAYERS: u32 = 6;
global MAX_POTS: u32 = 6;

/// Derive `(amounts, eligible, num_pots)` from player states.
///
/// `amounts[i]` is the chip total of pot `i` (0 for unused slots), and
/// `eligible[i][p]` is true when seat `p` may win pot `i`. Only seats
/// `0..num_players` are considered; the remaining slots are treated as inactive
/// (contribute nothing, not eligible).
pub fn derive_side_pots(
    commitments: [u32; MAX_PLAYERS],
    all_in: [bool; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
) -> ([u32; MAX_POTS], [[bool; MAX_PLAYERS]; MAX_POTS], u32) {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // Ascending, de-duplicated all-in contribution levels of non-folded players.
    let levels = distinct_all_in_levels(commitments, all_in, folded, num_players);

    // Top boundary: the highest amount committed by any non-folded player.
    let top_level = max_contender_committed(commitments, folded, num_players);

    let mut cand_amounts: [u32; MAX_POTS] = [0; MAX_POTS];
    let mut cand_eligible: [[bool; MAX_PLAYERS]; MAX_POTS] = [[false; MAX_PLAYERS]; MAX_POTS];
    let mut k: u32 = 0;
    let mut prev_level: u32 = 0;

    for li in 0..MAX_PLAYERS {
        let level = levels[li];
        if (level != 0) & (level > prev_level) & (level <= top_level) {
            // build_layer(prev_level, level)
            let amount = layer_amount(commitments, folded, num_players, prev_level, level);
            if amount > 0 {
                cand_amounts[k] = amount;
                cand_eligible[k] = set_eligibility(commitments, folded, num_players, level);
                k += 1;
            }
            prev_level = level;
        }
    }

    // Top layer: every remaining chip above the last all-in level, capped at the
    // highest contender commitment; eligibility capped at `top_level`. If the top
    // is already covered, sweep any folded dead money beyond it into the last pot.
    if top_level <= prev_level {
        cand_amounts = sweep_excess_dead_money(cand_amounts, k, commitments, folded, num_players, prev_level);
    } else {
        let amount = top_layer_amount(commitments, folded, num_players, prev_level);
        if amount > 0 {
            cand_amounts[k] = amount;
            cand_eligible[k] = set_eligibility(commitments, folded, num_players, top_level);
            k += 1;
        }
    }

    (cand_amounts, cand_eligible, k)
}

/// Distinct all-in levels of non-folded players with committed > 0, ascending,
/// padded with zero sentinels.
fn distinct_all_in_levels(
    commitments: [u32; MAX_PLAYERS],
    all_in: [bool; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
) -> [u32; MAX_PLAYERS] {
    let mut vals: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut n: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if (p < num_players) & all_in[p] & (!folded[p]) & (commitments[p] > 0) {
            vals[p] = commitments[p];
            n += 1;
        }
    }
    // Sort the first `n` entries ascending; entries beyond n remain 0.
    let sorted = sort_ascending(vals);
    // De-duplicate into `levels`, padded with 0 sentinels.
    let mut levels: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut m: u32 = 0;
    let mut prev: u32 = 0;
    for i in 0..MAX_PLAYERS {
        let v = sorted[i];
        if m < n & v > prev {
            levels[m] = v;
            m += 1;
            prev = v;
        }
    }
    levels
}

/// Bubble sort on fixed-size array (ascending). Used to order the all-in levels.
fn sort_ascending(a: [u32; MAX_PLAYERS]) -> [u32; MAX_PLAYERS] {
    let mut arr = a;
    for pass in 0..MAX_PLAYERS {
        for i in 0..(MAX_PLAYERS - 1) {
            if arr[i] > arr[i + 1] {
                let tmp = arr[i];
                arr[i] = arr[i + 1];
                arr[i + 1] = tmp;
            }
        }
    }
    arr
}

/// Highest committed amount among non-folded players.
fn max_contender_committed(
    commitments: [u32; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
) -> u32 {
    let mut max: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if (p < num_players) & (!folded[p]) & (commitments[p] > max) {
            max = commitments[p];
        }
    }
    max
}

/// Amount contributed to the layer (lower, upper] across all players.
fn layer_amount(
    commitments: [u32; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
    lower: u32,
    upper: u32,
) -> u32 {
    let _ = folded;
    let mut amount: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if (p < num_players) & (commitments[p] > lower) {
            let capped = min(commitments[p], upper);
            amount += capped - lower;
        }
    }
    amount
}

/// Eligibility row for a pot whose layer ceiling is `upper`. A non-folded player
/// is eligible iff their commitment reaches `upper` (they fully covered the layer).
/// This mirrors `build_layer` / `build_top_layer` in `pot.rs`; for the top pot the
/// caller already ensures `top_level > prev_level` before calling, so no extra
/// floor guard is needed.
fn set_eligibility(
    commitments: [u32; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
    upper: u32,
) -> [bool; MAX_PLAYERS] {
    let mut out: [bool; MAX_PLAYERS] = [false; MAX_PLAYERS];
    for p in 0..MAX_PLAYERS {
        let mut elig = false;
        if (p < num_players) & (!folded[p]) & (commitments[p] >= upper) {
            elig = true;
        }
        out[p] = elig;
    }
    out
}

/// Amount of the top layer (prev_level, +inf] capped at contender ceiling.
fn top_layer_amount(
    commitments: [u32; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
    lower: u32,
) -> u32 {
    let _ = folded;
    let mut amount: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if (p < num_players) & (commitments[p] > lower) {
            amount += commitments[p] - lower;
        }
    }
    amount
}

/// Add any folded dead money committed beyond `lower` into the last pot created.
fn sweep_excess_dead_money(
    amounts: [u32; MAX_POTS],
    k: u32,
    commitments: [u32; MAX_PLAYERS],
    folded: [bool; MAX_PLAYERS],
    num_players: u32,
    lower: u32,
) -> [u32; MAX_POTS] {
    let mut out = amounts;
    let mut excess: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if (p < num_players) & folded[p] & (commitments[p] > lower) {
            excess += commitments[p] - lower;
        }
    }
    if (k > 0) & (excess > 0) {
        let last = k - 1;
        out[last] += excess;
    }
    out
}

fn min(a: u32, b: u32) -> u32 {
    if a < b {
        a
    } else {
        b
    }
}

#[test]
fn no_all_in_single_main_pot() {
    let co = [100, 100, 100, 0, 0, 0];
    let ai = [false, false, false, false, false, false];
    let fd = [false, false, false, true, true, true];
    let (a, e, n) = derive_side_pots(co, ai, fd, 3);
    assert(n == 1);
    assert(a[0] == 300);
    assert(e[0][0]);
    assert(e[0][1]);
    assert(e[0][2]);
}

#[test]
fn three_way_unequal_all_ins() {
    // stacks 50 / 100 / 100 -> main 150 (3 eligible), side 100 (seats 1,2).
    let co = [50, 100, 100, 0, 0, 0];
    let ai = [true, true, false, false, false, false];
    let fd = [false, false, false, true, true, true];
    let (a, e, n) = derive_side_pots(co, ai, fd, 3);
    assert(n == 2);
    assert(a[0] == 150);
    assert(a[1] == 100);
    assert(e[0][0] & e[0][1] & e[0][2]);
    assert(!e[1][0] & e[1][1] & e[1][2]);
}

#[test]
fn four_way_three_distinct_all_in_levels() {
    // 20 / 50 / 80 / 80 -> (0,20]:80 all, (20,50]:90, (50,80]:60.
    let co = [20, 50, 80, 80, 0, 0];
    let ai = [true, true, true, false, false, false];
    let fd = [false, false, false, false, true, true];
    let (a, e, n) = derive_side_pots(co, ai, fd, 4);
    assert(n == 3);
    assert(a[0] == 80);
    assert(e[0][0] & e[0][1] & e[0][2] & e[0][3]);
    assert(a[1] == 90);
    assert(!e[1][0] & e[1][1] & e[1][2] & e[1][3]);
    assert(a[2] == 60);
    assert(!e[2][0] & !e[2][1] & e[2][2] & e[2][3]);
}

#[test]
fn folded_dead_money_with_distinct_all_in_levels() {
    // seat1 folded at 80; contenders all-in 40 and 100.
    // (0,40]: 120 -> eligible 0,2 ; (40,100]: seat1 40 dead + seat2 60 = 100 -> seat2.
    let co = [40, 80, 100, 0, 0, 0];
    let ai = [true, false, true, false, false, false];
    let fd = [false, true, false, true, true, true];
    let (a, e, n) = derive_side_pots(co, ai, fd, 3);
    assert(n == 2);
    assert(a[0] == 120);
    assert(e[0][0] & e[0][2]);
    assert(!e[0][1]);
    assert(a[1] == 100);
    assert(e[1][2]);
    assert(!e[1][0] & !e[1][1]);
}

#[test]
fn six_way_mixed_stacks() {
    // 10 / 25 / 25 / 60 / 100 / 100 -> 4 pots, all in main, top only 4,5.
    let co = [10, 25, 25, 60, 100, 100];
    let ai = [true, true, true, true, false, false];
    let fd = [false, false, false, false, false, false];
    let (a, e, n) = derive_side_pots(co, ai, fd, 6);
    assert(n == 4);
    let mut total: u32 = 0;
    for i in 0..4 {
        total += a[i];
    }
    assert(total == 320);
    assert(e[0][0] & e[0][5]);
    assert(e[3][4] & e[3][5]);
    assert(!e[3][0] & !e[3][1] & !e[3][2] & !e[3][3]);
}
