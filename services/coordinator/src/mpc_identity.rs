//! MPC node identity verification via Stellar addresses.
//!
//! Issue #237: MPC nodes authenticate using Stellar keypairs. The coordinator
//! verifies node identity against a committee registry (node_id -> Stellar
//! address) and every session message is signed by the sending node with its
//! Stellar keypair, so a spoofed or compromised endpoint cannot impersonate a
//! committee member.

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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
}

/// The exact byte string each node signs for a session message. Kept in one
/// place so the coordinator and node implementations stay in lock-step.
pub fn canonical_message(node_id: &str, session_id: &str, payload: &str, timestamp: i64) -> String {
    format!(
        "stellar-poker-mpc|{}|{}|{}|{}",
        node_id, session_id, payload, timestamp
    )
}

/// Verify that `msg` was genuinely signed by the Stellar keypair registered
/// for `msg.node_id` in the committee registry.
pub async fn verify_session_message(
    registry: &CommitteeRegistry,
    msg: &SignedSessionMessage,
) -> Result<(), String> {
    let address = lookup_address(registry, &msg.node_id)
        .await
        .ok_or_else(|| format!("node {} is not a registered committee member", msg.node_id))?;

    let message = canonical_message(&msg.node_id, &msg.session_id, &msg.payload, msg.timestamp);
    verify_signature(&address, &message, &msg.signature)
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

    let normalized: [u8; 64] = if decoded.len() == 64 {
        decoded
            .as_slice()
            .try_into()
            .map_err(|_| "malformed signature".to_string())?
    } else if decoded.len() == 68 {
        decoded[4..68]
            .try_into()
            .map_err(|_| "malformed signature".to_string())?
    } else if decoded.len() == 72 && decoded[4..8] == [0, 0, 0, 64] {
        decoded[8..72]
            .try_into()
            .map_err(|_| "malformed signature".to_string())?
    } else {
        return Err("unrecognized signature length".to_string());
    };
    Ok(Signature::from_bytes(&normalized))
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

        let message = canonical_message("0", "sess-1", "commitment:abc", 1_700_000_000);
        let sig = signing_key.sign(message.as_bytes());
        let msg = SignedSessionMessage {
            node_id: "0".into(),
            session_id: "sess-1".into(),
            payload: "commitment:abc".into(),
            signature: hex::encode(sig.to_bytes()),
            timestamp: 1_700_000_000,
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

        let message = canonical_message("0", "sess-1", "commitment:abc", 1_700_000_000);
        let sig = signing_key.sign(message.as_bytes());
        let msg = SignedSessionMessage {
            node_id: "0".into(),
            session_id: "sess-1".into(),
            payload: "commitment:TAMPERED".into(),
            signature: hex::encode(sig.to_bytes()),
            timestamp: 1_700_000_000,
        };

        assert!(verify_session_message(&registry, &msg).await.is_err());
    }
}
