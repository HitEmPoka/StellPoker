// Poseidon2 commitment utilities for Stellar Poker.
// Uses Poseidon2 permutation over BN254.

// Poseidon2 hash: message_length must equal array length
fn poseidon2_hash_2(a: Field, b: Field) -> Field {
    // For Noir 1.0.0-beta, poseidon2_permutation requires array len == message_length
    // Using a 4-element state with all 4 as message
    std::hash::poseidon2_permutation([a, b, 0, 0], 4)[0]
}

fn poseidon2_hash_3(a: Field, b: Field, c: Field) -> Field {
    std::hash::poseidon2_permutation([a, b, c, 0], 4)[0]
}

// Commit to a single card: H(card_value, salt)
pub fn commit_card(card: Field, salt: Field) -> Field {
    poseidon2_hash_2(card, salt)
}

// Commit to a player's hand (2 hole cards): H(commit1, commit2)
pub fn commit_hand(card1_commit: Field, card2_commit: Field) -> Field {
    poseidon2_hash_2(card1_commit, card2_commit)
}

// Commit to an Omaha player's hand (4 hole-card commitments):
// H(H(c1_commit, c2_commit), H(c3_commit, c4_commit)).
// Structured so the 4 hole cards are faithfully bound into a single commitment
// that the showdown circuit can verify against the dealt cards.
pub fn commit_hand_omaha(
    c1_commit: Field,
    c2_commit: Field,
    c3_commit: Field,
    c4_commit: Field,
) -> Field {
    let h12 = commit_hand(c1_commit, c2_commit);
    let h34 = commit_hand(c3_commit, c4_commit);
    commit_hand(h12, h34)
}

// Commit to 3 board cards
pub fn commit_board_3(c1: Field, c2: Field, c3: Field, salt: Field) -> Field {
    let h1 = poseidon2_hash_2(c1, c2);
    poseidon2_hash_3(h1, c3, salt)
}

// Commit to 1 additional board card
pub fn commit_board_1(card: Field, prev_board_commit: Field, salt: Field) -> Field {
    poseidon2_hash_3(prev_board_commit, card, salt)
}

// Verify a card commitment
pub fn verify_card_commitment(card: Field, salt: Field, expected_commit: Field) {
    let computed = commit_card(card, salt);
    assert(computed == expected_commit, "card commitment mismatch");
}

#[test]
fn test_card_commitment_roundtrip() {
    let card: Field = 42;
    let salt: Field = 123456789;
    let commitment = commit_card(card, salt);

    let commitment2 = commit_card(card, salt);
    assert(commitment == commitment2);

    let commitment3 = commit_card(card, 987654321);
    assert(commitment != commitment3);
}

#[test]
fn test_hand_commitment() {
    let c1_commit = commit_card(10, 111);
    let c2_commit = commit_card(25, 222);
    let hand = commit_hand(c1_commit, c2_commit);

    let hand2 = commit_hand(c1_commit, c2_commit);
    assert(hand == hand2);
}

#[test]
fn test_hand_commitment_omaha() {
    let c1 = commit_card(10, 111);
    let c2 = commit_card(25, 222);
    let c3 = commit_card(39, 333);
    let c4 = commit_card(44, 444);
    let hand = commit_hand_omaha(c1, c2, c3, c4);

    let hand2 = commit_hand_omaha(c1, c2, c3, c4);
    assert(hand == hand2);

    // Different 4th card must change the commitment.
    let c4b = commit_card(45, 445);
    let hand3 = commit_hand_omaha(c1, c2, c3, c4b);
    assert(hand != hand3);
}
