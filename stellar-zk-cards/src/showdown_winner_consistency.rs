//! Circuit/contract consistency suite for the showdown winner declaration
//! (issue #565, a focused follow-up to the differential fuzzing of #294).
//!
//! The showdown circuit publishes two values the poker-table contract settles
//! from: `winner_index`, the first seat holding the maximal hand score, and
//! `tie_mask`, one bit for every seat that shares that score. The contract's
//! own evaluator is [`evaluate_hand`]. This suite deals random hands to two to
//! six players, scores every seat with both evaluators, applies the circuit's
//! declaration rule to each set of scores, and compares the two declarations.
//!
//! Results fall into three classes:
//!
//! * **Exact** - same `winner_index` and `tie_mask`.
//! * **Kicker blind** - the declarations differ, but only because the circuit's
//!   `score_five` (`circuits/lib/src/cards.nr`) does not encode kickers for
//!   one pair, two pair, three of a kind or four of a kind. The circuit ties
//!   seats the contract separates by kicker. Every seat the contract ranks best
//!   is still in the circuit's `tie_mask`; the circuit only ever adds seats.
//! * **Hard** - anything else: the circuit drops or reorders a seat the
//!   contract ranks best, or ties seats of different categories. This is a
//!   failure.
//!
//! The suite fails on any hard divergence and on any divergence in a kicker-free
//! category (high card, straight, flush, full house, straight flush, royal
//! flush). Kicker-blind results are counted and reported so the nightly job
//! tracks them; `strict_agreement_including_kicker_ties` is the ignored test
//! that turns them into failures once the circuit encodes kickers.
//!
//! Iterations and the seed come from the environment so the nightly job can run
//! far more deals than a pull request does:
//!
//! * `SHOWDOWN_CONSISTENCY_ITERATIONS` - deals to play (default 10,000)
//! * `SHOWDOWN_CONSISTENCY_SEED` - PRNG seed (default fixed, for reproducibility)

use crate::evaluate_hand;
use crate::fuzz_hand_eval::circuit_evaluate_hand_rank;
use std::println;

const MAX_PLAYERS: usize = 6;
const DEFAULT_ITERATIONS: u64 = 10_000;
const DEFAULT_SEED: u64 = 0x5EED_CAFE_F00D_0565;

/// Categories whose circuit score encodes the whole hand, so the circuit and
/// the contract cannot legitimately disagree: high card (0), straight (4),
/// flush (5), full house (6), straight flush (8) and royal flush (9).
const KICKER_FREE_CATEGORIES: [u32; 6] = [0, 4, 5, 6, 8, 9];

// ---------------------------------------------------------------------------
// Deterministic randomness
// ---------------------------------------------------------------------------

/// SplitMix64: tiny, fast and identical on every platform, so a failing
/// `(seed, iteration)` pair reproduces anywhere.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn shuffled_deck(rng: &mut SplitMix64) -> [u32; 52] {
    let mut deck: [u32; 52] = core::array::from_fn(|i| i as u32);
    for i in (1..52usize).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        deck.swap(i, j);
    }
    deck
}

/// Seven cards per seat: two hole cards followed by the shared five-card board.
struct Deal {
    players: usize,
    cards: [[u32; 7]; MAX_PLAYERS],
}

fn random_deal(rng: &mut SplitMix64) -> Deal {
    let players = 2 + rng.below(5) as usize;
    let deck = shuffled_deck(rng);
    let mut cards = [[0u32; 7]; MAX_PLAYERS];
    for p in 0..players {
        cards[p][0] = deck[2 * p];
        cards[p][1] = deck[2 * p + 1];
        for b in 0..5 {
            cards[p][2 + b] = deck[2 * players + b];
        }
    }
    Deal { players, cards }
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// `(winner_index, tie_mask)` for a set of per-seat scores, by the rule in
/// `circuits/showdown_valid*/src/main.nr`: the winner is the first seat with a
/// strictly greater score than every earlier seat, and the mask holds one bit
/// (`1 << seat`) for every seat whose score equals the winner's. The contract
/// settles from these two values (`build_ranking_and_ties`).
fn declare_winner(scores: &[u32]) -> (u32, u32) {
    let mut winner_index = 0u32;
    let mut winner_score = 0u32;
    for (seat, &score) in scores.iter().enumerate() {
        if seat == 0 || score > winner_score {
            winner_index = seat as u32;
            winner_score = score;
        }
    }
    let mut tie_mask = 0u32;
    for (seat, &score) in scores.iter().enumerate() {
        if score == winner_score {
            tie_mask |= 1u32 << seat;
        }
    }
    (winner_index, tie_mask)
}

/// Structural rules every declaration must satisfy, whichever side made it.
fn assert_well_formed(who: &str, players: usize, declaration: (u32, u32)) {
    let (winner_index, tie_mask) = declaration;
    assert!((winner_index as usize) < players, "{who}: winner out of range");
    assert!(
        tie_mask & (1u32 << winner_index) != 0,
        "{who}: winner {winner_index} missing from tie mask {tie_mask:#b}"
    );
    assert!(
        tie_mask >> players == 0,
        "{who}: tie mask {tie_mask:#b} has bits beyond {players} seats"
    );
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Exact,
    KickerBlind,
    Hard(&'static str),
}

/// Compare the declaration derived from the contract's scores with the one
/// derived from the circuit's scores. Also returns the best hand's category.
fn classify(contract_scores: &[u32], circuit_scores: &[u32]) -> (Verdict, u32) {
    let contract = declare_winner(contract_scores);
    let circuit = declare_winner(circuit_scores);
    let best_category = contract_scores[contract.0 as usize] >> 28;

    if contract == circuit {
        return (Verdict::Exact, best_category);
    }

    // The circuit may merge seats the contract separates, but it must never
    // leave out a seat the contract ranks best.
    if contract.1 & !circuit.1 != 0 {
        return (
            Verdict::Hard("circuit tie mask omits a seat the contract ranks best"),
            best_category,
        );
    }
    // Merged seats must still share the contract's best category: the circuit
    // may only lose the tiebreak below the category, never the category.
    for (seat, &score) in contract_scores.iter().enumerate() {
        if circuit.1 & (1u32 << seat) != 0 && score >> 28 != best_category {
            return (
                Verdict::Hard("circuit ties seats of different categories"),
                best_category,
            );
        }
    }
    (Verdict::KickerBlind, best_category)
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Tally {
    exact: u64,
    kicker_blind: u64,
}

/// Play `iterations` random deals from `seed`. Panics with a reproducible
/// description on a hard divergence, on a divergence in a kicker-free category,
/// and, when `allow_kicker_blind` is false, on any divergence at all.
fn run(iterations: u64, seed: u64, allow_kicker_blind: bool) -> Tally {
    let mut rng = SplitMix64(seed);
    let mut tally = Tally::default();

    for iteration in 0..iterations {
        let deal = random_deal(&mut rng);
        let mut contract_scores = [0u32; MAX_PLAYERS];
        let mut circuit_scores = [0u32; MAX_PLAYERS];
        for p in 0..deal.players {
            contract_scores[p] = evaluate_hand(&deal.cards[p]).score;
            circuit_scores[p] = circuit_evaluate_hand_rank(deal.cards[p]);
        }
        let contract_scores = &contract_scores[..deal.players];
        let circuit_scores = &circuit_scores[..deal.players];

        assert_well_formed("contract", deal.players, declare_winner(contract_scores));
        assert_well_formed("circuit", deal.players, declare_winner(circuit_scores));

        let (verdict, best_category) = classify(contract_scores, circuit_scores);
        let context = |reason: &str| {
            std::format!(
                "{reason}\n  seed={seed} iteration={iteration} players={}\n  cards={:?}\n  \
                 contract scores={:?} -> {:?}\n  circuit scores={:?} -> {:?}",
                deal.players,
                &deal.cards[..deal.players],
                contract_scores,
                declare_winner(contract_scores),
                circuit_scores,
                declare_winner(circuit_scores),
            )
        };

        match verdict {
            Verdict::Exact => tally.exact += 1,
            Verdict::Hard(reason) => panic!("{}", context(reason)),
            Verdict::KickerBlind => {
                if KICKER_FREE_CATEGORIES.contains(&best_category) {
                    panic!(
                        "{}",
                        context("declarations differ in a kicker-free category")
                    );
                }
                if !allow_kicker_blind {
                    panic!("{}", context("declarations differ (kicker blind)"));
                }
                tally.kicker_blind += 1;
            }
        }
    }
    tally
}

fn report(iterations: u64, seed: u64, tally: &Tally) {
    println!(
        "[showdown-consistency] iterations={iterations} seed={seed} exact={} kicker_blind={}",
        tally.exact, tally.kicker_blind
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn circuit_and_contract_agree_on_the_showdown_winner() {
    let iterations = env_u64("SHOWDOWN_CONSISTENCY_ITERATIONS", DEFAULT_ITERATIONS);
    let seed = env_u64("SHOWDOWN_CONSISTENCY_SEED", DEFAULT_SEED);

    let tally = run(iterations, seed, true);
    report(iterations, seed, &tally);

    assert_eq!(tally.exact + tally.kicker_blind, iterations);
    // Most random deals are decided above the kicker, so exact agreement must
    // dominate; a collapse would mean the two evaluators have drifted apart.
    // Only meaningful with enough deals to average out.
    if iterations >= 1_000 {
        assert!(
            tally.exact > tally.kicker_blind,
            "kicker-blind results outnumber exact agreement: {tally:?}"
        );
    }
}

/// The same suite with every divergence a failure. Ignored while the circuit's
/// `score_five` drops kickers; run it with `-- --ignored` to see the count, and
/// remove the attribute when the circuit encodes them.
#[test]
#[ignore = "known divergence: circuit score_five has no kickers for pair, two pair, trips or quads"]
fn strict_agreement_including_kicker_ties() {
    let iterations = env_u64("SHOWDOWN_CONSISTENCY_ITERATIONS", DEFAULT_ITERATIONS);
    let seed = env_u64("SHOWDOWN_CONSISTENCY_SEED", DEFAULT_SEED);

    let tally = run(iterations, seed, false);
    report(iterations, seed, &tally);
}

#[test]
fn a_run_is_reproducible_from_its_seed() {
    let first = run(500, 7, true);
    let second = run(500, 7, true);

    assert_eq!(first.exact, second.exact);
    assert_eq!(first.kicker_blind, second.kicker_blind);
}

#[test]
fn declaration_rule_matches_the_circuit() {
    // First seat with the strictly greatest score wins; every equal seat is in
    // the mask, including seats before and after the winner.
    assert_eq!(declare_winner(&[5, 9, 3]), (1, 0b010));
    assert_eq!(declare_winner(&[9, 9, 3]), (0, 0b011));
    assert_eq!(declare_winner(&[3, 9, 9]), (1, 0b110));
    assert_eq!(declare_winner(&[7, 7, 7, 7, 7, 7]), (0, 0b111111));
    assert_eq!(declare_winner(&[1, 2]), (1, 0b10));
}

#[test]
fn circuit_ignores_the_kicker_on_a_pair_the_contract_ranks() {
    // Known divergence, pinned so it cannot change unnoticed.
    //
    // Board: 2c 5h 8d 9s Ah. Seat 0 holds Ac Qs, seat 1 holds Ad Kd: both
    // play a pair of aces, and seat 1 has the king kicker to seat 0's queen.
    //
    //   card = suit * 13 + rank, suits c/d/h/s = 0..3, ranks 2..A = 0..12.
    let board = [0, 29, 19, 46, 38];
    let seat0 = [12, 49];
    let seat1 = [25, 24];
    let mut cards = [[0u32; 7]; 2];
    for (p, hole) in [seat0, seat1].iter().enumerate() {
        cards[p][0] = hole[0];
        cards[p][1] = hole[1];
        cards[p][2..].copy_from_slice(&board);
    }

    let contract_scores = [
        evaluate_hand(&cards[0]).score,
        evaluate_hand(&cards[1]).score,
    ];
    let circuit_scores = [
        circuit_evaluate_hand_rank(cards[0]),
        circuit_evaluate_hand_rank(cards[1]),
    ];

    // Both seats make one pair (category 1).
    assert_eq!(contract_scores[0] >> 28, 1);
    assert_eq!(contract_scores[1] >> 28, 1);
    assert_eq!(circuit_scores[0] >> 20, 1);
    assert_eq!(circuit_scores[1] >> 20, 1);

    // The contract's evaluator separates them by kicker; the circuit's does not.
    assert_eq!(declare_winner(&contract_scores), (1, 0b10));
    assert_eq!(declare_winner(&circuit_scores), (0, 0b11));
    assert_eq!(
        classify(&contract_scores, &circuit_scores),
        (Verdict::KickerBlind, 1)
    );
}

#[test]
fn classification_flags_a_dropped_winner_as_hard() {
    // Seat 1 beats seat 0 for the contract, but the circuit prefers seat 0.
    let (verdict, _) = classify(&[1 << 28, 2 << 28], &[2 << 20, 1 << 20]);
    assert_eq!(
        verdict,
        Verdict::Hard("circuit tie mask omits a seat the contract ranks best")
    );
}

#[test]
fn classification_flags_a_cross_category_tie_as_hard() {
    // The circuit ties a pair with a high card, so its mask spans categories.
    let contract = [(1u32 << 28) | 5, 4u32];
    let circuit = [1u32 << 20, 1u32 << 20];
    let (verdict, _) = classify(&contract, &circuit);
    assert_eq!(
        verdict,
        Verdict::Hard("circuit ties seats of different categories")
    );
}
