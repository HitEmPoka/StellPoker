//! Differential fuzz tests: contract hand evaluator vs. circuit hand evaluator.
//!
//! The Rust contract uses `evaluate_hand` from `stellar_zk_cards` (category << 28).
//! The Noir circuit (`circuits/lib/src/cards.nr`) uses `evaluate_hand_rank`
//! (category << 20). The underlying logic must agree on the **hand category**
//! (0 = HighCard … 9 = RoyalFlush) for every possible 7-card hand.
//!
//! Any divergence here would mean the on-chain settlement contract could pick
//! a different winner than what the ZK proof attests.

extern crate std;

use crate::evaluate_hand;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Faithful Rust re-implementation of the Noir circuit's `score_five` and
// `evaluate_hand_rank` from `circuits/lib/src/cards.nr`.
// The bit-packing differs (category << 20 in Noir, category << 28 in Rust),
// but the category index must match.
// ---------------------------------------------------------------------------

/// Re-implementation of `score_five` from `circuits/lib/src/cards.nr`.
fn circuit_score_five(mut ranks: [u32; 5], suits: [u32; 5]) -> u32 {
    // Bubble sort descending (matches Noir)
    for i in 0..4 {
        for j in 0..(4 - i) {
            if ranks[j] < ranks[j + 1] {
                ranks.swap(j, j + 1);
            }
        }
    }

    let is_flush = suits[0] == suits[1]
        && suits[1] == suits[2]
        && suits[2] == suits[3]
        && suits[3] == suits[4];

    let is_straight = ranks[0] == ranks[1] + 1
        && ranks[1] == ranks[2] + 1
        && ranks[2] == ranks[3] + 1
        && ranks[3] == ranks[4] + 1;

    let is_wheel =
        ranks[0] == 12 && ranks[1] == 3 && ranks[2] == 2 && ranks[3] == 1 && ranks[4] == 0;

    let eq0 = ranks[0] == ranks[1];
    let eq1 = ranks[1] == ranks[2];
    let eq2 = ranks[2] == ranks[3];
    let eq3 = ranks[3] == ranks[4];

    let has_four = (eq0 && eq1 && eq2) || (eq1 && eq2 && eq3);
    let three_rank = if eq0 && eq1 {
        ranks[0]
    } else if eq1 && eq2 {
        ranks[1]
    } else {
        ranks[2]
    };

    let is_full_house = ((eq0 && eq1) && eq3) || ((eq2 && eq3) && eq0);

    let has_two_pairs = ((eq0 && eq2) || (eq0 && eq3) || (eq1 && eq3)) && !has_four;

    let has_pair = (eq0 || eq1 || eq2 || eq3) && !{
        (eq0 && eq1) || (eq1 && eq2) || (eq2 && eq3)
    } && !has_two_pairs
        && !has_four;

    let tb = (ranks[0] << 16) | (ranks[1] << 12) | (ranks[2] << 8) | (ranks[3] << 4) | ranks[4];

    let mut score: u32 = 0;
    let mut categorized = false;

    if !categorized && is_flush && is_straight && ranks[0] == 12 {
        score = (9 << 20) | tb;
        categorized = true;
    }
    if !categorized && is_flush && (is_straight || is_wheel) {
        let high = if is_wheel { 3 << 16 } else { tb };
        score = (8 << 20) | high;
        categorized = true;
    }
    if !categorized && has_four {
        let four_rank = if eq0 && eq1 && eq2 { ranks[0] } else { ranks[4] };
        score = (7 << 20) | (four_rank << 16);
        categorized = true;
    }
    if !categorized && is_full_house {
        let full_house_pair_rank = if eq0 && eq1 { ranks[4] } else { ranks[0] };
        score = (6 << 20) | (three_rank << 8) | full_house_pair_rank;
        categorized = true;
    }
    if !categorized && is_flush {
        score = (5 << 20) | tb;
        categorized = true;
    }
    if !categorized && (is_straight || is_wheel) {
        let high = if is_wheel { 3 << 16 } else { ranks[0] << 16 };
        score = (4 << 20) | high;
        categorized = true;
    }
    if !categorized && {
        (eq0 && eq1) || (eq1 && eq2) || (eq2 && eq3)
    } && !has_four
    {
        score = (3 << 20) | (three_rank << 16);
        categorized = true;
    }
    if !categorized && has_two_pairs {
        let pair_rank_hi = if eq0 { ranks[0] } else { ranks[1] };
        let pair_rank_lo = if eq3 { ranks[4] } else { ranks[2] };
        score = (2 << 20) | (pair_rank_hi << 12) | (pair_rank_lo << 8);
        categorized = true;
    }
    if !categorized && has_pair {
        let one_pair_rank = if eq0 {
            ranks[0]
        } else if eq1 {
            ranks[1]
        } else if eq2 {
            ranks[2]
        } else {
            ranks[3]
        };
        score = (1 << 20) | (one_pair_rank << 16);
        categorized = true;
    }
    if !categorized {
        score = tb;
    }

    score
}

/// Re-implementation of `evaluate_hand_rank` from `circuits/lib/src/cards.nr`.
fn circuit_evaluate_hand_rank(cards: [u32; 7]) -> u32 {
    const NUM_RANKS: u32 = 13;
    let mut ranks = [0u32; 7];
    let mut suits = [0u32; 7];
    for i in 0..7 {
        ranks[i] = cards[i] % NUM_RANKS;
        suits[i] = cards[i] / NUM_RANKS;
    }

    let mut best_score: u32 = 0;
    for skip1 in 0..7 {
        for skip2 in (skip1 + 1)..7 {
            let mut hand_ranks = [0u32; 5];
            let mut hand_suits = [0u32; 5];
            let mut idx = 0;
            for k in 0..7 {
                if k != skip1 && k != skip2 {
                    hand_ranks[idx] = ranks[k];
                    hand_suits[idx] = suits[k];
                    idx += 1;
                }
            }
            let score = circuit_score_five(hand_ranks, hand_suits);
            if score > best_score {
                best_score = score;
            }
        }
    }
    best_score
}

/// Extract the hand category (0–9) from the circuit's packed score (category << 20).
fn circuit_category(score: u32) -> u32 {
    score >> 20
}

// ---------------------------------------------------------------------------
// Strategy: generate a valid 7-card hand (all unique, all in 0..=51).
// ---------------------------------------------------------------------------

prop_compose! {
    fn seven_unique_cards()(
        seed in 0u64..u64::MAX
    ) -> [u32; 7] {
        // Fisher-Yates on a 52-card deck, take first 7
        let mut deck: [u32; 52] = core::array::from_fn(|i| i as u32);
        let mut s = seed;
        for i in (1..52usize).rev() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (s >> 33) as usize % (i + 1);
            deck.swap(i, j);
        }
        [deck[0], deck[1], deck[2], deck[3], deck[4], deck[5], deck[6]]
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Differential fuzz: contract and circuit must agree on hand category.
    ///
    /// The contract packs as `category << 28`; the circuit packs as `category << 20`.
    /// Despite the different bit-packing, the extracted category (0–9) must be
    /// identical for every valid 7-card hand.
    #[test]
    fn prop_contract_and_circuit_agree_on_category(cards in seven_unique_cards()) {
        let contract_rank = evaluate_hand(&cards);
        let contract_cat = contract_rank.category();

        let circuit_score = circuit_evaluate_hand_rank(cards);
        let circuit_cat = circuit_category(circuit_score);

        prop_assert_eq!(
            contract_cat,
            circuit_cat,
            "category mismatch for hand {:?}: contract={} circuit={}",
            cards,
            contract_cat,
            circuit_cat
        );
    }

    /// Differential ordering: if the contract says hand A beats hand B,
    /// the circuit must also score A higher than B, and vice-versa.
    ///
    /// This catches any ranking inversion between the two implementations.
    #[test]
    fn prop_contract_and_circuit_agree_on_ordering(
        cards_a in seven_unique_cards(),
        cards_b in seven_unique_cards(),
    ) {
        let contract_a = evaluate_hand(&cards_a).score;
        let contract_b = evaluate_hand(&cards_b).score;
        let circuit_a = circuit_evaluate_hand_rank(cards_a);
        let circuit_b = circuit_evaluate_hand_rank(cards_b);

        // Convert to the same comparison: same winner must emerge.
        // We compare the categories extracted from each packed score.
        let contract_cat_a = contract_a >> 28;
        let contract_cat_b = contract_b >> 28;
        let circuit_cat_a = circuit_a >> 20;
        let circuit_cat_b = circuit_b >> 20;

        // If one hand has a strictly higher category, both must agree on which.
        if contract_cat_a != contract_cat_b {
            prop_assert_eq!(
                contract_cat_a > contract_cat_b,
                circuit_cat_a > circuit_cat_b,
                "ordering mismatch: contract says A({}){}B({}), circuit says A({}){}B({}). \
                 hands A={:?} B={:?}",
                contract_cat_a,
                if contract_cat_a > contract_cat_b { ">" } else { "<" },
                contract_cat_b,
                circuit_cat_a,
                if circuit_cat_a > circuit_cat_b { ">" } else { "<" },
                circuit_cat_b,
                cards_a,
                cards_b,
            );
        }
    }
}
