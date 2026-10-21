// Game-type parameterization for hand evaluation (Issue #228).
//
// A single circuit can be parameterized by a `game_type` public input which
// selects the hand-evaluation rule:
//
//   GAME_TYPE_HOLDEM = 0  -> Texas Hold'em: best 5 of the player's 7 cards.
//   GAME_TYPE_OMAHA  = 1  -> Omaha: a legal 5-card hand MUST use EXACTLY 2 of
//                            the player's hole cards and EXACTLY 3 of the 5
//                            board cards.
//   GAME_TYPE_STUD   = 2  -> Seven-card stud / five-card stud: best 5 of the
//                            available 7 cards (with optional board).
//
// Each rule is implemented as a standalone function (a "sub-circuit") and the
// `evaluate_for_game` dispatcher selects among them. In Noir a runtime-valued
// `if` compiles both branches and multiplexes the output with a selector, so
// no unbounded dynamic branching is introduced; every branch stays a
// compile-time-bounded constraint region.

pub global GAME_TYPE_HOLDEM: u32 = 0;
pub global GAME_TYPE_OMAHA: u32 = 1;
pub global GAME_TYPE_STUD: u32 = 2;

// True when both hole cards have rank >= threshold (Issue #211). A "poker
// range" is parametrized by a minimum rank: e.g. threshold 10 means the player
// holds "queen or better". rank(card) is defined in cards::rank.
pub fn hole_range_is_satisfied(card1: u32, card2: u32, threshold: u32) -> bool {
    let rank1 = crate::cards::rank(card1 as Field);
    let rank2 = crate::cards::rank(card2 as Field);
    (rank1 >= threshold) & (rank2 >= threshold)
}

// Assert that both hole cards lie within the claimed range and are legal.
pub fn assert_hole_in_range(card1: u32, card2: u32, threshold: u32) {
    assert(threshold <= 12, "threshold out of range");
    assert(hole_range_is_satisfied(card1, card2, threshold), "hole cards outside range");
}

// Confirm a value is a supported game_type (0, 1 or 2).
pub fn assert_valid_game_type(game_type: u32) {
    let valid = (game_type == GAME_TYPE_HOLDEM)
        | (game_type == GAME_TYPE_OMAHA)
        | (game_type == GAME_TYPE_STUD);
    assert(valid, "invalid game_type");
}

// Texas Hold'em: best 5 of 7 (2 hole + 5 board).
pub fn evaluate_holdem(hole: [u32; 2], board: [u32; 5]) -> u32 {
    let cards: [u32; 7] = [
        hole[0],
        hole[1],
        board[0],
        board[1],
        board[2],
        board[3],
        board[4],
    ];
    crate::cards::evaluate_hand_rank(cards)
}

// Omaha with a 2-hole-card layout: because the hand is built from the exact
// pair of provided hole cards (2-choose-2 = 1 way) plus exactly 3 of the 5
// board cards (5-choose-3 = 10 ways), the "exactly 2 hole + exactly 3 board"
// rule is enforced structurally. A caller that tries to sneak a third hole
// card in has nowhere to put it; only board slots 0..4 are ever consumed.
pub fn evaluate_omaha_2hole(hole: [u32; 2], board: [u32; 5]) -> u32 {
    let mut best_score: u32 = 0;
    // C(5,3) = 10 choices of the three board cards.
    for b1 in 0..5 {
        for b2 in (b1 + 1)..5 {
            for b3 in (b2 + 1)..5 {
                let hand: [u32; 5] = [hole[0], hole[1], board[b1], board[b2], board[b3]];
                let score = crate::cards::best_five_cards(hand);
                if score > best_score {
                    best_score = score;
                }
            }
        }
    }
    best_score
}

// Seven-card stud: best 5 of the 7 available cards (3 down + 4 up, or the
// 2 hole + 5 board layout used by the shared showdown circuit).
pub fn evaluate_stud(hole: [u32; 2], board: [u32; 5]) -> u32 {
    evaluate_holdem(hole, board)
}

// Dispatch hand evaluation by a runtime `game_type` value.
//
// Exactly one branch is selected by the generated multiplexer. `game_type` is
// validated up front so any unsupported value fails the circuit.
pub fn evaluate_for_game(game_type: u32, hole: [u32; 2], board: [u32; 5]) -> u32 {
    assert_valid_game_type(game_type);
    if game_type == GAME_TYPE_HOLDEM {
        evaluate_holdem(hole, board)
    } else if game_type == GAME_TYPE_OMAHA {
        evaluate_omaha_2hole(hole, board)
    } else {
        evaluate_stud(hole, board)
    }
}

#[test]
fn test_game_type_holdem_matches_legacy_evaluation() {
    // The Hold'em branch must agree with the original shared evaluator so that
    // existing showdown behavior is unchanged (Issue #228 regression guard).
    // Hole: Ac, Ad   Board: 2c,3c,4c,5c,6c
    let hole: [u32; 2] = [12, 25];
    let board: [u32; 5] = [0, 1, 2, 3, 4];
    let seven: [u32; 7] = [12, 25, 0, 1, 2, 3, 4];
    let via_game = evaluate_for_game(0, hole, board);
    let via_legacy = crate::cards::evaluate_hand_rank(seven);
    assert(via_game == via_legacy, "holdem branch must match legacy evaluator");
}

#[test]
fn test_game_type_valid_range() {
    assert_valid_game_type(0);
    assert_valid_game_type(1);
    assert_valid_game_type(2);
}

#[test]
fn test_game_type_stud_has_five_card_best() {
    // A 7-card flush should be detected by the stud branch just like hold'em.
    let hole: [u32; 2] = [51, 50]; // As, Ks
    let board: [u32; 5] = [49, 48, 47, 0, 1]; // Qs, Js, Ts, 2c, 3c
    let score = evaluate_for_game(2, hole, board);
    let category = score >> 20;
    assert(category == 9, "stud should recognise the royal flush");
}

#[test]
fn test_hole_range_is_satisfied() {
    // Ac (12), Ad (25): both aces -> satisfies "queen or better" (10).
    assert(hole_range_is_satisfied(12, 25, 10));
    // Exact boundary: Ac (12) with rank 12 and threshold 12 (ace) -> in range.
    assert(hole_range_is_satisfied(51, 38, 12));
    // Kc (11) with threshold 11 (king) and Qc (10) -> in range.
    assert(hole_range_is_satisfied(11, 10, 10));
    // A low card (2c, rank 0) must fail a high threshold (10).
    assert(!hole_range_is_satisfied(0, 25, 10));
    // Two low cards pass only a low (or zero) threshold.
    assert(hole_range_is_satisfied(0, 1, 0));
    assert(!hole_range_is_satisfied(0, 1, 5));
}
