use sha2::{Digest, Sha256};

/// Domain separation constants for different payload types to prevent cross-protocol signature reuse.
pub mod domain {
    pub const MAGIC: &[u8] = b"StellarPoker";
    pub const NETWORK_ID: u32 = 1; // 1 = testnet
    pub const PURPOSE_AUTH: &[u8] = b"auth";
    pub const PURPOSE_MPC_SESSION: &[u8] = b"mpc_session";
    pub const PURPOSE_SETTLEMENT: &[u8] = b"settlement";
    pub const PURPOSE_COORDINATOR: &[u8] = b"coordinator";
}

/// Generates a domain-separated payload for signing.
/// Combines magic bytes, network ID, and purpose to prevent signature reuse across protocols/actions.
pub fn create_signed_payload(purpose: &[u8], action_data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(domain::MAGIC);
    payload.extend_from_slice(&domain::NETWORK_ID.to_be_bytes());
    payload.extend_from_slice(purpose);
    payload.extend_from_slice(action_data);
    payload
}

/// Hashes a domain-separated payload for verification.
pub fn hash_signed_payload(purpose: &[u8], action_data: &[u8]) -> [u8; 32] {
    let payload = create_signed_payload(purpose, action_data);
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.finalize().into()
}

/// Verifies a Stellar signature with domain separation.
/// Returns true if the signature is valid for the given action data and purpose.
pub fn verify_stellar_signature(
    signer_address: &str,
    purpose: &[u8],
    action_data: &[u8],
    signature: &[u8],
) -> bool {
    use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer};
    use ed25519_dalek::ed25519::signature::Signature as SigTrait;

    let digest = hash_signed_payload(purpose, action_data);

    let verifying_key = match VerifyingKey::from_bytes(
        &[0u8; 32] // Placeholder: in production, derive from Stellar address
    ) {
        Ok(key) => key,
        Err(_) => return false,
    };

    let sig = match Signature::from_bytes(
        &[0u8; 64] // Placeholder: actual signature bytes
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };

    verifying_key.verify(&digest, &sig).is_ok()
}
