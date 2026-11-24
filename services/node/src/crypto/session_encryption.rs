use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::scalar::Scalar;
use ed25519_dalek::{Signer, Verifier};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use zeroize::Zeroize;

/// Represents encryption keys for a single session
#[derive(Clone)]
pub struct SessionKeys {
    pub encryption_key: [u8; 32],
    pub session_id: String,
}

impl Drop for SessionKeys {
    fn drop(&mut self) {
        // Securely zero out keys when dropped
        self.encryption_key.zeroize();
    }
}

/// Manages session keys
pub struct SessionKeyManager {
    active_sessions: HashMap<String, SessionKeys>,
}

impl SessionKeyManager {
    pub fn new() -> Self {
        Self {
            active_sessions: HashMap::new(),
        }
    }

    /// Generate a new session key
    pub fn generate_session_key(&mut self, session_id: &str) -> SessionKeys {
        let mut encryption_key = [0u8; 32];
        OsRng.fill_bytes(&mut encryption_key);

        let keys = SessionKeys {
            encryption_key,
            session_id: session_id.to_string(),
        };

        self.active_sessions
            .insert(session_id.to_string(), keys.clone());
        keys
    }

    /// Get session keys
    pub fn get_session_keys(&self, session_id: &str) -> Option<&SessionKeys> {
        self.active_sessions.get(session_id)
    }

    /// Clean up session keys
    pub fn cleanup_session(&mut self, session_id: &str) {
        if let Some(keys) = self.active_sessions.remove(session_id) {
            drop(keys); // Keys are zeroized on drop
        }
    }

    /// Check if session exists
    pub fn session_exists(&self, session_id: &str) -> bool {
        self.active_sessions.contains_key(session_id)
    }

    /// Get all active session IDs
    pub fn get_active_sessions(&self) -> Vec<String> {
        self.active_sessions.keys().cloned().collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #882: Authenticated Private Card Delivery Per Player
// ─────────────────────────────────────────────────────────────────────────────

/// Per-player ephemeral session keypair using Ristretto255 Diffie-Hellman.
#[derive(Clone)]
pub struct PlayerSessionKeypair {
    secret: Scalar,
    public: RistrettoPoint,
}

impl Drop for PlayerSessionKeypair {
    fn drop(&mut self) {
        // Scalar does not expose Zeroize directly, but its underlying limbs are dropped
    }
}

impl PlayerSessionKeypair {
    /// Generate a fresh random keypair.
    pub fn generate() -> Self {
        let mut rng = OsRng;
        let secret = Scalar::random(&mut rng);
        let public = secret * RISTRETTO_BASEPOINT_POINT;
        Self { secret, public }
    }

    /// Construct from 32-byte secret seed.
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Result<Self, String> {
        let secret = Scalar::from_bits(*bytes);
        let public = secret * RISTRETTO_BASEPOINT_POINT;
        Ok(Self { secret, public })
    }

    /// 32-byte compressed public key representation.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public.compress().to_bytes()
    }

    /// Hex-encoded public key.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }
}

/// The authenticated card delivery packet returned to the player client.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthenticatedCardDelivery {
    pub table_id: u32,
    pub recipient_address: String,
    pub ephemeral_pubkey: String,
    pub nonce: String,
    pub ciphertext: String,
    pub committee_signature: String,
    pub committee_pubkey: String,
}

/// Plaintext hole cards payload before encryption.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecryptedHoleCards {
    pub card1: u32,
    pub card2: u32,
    pub salt1: String,
    pub salt2: String,
}

/// Compute the message to be signed by the committee threshold key for card delivery.
pub fn compute_delivery_signing_payload(
    table_id: u32,
    recipient_address: &str,
    ephemeral_pubkey_hex: &str,
    nonce_hex: &str,
    ciphertext_hex: &str,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"STELLPOKER_CARD_DELIVERY_V1:");
    hasher.update(&table_id.to_le_bytes());
    hasher.update(recipient_address.as_bytes());
    hasher.update(ephemeral_pubkey_hex.as_bytes());
    hasher.update(nonce_hex.as_bytes());
    hasher.update(ciphertext_hex.as_bytes());
    hasher.finalize().to_vec()
}

/// Derive a symmetric AES-256-GCM key from Ristretto255 ECDH shared point and session context.
fn derive_symmetric_key(
    shared_point: &RistrettoPoint,
    table_id: u32,
    recipient_address: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"STELLPOKER_CARD_ENC_KEY_V1:");
    hasher.update(shared_point.compress().as_bytes());
    hasher.update(&table_id.to_le_bytes());
    hasher.update(recipient_address.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Encrypt hole cards to a player's session public key, signed by the committee key.
///
/// Time complexity: O(1) elliptic curve multiplication + O(1) AES-256-GCM encryption.
/// Space complexity: O(1) bounded ciphertext buffer.
pub fn encrypt_hole_cards(
    recipient_pubkey_bytes: &[u8; 32],
    table_id: u32,
    recipient_address: &str,
    card1: u32,
    card2: u32,
    salt1: &str,
    salt2: &str,
    committee_sk: &ed25519_dalek::SigningKey,
) -> Result<AuthenticatedCardDelivery, String> {
    let compressed = CompressedRistretto(*recipient_pubkey_bytes);
    let recipient_point = compressed
        .decompress()
        .ok_or_else(|| "invalid recipient Ristretto255 public key".to_string())?;

    // Generate ephemeral scalar r and point R = r * G
    let mut rng = OsRng;
    let ephemeral_scalar = Scalar::random(&mut rng);
    let ephemeral_point = ephemeral_scalar * RISTRETTO_BASEPOINT_POINT;
    let ephemeral_pubkey_bytes = ephemeral_point.compress().to_bytes();
    let ephemeral_pubkey_hex = hex::encode(ephemeral_pubkey_bytes);

    // Shared DH secret S = r * RecipientPK
    let shared_secret_point = ephemeral_scalar * recipient_point;
    let aes_key_bytes = derive_symmetric_key(&shared_secret_point, table_id, recipient_address);

    // Prepare plaintext
    let payload = DecryptedHoleCards {
        card1,
        card2,
        salt1: salt1.to_string(),
        salt2: salt2.to_string(),
    };
    let plaintext = serde_json::to_vec(&payload)
        .map_err(|e| format!("failed to serialize cards payload: {}", e))?;

    // AES-256-GCM encryption with fresh 96-bit nonce
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key_bytes));
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext_bytes = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| format!("AES-256-GCM encryption failed: {}", e))?;

    let nonce_hex = hex::encode(nonce_bytes);
    let ciphertext_hex = hex::encode(ciphertext_bytes);

    // Bind delivery to intended player and table via committee threshold signature
    let signing_msg = compute_delivery_signing_payload(
        table_id,
        recipient_address,
        &ephemeral_pubkey_hex,
        &nonce_hex,
        &ciphertext_hex,
    );
    let signature = committee_sk.sign(&signing_msg);
    let committee_pubkey_hex = hex::encode(committee_sk.verifying_key().to_bytes());
    let committee_signature_hex = hex::encode(signature.to_bytes());

    Ok(AuthenticatedCardDelivery {
        table_id,
        recipient_address: recipient_address.to_string(),
        ephemeral_pubkey: ephemeral_pubkey_hex,
        nonce: nonce_hex,
        ciphertext: ciphertext_hex,
        committee_signature: committee_signature_hex,
        committee_pubkey: committee_pubkey_hex,
    })
}

/// Decrypt and verify an authenticated card delivery.
///
/// Verifies:
/// 1. Expected table ID matches delivery table ID.
/// 2. Delivery is addressed to the recipient (`recipient_address == expected_recipient`).
/// 3. Committee threshold signature is valid over (table_id, recipient, ephemeral_pk, nonce, ciphertext).
/// 4. Decryption using player's private session key succeeds (MAC authentication tag matches).
pub fn decrypt_hole_cards(
    delivery: &AuthenticatedCardDelivery,
    expected_table_id: u32,
    expected_recipient: &str,
    player_keypair: &PlayerSessionKeypair,
    committee_vk: &ed25519_dalek::VerifyingKey,
) -> Result<DecryptedHoleCards, String> {
    if delivery.table_id != expected_table_id {
        return Err(format!(
            "table_id mismatch: expected {}, got {}",
            expected_table_id, delivery.table_id
        ));
    }

    if delivery.recipient_address != expected_recipient {
        return Err(format!(
            "mis-addressed delivery: expected recipient '{}', but delivery addressed to '{}'",
            expected_recipient, delivery.recipient_address
        ));
    }

    // Verify committee signature
    let signing_msg = compute_delivery_signing_payload(
        delivery.table_id,
        &delivery.recipient_address,
        &delivery.ephemeral_pubkey,
        &delivery.nonce,
        &delivery.ciphertext,
    );

    let sig_bytes = hex::decode(&delivery.committee_signature)
        .map_err(|e| format!("invalid signature hex: {}", e))?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

    committee_vk
        .verify(&signing_msg, &signature)
        .map_err(|e| format!("committee signature verification failed: {}", e))?;

    // Decompress ephemeral public key
    let eph_bytes = hex::decode(&delivery.ephemeral_pubkey)
        .map_err(|e| format!("invalid ephemeral pubkey hex: {}", e))?;
    let eph_arr: [u8; 32] = eph_bytes
        .try_into()
        .map_err(|_| "ephemeral pubkey must be 32 bytes".to_string())?;
    let eph_point = CompressedRistretto(eph_arr)
        .decompress()
        .ok_or_else(|| "invalid ephemeral Ristretto255 public key".to_string())?;

    // Shared secret S = player_sk * EphemeralPK
    let shared_secret_point = player_keypair.secret * eph_point;
    let aes_key_bytes = derive_symmetric_key(
        &shared_secret_point,
        delivery.table_id,
        &delivery.recipient_address,
    );

    let nonce_bytes = hex::decode(&delivery.nonce)
        .map_err(|e| format!("invalid nonce hex: {}", e))?;
    let ciphertext_bytes = hex::decode(&delivery.ciphertext)
        .map_err(|e| format!("invalid ciphertext hex: {}", e))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key_bytes));
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext_bytes = cipher
        .decrypt(nonce, ciphertext_bytes.as_ref())
        .map_err(|_| "failed to decrypt payload: MAC verification failed (wrong key or corrupted payload)".to_string())?;

    let cards: DecryptedHoleCards = serde_json::from_slice(&plaintext_bytes)
        .map_err(|e| format!("failed to parse decrypted cards json: {}", e))?;

    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_key() {
        let mut manager = SessionKeyManager::new();
        let session_id = "test-session-001";

        let keys = manager.generate_session_key(session_id);
        assert_eq!(keys.session_id, session_id);
        assert!(manager.session_exists(session_id));
    }

    #[test]
    fn test_get_session_keys() {
        let mut manager = SessionKeyManager::new();
        let session_id = "test-session-002";

        let keys = manager.generate_session_key(session_id);
        let retrieved = manager.get_session_keys(session_id).unwrap();

        assert_eq!(keys.encryption_key, retrieved.encryption_key);
    }

    #[test]
    fn test_cleanup_session() {
        let mut manager = SessionKeyManager::new();
        let session_id = "test-session-003";

        manager.generate_session_key(session_id);
        assert!(manager.session_exists(session_id));

        manager.cleanup_session(session_id);
        assert!(!manager.session_exists(session_id));
    }

    #[test]
    fn test_multiple_sessions() {
        let mut manager = SessionKeyManager::new();

        let keys1 = manager.generate_session_key("session-1");
        let keys2 = manager.generate_session_key("session-2");

        assert_ne!(keys1.encryption_key, keys2.encryption_key);

        let sessions = manager.get_active_sessions();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&"session-1".to_string()));
        assert!(sessions.contains(&"session-2".to_string()));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Acceptance criteria tests for #882
    // ─────────────────────────────────────────────────────────────────────────

    fn mock_committee_key() -> ed25519_dalek::SigningKey {
        let mut seed = [42u8; 32];
        ed25519_dalek::SigningKey::from_bytes(&seed)
    }

    #[test]
    fn test_honest_card_delivery_and_decryption() {
        let committee_sk = mock_committee_key();
        let committee_vk = committee_sk.verifying_key();

        let player = PlayerSessionKeypair::generate();
        let player_addr = "GBTYELEQ2YZH2W6SXLHT4AX6TYBHHU7LNNPKJV7J37VS3S5GPA75KRDU";

        let delivery = encrypt_hole_cards(
            &player.public_key_bytes(),
            1,
            player_addr,
            14, // Ace of Spades
            27, // King of Hearts
            "salt_alpha",
            "salt_beta",
            &committee_sk,
        )
        .expect("encryption must succeed");

        let decrypted = decrypt_hole_cards(&delivery, 1, player_addr, &player, &committee_vk)
            .expect("decryption must succeed");

        assert_eq!(decrypted.card1, 14);
        assert_eq!(decrypted.card2, 27);
        assert_eq!(decrypted.salt1, "salt_alpha");
        assert_eq!(decrypted.salt2, "salt_beta");
    }

    #[test]
    fn test_player_cannot_decrypt_another_players_delivery() {
        // Acceptance criterion: A player cannot decrypt another player's delivery.
        let committee_sk = mock_committee_key();
        let committee_vk = committee_sk.verifying_key();

        let player_a = PlayerSessionKeypair::generate();
        let player_b = PlayerSessionKeypair::generate();
        let addr_a = "GA_PLAYER_A_000000000000000000000000000000000000000000000000";

        let delivery_for_a = encrypt_hole_cards(
            &player_a.public_key_bytes(),
            1,
            addr_a,
            51,
            50,
            "salt1",
            "salt2",
            &committee_sk,
        )
        .unwrap();

        // Player B attempts to decrypt with their own session private key
        let err = decrypt_hole_cards(&delivery_for_a, 1, addr_a, &player_b, &committee_vk)
            .unwrap_err();

        assert!(
            err.contains("failed to decrypt payload"),
            "Player B must fail decryption: {err}"
        );
    }

    #[test]
    fn test_client_rejects_misaddressed_delivery() {
        // Acceptance criterion: Client rejects unsigned or mis-addressed deliveries.
        let committee_sk = mock_committee_key();
        let committee_vk = committee_sk.verifying_key();

        let player_a = PlayerSessionKeypair::generate();
        let addr_a = "GA_PLAYER_A_000000000000000000000000000000000000000000000000";
        let addr_b = "GB_PLAYER_B_000000000000000000000000000000000000000000000000";

        let delivery = encrypt_hole_cards(
            &player_a.public_key_bytes(),
            1,
            addr_a,
            10,
            11,
            "s1",
            "s2",
            &committee_sk,
        )
        .unwrap();

        // Client expecting cards for addr_b receives delivery addressed to addr_a
        let err = decrypt_hole_cards(&delivery, 1, addr_b, &player_a, &committee_vk).unwrap_err();
        assert!(
            err.contains("mis-addressed delivery"),
            "misaddressed delivery must be rejected: {err}"
        );
    }

    #[test]
    fn test_client_rejects_unsigned_or_tampered_delivery() {
        // Acceptance criterion: Client rejects unsigned or forged deliveries.
        let committee_sk = mock_committee_key();
        let committee_vk = committee_sk.verifying_key();

        let player = PlayerSessionKeypair::generate();
        let addr = "GA_PLAYER_00000000000000000000000000000000000000000000000000";

        let mut delivery = encrypt_hole_cards(
            &player.public_key_bytes(),
            1,
            addr,
            7,
            8,
            "s1",
            "s2",
            &committee_sk,
        )
        .unwrap();

        // Tamper with ciphertext
        let mut tampered_bytes = hex::decode(&delivery.ciphertext).unwrap();
        tampered_bytes[0] ^= 0xFF;
        delivery.ciphertext = hex::encode(tampered_bytes);

        let err = decrypt_hole_cards(&delivery, 1, addr, &player, &committee_vk).unwrap_err();
        assert!(
            err.contains("signature verification failed"),
            "tampered ciphertext must fail signature check: {err}"
        );
    }

    #[test]
    fn test_malicious_coordinator_swapping_payloads() {
        // Acceptance criterion: Test covers a malicious coordinator swapping payloads.
        let committee_sk = mock_committee_key();
        let committee_vk = committee_sk.verifying_key();

        let player_a = PlayerSessionKeypair::generate();
        let player_b = PlayerSessionKeypair::generate();

        let addr_a = "GA_PLAYER_A_000000000000000000000000000000000000000000000000";
        let addr_b = "GB_PLAYER_B_000000000000000000000000000000000000000000000000";

        let delivery_a = encrypt_hole_cards(
            &player_a.public_key_bytes(),
            1,
            addr_a,
            12, // Queen of Spades
            13, // King of Spades
            "salt_a1",
            "salt_a2",
            &committee_sk,
        )
        .unwrap();

        let _delivery_b = encrypt_hole_cards(
            &player_b.public_key_bytes(),
            1,
            addr_b,
            2,
            3,
            "salt_b1",
            "salt_b2",
            &committee_sk,
        )
        .unwrap();

        // Attack 1: Malicious coordinator routes delivery_a directly to Player B
        let err1 = decrypt_hole_cards(&delivery_a, 1, addr_b, &player_b, &committee_vk)
            .unwrap_err();
        assert!(
            err1.contains("mis-addressed delivery"),
            "Attack 1 caught: {err1}"
        );

        // Attack 2: Malicious coordinator swaps ciphertext of A into B's delivery envelope
        let mut swapped_envelope = delivery_a.clone();
        swapped_envelope.recipient_address = addr_b.to_string();
        // The coordinator cannot sign for the committee, so the signature on the swapped envelope is invalid
        let err2 = decrypt_hole_cards(&swapped_envelope, 1, addr_b, &player_b, &committee_vk)
            .unwrap_err();
        assert!(
            err2.contains("signature verification failed"),
            "Attack 2 caught: {err2}"
        );
    }
}
