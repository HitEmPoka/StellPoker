//! Threshold signing for contract submissions from committee (Issue #501).
//!
//! Replaces single-key coordinator submission with a t-of-n threshold signature
//! scheme (default: 2-of-3) for settlement-related contract calls (e.g. `submit_showdown`).
//! If the threshold quorum cannot be met within the timeout window, a documented
//! fallback path is triggered or evaluated.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::mpc_identity::{self, CommitteeRegistry};
use crate::soroban::SorobanConfig;

/// Configuration for threshold signing committee operations.
#[derive(Clone, Debug)]
pub struct ThresholdSigningConfig {
    /// Number of valid signatures required to form a quorum (t in t-of-n, default: 2).
    pub threshold: usize,
    /// Total number of committee members (n in t-of-n, default: 3).
    pub committee_size: usize,
    /// Timeout in milliseconds for gathering threshold signature shares.
    pub timeout_ms: u64,
    /// Whether emergency fallback to coordinator single-key signing is permitted when threshold is not met.
    pub allow_fallback: bool,
}

impl Default for ThresholdSigningConfig {
    fn default() -> Self {
        Self {
            threshold: 2,
            committee_size: 3,
            timeout_ms: 1500,
            allow_fallback: true,
        }
    }
}

/// Settlement call parameters to be authorized by committee threshold signatures.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SettlementCallPayload {
    pub table_id: u32,
    pub hand_number: u32,
    pub contract_id: String,
    pub method: String,
    pub pot_total: i128,
    pub winners: Vec<String>,
    pub nonce: u64,
}

/// A signature share produced by an individual committee node.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ThresholdSignatureShare {
    pub node_id: String,
    pub stellar_address: String,
    pub signature: String,
    pub nonce: u64,
}

/// Result of a threshold-authorized contract submission.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ThresholdSubmissionResult {
    pub tx_hash: String,
    pub signatures_collected: usize,
    pub required_threshold: usize,
    pub used_fallback: bool,
    pub duration_ms: u64,
}

/// Domain separation magic and purpose identifiers to prevent cross-protocol signature reuse.
pub mod domain {
    pub const MAGIC: &[u8] = b"StellarPoker";
    pub const NETWORK_ID: u32 = 1; // 1 = testnet, varies by network
    pub const PURPOSE_SETTLEMENT: &[u8] = b"settlement";
    pub const PURPOSE_MPC_SESSION: &[u8] = b"mpc_session";
    pub const PURPOSE_COORDINATOR: &[u8] = b"coordinator";
}

/// Compute the canonical cryptographic digest over a settlement transaction payload
/// with Stellar domain separation to prevent signature reuse across protocols.
pub fn canonical_settlement_digest(payload: &SettlementCallPayload) -> [u8; 32] {
    let mut hasher = Sha256::new();

    // Domain separation: magic + network ID + purpose
    hasher.update(domain::MAGIC);
    hasher.update(domain::NETWORK_ID.to_be_bytes());
    hasher.update(domain::PURPOSE_SETTLEMENT);

    // Payload data with delimiters
    hasher.update(b"|");
    hasher.update(payload.table_id.to_be_bytes());
    hasher.update(b"|");
    hasher.update(payload.hand_number.to_be_bytes());
    hasher.update(b"|");
    hasher.update(payload.contract_id.as_bytes());
    hasher.update(b"|");
    hasher.update(payload.method.as_bytes());
    hasher.update(b"|");
    hasher.update(payload.pot_total.to_be_bytes());
    hasher.update(b"|");
    for winner in &payload.winners {
        hasher.update(winner.as_bytes());
        hasher.update(b";");
    }
    hasher.update(b"|");
    hasher.update(payload.nonce.to_be_bytes());
    hasher.finalize().into()
}

/// Verify a single node's signature share against its registered address in the committee registry.
pub async fn verify_signature_share(
    registry: &CommitteeRegistry,
    share: &ThresholdSignatureShare,
    digest: &[u8; 32],
) -> Result<(), String> {
    let registered_addr = mpc_identity::lookup_address(registry, &share.node_id)
        .await
        .ok_or_else(|| format!("node {} not found in committee registry", share.node_id))?;

    if registered_addr != share.stellar_address {
        return Err(format!(
            "stellar address mismatch for node {}: expected {}, got {}",
            share.node_id, registered_addr, share.stellar_address
        ));
    }

    let message_hex = hex::encode(digest);
    mpc_identity::verify_signature(&share.stellar_address, &message_hex, &share.signature)
}

/// Filter and verify collected signature shares against the committee registry.
pub async fn verify_collected_shares(
    registry: &CommitteeRegistry,
    shares: &[ThresholdSignatureShare],
    digest: &[u8; 32],
) -> Vec<ThresholdSignatureShare> {
    let mut valid_shares = Vec::new();
    let mut seen_nodes = std::collections::HashSet::new();

    for share in shares {
        if !seen_nodes.insert(share.node_id.clone()) {
            tracing::warn!(
                peer = %share.node_id,
                "skipping duplicate signature share from node"
            );
            continue;
        }

        match verify_signature_share(registry, share, digest).await {
            Ok(()) => valid_shares.push(share.clone()),
            Err(e) => {
                tracing::warn!(
                    peer = %share.node_id,
                    error = %e,
                    "invalid signature share rejected"
                );
            }
        }
    }

    valid_shares
}

/// Prototype threshold-signed contract submission for hand settlement.
///
/// If threshold quorum is met (>= config.threshold valid shares):
///   Submits the settlement call with multi-party authorization.
/// If threshold quorum is NOT met (< config.threshold valid shares):
///   Logs diagnostic telemetry and evaluates the fallback path:
///   - If allow_fallback is true: falls back to coordinator emergency signing.
///   - If allow_fallback is false: aborts with an explicit error to trigger protocol timeout.
pub async fn submit_settlement_with_threshold(
    config: &ThresholdSigningConfig,
    _soroban_config: &SorobanConfig,
    registry: &CommitteeRegistry,
    payload: &SettlementCallPayload,
    candidate_shares: &[ThresholdSignatureShare],
) -> Result<ThresholdSubmissionResult, String> {
    let start = Instant::now();
    let digest = canonical_settlement_digest(payload);

    let valid_shares = verify_collected_shares(registry, candidate_shares, &digest).await;
    let count = valid_shares.len();

    if count >= config.threshold {
        tracing::info!(
            table_id = payload.table_id,
            hand_number = payload.hand_number,
            valid_shares = count,
            required_threshold = config.threshold,
            "threshold quorum met: proceeding with threshold settlement submission"
        );

        let duration_ms = start.elapsed().as_millis() as u64;
        let simulated_tx_hash = format!(
            "threshold_tx_{}_{}",
            payload.table_id,
            hex::encode(&digest[..8])
        );

        Ok(ThresholdSubmissionResult {
            tx_hash: simulated_tx_hash,
            signatures_collected: count,
            required_threshold: config.threshold,
            used_fallback: false,
            duration_ms,
        })
    } else {
        tracing::warn!(
            table_id = payload.table_id,
            hand_number = payload.hand_number,
            valid_shares = count,
            required_threshold = config.threshold,
            allow_fallback = config.allow_fallback,
            "threshold quorum NOT met for settlement"
        );

        if config.allow_fallback {
            tracing::warn!(
                table_id = payload.table_id,
                "activating fallback path: single-key coordinator settlement"
            );

            let duration_ms = start.elapsed().as_millis() as u64;
            let fallback_tx_hash = format!(
                "fallback_single_key_tx_{}_{}",
                payload.table_id,
                hex::encode(&digest[..8])
            );

            Ok(ThresholdSubmissionResult {
                tx_hash: fallback_tx_hash,
                signatures_collected: count,
                required_threshold: config.threshold,
                used_fallback: true,
                duration_ms,
            })
        } else {
            Err(format!(
                "threshold signing failed: collected {} of {} required shares and fallback is disabled",
                count, config.threshold
            ))
        }
    }
}

/// Helper to simulate a committee node signing a settlement digest.
pub fn create_test_signature_share(
    node_id: &str,
    signing_key: &SigningKey,
    digest: &[u8; 32],
    nonce: u64,
) -> ThresholdSignatureShare {
    let stellar_address =
        stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string();
    let message_hex = hex::encode(digest);
    let signature = signing_key.sign(message_hex.as_bytes());

    ThresholdSignatureShare {
        node_id: node_id.to_string(),
        stellar_address,
        signature: hex::encode(signature.to_bytes()),
        nonce,
    }
}

/// Latency benchmark result comparing single-key vs threshold signing paths.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyBenchmarkReport {
    pub single_key_duration_micros: u128,
    pub threshold_2_of_3_duration_micros: u128,
    pub threshold_3_of_3_duration_micros: u128,
    pub overhead_micros: u128,
    pub overhead_percentage: f64,
}

/// Benchmark the latency impact of threshold signing vs single-key signing on hand settlement.
pub async fn benchmark_threshold_latency(iterations: usize) -> LatencyBenchmarkReport {
    use rand::rngs::OsRng;

    let payload = SettlementCallPayload {
        table_id: 1,
        hand_number: 42,
        contract_id: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
        method: "submit_showdown".into(),
        pot_total: 50_000_000,
        winners: vec!["GBRPYHIL2CI3WHZDTOOQFC6EB4KJJGUJSY3NXMOCLWEZDTWE47XLNZT7".into()],
        nonce: 1001,
    };
    let digest = canonical_settlement_digest(&payload);

    let mut csprng = OsRng;
    let node0_sk = SigningKey::generate(&mut csprng);
    let node1_sk = SigningKey::generate(&mut csprng);
    let node2_sk = SigningKey::generate(&mut csprng);

    let registry = mpc_identity::new_registry();
    let addr0 = stellar_strkey::ed25519::PublicKey(node0_sk.verifying_key().to_bytes()).to_string();
    let addr1 = stellar_strkey::ed25519::PublicKey(node1_sk.verifying_key().to_bytes()).to_string();
    let addr2 = stellar_strkey::ed25519::PublicKey(node2_sk.verifying_key().to_bytes()).to_string();

    mpc_identity::register_node_identity(&registry, "0", &addr0).await.unwrap();
    mpc_identity::register_node_identity(&registry, "1", &addr1).await.unwrap();
    mpc_identity::register_node_identity(&registry, "2", &addr2).await.unwrap();

    let share0 = create_test_signature_share("0", &node0_sk, &digest, 1);
    let share1 = create_test_signature_share("1", &node1_sk, &digest, 2);
    let share2 = create_test_signature_share("2", &node2_sk, &digest, 3);

    // 1. Single-key baseline: 1 signature & verification
    let t0 = Instant::now();
    for _ in 0..iterations {
        let msg = hex::encode(digest);
        let sig = node0_sk.sign(msg.as_bytes());
        mpc_identity::verify_signature(&addr0, &msg, &hex::encode(sig.to_bytes())).unwrap();
    }
    let single_key_duration = t0.elapsed().as_micros() / (iterations as u128);

    // 2. Threshold 2-of-3: verify 2 shares
    let t1 = Instant::now();
    for _ in 0..iterations {
        let shares = vec![share0.clone(), share1.clone()];
        let verified = verify_collected_shares(&registry, &shares, &digest).await;
        assert_eq!(verified.len(), 2);
    }
    let threshold_2_duration = t1.elapsed().as_micros() / (iterations as u128);

    // 3. Threshold 3-of-3: verify 3 shares
    let t2 = Instant::now();
    for _ in 0..iterations {
        let shares = vec![share0.clone(), share1.clone(), share2.clone()];
        let verified = verify_collected_shares(&registry, &shares, &digest).await;
        assert_eq!(verified.len(), 3);
    }
    let threshold_3_duration = t2.elapsed().as_micros() / (iterations as u128);

    let overhead = threshold_2_duration.saturating_sub(single_key_duration);
    let overhead_pct = if single_key_duration > 0 {
        (overhead as f64 / single_key_duration as f64) * 100.0
    } else {
        0.0
    };

    LatencyBenchmarkReport {
        single_key_duration_micros: single_key_duration,
        threshold_2_of_3_duration_micros: threshold_2_duration,
        threshold_3_of_3_duration_micros: threshold_3_duration,
        overhead_micros: overhead,
        overhead_percentage: overhead_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    async fn setup_test_committee() -> (
        CommitteeRegistry,
        (SigningKey, String),
        (SigningKey, String),
        (SigningKey, String),
    ) {
        let mut csprng = OsRng;
        let sk0 = SigningKey::generate(&mut csprng);
        let sk1 = SigningKey::generate(&mut csprng);
        let sk2 = SigningKey::generate(&mut csprng);

        let addr0 =
            stellar_strkey::ed25519::PublicKey(sk0.verifying_key().to_bytes()).to_string();
        let addr1 =
            stellar_strkey::ed25519::PublicKey(sk1.verifying_key().to_bytes()).to_string();
        let addr2 =
            stellar_strkey::ed25519::PublicKey(sk2.verifying_key().to_bytes()).to_string();

        let registry = mpc_identity::new_registry();
        mpc_identity::register_node_identity(&registry, "0", &addr0).await.unwrap();
        mpc_identity::register_node_identity(&registry, "1", &addr1).await.unwrap();
        mpc_identity::register_node_identity(&registry, "2", &addr2).await.unwrap();

        (registry, (sk0, addr0), (sk1, addr1), (sk2, addr2))
    }

    fn sample_payload() -> SettlementCallPayload {
        SettlementCallPayload {
            table_id: 1,
            hand_number: 10,
            contract_id: "CDKJ56S7K7QC4LG6SFF2OGDTG6N4QBCJOVRWHY7MKCWD5JPQ6MDAHRAM".into(),
            method: "submit_showdown".into(),
            pot_total: 100_000_000,
            winners: vec!["GBRPYHIL2CI3WHZDTOOQFC6EB4KJJGUJSY3NXMOCLWEZDTWE47XLNZT7".into()],
            nonce: 777,
        }
    }

    #[tokio::test]
    async fn threshold_submission_succeeds_with_two_of_three_quorum() {
        let (registry, (sk0, _), (sk1, _), _) = setup_test_committee().await;
        let payload = sample_payload();
        let digest = canonical_settlement_digest(&payload);

        let share0 = create_test_signature_share("0", &sk0, &digest, 1);
        let share1 = create_test_signature_share("1", &sk1, &digest, 2);

        let config = ThresholdSigningConfig {
            threshold: 2,
            committee_size: 3,
            timeout_ms: 1000,
            allow_fallback: false,
        };

        let soroban_cfg = SorobanConfig::from_env();
        let res = submit_settlement_with_threshold(
            &config,
            &soroban_cfg,
            &registry,
            &payload,
            &[share0, share1],
        )
        .await
        .expect("2-of-3 threshold must succeed");

        assert_eq!(res.signatures_collected, 2);
        assert_eq!(res.required_threshold, 2);
        assert!(!res.used_fallback);
        assert!(res.tx_hash.starts_with("threshold_tx_"));
    }

    #[tokio::test]
    async fn threshold_submission_triggers_fallback_when_threshold_not_met() {
        let (registry, (sk0, _), _, _) = setup_test_committee().await;
        let payload = sample_payload();
        let digest = canonical_settlement_digest(&payload);

        // Only 1 node responds (below threshold of 2)
        let share0 = create_test_signature_share("0", &sk0, &digest, 1);

        let config = ThresholdSigningConfig {
            threshold: 2,
            committee_size: 3,
            timeout_ms: 1000,
            allow_fallback: true, // fallback allowed
        };

        let soroban_cfg = SorobanConfig::from_env();
        let res = submit_settlement_with_threshold(
            &config,
            &soroban_cfg,
            &registry,
            &payload,
            &[share0],
        )
        .await
        .expect("fallback submission should succeed when permitted");

        assert_eq!(res.signatures_collected, 1);
        assert!(res.used_fallback);
        assert!(res.tx_hash.starts_with("fallback_single_key_tx_"));
    }

    #[tokio::test]
    async fn threshold_submission_fails_hard_when_threshold_not_met_and_fallback_disabled() {
        let (registry, (sk0, _), _, _) = setup_test_committee().await;
        let payload = sample_payload();
        let digest = canonical_settlement_digest(&payload);

        let share0 = create_test_signature_share("0", &sk0, &digest, 1);

        let config = ThresholdSigningConfig {
            threshold: 2,
            committee_size: 3,
            timeout_ms: 1000,
            allow_fallback: false, // fallback disallowed
        };

        let soroban_cfg = SorobanConfig::from_env();
        let err = submit_settlement_with_threshold(
            &config,
            &soroban_cfg,
            &registry,
            &payload,
            &[share0],
        )
        .await
        .expect_err("submission must fail when threshold not met and fallback disabled");

        assert!(err.contains("threshold signing failed"));
        assert!(err.contains("collected 1 of 2"));
    }

    #[tokio::test]
    async fn rejects_invalid_and_duplicate_signature_shares() {
        let (registry, (sk0, _), (sk1, _), _) = setup_test_committee().await;
        let payload = sample_payload();
        let digest = canonical_settlement_digest(&payload);

        let share0 = create_test_signature_share("0", &sk0, &digest, 1);
        let mut tampered_share1 = create_test_signature_share("1", &sk1, &digest, 2);
        tampered_share1.signature = "00".repeat(64); // corrupt signature

        let shares = vec![share0.clone(), tampered_share1, share0]; // share0 repeated
        let valid = verify_collected_shares(&registry, &shares, &digest).await;

        // Only 1 unique valid share should survive
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].node_id, "0");
    }

    #[tokio::test]
    async fn latency_benchmark_runs_and_verifies_computational_bounds() {
        let report = benchmark_threshold_latency(10).await;
        println!("Threshold Signing Benchmark: {:?}", report);
        // Crypto verification on 2 shares is well under 50ms in microbenchmarks
        assert!(report.threshold_2_of_3_duration_micros < 50_000);
    }
}
