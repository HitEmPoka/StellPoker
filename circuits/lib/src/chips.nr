/// Explicit 64-bit chip numeric domain (Issue #889).
///
/// Chip amounts are stored as field elements in the ACIR witness so that they
/// can be added without overflowing the native integer types. A field element,
/// however, is not an integer: without an explicit range check a malicious
/// prover can supply `2^64`, `p - x`, or any other field-wrapped value that
/// happens to satisfy an equality after modular reduction.
///
/// Every circuit that consumes a chip amount must therefore funnel the value
/// through [`assert_chip_amount`] before comparing or summing it. The helper
/// decomposes the value into 64 bits and reconstructs it as a field element,
/// which is unsatisfiable for any value outside `[0, 2^64)` because the field
/// modulus is far larger than `2^64`.
///
/// Recognised domain: `0 <= amount <= 2^64 - 1`
/// (`18_446_744_073_709_551_615`).
///
/// Totals that aggregate several chip amounts are range-checked as amounts too:
/// a sum of 6 maximal stacks (just under `2^67`) is intentionally outside the
/// per-amount domain, so callers must keep aggregate values below `2^64` or
/// split them into domain-sized buckets.
pub global CHIP_BITS: u32 = 64;

/// Largest value admitted by the chip domain (`2^64 - 1`).
pub global MAX_CHIP_AMOUNT: Field = 18446744073709551615;

/// Assert that `amount` is a canonical unsigned 64-bit integer.
///
/// Returns the value unchanged so it can be used inline in arithmetic after
/// the range check. The bit decomposition below is the actual constraint:
/// values `>= 2^64` cannot be represented by 64 bits, so the reconstruction
/// equality has no satisfying witness.
pub fn assert_chip_amount(amount: Field) -> Field {
    let bits = amount.to_be_bits::<64>();
    let mut reconstructed: Field = 0;
    for i in 0..64 {
        reconstructed = reconstructed * 2 + (bits[i] as Field);
    }
    assert(reconstructed == amount, "chip amount exceeds 64-bit chip domain");
    amount
}

/// Range-check every element of a chip amount array.
pub fn assert_chip_amounts<let N: u32>(amounts: [Field; N]) {
    for i in 0..N {
        assert_chip_amount(amounts[i]);
    }
}

/// Range-check and sum a chip amount array.
///
/// The returned total is a plain field element; callers that expose it or use
/// it as an amount must range-check it again with [`assert_chip_amount`].
pub fn sum_chip_amounts<let N: u32>(amounts: [Field; N]) -> Field {
    let mut total: Field = 0;
    for i in 0..N {
        total += assert_chip_amount(amounts[i]);
    }
    total
}

/// Assert the canonical chip conservation equation
/// `sum(bets) - rake == total_pot`, with every operand in the 64-bit domain.
pub fn assert_pot_conservation<let N: u32>(
    bets: [Field; N],
    rake: Field,
    total_pot: Field,
) {
    let gross = sum_chip_amounts(bets);
    assert_chip_amount(rake);
    assert_chip_amount(total_pot);
    assert(gross - rake == total_pot, "pot total mismatch");
    // Re-check the aggregate: a gross pot above 2^64 is outside the domain
    // even when the per-bet values are individually in range.
    assert_chip_amount(gross);
}

#[test]
fn test_zero_is_in_domain() {
    assert_chip_amount(0);
}

#[test]
fn test_u64_max_is_in_domain() {
    let max: Field = 18446744073709551615;
    assert_chip_amount(max);
    assert(max == MAX_CHIP_AMOUNT);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_u64_max_plus_one_rejected() {
    let above: Field = 18446744073709551616;
    assert_chip_amount(above);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_field_wrapped_amount_rejected() {
    // A value that often appears when a caller subtracts past zero in the
    // field (e.g. `0 - 1`). It must not be mistaken for a chip amount.
    let wrapped: Field = 0 - 1;
    assert_chip_amount(wrapped);
}

#[test]
fn test_sum_stays_in_domain() {
    let bets: [Field; 3] = [10, 20, 30];
    let total = sum_chip_amounts(bets);
    assert(total == 60);
    assert_chip_amount(total);
}

#[test(should_fail_with = "chip amount exceeds 64-bit chip domain")]
fn test_aggregate_overflow_rejected() {
    let bets: [Field; 2] = [18446744073709551615, 18446744073709551615];
    let total = sum_chip_amounts(bets);
    assert_chip_amount(total);
}

#[test]
fn test_pot_conservation_accepts_valid_total() {
    let bets: [Field; 3] = [100, 200, 300];
    assert_pot_conservation(bets, 50, 550);
}

#[test(should_fail_with = "pot total mismatch")]
fn test_pot_conservation_rejects_mismatch() {
    let bets: [Field; 3] = [100, 200, 300];
    assert_pot_conservation(bets, 50, 551);
}
