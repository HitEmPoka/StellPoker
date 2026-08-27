/// Showdown Validity Circuit - Omaha variant
///
/// Extends the standard showdown circuit with Omaha rules:
///   - Each player is dealt exactly 4 hole cards.
///   - The best 5-card hand MUST be formed from EXACTLY 2 hole cards and
///     EXACTLY 3 of the 5 board cards. The 4-choose-2 (hole) x 5-choose-3
///     (board) combinatorics is enforced by enumerating every legal 2+3
///     construction in `evaluate_omaha_hand` (see lib/src/cards.nr).
///
/// The circuit derives the shared deck, verifies the deck root, checks every
/// active player's 4-card hand commitment, evaluates the Omaha best hand for
/// each player, and outputs the winner index plus the tie mask.

use stellar_poker_lib::cards;
use stellar_poker_lib::commitments;
use stellar_poker_lib::merkle;
use stellar_poker_lib::shuffle;

/// Deck size (single 52-card deck; Omaha does not use a multi-deck shoe).
global DECK_SIZE: u32 = 52;

/// Tree leaf count must be >= DECK_SIZE and power of 2.
global TREE_LEAVES: u32 = 64;
global TREE_DEPTH: u32 = 6;

/// Each Omaha player holds 4 hole cards.
global HOLE_CARDS: u32 = 4;

global MAX_PLAYERS: u32 = 6;

fn main(
    // Private inputs (secret-shared in MPC)
    party0_permutation: [u32; 52],
    party1_permutation: [u32; 52],
    party2_permutation: [u32; 52],
    party0_salts: [Field; 52],
    party1_salts: [Field; 52],
    party2_salts: [Field; 52],

    // Public inputs
    num_active_players: pub u32,
    hand_commitments: pub [Field; MAX_PLAYERS],
    board_indices: pub [u32; 5],
    deck_root: pub Field,
) -> pub (
    [u32; MAX_PLAYERS],
    [u32; MAX_PLAYERS],
    [u32; MAX_PLAYERS],
    [u32; MAX_PLAYERS],
    u32,
    u32,
) {
    assert(num_active_players >= 2, "need at least 2 players");
    assert(num_active_players <= MAX_PLAYERS, "too many active players");

    let (deck, salts) = shuffle::derive_shared_deck_and_salts(
        party0_permutation,
        party1_permutation,
        party2_permutation,
        party0_salts,
        party1_salts,
        party2_salts,
    );

    // 1. Recompute deck root and verify consistency with deal/reveal phases.
    let mut leaves: [Field; TREE_LEAVES] = [0; TREE_LEAVES];
    for i in 0..DECK_SIZE {
        leaves[i] = commitments::commit_card(deck[i], salts[i]);
    }
    let computed_root = merkle::compute_merkle_root_generic::<64, 6>(leaves);
    assert(computed_root == deck_root, "deck root mismatch");

    // 2. Resolve hole + board cards and check index uniqueness.
    // Omaha: player p occupies hole slots [4p, 4p+1, 4p+2, 4p+3].
    let mut used_indices: [bool; 52] = [false; 52];
    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            let base = p * HOLE_CARDS;
            for h in 0..HOLE_CARDS {
                let idx = base + h;
                assert(idx < 52, "hole card index out of range");
                assert(!used_indices[idx], "duplicate dealt index");
                used_indices[idx] = true;
            }
        }
    }

    let mut board_cards: [u32; 5] = [0; 5];
    for i in 0..5 {
        let idx = board_indices[i];
        assert(idx < 52, "board index out of range");
        assert(!used_indices[idx], "board index already used");
        used_indices[idx] = true;

        board_cards[i] = deck[idx] as u32;
    }

    // 3. Verify per-player commitments (4 hole cards) and evaluate Omaha hands.
    let mut hole_card1: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut hole_card2: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut hole_card3: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut hole_card4: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut hand_scores: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            let base = p * HOLE_CARDS;
            let idx1 = base;
            let idx2 = base + 1;
            let idx3 = base + 2;
            let idx4 = base + 3;

            hole_card1[p] = deck[idx1] as u32;
            hole_card2[p] = deck[idx2] as u32;
            hole_card3[p] = deck[idx3] as u32;
            hole_card4[p] = deck[idx4] as u32;

            let c1_commit = commitments::commit_card(deck[idx1], salts[idx1]);
            let c2_commit = commitments::commit_card(deck[idx2], salts[idx2]);
            let c3_commit = commitments::commit_card(deck[idx3], salts[idx3]);
            let c4_commit = commitments::commit_card(deck[idx4], salts[idx4]);
            let expected_commit = commitments::commit_hand_omaha(
                c1_commit,
                c2_commit,
                c3_commit,
                c4_commit,
            );
            assert(expected_commit == hand_commitments[p], "hand commitment mismatch");

            let hole_cards: [u32; 4] = [
                hole_card1[p],
                hole_card2[p],
                hole_card3[p],
                hole_card4[p],
            ];
            hand_scores[p] = cards::evaluate_omaha_hand(hole_cards, board_cards);
        }
    }

    // 4. Compute winner as first seat with maximal score and mark all tied seats.
    let mut winner_index: u32 = 0;
    let mut winner_score: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            if (p == 0) | (hand_scores[p] > winner_score) {
                winner_index = p;
                winner_score = hand_scores[p];
            }
        }
    }

    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            assert(winner_score >= hand_scores[p], "declared winner is not maximal");
        }
    }

    let bit_values: [u32; MAX_PLAYERS] = [1, 2, 4, 8, 16, 32];
    let mut tie_mask: u32 = 0;
    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            if hand_scores[p] == winner_score {
                tie_mask = tie_mask + bit_values[p];
            }
        }
    }

    (hole_card1, hole_card2, hole_card3, hole_card4, winner_index, tie_mask)
}
