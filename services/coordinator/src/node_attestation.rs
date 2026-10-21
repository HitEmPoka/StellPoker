//! Node attestation of build provenance (Issue #587).
//!
//! Every MPC node, at startup, hashes its own executable, signs a
//! [`BuildAttestation`] with its committee Stellar keypair, and reports it to
//! the coordinator. The coordinator verifies the signature against the
//! committee registry (`mpc_identity`) and optionally against an allow-list
//! of accepted binary hashes, then remembers it.
//!
//! This is the runtime half of the project's SLSA aspirations: `#16` produced
//! SBOMs for artifacts, `scripts/generate-provenance.sh` emits SLSA-style
//! provenance for circuits, and this module makes each node *attest at
//! runtime which binary it is actually running*.
//!
//! ## Strict mode
//!
//! When `MPC_ATTESTATION_STRICT=1` the coordinator reports a node as healthy
//! only if it has a fresh, valid attestation. Combined with the health loop
//! that feeds `select_mpc_nodes`, this means an unattested peer is refused
//! committee work. When `MPC_ATTESTATION_ACCEPTED_HASHES` is set (comma
//! separated SHA-256 hex digests) a node whose binary hash is not on the list
//! is rejected outright, even in non-strict mode.
//!
//! The canonical signing payload is defined once, here, and mirrored
//! byte-for-byte in `services/node/src/attestation.rs`. Do not change one
//! without the other.

use crate::mpc_identity;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A signed statement by an MPC node about the binary it is running.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildAttestation {
    pub node_id: u32,
    /// SHA-256 (lowercase hex) of the node's executable.
    pub binary_sha256: String,
    pub binary_size: u64,
    pub package_version: String,
    pub git_commit: Option<String>,
    pub build_target: String,
    /// Unix seconds when the node produced the attestation.
    pub generated_at: i64,
    /// Claimed SLSA level (0 = unknown, 3 = hardened build).
    pub slsa_level: u8,
    /// Node's committee Stellar address (G...).
    pub signer: String,
    /// Ed25519 signature (hex or base64) over [`Self::signing_payload`].
    pub signature: String,
}

/// The exact byte string a node signs. Mirrored in the node crate.
#[allow(clippy::too_many_arguments)]
pub fn canonical_attestation_message(
    node_id: u32,
    binary_sha256: &str,
    binary_size: u64,
    package_version: &str,
    git_commit: Option<&str>,
    build_target: &str,
    generated_at: i64,
    slsa_level: u8,
    signer: &str,
) -> String {
    format!(
        "stellar-poker-attestation|{node_id}|{binary_sha256}|{binary_size}|{package_version}|{}|{build_target}|{generated_at}|{slsa_level}|{signer}",
        git_commit.unwrap_or("-")
    )
}

impl BuildAttestation {
    pub fn signing_payload(&self) -> String {
        canonical_attestation_message(
            self.node_id,
            &self.binary_sha256,
            self.binary_size,
            &self.package_version,
            self.git_commit.as_deref(),
            &self.build_target,
            self.generated_at,
            self.slsa_level,
            &self.signer,
        )
    }
}

/// Attestation verification policy.
#[derive(Clone, Debug)]
pub struct AttestationConfig {
    /// Refuse (mark unhealthy) any node without a fresh, valid attestation.
    pub strict: bool,
    /// If non-empty, only these lowercased SHA-256 digests are accepted.
    pub accepted_binary_hashes: HashSet<String>,
    /// Reject attestations older than this (0 disables the check).
    pub max_age_seconds: i64,
}

impl Default for AttestationConfig {
    fn default() -> Self {
        Self {
            strict: false,
            accepted_binary_hashes: HashSet::new(),
            max_age_seconds: 3600,
        }
    }
}

impl AttestationConfig {
    pub fn from_env() -> Self {
        let d = Self::default();
        let accepted = std::env::var("MPC_ATTESTATION_ACCEPTED_HASHES")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        Self {
            strict: std::env::var("MPC_ATTESTATION_STRICT")
                .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(d.strict),
            accepted_binary_hashes: accepted,
            max_age_seconds: std::env::var("MPC_ATTESTATION_MAX_AGE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.max_age_seconds),
        }
    }
}

/// A stored attestation plus when the coordinator received it.
#[derive(Clone, Debug)]
pub struct StoredAttestation {
    pub attestation: BuildAttestation,
    pub received_at: i64,
}

pub type AttestationStore = Arc<RwLock<HashMap<String, StoredAttestation>>>;

pub fn new_store() -> AttestationStore {
    Arc::new(RwLock::new(HashMap::new()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttestationError {
    /// No committee address is registered for the claimed node id.
    UnregisteredNode(String),
    /// The signature did not verify, or `signer` did not match the registry.
    BadSignature,
    /// The binary hash is not on the configured allow-list.
    HashNotAccepted(String),
    /// The attestation is older than `max_age_seconds`.
    Stale,
}

impl std::fmt::Display for AttestationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttestationError::UnregisteredNode(n) => {
                write!(f, "node {n} has no committee identity registered")
            }
            AttestationError::BadSignature => write!(f, "attestation signature did not verify"),
            AttestationError::HashNotAccepted(h) => {
                write!(f, "binary hash {h} is not on the accepted list")
            }
            AttestationError::Stale => write!(f, "attestation is stale"),
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Verify and store an attestation.
pub async fn submit_attestation(
    store: &AttestationStore,
    config: &AttestationConfig,
    registry: &mpc_identity::CommitteeRegistry,
    att: BuildAttestation,
) -> Result<(), AttestationError> {
    let node_key = att.node_id.to_string();
    let address = mpc_identity::lookup_address(registry, &node_key)
        .await
        .ok_or_else(|| AttestationError::UnregisteredNode(node_key.clone()))?;

    if att.signer != address {
        tracing::warn!(
            node_id = %node_key,
            claimed = %att.signer,
            expected = %address,
            "rejected attestation: signer does not match committee registry"
        );
        return Err(AttestationError::BadSignature);
    }

    if !config.accepted_binary_hashes.is_empty()
        && !config
            .accepted_binary_hashes
            .contains(&att.binary_sha256.to_ascii_lowercase())
    {
        return Err(AttestationError::HashNotAccepted(att.binary_sha256.clone()));
    }

    mpc_identity::verify_signature(&address, &att.signing_payload(), &att.signature)
        .map_err(|_| AttestationError::BadSignature)?;

    let now = now_unix();
    if config.max_age_seconds > 0 && now - att.generated_at > config.max_age_seconds {
        return Err(AttestationError::Stale);
    }

    tracing::info!(
        node_id = %node_key,
        binary_sha256 = %att.binary_sha256,
        version = %att.package_version,
        "recorded build attestation"
    );
    store.write().await.insert(
        node_key,
        StoredAttestation {
            attestation: att,
            received_at: now,
        },
    );
    Ok(())
}

/// Whether `node_id` currently satisfies the attestation policy. In
/// non-strict mode this is always true.
pub async fn is_attested(
    store: &AttestationStore,
    config: &AttestationConfig,
    node_id: &str,
    now: i64,
) -> bool {
    if !config.strict {
        return true;
    }
    let guard = store.read().await;
    let Some(record) = guard.get(node_id) else {
        return false;
    };
    if config.max_age_seconds > 0 && now - record.received_at > config.max_age_seconds {
        return false;
    }
    if !config.accepted_binary_hashes.is_empty()
        && !config
            .accepted_binary_hashes
            .contains(&record.attestation.binary_sha256.to_ascii_lowercase())
    {
        return false;
    }
    true
}

/// Every stored attestation, for the status endpoint.
pub async fn list_attestations(store: &AttestationStore) -> Vec<StoredAttestation> {
    store.read().await.values().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn keypair() -> (SigningKey, String) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let address =
            stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string();
        (signing_key, address)
    }

    fn sample(signing_key: &SigningKey, address: &str, node_id: u32) -> BuildAttestation {
        let mut att = BuildAttestation {
            node_id,
            binary_sha256: "a".repeat(64),
            binary_size: 42,
            package_version: "0.1.0".into(),
            git_commit: Some("deadbeef".into()),
            build_target: "x86_64-unknown-linux-gnu".into(),
            generated_at: now_unix(),
            slsa_level: 3,
            signer: address.to_string(),
            signature: String::new(),
        };
        let sig = signing_key.sign(att.signing_payload().as_bytes());
        att.signature = hex::encode(sig.to_bytes());
        att
    }

    #[tokio::test]
    async fn valid_attestation_is_recorded() {
        let (sk, address) = keypair();
        let registry = mpc_identity::new_registry();
        mpc_identity::register_node_identity(&registry, "0", &address)
            .await
            .unwrap();
        let store = new_store();
        let config = AttestationConfig::default();

        let att = sample(&sk, &address, 0);
        assert_eq!(
            submit_attestation(&store, &config, &registry, att).await,
            Ok(())
        );
        assert_eq!(list_attestations(&store).await.len(), 1);
    }

    #[tokio::test]
    async fn tampered_payload_is_rejected() {
        let (sk, address) = keypair();
        let registry = mpc_identity::new_registry();
        mpc_identity::register_node_identity(&registry, "0", &address)
            .await
            .unwrap();
        let store = new_store();

        let mut att = sample(&sk, &address, 0);
        att.binary_sha256 = "b".repeat(64); // mutate after signing
        assert_eq!(
            submit_attestation(&store, &AttestationConfig::default(), &registry, att).await,
            Err(AttestationError::BadSignature)
        );
    }

    #[tokio::test]
    async fn unregistered_node_is_rejected() {
        let (sk, address) = keypair();
        let registry = mpc_identity::new_registry();
        let store = new_store();
        let att = sample(&sk, &address, 9);
        assert_eq!(
            submit_attestation(&store, &AttestationConfig::default(), &registry, att).await,
            Err(AttestationError::UnregisteredNode("9".into()))
        );
    }

    #[tokio::test]
    async fn allow_list_rejects_unknown_hash() {
        let (sk, address) = keypair();
        let registry = mpc_identity::new_registry();
        mpc_identity::register_node_identity(&registry, "0", &address)
            .await
            .unwrap();
        let store = new_store();
        let config = AttestationConfig {
            accepted_binary_hashes: HashSet::from(["f".repeat(64)]),
            ..AttestationConfig::default()
        };
        let att = sample(&sk, &address, 0);
        assert!(matches!(
            submit_attestation(&store, &config, &registry, att).await,
            Err(AttestationError::HashNotAccepted(_))
        ));
    }

    #[tokio::test]
    async fn strict_mode_requires_a_fresh_attestation() {
        let (sk, address) = keypair();
        let registry = mpc_identity::new_registry();
        mpc_identity::register_node_identity(&registry, "0", &address)
            .await
            .unwrap();
        let store = new_store();
        let strict = AttestationConfig {
            strict: true,
            ..AttestationConfig::default()
        };

        assert!(!is_attested(&store, &strict, "0", now_unix()).await);

        let att = sample(&sk, &address, 0);
        submit_attestation(&store, &strict, &registry, att)
            .await
            .unwrap();
        assert!(is_attested(&store, &strict, "0", now_unix()).await);

        // Non-strict is always satisfied.
        assert!(is_attested(&store, &AttestationConfig::default(), "1", now_unix()).await);
    }

    #[test]
    fn canonical_payload_is_stable() {
        let payload = canonical_attestation_message(
            1,
            "abc",
            10,
            "0.1.0",
            None,
            "target",
            123,
            3,
            "GABC",
        );
        assert_eq!(
            payload,
            "stellar-poker-attestation|1|abc|10|0.1.0|-|target|123|3|GABC"
        );
    }
}
