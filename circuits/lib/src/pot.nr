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

/// Odd-chip distribution logic (Issue #534 / Split Pot Remainder Award).
///
/// Pure Noir implementation of odd-chip remainder rules matching
/// `contracts/poker-table/src/pot.rs` and Texas Hold'em rules.
///
/// In split pots, when the pot amount cannot be divided evenly among all
/// eligible tied winners, the remainder (odd chips) must be awarded deterministically:
///
/// Rules:
/// 1. Base share: each of the W tied winners receives floor(pot_amount / W) chips.
/// 2. Remainder: R = pot_amount % W chips remain (0 <= R < W).
/// 3. Priority Order:
///    - `RULE_LEFT_OF_BUTTON (0)`: Remainder chips are awarded one-by-one to tied
///      winners in clockwise order starting from the seat directly to the left of
///      the dealer button: `(dealer_seat + 1) % num_players`.
///    - `RULE_EARLIEST_SEAT (1)`: Remainder chips are awarded in ascending seat
///      index order (0, 1, 2, ...), matching `contracts/poker-table/src/pot.rs`
///      `distribute_pots_with_ties`.
/// 4. Conservation: Exactly R players receive (base_share + 1) chips, and W - R
///    players receive base_share chips. Total awarded chips equals pot_amount.

pub global RULE_LEFT_OF_BUTTON: u32 = 0;
pub global RULE_EARLIEST_SEAT: u32 = 1;

/// Clockwise priority starting seat for odd chip distribution.
pub fn odd_chip_start_seat(dealer_seat: u32, num_players: u32, rule: u32) -> u32 {
    if rule == RULE_LEFT_OF_BUTTON {
        (dealer_seat + 1) % num_players
    } else {
        0
    }
}

/// Compute clockwise distance of a seat from the starting priority seat.
pub fn clockwise_distance(seat: u32, start_seat: u32, num_players: u32) -> u32 {
    (seat + num_players - start_seat) % num_players
}

/// Compute deterministic odd-chip distribution for a split pot among tied winners.
///
/// Returns `(payouts, base_share, remainder)` where:
/// - `payouts[p]` is the payout to seat `p` (0 if seat `p` is not a tied winner).
/// - `base_share` is the integer division `pot_amount / num_tied_winners`.
/// - `remainder` is `pot_amount % num_tied_winners`.
pub fn compute_odd_chip_distribution(
    pot_amount: u32,
    num_players: u32,
    dealer_seat: u32,
    tied_winners: [bool; MAX_PLAYERS],
    rule: u32,
) -> ([u32; MAX_PLAYERS], u32, u32) {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");
    assert(dealer_seat < num_players, "dealer seat out of bounds");
    assert((rule == RULE_LEFT_OF_BUTTON) | (rule == RULE_EARLIEST_SEAT), "invalid rule");

    // 1. Count tied winners and ensure inactive seats are not marked as tied.
    let mut num_tied: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            if tied_winners[p] {
                num_tied += 1;
            }
        } else {
            assert(!tied_winners[p], "inactive seat cannot be tied winner");
        }
    }
    assert(num_tied >= 2, "split pot requires at least 2 tied winners");

    // 2. Base share and remainder calculation.
    let base_share = pot_amount / num_tied;
    let remainder = pot_amount % num_tied;

    // 3. Priority start seat.
    let start_seat = odd_chip_start_seat(dealer_seat, num_players, rule);

    // 4. Compute payouts for each seat based on clockwise priority ranking.
    let mut payouts: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for p in 0..MAX_PLAYERS {
        if (p < num_players) & tied_winners[p] {
            let dist_p = clockwise_distance(p, start_seat, num_players);

            // Count how many tied winners have strictly higher priority (smaller clockwise distance).
            let mut rank_p: u32 = 0;
            for q in 0..MAX_PLAYERS {
                if (q < num_players) & (q != p) & tied_winners[q] {
                    let dist_q = clockwise_distance(q, start_seat, num_players);
                    if dist_q < dist_p {
                        rank_p += 1;
                    }
                }
            }

            // Remainder chips go to the lowest ranks (0 .. remainder - 1).
            let odd_chip = if rank_p < remainder { 1 } else { 0 };
            payouts[p] = base_share + odd_chip;
        }
    }

    // 5. Assert chip conservation: sum of payouts == pot_amount.
    let mut total_payouts: u32 = 0;
    for p in 0..MAX_PLAYERS {
        total_payouts += payouts[p];
    }
    assert(total_payouts == pot_amount, "chip conservation violated in odd chip calculation");

    (payouts, base_share, remainder)
}

/// Verify that claimed payouts match the deterministic odd-chip distribution.
///
/// Returns `(base_share, remainder)`.
pub fn verify_odd_chip_distribution(
    pot_amount: u32,
    num_players: u32,
    dealer_seat: u32,
    tied_winners: [bool; MAX_PLAYERS],
    rule: u32,
    claimed_payouts: [u32; MAX_PLAYERS],
) -> (u32, u32) {
    let (expected_payouts, base_share, remainder) = compute_odd_chip_distribution(
        pot_amount,
        num_players,
        dealer_seat,
        tied_winners,
        rule,
    );

    for p in 0..MAX_PLAYERS {
        assert(
            claimed_payouts[p] == expected_payouts[p],
            "payout mismatch at player seat",
        );
    }

    (base_share, remainder)
}

/// Distribute multiple pots (main + side pots) with tied winners and odd chips,
/// matching `contracts/poker-table/src/pot.rs` `distribute_pots_with_ties`.
pub fn distribute_pots_with_odd_chips(
    pot_amounts: [u32; MAX_POTS],
    num_pots: u32,
    eligible: [[bool; MAX_PLAYERS]; MAX_POTS],
    tied_winners: [bool; MAX_PLAYERS],
    dealer_seat: u32,
    num_players: u32,
    rule: u32,
) -> [u32; MAX_PLAYERS] {
    let mut total_payouts: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for pi in 0..MAX_POTS {
        if (pi < num_pots) & (pot_amounts[pi] > 0) {
            // Find tied winners eligible for this pot layer
            let mut pot_tied: [bool; MAX_PLAYERS] = [false; MAX_PLAYERS];
            let mut pot_recipients_count: u32 = 0;
            for p in 0..MAX_PLAYERS {
                if (p < num_players) & tied_winners[p] & eligible[pi][p] {
                    pot_tied[p] = true;
                    pot_recipients_count += 1;
                }
            }

            if pot_recipients_count >= 2 {
                let (layer_payouts, _, _) = compute_odd_chip_distribution(
                    pot_amounts[pi],
                    num_players,
                    dealer_seat,
                    pot_tied,
                    rule,
                );
                for p in 0..MAX_PLAYERS {
                    total_payouts[p] += layer_payouts[p];
                }
            } else if pot_recipients_count == 1 {
                // Single winner for this pot
                for p in 0..MAX_PLAYERS {
                    if pot_tied[p] {
                        total_payouts[p] += pot_amounts[pi];
                    }
                }
            }
        }
    }

    total_payouts
}

#[test]
fn test_odd_chip_three_way_zero_remainder() {
    // 3-way split: 300 chips among 3 players -> 100 each, 0 remainder
    let tied = [true, true, true, false, false, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(300, 3, 0, tied, RULE_LEFT_OF_BUTTON);
    assert(base_share == 100);
    assert(remainder == 0);
    assert(payouts[0] == 100);
    assert(payouts[1] == 100);
    assert(payouts[2] == 100);
}

#[test]
fn test_odd_chip_three_way_remainder_one_left_of_button() {
    // 3-way split: 100 chips among 3 players (seats 0, 1, 2). Dealer at seat 0.
    // 100 / 3 = 33 r 1.
    // Seat 1 is left of button (clockwise dist = 0) -> receives 34 chips.
    // Seats 2 and 0 receive 33 chips each.
    let tied = [true, true, true, false, false, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(100, 3, 0, tied, RULE_LEFT_OF_BUTTON);
    assert(base_share == 33);
    assert(remainder == 1);
    assert(payouts[0] == 33);
    assert(payouts[1] == 34);
    assert(payouts[2] == 33);
}

#[test]
fn test_odd_chip_three_way_remainder_two_left_of_button() {
    // 3-way split: 101 chips among 3 players (seats 0, 1, 2). Dealer at seat 0.
    // 101 / 3 = 33 r 2.
    // Remainder 2 chips:
    // 1st odd chip -> seat 1 (left of button, dist 0). Payout = 34.
    // 2nd odd chip -> seat 2 (next clockwise, dist 1). Payout = 34.
    // Seat 0 (button, dist 2) receives base share = 33.
    let tied = [true, true, true, false, false, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(101, 3, 0, tied, RULE_LEFT_OF_BUTTON);
    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 33);
    assert(payouts[1] == 34);
    assert(payouts[2] == 34);
}

#[test]
fn test_odd_chip_three_way_dealer_at_seat_one() {
    // 3-way split: 101 chips among 3 players. Dealer at seat 1.
    // Left of button is seat 2 (dist 0), then seat 0 (dist 1), then seat 1 (dist 2).
    // Remainder 2 chips -> seat 2: 34, seat 0: 34, seat 1: 33.
    let tied = [true, true, true, false, false, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(101, 3, 1, tied, RULE_LEFT_OF_BUTTON);
    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 34);
    assert(payouts[1] == 33);
    assert(payouts[2] == 34);
}

#[test]
fn test_odd_chip_three_way_dealer_at_seat_two() {
    // 3-way split: 100 chips among 3 players. Dealer at seat 2.
    // Left of button is seat 0 (dist 0), then seat 1 (dist 1), then seat 2 (dist 2).
    // Remainder 1 chip -> seat 0: 34, seat 1: 33, seat 2: 33.
    let tied = [true, true, true, false, false, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(100, 3, 2, tied, RULE_LEFT_OF_BUTTON);
    assert(base_share == 33);
    assert(remainder == 1);
    assert(payouts[0] == 34);
    assert(payouts[1] == 33);
    assert(payouts[2] == 33);
}

#[test]
fn test_odd_chip_three_way_earliest_seat_rule() {
    // 3-way split under RULE_EARLIEST_SEAT (matching contracts/poker-table/src/pot.rs):
    // 101 chips among seats 0, 1, 2.
    // Remainder 2 chips go to earliest seats: seat 0 (34), seat 1 (34), seat 2 (33).
    let tied = [true, true, true, false, false, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(101, 3, 0, tied, RULE_EARLIEST_SEAT);
    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 34);
    assert(payouts[1] == 34);
    assert(payouts[2] == 33);
}

#[test]
fn test_odd_chip_three_way_non_contiguous_seats() {
    // 5 players at table (seats 0..4). Dealer at seat 2.
    // Tied winners are seats 1, 3, 4 (3-way tie).
    // Left of button (seat 2) is seat 3 (dist 0).
    // Clockwise order among tied winners: seat 3 (dist 0), seat 4 (dist 1), seat 1 (dist 3).
    // Pot = 101 (remainder 2 chips).
    // Seat 3 gets 34, seat 4 gets 34, seat 1 gets 33.
    let tied = [false, true, false, true, true, false];
    let (payouts, base_share, remainder) = compute_odd_chip_distribution(101, 5, 2, tied, RULE_LEFT_OF_BUTTON);
    assert(base_share == 33);
    assert(remainder == 2);
    assert(payouts[0] == 0);
    assert(payouts[1] == 33);
    assert(payouts[2] == 0);
    assert(payouts[3] == 34);
    assert(payouts[4] == 34);
    assert(payouts[5] == 0);
}
