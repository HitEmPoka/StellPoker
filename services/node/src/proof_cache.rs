//! Node-side proof caching keyed by (circuit, public_inputs) (Issue #579).
//!
//! Mirrors the coordinator's content-addressable cache pattern
//! (`services/coordinator/src/proof_cache.rs`, #108) and its circuit
//! artifact-hashing convention (`services/coordinator/src/circuit_pins.rs`),
//! independently re-implemented here since this is a separate crate with no
//! shared library to import either from.
//!
//! # Why this isn't a "check cache before running MPC" cache
//!
//! `public_inputs` are an *output* of MPC proof generation (derived from
//! secret-shared private inputs reconstructed during the computation), not
//! known before `run_proof_generation` runs — so a cache keyed by
//! `public_inputs` cannot be consulted before generation the way the
//! coordinator's proof-bytes cache can. What this cache actually provides:
//! after a session successfully generates a proof, the result is cached
//! under `(circuit_name, circuit_artifact_hash, public_inputs)`. A caller
//! retrying a session that *already completed* a phase (e.g. re-driving a
//! reveal after a timeout, once it has the public inputs from the earlier
//! attempt) can check this cache first via [`get`] and skip re-running the
//! expensive MPC computation entirely.
//!
//! # Invalidation
//!
//! The circuit artifact's own SHA-256 hash is folded into the cache key.
//! A circuit/VK recompile changes that hash, so entries keyed against the
//! old artifact become structurally unreachable — no explicit eviction
//! logic needed, same approach the coordinator's `circuit_pins.rs` uses for
//! its own artifact-change detection.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

type ProofCacheStore = Arc<RwLock<HashMap<String, Vec<u8>>>>;

static PROOF_CACHE: OnceLock<ProofCacheStore> = OnceLock::new();

fn store() -> &'static ProofCacheStore {
    PROOF_CACHE.get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
}

/// SHA-256 hex digest of a circuit's ACIR artifact file, following the same
/// `<circuit_dir>/<circuit_name>/target/<circuit_name>.json` path convention
/// `run_proof_generation` already uses to locate it.
pub fn circuit_artifact_hash(circuit_dir: &str, circuit_name: &str) -> Result<String, String> {
    let path = format!(
        "{}/{}/target/{}.json",
        circuit_dir.trim_end_matches('/'),
        circuit_name,
        circuit_name,
    );
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("cannot read circuit artifact '{}': {}", path, e))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Cache key: circuit name + its current artifact hash (for invalidation on
/// circuit/VK change) + the public inputs the proof was generated against.
pub fn cache_key(circuit_name: &str, artifact_hash: &str, public_inputs: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(circuit_name.as_bytes());
    hasher.update(b"|");
    hasher.update(artifact_hash.as_bytes());
    for pi in public_inputs {
        hasher.update(b"|");
        hasher.update(pi.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Look up a cached proof for this exact (circuit, artifact, public_inputs)
/// combination, if one has been generated before.
pub async fn get(key: &str) -> Option<Vec<u8>> {
    let cache = store().read().await;
    cache.get(key).cloned()
}

/// Record a successfully generated proof, content-addressed by `key` (see
/// [`cache_key`]).
pub async fn insert(key: String, proof_bytes: Vec<u8>) {
    let mut cache = store().write().await;
    cache.insert(key, proof_bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_circuit_artifact_and_inputs_hash_to_the_same_key() {
        let inputs = vec!["1".to_string(), "2".to_string()];
        assert_eq!(
            cache_key("deal_valid", "abc123", &inputs),
            cache_key("deal_valid", "abc123", &inputs)
        );
    }

    #[test]
    fn a_different_artifact_hash_changes_the_key_even_for_the_same_circuit_and_inputs() {
        let inputs = vec!["1".to_string()];
        let before = cache_key("deal_valid", "abc123", &inputs);
        let after_recompile = cache_key("deal_valid", "def456", &inputs);
        assert_ne!(
            before, after_recompile,
            "a circuit/VK version change must invalidate prior cache entries"
        );
    }

    #[test]
    fn different_public_inputs_change_the_key() {
        let a = cache_key("deal_valid", "abc123", &["1".to_string()]);
        let b = cache_key("deal_valid", "abc123", &["2".to_string()]);
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn cache_miss_then_hit_after_insert() {
        let key = cache_key("deal_valid", "unique-test-hash", &["42".to_string()]);
        assert_eq!(get(&key).await, None);

        insert(key.clone(), vec![1, 2, 3, 4]).await;
        assert_eq!(get(&key).await, Some(vec![1, 2, 3, 4]));
    }
}
