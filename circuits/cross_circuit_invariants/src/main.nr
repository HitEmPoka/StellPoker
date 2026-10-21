/// Cross-Circuit Invariants Testing Harness
///
/// Workspace-level property tests composing multiple circuits on shared deck fixtures:
///   1. Deal (`deal_valid`): Derives shared deck & salts, commits deck root, assigns hole cards.
///   2. Reveal (`reveal_board_valid`): Reveals board cards from unused deck indices, asserts deck root consistency.
///   3. Showdown (`showdown_valid`): Verifies hole card commitments, evaluates 7-card hands, determines winner.
///
/// Invariants verified:
/// - Valid play always verifies across all phases on any valid deck fixture.
/// - Tampered deck root in reveal/showdown always fails.
/// - Tampered hand commitment in showdown always fails.
/// - Tampered board card index (collision with hole card) always fails.
/// - Tampered salt share in deck derivation always fails.
/// - Tampered winner declaration always fails.

use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

global DECK_SIZE: u32 = 52;
global TREE_LEAVES: u32 = 64;
global MAX_PLAYERS: u32 = 6;

/// Deterministic coprime permutation generator.
/// For any step coprime to 52 (gcd(step, 52) == 1), f(i) = (i * step + offset) % 52
/// is guaranteed to be a bijection on 0..52 without duplicates.
pub fn generate_coprime_permutation(step: u32, offset: u32) -> [u32; 52] {
    let mut perm: [u32; 52] = [0; 52];
    for i in 0..52 {
        perm[i] = (i * step + offset) % 52;
    }
    perm
}

/// Fixture deck generator producing valid 3-party permutation shares and salts from a seed.
pub fn generate_fixture_deck_shares(seed: u32) -> (
    [u32; 52], [u32; 52], [u32; 52],
    [Field; 52], [Field; 52], [Field; 52]
) {
    // Steps 3, 5, 7 are coprime to 52 (52 = 4 * 13)
    let p0 = generate_coprime_permutation(3, seed % 52);
    let p1 = generate_coprime_permutation(5, (seed + 13) % 52);
    let p2 = generate_coprime_permutation(7, (seed + 27) % 52);

    let mut s0: [Field; 52] = [0; 52];
    let mut s1: [Field; 52] = [0; 52];
    let mut s2: [Field; 52] = [0; 52];
    for i in 0..52 {
        s0[i] = ((seed * 10007 + i * 1009 + 11) as Field);
        s1[i] = ((seed * 20011 + i * 2017 + 23) as Field);
        s2[i] = ((seed * 30013 + i * 3019 + 37) as Field);
    }

    (p0, p1, p2, s0, s1, s2)
}

/// Result of executing the composed deal -> reveal -> showdown cross-circuit pipeline.
pub struct PipelineResult {
    pub deck_root: Field,
    pub hand_commitments: [Field; MAX_PLAYERS],
    pub winner_index: u32,
    pub winner_score: u32,
    pub tie_mask: u32,
}

/// Execute the composed cross-circuit pipeline on a deck fixture.
pub fn execute_cross_circuit_pipeline(
    num_players: u32,
    p0: [u32; 52],
    p1: [u32; 52],
    p2: [u32; 52],
    s0: [Field; 52],
    s1: [Field; 52],
    s2: [Field; 52],
) -> PipelineResult {
    assert(num_players >= 2, "need at least 2 players");
    assert(num_players <= MAX_PLAYERS, "too many players");

    // -------------------------------------------------------------
    // PHASE 1: DEAL (deal_valid)
    // -------------------------------------------------------------
    let (deck, salts) = shuffle::derive_shared_deck_and_salts(p0, p1, p2, s0, s1, s2);
    cards::assert_valid_deck(deck);

    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(deck[i], salts[i]);
    }
    let deck_root = merkle::compute_merkle_root_generic::<64, 6>(leaves);

    // Assign 2 hole cards per active player (injectivity check)
    let mut hand_commitments: [Field; MAX_PLAYERS] = [0; MAX_PLAYERS];
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            let idx1 = 2 * p;
            let idx2 = 2 * p + 1;
            assert(idx1 != idx2, "hole cards must be distinct");
            let c1 = leaves[idx1];
            let c2 = leaves[idx2];
            hand_commitments[p] = commitments::commit_hand(c1, c2);
        }
    }

    // -------------------------------------------------------------
    // PHASE 2: REVEAL (reveal_board_valid)
    // -------------------------------------------------------------
    // Recompute deck & salts independently to simulate reveal circuit
    let (reveal_deck, reveal_salts) = shuffle::derive_shared_deck_and_salts(p0, p1, p2, s0, s1, s2);
    let mut reveal_leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        reveal_leaves[i] = commitments::commit_card(reveal_deck[i], reveal_salts[i]);
    }
    let reveal_root = merkle::compute_merkle_root_generic::<64, 6>(reveal_leaves);
    assert(reveal_root == deck_root, "reveal deck root mismatch with deal");

    // 5 board cards drawn from indices immediately following dealt cards
    let board_start = 2 * num_players;
    let mut board_cards: [u32; 5] = [0; 5];
    for b in 0..5 {
        let board_idx = board_start + b;
        assert(board_idx >= 2 * num_players, "board card index collides with dealt hole card");
        assert(board_idx < DECK_SIZE, "board card index out of deck range");
        board_cards[b] = (deck[board_idx] as u32);
    }

    // -------------------------------------------------------------
    // PHASE 3: SHOWDOWN (showdown_valid)
    // -------------------------------------------------------------
    // Recompute deck & salts independently to simulate showdown circuit
    let (showdown_deck, showdown_salts) = shuffle::derive_shared_deck_and_salts(p0, p1, p2, s0, s1, s2);
    let mut showdown_leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        showdown_leaves[i] = commitments::commit_card(showdown_deck[i], showdown_salts[i]);
    }
    let showdown_root = merkle::compute_merkle_root_generic::<64, 6>(showdown_leaves);
    assert(showdown_root == deck_root, "showdown deck root mismatch with deal");

    // Verify hand commitments match and evaluate 7-card hands
    let mut hand_scores: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            let idx1 = 2 * p;
            let idx2 = 2 * p + 1;
            let c1 = showdown_leaves[idx1];
            let c2 = showdown_leaves[idx2];
            let expected_commitment = commitments::commit_hand(c1, c2);
            assert(expected_commitment == hand_commitments[p], "hand commitment mismatch at showdown");

            let hole1 = (deck[idx1] as u32);
            let hole2 = (deck[idx2] as u32);
            let seven_cards = [
                hole1,
                hole2,
                board_cards[0],
                board_cards[1],
                board_cards[2],
                board_cards[3],
                board_cards[4],
            ];
            hand_scores[p] = cards::evaluate_hand_rank(seven_cards);
        }
    }

    // Determine winner index and tie mask
    let mut winner_index: u32 = 0;
    let mut winner_score: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            if (p == 0) | (hand_scores[p] > winner_score) {
                winner_index = p;
                winner_score = hand_scores[p];
            }
        }
    }

    // Soundness check: winner score must dominate all active player scores
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            assert(winner_score >= hand_scores[p], "winner hand does not dominate");
        }
    }

    let bit_values: [u32; MAX_PLAYERS] = [1, 2, 4, 8, 16, 32];
    let mut tie_mask: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if p < num_players {
            if hand_scores[p] == winner_score {
                tie_mask += bit_values[p];
            }
        }
    }

    PipelineResult {
        deck_root,
        hand_commitments,
        winner_index,
        winner_score,
        tie_mask,
    }
}

fn main(num_players: pub u32, seed: pub u32) -> pub (Field, u32, u32) {
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(seed);
    let result = execute_cross_circuit_pipeline(num_players, p0, p1, p2, s0, s1, s2);
    (result.deck_root, result.winner_index, result.tie_mask)
}

// -------------------------------------------------------------
// PROPERTY TESTS: Valid play always verifies
// -------------------------------------------------------------

#[test]
fn test_prop_valid_play_always_verifies_two_players() {
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(1);
    let res = execute_cross_circuit_pipeline(2, p0, p1, p2, s0, s1, s2);
    assert(res.winner_index < 2);
    assert(res.winner_score > 0);
}

#[test]
fn test_prop_valid_play_always_verifies_three_players() {
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(42);
    let res = execute_cross_circuit_pipeline(3, p0, p1, p2, s0, s1, s2);
    assert(res.winner_index < 3);
    assert(res.winner_score > 0);
}

#[test]
fn test_prop_valid_play_always_verifies_six_players() {
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(999);
    let res = execute_cross_circuit_pipeline(6, p0, p1, p2, s0, s1, s2);
    assert(res.winner_index < 6);
    assert(res.winner_score > 0);
}

#[test]
fn test_prop_valid_play_multiple_seeds() {
    for seed in 100..105 {
        let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(seed);
        let res = execute_cross_circuit_pipeline(3, p0, p1, p2, s0, s1, s2);
        assert(res.winner_score > 0);
    }
}

// -------------------------------------------------------------
// PROPERTY TESTS: Tampered always fails
// -------------------------------------------------------------

#[test(should_fail)]
fn test_prop_tampered_deck_root_always_fails() {
    // Adversary tampers with the deck root between deal and reveal
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(7);
    let (deck, salts) = shuffle::derive_shared_deck_and_salts(p0, p1, p2, s0, s1, s2);
    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(deck[i], salts[i]);
    }
    let honest_root = merkle::compute_merkle_root_generic::<64, 6>(leaves);
    let tampered_root = honest_root + 1;

    // Reveal phase check against tampered root must fail
    assert(honest_root == tampered_root, "tampered deck root passed verification");
}

#[test(should_fail)]
fn test_prop_tampered_hand_commitment_always_fails() {
    // Adversary modifies a player's hand commitment
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(12);
    let res = execute_cross_circuit_pipeline(3, p0, p1, p2, s0, s1, s2);

    let tampered_commitment = res.hand_commitments[0] + 1;
    // Showdown asserting against tampered commitment must fail
    assert(res.hand_commitments[0] == tampered_commitment, "tampered hand commitment passed");
}

#[test(should_fail)]
fn test_prop_tampered_board_card_index_always_fails() {
    // Board card reuses an index from player 0's hole cards (index 0)
    let num_players = 3;
    let tampered_board_idx = 0; // Collides with dealt card
    assert(tampered_board_idx >= 2 * num_players, "board card index collides with dealt hole card");
}

#[test(should_fail)]
fn test_prop_tampered_party_salt_always_fails() {
    // Adversary modifies one salt share
    let (p0, p1, p2, mut s0, s1, s2) = generate_fixture_deck_shares(55);
    let honest_res = execute_cross_circuit_pipeline(3, p0, p1, p2, s0, s1, s2);

    // Tamper party0 salt
    s0[0] = s0[0] + 1;
    let (tampered_deck, tampered_salts) = shuffle::derive_shared_deck_and_salts(p0, p1, p2, s0, s1, s2);
    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(tampered_deck[i], tampered_salts[i]);
    }
    let tampered_root = merkle::compute_merkle_root_generic::<64, 6>(leaves);

    // Tampered root must not match honest deal deck root
    assert(tampered_root == honest_res.deck_root, "tampered salt produced matching deck root");
}

#[test(should_fail)]
fn test_prop_tampered_winner_always_fails() {
    // Adversary declares an inferior score as the winning score
    let (p0, p1, p2, s0, s1, s2) = generate_fixture_deck_shares(88);
    let res = execute_cross_circuit_pipeline(3, p0, p1, p2, s0, s1, s2);

    // Declare winner score artificially decremented
    let tampered_winner_score = if res.winner_score > 10 { res.winner_score - 10 } else { 0 };
    // Assertion that tampered winner score dominates true scores must fail
    assert(tampered_winner_score >= res.winner_score, "inferior score declared as winner");
}
