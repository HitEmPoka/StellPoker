/// P-256 wallet authorization gadget (Issue #891).
///
/// A player authorizes one action by signing a domain-separated message that
/// binds the chain, table, hand commitment, action, amount, nonce and expiry.
/// The public key is supplied as witness data but must be bound to the player's
/// committed identity by the calling circuit; this module only proves that the
/// signature is valid for the supplied key and message.
///
/// Replay protection is provided by `nonce` (unique per hand/action) and
/// `expiry` (a ledger timestamp upper bound). A signed message cannot be reused
/// for a different table, hand, action or amount, and it stops being valid
/// after `expiry`. See `docs/wallet-auth-circuit.md`.

use std::ecdsa_secp256r1::verify_signature;
use std::hash::blake2s;

/// Build the 32-byte Blake2s digest that the wallet signs. Each field is
/// encoded as a fixed 32-byte big-endian value so there is no ambiguity
/// between neighbouring fields.
pub fn action_message(
    chain_id: Field,
    table_id: Field,
    hand_commitment: Field,
    action: Field,
    amount: Field,
    nonce: Field,
    expiry: Field,
) -> [u8; 32] {
    // ASCII "StellPoker/action/v1" — domain separator for this signature type.
    let domain: [u8; 20] = [
        83, 116, 101, 108, 108, 80, 111, 107, 101, 114, 47, 97, 99, 116, 105, 111, 110, 47, 118,
        49,
    ];
    let chain_bytes = chain_id.to_be_bytes::<32>();
    let table_bytes = table_id.to_be_bytes::<32>();
    let hand_bytes = hand_commitment.to_be_bytes::<32>();
    let action_bytes = action.to_be_bytes::<32>();
    let amount_bytes = amount.to_be_bytes::<32>();
    let nonce_bytes = nonce.to_be_bytes::<32>();
    let expiry_bytes = expiry.to_be_bytes::<32>();

    let mut preimage: [u8; 244] = [0; 244];
    for i in 0..20 {
        preimage[i] = domain[i];
    }
    for i in 0..32 {
        preimage[20 + i] = chain_bytes[i];
        preimage[52 + i] = table_bytes[i];
        preimage[84 + i] = hand_bytes[i];
        preimage[116 + i] = action_bytes[i];
        preimage[148 + i] = amount_bytes[i];
        preimage[180 + i] = nonce_bytes[i];
        preimage[212 + i] = expiry_bytes[i];
    }
    blake2s::<32>(preimage)
}

/// Verify a P-256 signature over the action message. Returns false for an
/// invalid signature, an expired authorization or a zero nonce. Callers must
/// additionally bind `public_key_x/public_key_y` to the seat's wallet identity.
pub fn is_authorized_wallet(
    chain_id: Field,
    table_id: Field,
    hand_commitment: Field,
    action: Field,
    amount: Field,
    nonce: Field,
    expiry: Field,
    now: Field,
    public_key_x: [u8; 32],
    public_key_y: [u8; 32],
    signature: [u8; 64],
) -> bool {
    if nonce == 0 {
        return false;
    }
    if now > expiry {
        return false;
    }
    let message = action_message(chain_id, table_id, hand_commitment, action, amount, nonce, expiry);
    verify_signature(message, public_key_x, public_key_y, signature)
}

/// Asserting form used by action circuits.
pub fn assert_authorized_wallet(
    chain_id: Field,
    table_id: Field,
    hand_commitment: Field,
    action: Field,
    amount: Field,
    nonce: Field,
    expiry: Field,
    now: Field,
    public_key_x: [u8; 32],
    public_key_y: [u8; 32],
    signature: [u8; 64],
) {
    assert(
        is_authorized_wallet(
            chain_id,
            table_id,
            hand_commitment,
            action,
            amount,
            nonce,
            expiry,
            now,
            public_key_x,
            public_key_y,
            signature,
        ),
        "invalid wallet authorization",
    );
}

#[test]
fn test_zero_nonce_is_rejected() {
    let x: [u8; 32] = [0; 32];
    let y: [u8; 32] = [0; 32];
    let sig: [u8; 64] = [0; 64];
    assert(!is_authorized_wallet(1, 2, 3, 4, 5, 0, 100, 10, x, y, sig));
}

#[test]
fn test_expired_authorization_is_rejected() {
    let x: [u8; 32] = [0; 32];
    let y: [u8; 32] = [0; 32];
    let sig: [u8; 64] = [0; 64];
    assert(!is_authorized_wallet(1, 2, 3, 4, 5, 1, 9, 10, x, y, sig));
}

#[test(should_fail_with = "invalid wallet authorization")]
fn test_invalid_signature_is_rejected() {
    let x: [u8; 32] = [0; 32];
    let y: [u8; 32] = [0; 32];
    let sig: [u8; 64] = [0; 64];
    assert_authorized_wallet(1, 2, 3, 4, 5, 1, 100, 10, x, y, sig);
}

#[test]
fn test_message_is_domain_separated() {
    // A different table must produce a different digest, so a signature for
    // one table cannot be replayed at another.
    let a = action_message(1, 2, 3, 4, 5, 6, 7);
    let b = action_message(1, 9, 3, 4, 5, 6, 7);
    assert(a != b);
}

#[test]
fn test_message_is_deterministic() {
    let a = action_message(1, 2, 3, 4, 5, 6, 7);
    let b = action_message(1, 2, 3, 4, 5, 6, 7);
    assert(a == b);
}
