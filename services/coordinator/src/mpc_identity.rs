//! MPC node identity verification via Stellar addresses.
//!
//! Issue #237: MPC nodes authenticate using Stellar keypairs. The coordinator
//! verifies node identity against a committee registry (node_id -> Stellar
//! address) and every session message is signed by the sending node with its
//! Stellar keypair, so a spoofed or compromised endpoint cannot impersonate a
//! committee member.
//!
//! Issue #500: Replay-protected nonces for all MPC session messages.
//! Every session message carries a unique nonce verified upon receipt.
//! Duplicate nonces are rejected and logged with the sender's peer identity.

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// node_id -> Stellar (ed25519) public address (G...) allowed to act as that
/// committee member. This is the "committee-registry" of trusted node
/// identities.
pub type CommitteeRegistry = Arc<RwLock<HashMap<String, String>>>;

pub fn new_registry() -> CommitteeRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Seed the registry from a fixed node_id -> address map, e.g. sourced from
/// `MPC_NODE_<n>_ADDRESS` env vars or the on-chain committee registry
/// contract.
pub async fn seed_registry(registry: &CommitteeRegistry, entries: &[(String, String)]) {
    let mut guard = registry.write().await;
    for (node_id, address) in entries {
        if is_valid_stellar_address(address) {
            guard.insert(node_id.clone(), address.clone());
        } else {
            tracing::warn!("skipping invalid Stellar address for MPC node {}", node_id);
        }
    }
}

/// Register (or update) a single node's identity in the committee registry.
pub async fn register_node_identity(
    registry: &CommitteeRegistry,
    node_id: &str,
    stellar_address: &str,
) -> Result<(), String> {
    if !is_valid_stellar_address(stellar_address) {
        return Err(format!("invalid Stellar address: {}", stellar_address));
    }
    registry
        .write()
        .await
        .insert(node_id.to_string(), stellar_address.to_string());
    Ok(())
}

/// Look up the Stellar address the committee registry trusts for `node_id`.
pub async fn lookup_address(registry: &CommitteeRegistry, node_id: &str) -> Option<String> {
    registry.read().await.get(node_id).cloned()
}

pub fn is_valid_stellar_address(address: &str) -> bool {
    stellar_strkey::ed25519::PublicKey::from_string(address).is_ok()
}

/// Tracks seen nonces per (node_id, session_id) to prevent message replay attacks (Issue #500).
#[derive(Clone, Debug, Default)]
pub struct SessionNonceTracker {
    seen_nonces: Arc<RwLock<HashMap<(String, String), HashSet<u64>>>>,
}

impl SessionNonceTracker {
    pub fn new() -> Self {
        Self {
            seen_nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check and record a message nonce. Returns `Ok(())` if the nonce is fresh,
    /// or `Err` with a rejection message if the nonce was already used by this peer.
    pub async fn check_and_record(
        &self,
        node_id: &str,
        session_id: &str,
        nonce: u64,
    ) -> Result<(), String> {
        let key = (node_id.to_string(), session_id.to_string());
        let mut guard = self.seen_nonces.write().await;
        let nonces = guard.entry(key).or_default();
        if !nonces.insert(nonce) {
            tracing::warn!(
                peer = %node_id,
                session_id = %session_id,
                nonce = nonce,
                "rejected replayed MPC session message"
            );
            return Err(format!(
                "replay detected: nonce {} already used by peer {} for session {}",
                nonce, node_id, session_id
            ));
        }
        Ok(())
    }

    /// Reset tracked nonces for a session upon session completion.
    pub async fn cleanup_session(&self, session_id: &str) {
        let mut guard = self.seen_nonces.write().await;
        guard.retain(|(_, sid), _| sid != session_id);
    }
}

/// A session message signed by an MPC node with its Stellar keypair.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SignedSessionMessage {
    pub node_id: String,
    pub session_id: String,
    /// Opaque payload (e.g. a share commitment digest, a progress update).
    pub payload: String,
    /// Signature (hex or base64) over `canonical_message`.
    pub signature: String,
    pub timestamp: i64,
    /// Per-message unique nonce or monotonic counter (Issue #500).
    #[serde(default)]
    pub nonce: u64,
}

/// The exact byte string each node signs for a session message. Kept in one
/// place so the coordinator and node implementations stay in lock-step.
pub fn canonical_message(
    node_id: &str,
    session_id: &str,
    payload: &str,
    timestamp: i64,
    nonce: u64,
) -> String {
    format!(
        "stellar-poker-mpc|{}|{}|{}|{}|{}",
        node_id, session_id, payload, timestamp, nonce
    )
}

/// Verify that `msg` was genuinely signed by the Stellar keypair registered
/// for `msg.node_id` in the committee registry.
pub async fn verify_session_message(
    registry: &CommitteeRegistry,
    msg: &SignedSessionMessage,
) -> Result<(), String> {
    verify_session_message_with_tracker(registry, msg, None).await
}

/// Verify `msg` signature and ensure its nonce is not a replay via `SessionNonceTracker`.
pub async fn verify_session_message_with_tracker(
    registry: &CommitteeRegistry,
    msg: &SignedSessionMessage,
    tracker: Option<&SessionNonceTracker>,
) -> Result<(), String> {
    let address = lookup_address(registry, &msg.node_id)
        .await
        .ok_or_else(|| format!("node {} is not a registered committee member", msg.node_id))?;

    let message = canonical_message(
        &msg.node_id,
        &msg.session_id,
        &msg.payload,
        msg.timestamp,
        msg.nonce,
    );
    verify_signature(&address, &message, &msg.signature)?;

    if let Some(t) = tracker {
        t.check_and_record(&msg.node_id, &msg.session_id, msg.nonce)
            .await?;
    }

    Ok(())
}

/// Verify a raw Ed25519 signature (over `message`) against a Stellar
/// `address`. Supports the same signature encodings the player-auth path
/// does: raw signature bytes verified directly, and the SEP-53
/// `"Stellar Signed Message:\n" + message` wrapped form used by wallets.
pub fn verify_signature(address: &str, message: &str, signature_raw: &str) -> Result<(), String> {
    let stellar_pk = stellar_strkey::ed25519::PublicKey::from_string(address)
        .map_err(|_| "malformed Stellar address".to_string())?;
    let verifying_key = VerifyingKey::from_bytes(&stellar_pk.0)
        .map_err(|_| "invalid Ed25519 public key".to_string())?;

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
        .map_err(|_| "signature verification failed".to_string())
}

fn decode_signature(signature_raw: &str) -> Result<Signature, String> {
    let s = signature_raw.trim();

    let decoded = if let Some(hex_str) = s.strip_prefix("0x") {
        hex::decode(hex_str).map_err(|_| "invalid hex signature".to_string())?
    } else if s.len() == 128 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        hex::decode(s).map_err(|_| "invalid hex signature".to_string())?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| "invalid base64 signature".to_string())?
    };

    if decoded.len() == 64 {
        let bytes: [u8; 64] = decoded
            .try_into()
            .map_err(|_| "malformed signature".to_string())?;
        Ok(Signature::from_bytes(&bytes))
    } else if decoded.len() == 65 {
        let bytes: [u8; 64] = decoded[..64]
            .try_into()
            .map_err(|_| "malformed signature".to_string())?;
        Ok(Signature::from_bytes(&bytes))
    } else if decoded.len() == 70 || decoded.len() == 71 || decoded.len() == 72 {
        let bytes: [u8; 64] = decoded[decoded.len() - 64..]
            .try_into()
            .map_err(|_| "malformed signature".to_string())?;
        Ok(Signature::from_bytes(&bytes))
    } else {
        Err("unrecognized signature length".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn keypair() -> (SigningKey, String) {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let address =
            stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string();
        (signing_key, address)
    }

    #[tokio::test]
    async fn verifies_correctly_signed_session_message() {
        use ed25519_dalek::Signer;

        let (signing_key, address) = keypair();
        let registry = new_registry();
        register_node_identity(&registry, "0", &address)
            .await
            .unwrap();

        let message = canonical_message("0", "sess-1", "commitment:abc", 1_700_000_000, 101);
        let sig = signing_key.sign(message.as_bytes());
        let msg = SignedSessionMessage {
            node_id: "0".into(),
            session_id: "sess-1".into(),
            payload: "commitment:abc".into(),
            signature: hex::encode(sig.to_bytes()),
            timestamp: 1_700_000_000,
            nonce: 101,
        };

        assert!(verify_session_message(&registry, &msg).await.is_ok());
    }

    #[tokio::test]
    async fn rejects_message_from_unregistered_node() {
        let registry = new_registry();
        let msg = SignedSessionMessage {
            node_id: "unknown".into(),
            session_id: "sess-1".into(),
            payload: "x".into(),
            signature: "00".repeat(64),
            timestamp: 0,
            nonce: 1,
        };
        assert!(verify_session_message(&registry, &msg).await.is_err());
    }

    #[tokio::test]
    async fn rejects_tampered_payload() {
        use ed25519_dalek::Signer;

        let (signing_key, address) = keypair();
        let registry = new_registry();
        register_node_identity(&registry, "0", &address)
            .await
            .unwrap();

        let message = canonical_message("0", "sess-1", "commitment:abc", 1_700_000_000, 101);
        let sig = signing_key.sign(message.as_bytes());
        let msg = SignedSessionMessage {
            node_id: "0".into(),
            session_id: "sess-1".into(),
            payload: "commitment:TAMPERED".into(),
            signature: hex::encode(sig.to_bytes()),
            timestamp: 1_700_000_000,
            nonce: 101,
        };

        assert!(verify_session_message(&registry, &msg).await.is_err());
    }

    #[tokio::test]
    async fn rejects_duplicated_replayed_message() {
        use ed25519_dalek::Signer;

        let (signing_key, address) = keypair();
        let registry = new_registry();
        register_node_identity(&registry, "0", &address)
            .await
            .unwrap();
        let tracker = SessionNonceTracker::new();

        let message = canonical_message("0", "sess-replay", "commitment:abc", 1_700_000_000, 202);
        let sig = signing_key.sign(message.as_bytes());
        let msg = SignedSessionMessage {
            node_id: "0".into(),
            session_id: "sess-replay".into(),
            payload: "commitment:abc".into(),
            signature: hex::encode(sig.to_bytes()),
            timestamp: 1_700_000_000,
            nonce: 202,
        };

        // First verification succeeds
        assert!(
            verify_session_message_with_tracker(&registry, &msg, Some(&tracker))
                .await
                .is_ok()
        );

        // Replayed message with the same nonce must be rejected
        let err = verify_session_message_with_tracker(&registry, &msg, Some(&tracker))
            .await
            .unwrap_err();
        assert!(
            err.contains("replay detected"),
            "expected replay detection, got: {err}"
        );
        assert!(
            err.contains("peer 0"),
            "expected peer identity in error, got: {err}"
        );
    }

    #[tokio::test]
    async fn fuzz_property_test_replay_rejection() {
        use ed25519_dalek::Signer;
        use rand::Rng;

        let (signing_key, address) = keypair();
        let registry = new_registry();
        register_node_identity(&registry, "party-fuzz", &address)
            .await
            .unwrap();
        let tracker = SessionNonceTracker::new();

        let mut rng = rand::thread_rng();
        let mut nonces = Vec::new();

        // 100 distinct random nonces must all succeed
        for _ in 0..100 {
            let nonce = rng.gen::<u64>();
            nonces.push(nonce);

            let message = canonical_message(
                "party-fuzz",
                "session-fuzz",
                "payload-fuzz",
                1_700_000_000,
                nonce,
            );
            let sig = signing_key.sign(message.as_bytes());
            let msg = SignedSessionMessage {
                node_id: "party-fuzz".into(),
                session_id: "session-fuzz".into(),
                payload: "payload-fuzz".into(),
                signature: hex::encode(sig.to_bytes()),
                timestamp: 1_700_000_000,
                nonce,
            };

            assert!(
                verify_session_message_with_tracker(&registry, &msg, Some(&tracker))
                    .await
                    .is_ok(),
                "fresh nonce {} failed",
                nonce
            );
        }

        // Replaying any previously used nonce must fail hard
        for &nonce in &nonces {
            let message = canonical_message(
                "party-fuzz",
                "session-fuzz",
                "payload-fuzz",
                1_700_000_000,
                nonce,
            );
            let sig = signing_key.sign(message.as_bytes());
            let msg = SignedSessionMessage {
                node_id: "party-fuzz".into(),
                session_id: "session-fuzz".into(),
                payload: "payload-fuzz".into(),
                signature: hex::encode(sig.to_bytes()),
                timestamp: 1_700_000_000,
                nonce,
            };

            let res = verify_session_message_with_tracker(&registry, &msg, Some(&tracker)).await;
            assert!(
                res.is_err(),
                "duplicated message with nonce {} was not rejected",
                nonce
            );
            assert!(
                res.unwrap_err().contains("replay detected"),
                "expected replay error message"
            );
        }
    }
}
