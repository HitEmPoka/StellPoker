/// Action-order circuit, hardened with wallet authorization (Issue #891).
///
/// Proves that seats/actions are well formed and that the acting wallet signed
/// a domain-separated message binding chain, table, hand commitment, action,
/// amount, nonce and expiry. The nonce and expiry make a captured signature
/// non-replayable across hands or after its deadline.

use stellar_poker_lib::wallet_auth;

fn main(
    seat_positions: [u32; 6],
    action_sequence: [u32; 6],
    chain_id: Field,
    table_id: Field,
    hand_commitment: Field,
    action: Field,
    amount: Field,
    nonce: Field,
    expiry: Field,
    now: Field,
    wallet_pub_x: [u8; 32],
    wallet_pub_y: [u8; 32],
    wallet_signature: [u8; 64],
    pub_is_valid: pub bool,
) {
    // Seat and action indices must be in the six-seat domain and the action
    // sequence must not repeat a seat.
    for i in 0..6 {
        assert(seat_positions[i] < 6, "seat position out of range");
        assert(action_sequence[i] < 6, "action sequence index out of range");
        for j in i + 1..6 {
            assert(action_sequence[i] != action_sequence[j], "duplicate action sequence index");
        }
    }

    wallet_auth::assert_authorized_wallet(
        chain_id,
        table_id,
        hand_commitment,
        action,
        amount,
        nonce,
        expiry,
        now,
        wallet_pub_x,
        wallet_pub_y,
        wallet_signature,
    );

    assert(pub_is_valid == true);
}

#[test(should_fail_with = "invalid wallet authorization")]
fn test_invalid_wallet_signature_rejected() {
    let mut action_sequence: [u32; 6] = [0; 6];
    for i in 0..6 {
        action_sequence[i] = i as u32;
    }
    let x: [u8; 32] = [0; 32];
    let y: [u8; 32] = [0; 32];
    let sig: [u8; 64] = [0; 64];

    main(
        [0, 1, 2, 3, 4, 5],
        action_sequence,
        1,
        2,
        3,
        4,
        5,
        1,
        100,
        10,
        x,
        y,
        sig,
        true,
    );
}
