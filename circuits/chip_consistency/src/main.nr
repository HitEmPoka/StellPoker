/// Chip Count Consistency Circuit (Issue #227, #889)
///
/// Proves that total chip supply is conserved across a table:
///
///   sum(player_stacks) + pot + rake == total_buy_ins
///
/// This is a fundamental invariant for poker tables: every chip that entered
/// the table via buy-ins must be accounted for in player stacks, the current
/// pot, or the rake balance. No chips can be created or destroyed.
///
/// # Numeric domain (Issue #889)
///
/// All amounts are field elements constrained to the explicit unsigned 64-bit
/// chip domain `[0, 2^64 - 1]` by `stellar_poker_lib::chips`. Without those
/// range checks a field-wrapped amount could pass the conservation equality
/// while being far outside the realistic chip supply. Aggregates are checked
/// with the same domain, so an overflowing sum of two maximal stacks is also
/// rejected.
///
/// # Inputs (all private)
/// - `player_stacks`: current chip stacks for each seat
/// - `num_players`: how many seats are occupied
/// - `pot`: total chips currently in the pot(s)
/// - `rake`: total rake collected this session
/// - `total_buy_ins`: cumulative buy-in amount for all players
///
/// # Public inputs
/// - `pub_total_buy_ins`: the on-chain total buy-in (for verification)
/// - `table_id`: identifier tying this proof to a specific table
///
/// # Returns
/// - `pub bool` — true if chips are consistent (always returns true on success)

use stellar_poker_lib::chips;

global MAX_PLAYERS: u32 = 6;

fn main(
    player_stacks: [Field; MAX_PLAYERS],
    num_players: u32,
    pot: Field,
    rake: Field,
    total_buy_ins: Field,

    pub_total_buy_ins: pub Field,
    table_id: pub u32,
) -> pub bool {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // Explicit 64-bit domain on every chip amount (zero included).
    chips::assert_chip_amounts(player_stacks);
    chips::assert_chip_amount(pot);
    chips::assert_chip_amount(rake);
    chips::assert_chip_amount(total_buy_ins);
    chips::assert_chip_amount(pub_total_buy_ins);

    // Verify the claimed total buy-ins matches the public value.
    assert(total_buy_ins == pub_total_buy_ins, "total buy-ins mismatch");

    // Sum all player stacks (after range checks, in the field domain).
    let mut stacks_sum: Field = 0;
    for i in 0..MAX_PLAYERS {
        if (i as u32) < num_players {
            stacks_sum += player_stacks[i];
        }
    }

    // Core invariant: stacks + pot + rake == total buy-ins.
    let accounted = stacks_sum + pot + rake;
    assert(accounted == total_buy_ins, "chip conservation violated");
    // The aggregate is a chip amount too: a sum above 2^64-1 is out of domain.
    chips::assert_chip_amount(accounted);

    // `table_id` binds the proof to a table but is not a chip amount.
    let _ = table_id;

    true
}

#[test]
fn test_zero_amounts_conserve() {
    let ok = main([0, 0, 0, 0, 0, 0], 2, 0, 0, 0, 0, 7);
    assert(ok);
}

#[test]
fn test_max_u64_stack_conserves() {
    let max: Field = 18446744073709551615;
    let ok = main([max, 0, 0, 0, 0, 0], 2, 0, 0, max, max, 7);
    assert(ok);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_max_plus_one_stack_rejected() {
    let above: Field = 18446744073709551616;
    main([above, 0, 0, 0, 0, 0], 2, 0, 0, above, above, 7);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_max_plus_one_pot_rejected() {
    let above: Field = 18446744073709551616;
    main([0, 0, 0, 0, 0, 0], 2, above, 0, above, above, 7);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_max_plus_one_rake_rejected() {
    let above: Field = 18446744073709551616;
    main([0, 0, 0, 0, 0, 0], 2, 0, above, above, above, 7);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_aggregate_overflow_rejected() {
    let max: Field = 18446744073709551615;
    // Two maximal stacks sum to just under 2^65, outside the chip domain.
    let total: Field = 36893488147419103230;
    main([max, max, 0, 0, 0, 0], 2, 0, 0, total, total, 7);
}

#[test(should_fail_with = "chip conservation violated")]
fn test_conservation_violation_rejected() {
    // All values are in domain, but stacks (20) + pot + rake != 21.
    main([10, 10, 0, 0, 0, 0], 2, 0, 0, 21, 21, 7);
}
