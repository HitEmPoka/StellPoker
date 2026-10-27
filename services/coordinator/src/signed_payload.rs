use axum::http::StatusCode;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// Verify a Stellar wallet signature against a signed payload.
pub fn verify_signature(
    address: &str,
    message: &str,
    signature_raw: &str,
) -> Result<(), StatusCode> {
    let stellar_pk = stellar_strkey::ed25519::PublicKey::from_string(address)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let verifying_key =
        VerifyingKey::from_bytes(&stellar_pk.0).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let signature = decode_signature(signature_raw)?;

    if verifying_key.verify(message.as_bytes(), &signature).is_ok() {
        return Ok(());
    }

    let mut hasher = Sha256::new();
    hasher.update(b"Stellar Signed Message:\n");
    hasher.update(message.as_bytes());
    let message_hash: [u8; 32] = hasher.finalize().into();

    verifying_key
        .verify(&message_hash, &signature)
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

fn decode_signature(signature_raw: &str) -> Result<Signature, StatusCode> {
    let s = signature_raw.trim();
    let decoded = if let Some(hex) = s.strip_prefix("0x") {
        hex::decode(hex).map_err(|_| StatusCode::UNAUTHORIZED)?
    } else if s.len() == 128 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        hex::decode(s).map_err(|_| StatusCode::UNAUTHORIZED)?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    };

    let normalized: [u8; 64] = if decoded.len() == 64 {
        decoded
            .as_slice()
            .try_into()
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    } else if decoded.len() == 68 {
        decoded[4..68]
            .try_into()
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    } else if decoded.len() == 72 && decoded[4..8] == [0, 0, 0, 64] {
        decoded[8..72]
            .try_into()
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    Ok(Signature::from_bytes(&normalized))
}
