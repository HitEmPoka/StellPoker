//! Node attestation of build provenance (Issue #587).
//!
//! At startup each MPC node hashes its own executable, signs a
//! [`BuildAttestation`] with its committee Stellar keypair, and reports it to
//! the coordinator (`POST /api/mpc/attestation`). The coordinator verifies
//! the signature against the committee registry and can run in a strict mode
//! that refuses to hand work to unattested nodes.
//!
//! This complements the SBOM (`#16`, `scripts/generate-sbom.js`) and the
//! circuit provenance generator (`scripts/generate-provenance.sh`) by
//! attesting, at runtime, *which binary an operator is actually running*.
//!
//! The canonical signing payload is defined once and mirrored byte-for-byte
//! in `services/coordinator/src/node_attestation.rs`. Do not change one
//! without the other.

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

/// A signed statement by this node about the binary it is running.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildAttestation {
    pub node_id: u32,
    /// SHA-256 (lowercase hex) of the node's executable.
    pub binary_sha256: String,
    pub binary_size: u64,
    pub package_version: String,
    pub git_commit: Option<String>,
    pub build_target: String,
    /// Unix seconds when this node produced the attestation.
    pub generated_at: i64,
    /// Claimed SLSA level (0 = unknown, 3 = hardened build).
    pub slsa_level: u8,
    /// This node's committee Stellar address (G...).
    pub signer: String,
    /// Ed25519 signature (hex) over [`Self::signing_payload`].
    pub signature: String,
}

/// The exact byte string a node signs. Mirrored in the coordinator crate.
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

/// SHA-256 (lowercase hex) of an arbitrary byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Hash a file, returning `(sha256_hex, size_bytes)`.
pub fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let bytes = std::fs::read(path)?;
    let size = bytes.len() as u64;
    Ok((sha256_hex(&bytes), size))
}

/// The build target triple-ish string, e.g. `x86_64-linux`.
pub fn build_target() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// Compile-time git commit if the build supplied one, else a runtime
/// `GIT_COMMIT` override, else `None`.
pub fn git_commit() -> Option<String> {
    option_env!("GIT_COMMIT")
        .map(|s| s.to_string())
        .or_else(|| std::env::var("GIT_COMMIT").ok())
        .filter(|s| !s.is_empty())
}

/// Parse a Stellar seed (`S...`) into an Ed25519 signing key.
pub fn load_signing_key(secret_key: &str) -> Result<SigningKey, String> {
    let private = stellar_strkey::ed25519::PrivateKey::from_string(secret_key.trim())
        .map_err(|e| format!("invalid Stellar secret key: {e}"))?;
    Ok(SigningKey::from_bytes(&private.0))
}

/// The Stellar public address (`G...`) for a signing key.
pub fn public_address(signing_key: &SigningKey) -> String {
    stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build and sign an attestation from an already-computed binary digest.
/// Split out from [`create_attestation`] so it can be tested without reading
/// the current process's executable.
pub fn build_attestation(
    node_id: u32,
    signing_key: &SigningKey,
    binary_sha256: String,
    binary_size: u64,
) -> BuildAttestation {
    let signer = public_address(signing_key);
    let git_commit = git_commit();
    let generated_at = now_unix();
    let payload = canonical_attestation_message(
        node_id,
        &binary_sha256,
        binary_size,
        env!("CARGO_PKG_VERSION"),
        git_commit.as_deref(),
        &build_target(),
        generated_at,
        3,
        &signer,
    );
    let signature = signing_key.sign(payload.as_bytes());
    BuildAttestation {
        node_id,
        binary_sha256,
        binary_size,
        package_version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit,
        build_target: build_target(),
        generated_at,
        slsa_level: 3,
        signer,
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Hash this process's own executable and sign an attestation for it.
pub fn create_attestation(
    node_id: u32,
    signing_key: &SigningKey,
) -> Result<BuildAttestation, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate current executable: {e}"))?;
    let (hash, size) =
        hash_file(&exe).map_err(|e| format!("cannot hash {}: {e}", exe.display()))?;
    tracing::info!(
        node_id,
        binary = %exe.display(),
        binary_sha256 = %hash,
        binary_size = size,
        "attesting local build"
    );
    Ok(build_attestation(node_id, signing_key, hash, size))
}

/// Spawn a background task that reports this node's attestation to the
/// coordinator on an interval. No-op if `STELLAR_SECRET_KEY` is unset — a
/// node without a committee identity cannot sign, and refusing to sign is
/// safer than signing with the wrong key.
pub fn spawn_attestation_reporter(node_id: u32, coordinator_url: String, interval_secs: u64) {
    let secret = match std::env::var("STELLAR_SECRET_KEY") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => {
            tracing::warn!(
                "STELLAR_SECRET_KEY not set; node build attestation reporting disabled"
            );
            return;
        }
    };
    let signing_key = match load_signing_key(&secret) {
        Ok(k) => k,
        Err(e) => {
            tracing::error!("cannot load signing key for attestation: {}", e);
            return;
        }
    };

    tokio::spawn(async move {
        let client = crate::pool::peer_client();
        let url = format!("{}/api/mpc/attestation", coordinator_url.trim_end_matches('/'));
        let interval = Duration::from_secs(interval_secs.max(30));

        loop {
            match create_attestation(node_id, &signing_key) {
                Ok(att) => match client.post(&url).json(&att).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::debug!(node_id, "build attestation reported");
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            node_id,
                            status = %resp.status(),
                            "build attestation rejected by coordinator"
                        );
                    }
                    Err(e) => tracing::warn!(node_id, err = %e, "build attestation report failed"),
                },
                Err(e) => tracing::error!(node_id, err = %e, "failed to create attestation"),
            }
            tokio::time::sleep(interval).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        // SHA-256 of the empty string.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn canonical_payload_is_stable() {
        assert_eq!(
            canonical_attestation_message(2, "abc", 5, "0.1.0", Some("c0ffee"), "t", 7, 3, "G"),
            "stellar-poker-attestation|2|abc|5|0.1.0|c0ffee|t|7|3|G"
        );
    }

    #[test]
    fn signed_attestation_verifies() {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let att = build_attestation(1, &signing_key, "a".repeat(64), 1024);
        let verifying = signing_key.verifying_key();
        assert!(verifying
            .verify(
                att.signing_payload().as_bytes(),
                &ed25519_dalek::Signature::from_bytes(
                    &<[u8; 64]>::try_from(hex::decode(&att.signature).unwrap().as_slice())
                        .unwrap()
                )
            )
            .is_ok());
        assert_eq!(att.signer, public_address(&signing_key));
        assert_eq!(att.slsa_level, 3);
    }

    #[test]
    fn secret_key_round_trip() {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let secret = stellar_strkey::ed25519::PrivateKey(signing_key.to_bytes()).to_string();
        let loaded = load_signing_key(&secret).unwrap();
        assert_eq!(loaded.verifying_key(), signing_key.verifying_key());
    }
}
