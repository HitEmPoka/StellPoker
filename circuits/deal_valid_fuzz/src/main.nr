/// Issue #888: executable 10k-run deal-witness fuzz harness.
///
/// `nargo execute` returns a non-zero public value when a generated witness is
/// accepted/rejected contrary to its expected validity. The mismatch seed is
/// printed to stdout before the value is returned so CI logs are reproducible.
/// The generator only emits well-typed Noir values — the mutations are
/// semantic (duplicate cards, out-of-range indices, reordered salts), not
/// malformed encodings — because the circuit's ABI already rejects the latter.
///
/// The core validation predicate is shared with `circuits/lib` and mirrors the
/// constraints in `deal_valid`; VRF verification is exercised separately by
/// `deal_valid`'s own tests, since this harness targets witness validation.

use stellar_poker_lib::deal_fuzz;

pub global FUZZ_RUNS: u32 = 10_000;
pub global FUZZ_START_SEED: u32 = 0;

fn main() -> pub u32 {
    deal_fuzz::run_fuzz(FUZZ_START_SEED, FUZZ_RUNS)
}
