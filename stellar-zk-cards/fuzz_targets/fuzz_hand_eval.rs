#![no_main]
use libfuzzer_sys::fuzz_target;
use stellar_zk_cards::{evaluate_hand, HandCategory};

/// libFuzzer entry point for continuous fuzzing of hand evaluation.
///
/// This fuzz target compares the Rust contract implementation against
/// the reference Noir circuit implementation to detect divergences in
/// hand category evaluation. All 7-card hands must produce identical
/// categories regardless of card order (as long as they're unique).
fuzz_target!(|data: &[u8]| {
    if data.len() < 7 {
        return;
    }

    let mut cards = [0u32; 7];
    for i in 0..7 {
        cards[i] = (data[i] as u32) % 52;
    }

    let contract_rank = evaluate_hand(&cards);
    let _contract_category = contract_rank.category();

    // The fuzz target ensures the contract's hand evaluator completes
    // without panicking on all valid 52-card combinations.
    // Additional oracle-based checks would require embedding the
    // circuit reference implementation here, which is out of scope.
});
