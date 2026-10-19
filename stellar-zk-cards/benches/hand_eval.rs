//! Criterion benchmarks for the reusable hand evaluator (issue #652).
//!
//! The evaluator runs the same 21-combination best-of-seven search for every
//! showdown, so a small per-call regression compounds across every hand. The
//! benchmark names are part of the regression contract: CI compares the
//! `median_ns` of each name against `benchmark_data/hand_evaluator_baseline.json`
//! and fails when one regresses by more than 10%.
//!
//! Hands intentionally cover every category, including the best and worst
//! five-card combinations, so a change that helps one path but penalises
//! another is visible instead of being averaged away.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use stellar_zk_cards::evaluate_hand;

/// `(name, seven card values)` for each hand category.
///
/// Cards use the crate's `suit * 13 + rank` encoding (0 = 2 … 12 = Ace).
/// The two extra cards are chosen not to change the intended category.
const HANDS: [(&str, [u32; 7]); 10] = [
    // A K Q J 9, no pair.
    ("high_card", [51, 37, 23, 9, 20, 5, 0]),
    // A A K Q J.
    ("one_pair", [51, 38, 11, 23, 35, 7, 14]),
    // A A K K Q.
    ("two_pair", [51, 38, 11, 24, 36, 9, 14]),
    // A A A K Q.
    ("three_of_a_kind", [51, 38, 25, 11, 36, 9, 14]),
    // A K Q J T.
    ("straight", [8, 22, 36, 50, 38, 0, 14]),
    // A K Q J 9, all hearts.
    ("flush", [38, 37, 36, 35, 33, 0, 14]),
    // A A A K K.
    ("full_house", [51, 38, 25, 11, 37, 23, 0]),
    // A A A A K.
    ("four_of_a_kind", [51, 38, 25, 12, 37, 23, 0]),
    // 9 8 7 6 5, all hearts.
    ("straight_flush", [33, 32, 31, 30, 29, 12, 24]),
    // A K Q J T, all hearts.
    ("royal_flush", [34, 35, 36, 37, 38, 0, 14]),
];

fn bench_hand_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("hand_eval");
    for (name, cards) in HANDS {
        group.bench_function(name, |b| {
            b.iter(|| evaluate_hand(black_box(&cards)));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_hand_eval);
criterion_main!(benches);
