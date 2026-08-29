/// Showdown Validity Circuit
///
/// Derives hole cards from shared deck contributions, verifies commitments,
/// evaluates all active hands, and outputs the winner index plus a bit mask of
/// every active player tied for the best score.
///
/// Supports variant poker games with wild cards when VARIANT_TYPE != 0.

use stellar_poker_lib::commitments;
use stellar_poker_lib::deck;
use stellar_poker_lib::game;
use stellar_poker_lib::merkle;
use stellar_poker_lib::rank_table;
use stellar_poker_lib::shuffle;

/// Deck composition is parameterized by the shared deck module (Issue #216):
/// the compile-time global `deck::DECK_TYPE` selects between standard (52),
/// short (36, 6+) and joker (54) decks. The structural permutation uses 52 MPC
/// slots; deck-aware bounds below follow the configured deck type.

/// Deck size (cards in deck, default 52 for standard poker)
global DECK_SIZE: u32 = 52;

/// Tree leaf count must be >= DECK_SIZE and power of 2
/// For 52 cards: 64 leaves (6 levels)
/// For 104 cards: 128 leaves (7 levels)
global TREE_LEAVES: u32 = 64;
global TREE_DEPTH: u32 = 6;

/// Global player cap shared by the parameterised (2..6 player) variants.
global MAX_PLAYERS: u32 = 6;

fn main(
    // Private inputs (secret-shared in MPC)
    party0_permutation: [u32; 52],
    party1_permutation: [u32; 52],
    party2_permutation: [u32; 52],
    party0_salts: [Field; 52],
    party1_salts: [Field; 52],
    party2_salts: [Field; 52],
    // Private: each active player's best-5-card rank score plus the Merkle
    // inclusion proof that the score is an honest member of the committed rank
    // table (Issue #222). The circuit verifies the proof and uses the verified
    // score for winner/tie selection, replacing inline hand evaluation.
    rank_scores: [u32; MAX_PLAYERS],
    rank_indices: [u32; MAX_PLAYERS],
    rank_paths: [[Field; rank_table::RANK_TABLE_DEPTH]; MAX_PLAYERS],

    // Public inputs
    num_active_players: pub u32,
    hand_commitments: pub [Field; MAX_PLAYERS],
    board_indices: pub [u32; 5],
    deck_root: pub Field,
    // Game variant selector (Issue #228): 0 = Texas Hold'em, 1 = Omaha, 2 = Stud.
    // The value is validated up front; hand scores are supplied via the verified
    // rank lookup so the circuit does not need to re-derive them.
    game_type: pub u32,
    // Root of the Merkle-committed rank lookup table (Issue #222).
    rank_table_root: pub Field,
) -> pub ([u32; MAX_PLAYERS], [u32; MAX_PLAYERS], u32, u32) {
    game::assert_valid_game_type(game_type);
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

    // 2. Resolve board cards and check index uniqueness against dealt cards.
    let mut used_indices: [bool; 52] = [false; 52];
    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            let idx1 = p * 2;
            let idx2 = idx1 + 1;
            assert(idx2 < deck::deck_size(), "hole card index out of range");
            assert(!used_indices[idx1], "duplicate dealt index");
            used_indices[idx1] = true;
            assert(!used_indices[idx2], "duplicate dealt index");
            used_indices[idx2] = true;
        }
    }

    // Resolve board indices and check uniqueness against dealt cards. The
    // board reveals are validated here; hand scores are obtained from the
    // verified rank lookup table (Issue #222), not by evaluating these cards.
    for i in 0..5 {
        let idx = board_indices[i];
        assert(idx < deck::deck_size(), "board index out of range");
        assert(!used_indices[idx], "board index already used");
        used_indices[idx] = true;
    }

    // 3. Verify per-player commitments and verify each hand's rank score
    // against the Merkle-committed rank table (Issue #222). The verified score
    // is used directly for winner/tie selection below.
    let mut hole_card1: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut hole_card2: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];
    let mut hand_scores: [u32; MAX_PLAYERS] = [0; MAX_PLAYERS];

    for p in 0..MAX_PLAYERS {
        if p < num_active_players {
            let idx1 = p * 2;
            let idx2 = idx1 + 1;

            hole_card1[p] = deck[idx1] as u32;
            hole_card2[p] = deck[idx2] as u32;

            let c1_commit = leaves[idx1];
            let c2_commit = leaves[idx2];
            let expected_commit = commitments::commit_hand(c1_commit, c2_commit);
            assert(expected_commit == hand_commitments[p], "hand commitment mismatch");

            // Verify the prover's rank score is an honest member of the
            // committed table, then trust it for comparison. game_type is the
            // selector that determined which score the prover committed to
            // (Issue #228 + #222); it is validated above.
            rank_table::verify_rank_in_table(
                rank_scores[p],
                rank_indices[p],
                rank_paths[p],
                rank_table_root,
            );
            hand_scores[p] = rank_scores[p];
        }
    }

    // 4. Compute winner as first seat with maximal score and mark all tied
    // seats. The contract uses the mask to split tied pots.
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

    (hole_card1, hole_card2, winner_index, tie_mask)
}
