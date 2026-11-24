/// Pot calculation circuit (Issue #889).
///
/// Proves that the public pot total equals the sum of all bets minus rake.
/// Every chip amount — bets, declared side pots, rake and the public total —
/// is first constrained to the explicit 64-bit chip domain by the shared
/// `chips` module, so a field-wrapped witness cannot satisfy the equation by
/// wrapping around the BN254 modulus.
///
/// `side_pots` are the ordered layer amounts produced by
/// `stellar_poker_lib::pot::derive_side_pots`. Their sum must partition the
/// published total pot, which also prevents hidden chips being injected
/// through otherwise-unchecked side-pot witnesses.

use stellar_poker_lib::chips;

fn main(bets: [Field; 6], side_pots: [Field; 6], rake: Field, pub_total_pot: pub Field) {
    // Range-check all chip inputs before any modular arithmetic.
    chips::assert_chip_amounts(bets);
    chips::assert_chip_amounts(side_pots);
    chips::assert_chip_amount(rake);
    chips::assert_chip_amount(pub_total_pot);

    // Main equation: bets collected minus rake equals the published pot.
    let gross = chips::sum_chip_amounts(bets);
    assert(gross - rake == pub_total_pot, "pot total mismatch");
    chips::assert_chip_amount(gross);

    // The side-pot layers must account for every chip in the published pot.
    let side_total = chips::sum_chip_amounts(side_pots);
    assert(side_total == pub_total_pot, "side pots do not sum to total pot");
}

#[test]
fn test_valid_pot_with_side_pots() {
    main([100, 100, 100, 0, 0, 0], [300, 0, 0, 0, 0, 0], 25, 275);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_bet_above_u64_max_rejected() {
    main(
        [18446744073709551616, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0],
        0,
        0,
    );
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_side_pot_above_u64_max_rejected() {
    main(
        [100, 0, 0, 0, 0, 0],
        [18446744073709551616, 0, 0, 0, 0, 0],
        0,
        100,
    );
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_rake_above_u64_max_rejected() {
    main([100, 0, 0, 0, 0, 0], [100, 0, 0, 0, 0, 0], 18446744073709551616, 0);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_public_total_above_u64_max_rejected() {
    main([100, 0, 0, 0, 0, 0], [100, 0, 0, 0, 0, 0], 0, 18446744073709551616);
}

#[test(should_fail_with = "side pots do not sum to total pot")]
fn test_side_pot_partition_mismatch_rejected() {
    main([100, 100, 0, 0, 0, 0], [150, 0, 0, 0, 0, 0], 0, 200);
}

#[test]
fn test_zero_amounts_are_valid() {
    main([0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0], 0, 0);
}

#[test]
fn test_u64_max_as_single_bet() {
    main(
        [18446744073709551615, 0, 0, 0, 0, 0],
        [18446744073709551615, 0, 0, 0, 0, 0],
        0,
        18446744073709551615,
    );
}
