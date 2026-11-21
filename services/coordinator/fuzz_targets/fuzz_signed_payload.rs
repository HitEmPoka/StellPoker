#![no_main]

use coordinator::signed_payload::verify_signature;
use ed25519_dalek::{Signer, SigningKey};
use libfuzzer_sys::fuzz_target;
use base64::Engine;

fn fixed_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[0x42; 32])
}

fn encode_signature(bytes: &[u8], mode: u8) -> String {
    match mode % 4 {
        0 => hex::encode(bytes),
        1 => format!("0x{}", hex::encode(bytes)),
        2 => base64::engine::general_purpose::STANDARD.encode(bytes),
        _ => base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

/// Exercise the production verifier with valid and malformed encodings.
///
/// The target deliberately keeps the public key valid so mutations reach
/// signature parsing and both SEP-53 and legacy message verification paths.
fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }

    let signing_key = fixed_signing_key();
    let address = stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes())
        .to_string();
    let message = String::from_utf8_lossy(&data[1..]).into_owned();
    let signature = signing_key.sign(message.as_bytes()).to_bytes();
    let mut candidate = signature.to_vec();

    if data[0] & 1 == 1 {
        let index = (data[1] as usize) % candidate.len();
        candidate[index] ^= data[2].max(1);
    }

    if data[0] & 2 == 2 {
        candidate.extend_from_slice(&data[2..data.len().min(10)]);
    }

    let encoded = encode_signature(&candidate, data[0]);
    let result = verify_signature(&address, &message, &encoded);

    assert!(result.is_ok() || result == Err(axum::http::StatusCode::UNAUTHORIZED));
});
