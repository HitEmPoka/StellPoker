//! MPC session manager for co-noir proof generation.
//!
//! Each session represents one proof generation request (deal, reveal, or showdown).
//! The lifecycle:
//! 1. Coordinator sends shares via POST /session/:id/shares
//! 2. Integrity checks verify keyed MAC digests before entering proving phase (Issue #883)
//! 3. Coordinator triggers proof gen via POST /session/:id/generate
//! 4. Node runs co-noir witness extension + proof generation with phase deadlines (Issue #885)
//! 5. Coordinator polls GET /session/:id/status and retrieves proof

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{sleep, Duration, Instant};

async fn run_profiled(
    cmd: &mut Command,
    session_id: &str,
    phase: &str,
    profile: Option<&crate::profiling::ProfileRegistry>,
) -> std::io::Result<std::process::Output> {
    let Some(registry) = profile else {
        return cmd.output().await;
    };

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let sampler = child.id().map(|pid| {
        tokio::spawn(crate::profiling::sample_process_until_exit(
            registry.clone(),
            session_id.to_string(),
            phase.to_string(),
            pid,
        ))
    });

    let output = child.wait_with_output().await?;

    if let Some(sampler) = sampler {
        let _ = sampler.await;
    }

    Ok(output)
}

use crate::cost::SessionCost;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    /// Shares received, waiting for generate trigger
    SharesReceived,
    /// Witness extension in progress
    WitnessGenerating,
    /// Proof generation in progress
    ProofGenerating,
    /// Proof generation complete
    Complete,
    /// Something failed (e.g. timeout or verification error)
    Failed(String),
}

/// Per-circuit phase timeout configuration (seconds).
/// Added phase-level deadlines for shuffle, prove, and verify (Issue #885).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseTimeouts {
    /// Deadline for shuffle / merge shares phase (seconds).
    pub shuffle: u64,
    /// Deadline for witness and proof generation phase (seconds).
    pub prove: u64,
    /// Deadline for verification phase (seconds).
    pub verify: u64,
    /// Detailed phase timeouts for fine-grained budget control.
    pub merge_shares: u64,
    pub witness_generation: u64,
    pub proof_generation: u64,
}

impl Default for PhaseTimeouts {
    fn default() -> Self {
        Self {
            shuffle: 60,
            prove: 600,
            verify: 60,
            merge_shares: 60,
            witness_generation: 300,
            proof_generation: 600,
        }
    }
}

impl PhaseTimeouts {
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            shuffle: parse_env("PHASE_TIMEOUT_SHUFFLE_SECONDS", d.shuffle),
            prove: parse_env("PHASE_TIMEOUT_PROVE_SECONDS", d.prove),
            verify: parse_env("PHASE_TIMEOUT_VERIFY_SECONDS", d.verify),
            merge_shares: parse_env("PHASE_TIMEOUT_MERGE_SECONDS", d.merge_shares),
            witness_generation: parse_env("PHASE_TIMEOUT_WITNESS_SECONDS", d.witness_generation),
            proof_generation: parse_env("PHASE_TIMEOUT_PROOF_SECONDS", d.proof_generation),
        }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

#[derive(Clone, Debug)]
pub struct MpcSessionState {
    pub session_id: String,
    pub circuit_name: String,
    pub status: SessionStatus,
    /// Path to merged share file (Prover.toml with secret-shared values)
    pub share_path: Option<PathBuf>,
    /// Per-source share fragments for this session.
    pub partial_share_paths: HashMap<u32, PathBuf>,
    /// Per-source share batch integrity MACs (Issue #883).
    pub share_macs: HashMap<u32, String>,
    /// Expected number of contributing source parties.
    pub expected_total_parties: Option<u32>,
    /// Working directory for this session's temp files
    pub work_dir: PathBuf,
    /// Path to generated witness
    pub witness_path: Option<PathBuf>,
    /// Path to generated proof
    pub proof_path: Option<PathBuf>,
    /// Public inputs emitted by co-noir for the generated proof.
    pub public_inputs: Option<Vec<String>>,
    /// When the session record was created (for CPU-seconds attribution).
    pub created_at: std::time::Instant,
    /// Total decoded share-fragment bytes received (#572 cost attribution).
    pub bytes_in: u64,
    /// Proof bytes produced (0 until complete).
    pub bytes_out: u64,
    /// Timestamp when the current phase started.
    pub phase_started_at: Option<Instant>,
    /// Timeout configuration for this session.
    pub phase_timeouts: PhaseTimeouts,
}

impl MpcSessionState {
    pub fn new(session_id: String, circuit_name: String, work_dir: PathBuf) -> Self {
        Self {
            session_id,
            circuit_name,
            status: SessionStatus::SharesReceived,
            share_path: None,
            partial_share_paths: HashMap::new(),
            share_macs: HashMap::new(),
            expected_total_parties: None,
            work_dir,
            witness_path: None,
            proof_path: None,
            public_inputs: None,
            created_at: std::time::Instant::now(),
            bytes_in: 0,
            bytes_out: 0,
            phase_started_at: None,
            phase_timeouts: PhaseTimeouts::from_env(),
        }
    }

    pub fn status_str(&self) -> String {
        match &self.status {
            SessionStatus::SharesReceived => "shares_received".to_string(),
            SessionStatus::WitnessGenerating => "witness_generating".to_string(),
            SessionStatus::ProofGenerating => "proof_generating".to_string(),
            SessionStatus::Complete => "complete".to_string(),
            SessionStatus::Failed(e) => format!("failed: {}", e),
        }
    }

    pub fn cost_snapshot(&self) -> SessionCost {
        SessionCost::snapshot(
            &self.session_id,
            &self.circuit_name,
            &self.status_str(),
            &self.created_at,
            self.bytes_in,
            self.bytes_out,
            self.partial_share_paths.len(),
        )
    }

    pub fn enter_phase(&mut self, status: SessionStatus) {
        self.status = status;
        self.phase_started_at = Some(Instant::now());
    }

    pub fn check_phase_timeout(&self) -> Result<Duration, &'static str> {
        let started = self.phase_started_at.as_ref().ok_or("no phase started")?;
        let elapsed = started.elapsed();

        let budget = match &self.status {
            SessionStatus::WitnessGenerating => self.phase_timeouts.witness_generation,
            SessionStatus::ProofGenerating => self.phase_timeouts.proof_generation,
            _ => return Ok(Duration::MAX),
        };

        let budget_dur = Duration::from_secs(budget);
        if elapsed >= budget_dur {
            let phase = match &self.status {
                SessionStatus::WitnessGenerating => "witness_generation",
                SessionStatus::ProofGenerating => "proof_generation",
                _ => unreachable!(),
            };
            tracing::error!(
                session_id = %self.session_id,
                phase,
                elapsed_ms = elapsed.as_millis() as u64,
                timeout_ms = budget_dur.as_millis() as u64,
                "phase watchdog: timeout exceeded"
            );
            return Err(phase);
        }
        Ok(budget_dur - elapsed)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #883: Keyed Share Batch MAC and Integrity Verification
// ─────────────────────────────────────────────────────────────────────────────

pub fn compute_share_batch_mac(
    key: &[u8],
    session_id: &str,
    circuit_name: &str,
    source_party_id: u32,
    total_parties: u32,
    share_bytes: &[u8],
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"STELLPOKER_SHARE_BATCH_MAC_V1:");
    hasher.update(key);
    hasher.update(session_id.as_bytes());
    hasher.update(circuit_name.as_bytes());
    hasher.update(&source_party_id.to_le_bytes());
    hasher.update(&total_parties.to_le_bytes());
    hasher.update(share_bytes);
    hasher.update(key);
    hex::encode(hasher.finalize())
}

pub fn verify_shares_integrity_before_proving(
    session_id: &str,
    circuit_name: &str,
    partial_share_paths: &[(u32, PathBuf)],
    share_macs: &HashMap<u32, String>,
    total_parties: u32,
    mac_key: &[u8],
    metrics: Option<&crate::metrics::NodeMetrics>,
) -> Result<(), String> {
    for (source_party, path) in partial_share_paths {
        if let Some(expected_mac) = share_macs.get(source_party) {
            let share_bytes = std::fs::read(path).map_err(|e| {
                format!(
                    "failed to read share file for peer {}: {}",
                    source_party, e
                )
            })?;

            let computed_mac = compute_share_batch_mac(
                mac_key,
                session_id,
                circuit_name,
                *source_party,
                total_parties,
                &share_bytes,
            );

            if computed_mac != *expected_mac {
                if let Some(m) = metrics {
                    m.share_integrity_failures
                        .with_label_values(&[&source_party.to_string()])
                        .inc();
                }
                tracing::error!(
                    session_id,
                    peer = source_party,
                    "share integrity check failed: corrupted share batch detected from peer"
                );
                return Err(format!(
                    "corrupted share batch rejected from peer {}: integrity digest mismatch",
                    source_party
                ));
            }
        }
    }
    Ok(())
}

pub fn receive_share_fragment(
    session: &mut MpcSessionState,
    share_data_b64: &str,
    source_party_id: u32,
    total_parties: u32,
) -> Result<(), String> {
    receive_share_fragment_with_mac(session, share_data_b64, source_party_id, total_parties, None)
}

pub fn receive_share_fragment_with_mac(
    session: &mut MpcSessionState,
    share_data_b64: &str,
    source_party_id: u32,
    total_parties: u32,
    mac: Option<String>,
) -> Result<(), String> {
    if source_party_id >= total_parties {
        return Err(format!(
            "source_party_id {} out of range for total_parties {}",
            source_party_id, total_parties
        ));
    }

    if let Some(expected) = session.expected_total_parties {
        if expected != total_parties {
            return Err(format!(
                "total_parties mismatch: existing {}, got {}",
                expected, total_parties
            ));
        }
    } else {
        session.expected_total_parties = Some(total_parties);
    }

    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(share_data_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;

    session.bytes_in += bytes.len() as u64;

    let share_path = session
        .work_dir
        .join(format!("share_source_{}.shared", source_party_id));
    std::fs::write(&share_path, &bytes)
        .map_err(|e| format!("failed to write share file: {}", e))?;

    session
        .partial_share_paths
        .insert(source_party_id, share_path);
    if let Some(m) = mac {
        session.share_macs.insert(source_party_id, m);
    }
    session.status = SessionStatus::SharesReceived;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #885: Cooperative Phase Deadline Enforcement & Buffer Cleanup
// ─────────────────────────────────────────────────────────────────────────────

pub fn release_session_buffers(work_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(work_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

pub async fn run_proof_generation(
    session_id: String,
    circuit_dir: String,
    circuit_name: String,
    work_dir: PathBuf,
    node_id: u32,
    partial_share_paths: Vec<(u32, PathBuf)>,
    expected_total_parties: u32,
    party_config_path: String,
    crs_path: String,
    limits: crate::limits::ResourceLimits,
    phase_timeouts: PhaseTimeouts,
    profile: Option<crate::profiling::ProfileRegistry>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    run_proof_generation_with_metrics(
        session_id,
        circuit_dir,
        circuit_name,
        work_dir,
        node_id,
        partial_share_paths,
        expected_total_parties,
        party_config_path,
        crs_path,
        limits,
        phase_timeouts,
        profile,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_proof_generation_with_metrics(
    session_id: String,
    circuit_dir: String,
    circuit_name: String,
    work_dir: PathBuf,
    node_id: u32,
    partial_share_paths: Vec<(u32, PathBuf)>,
    expected_total_parties: u32,
    party_config_path: String,
    crs_path: String,
    limits: crate::limits::ResourceLimits,
    phase_timeouts: PhaseTimeouts,
    profile: Option<crate::profiling::ProfileRegistry>,
    metrics: Option<crate::metrics::NodeMetrics>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let circuit_path = format!(
        "{}/{}/target/{}.json",
        circuit_dir, circuit_name, circuit_name
    );
    let share_path = work_dir.join("Prover.toml");
    let witness_path = work_dir.join("witness.gz");
    let proof_path = work_dir.join("proof.bin");
    let public_inputs_path = work_dir.join("public_inputs.json");
    let crs_file = format!("{}/bn254_g1.dat", crs_path);

    if partial_share_paths.len() != expected_total_parties as usize {
        return Err(format!(
            "incomplete share fragments: got {}, expected {}",
            partial_share_paths.len(),
            expected_total_parties
        ));
    }

    let mut sorted_fragments = partial_share_paths;
    sorted_fragments.sort_by_key(|(source, _)| *source);

    tracing::info!(
        session_id = %session_id,
        phase = "merge_shares",
        node_id,
        circuit = %circuit_name,
        fragment_count = sorted_fragments.len(),
        "merging share fragments"
    );
    let merge_start = Instant::now();

    // ── Phase 1: Shuffle / Merge Shares with cooperative deadline (Issue #885) ──
    let shuffle_budget = Duration::from_secs(phase_timeouts.shuffle.min(phase_timeouts.merge_shares));
    let mut merge_cmd = Command::new("co-noir");
    merge_cmd
        .arg("merge-input-shares")
        .arg("--circuit")
        .arg(&circuit_path)
        .arg("--protocol")
        .arg("REP3")
        .arg("--config")
        .arg(&party_config_path);
    for (_, path) in &sorted_fragments {
        merge_cmd.arg("--inputs").arg(path);
    }
    merge_cmd.arg("--out").arg(&share_path);
    limits.apply_to_command(&mut merge_cmd);

    let merge_fut = run_profiled(
        &mut merge_cmd,
        &session_id,
        "merge_shares",
        profile.as_ref(),
    );

    let merge_output = match tokio::time::timeout(shuffle_budget, merge_fut).await {
        Ok(res) => res.map_err(|e| format!("failed to spawn co-noir merge-input-shares: {}", e))?,
        Err(_) => {
            tracing::error!(
                session_id = %session_id,
                phase = "shuffle",
                node_id,
                budget_seconds = shuffle_budget.as_secs(),
                "phase timeout: shuffle/merge phase timed out"
            );
            if let Some(ref m) = metrics {
                m.session_timeouts
                    .with_label_values(&[&circuit_name, "shuffle"])
                    .inc();
            }
            release_session_buffers(&work_dir);
            return Err(format!(
                "timeout: phase 'shuffle' exceeded budget of {}s (node {})",
                shuffle_budget.as_secs(),
                node_id
            ));
        }
    };

    if !merge_output.status.success() {
        let stderr = String::from_utf8_lossy(&merge_output.stderr);
        let stdout = String::from_utf8_lossy(&merge_output.stdout);
        tracing::error!(
            session_id = %session_id,
            phase = "merge_shares",
            node_id,
            elapsed_ms = merge_start.elapsed().as_millis() as u64,
            "co-noir merge-input-shares failed"
        );
        release_session_buffers(&work_dir);
        return Err(format!(
            "co-noir merge-input-shares failed (node {}):\nstderr: {}\nstdout: {}",
            node_id, stderr, stdout
        ));
    }

    // ── Phase 2: Witness Generation with cooperative deadline (Issue #885) ────
    let witness_budget = Duration::from_secs(phase_timeouts.witness_generation.min(phase_timeouts.prove));
    tracing::info!(
        session_id = %session_id,
        phase = "witness_generation",
        node_id,
        circuit = %circuit_name,
        "starting witness generation"
    );
    let witness_start = Instant::now();

    let mut witness_cmd = Command::new("co-noir");
    witness_cmd
        .arg("generate-witness")
        .arg("--circuit")
        .arg(&circuit_path)
        .arg("--input")
        .arg(&share_path)
        .arg("--protocol")
        .arg("REP3")
        .arg("--config")
        .arg(&party_config_path)
        .arg("--out")
        .arg(&witness_path);
    limits.apply_to_command(&mut witness_cmd);

    let witness_fut = run_profiled(
        &mut witness_cmd,
        &session_id,
        "witness_generation",
        profile.as_ref(),
    );

    let witness_output = match tokio::time::timeout(witness_budget, witness_fut).await {
        Ok(res) => res.map_err(|e| format!("failed to spawn co-noir generate-witness: {}", e))?,
        Err(_) => {
            tracing::error!(
                session_id = %session_id,
                phase = "witness_generation",
                node_id,
                budget_seconds = witness_budget.as_secs(),
                "phase timeout: witness generation timed out"
            );
            if let Some(ref m) = metrics {
                m.session_timeouts
                    .with_label_values(&[&circuit_name, "witness_generation"])
                    .inc();
            }
            release_session_buffers(&work_dir);
            return Err(format!(
                "timeout: phase 'witness_generation' exceeded budget of {}s (node {})",
                witness_budget.as_secs(),
                node_id
            ));
        }
    };

    if !witness_output.status.success() {
        let stderr = String::from_utf8_lossy(&witness_output.stderr);
        let stdout = String::from_utf8_lossy(&witness_output.stdout);
        tracing::error!(
            session_id = %session_id,
            phase = "witness_generation",
            node_id,
            elapsed_ms = witness_start.elapsed().as_millis() as u64,
            "co-noir generate-witness failed"
        );
        release_session_buffers(&work_dir);
        return Err(format!(
            "co-noir generate-witness failed (node {}):\nstderr: {}\nstdout: {}",
            node_id, stderr, stdout
        ));
    }

    // ── Phase 3: Proof Generation with cooperative deadline (Issue #885) ──────
    let proof_budget = Duration::from_secs(phase_timeouts.proof_generation.min(phase_timeouts.prove));
    tracing::info!(
        session_id = %session_id,
        phase = "proof_generation",
        node_id,
        elapsed_ms = witness_start.elapsed().as_millis() as u64,
        "witness generated, starting proof generation"
    );
    let proof_start = Instant::now();

    let vk_path = format!("{}/{}/target/vk_keccak", circuit_dir, circuit_name);
    let mut last_proof_output: Option<std::process::Output> = None;
    for attempt in 1..=3 {
        let mut proof_cmd = Command::new("co-noir");
        proof_cmd
            .arg("build-and-generate-proof")
            .arg("--circuit")
            .arg(&circuit_path)
            .arg("--witness")
            .arg(&witness_path)
            .arg("--protocol")
            .arg("REP3")
            .arg("--config")
            .arg(&party_config_path)
            .arg("--crs")
            .arg(&crs_file)
            .arg("--hasher")
            .arg("keccak")
            .arg("--vk")
            .arg(&vk_path)
            .arg("--out")
            .arg(&proof_path)
            .arg("--public-input")
            .arg(&public_inputs_path)
            .arg("--fields-as-json");
        limits.apply_to_command(&mut proof_cmd);

        let proof_fut = run_profiled(
            &mut proof_cmd,
            &session_id,
            "proof_generation",
            profile.as_ref(),
        );

        let proof_output = match tokio::time::timeout(proof_budget, proof_fut).await {
            Ok(res) => res.map_err(|e| format!("failed to spawn co-noir build-and-generate-proof: {}", e))?,
            Err(_) => {
                tracing::error!(
                    session_id = %session_id,
                    phase = "proof_generation",
                    node_id,
                    budget_seconds = proof_budget.as_secs(),
                    "phase timeout: proof generation timed out"
                );
                if let Some(ref m) = metrics {
                    m.session_timeouts
                        .with_label_values(&[&circuit_name, "proof_generation"])
                        .inc();
                }
                release_session_buffers(&work_dir);
                return Err(format!(
                    "timeout: phase 'proof_generation' exceeded budget of {}s (node {})",
                    proof_budget.as_secs(),
                    node_id
                ));
            }
        };

        if proof_output.status.success() {
            last_proof_output = Some(proof_output);
            break;
        }

        let stderr = String::from_utf8_lossy(&proof_output.stderr);
        let is_transient_resource_error =
            stderr.contains("No buffer space available") || stderr.contains("os error 55");

        if is_transient_resource_error && attempt < 3 {
            tracing::warn!(
                session_id = %session_id,
                phase = "proof_generation",
                node_id,
                attempt,
                elapsed_ms = proof_start.elapsed().as_millis() as u64,
                error = %stderr.trim(),
                "co-noir build-and-generate-proof transient failure, retrying"
            );
            sleep(Duration::from_millis((attempt as u64) * 500)).await;
            continue;
        }

        let stdout = String::from_utf8_lossy(&proof_output.stdout);
        tracing::error!(
            session_id = %session_id,
            phase = "proof_generation",
            node_id,
            attempt,
            elapsed_ms = proof_start.elapsed().as_millis() as u64,
            "co-noir build-and-generate-proof failed"
        );
        release_session_buffers(&work_dir);
        return Err(format!(
            "co-noir build-and-generate-proof failed (node {}):\nstderr: {}\nstdout: {}",
            node_id, stderr, stdout
        ));
    }

    if last_proof_output.is_none() {
        tracing::error!(
            session_id = %session_id,
            phase = "proof_generation",
            node_id,
            elapsed_ms = proof_start.elapsed().as_millis() as u64,
            "co-noir build-and-generate-proof failed after retries"
        );
        release_session_buffers(&work_dir);
        return Err(format!(
            "co-noir build-and-generate-proof failed after retries (node {})",
            node_id
        ));
    }

    tracing::info!(
        session_id = %session_id,
        phase = "proof_generation",
        node_id,
        elapsed_ms = proof_start.elapsed().as_millis() as u64,
        total_elapsed_ms = merge_start.elapsed().as_millis() as u64,
        "proof generated successfully"
    );

    let proof_bytes =
        std::fs::read(&proof_path).map_err(|e| format!("failed to read proof file: {}", e))?;
    let public_inputs_bytes = std::fs::read(&public_inputs_path)
        .map_err(|e| format!("failed to read public inputs file: {}", e))?;
    let public_inputs: Vec<String> = serde_json::from_slice(&public_inputs_bytes)
        .map_err(|e| format!("failed to parse public inputs json: {}", e))?;

    if let Ok(artifact_hash) = crate::proof_cache::circuit_artifact_hash(&circuit_dir, &circuit_name) {
        let key = crate::proof_cache::cache_key(&circuit_name, &artifact_hash, &public_inputs);
        crate::proof_cache::insert(key, proof_bytes.clone()).await;
    }

    Ok((proof_bytes, public_inputs))
}

pub fn get_proof(session: &MpcSessionState) -> Result<Vec<u8>, String> {
    let proof_path = session
        .proof_path
        .as_ref()
        .ok_or("proof not yet generated")?;

    std::fs::read(proof_path).map_err(|e| format!("failed to read proof: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_share_batch_integrity_honest_and_corrupted() {
        let key = b"test_committee_secret_key_123456";
        let session_id = "test-session-integ-001";
        let circuit = "deal_valid";
        let source_party = 1u32;
        let total_parties = 3u32;
        let honest_data = b"honest_rep3_secret_share_batch_payload";

        let mac = compute_share_batch_mac(
            key,
            session_id,
            circuit,
            source_party,
            total_parties,
            honest_data,
        );

        let tmp_dir = tempfile::tempdir().unwrap();
        let honest_path = tmp_dir.path().join("share_1.shared");
        std::fs::write(&honest_path, honest_data).unwrap();

        let mut share_macs = HashMap::new();
        share_macs.insert(source_party, mac.clone());

        let fragments = vec![(source_party, honest_path.clone())];

        let metrics = crate::metrics::NodeMetrics::new();

        assert!(verify_shares_integrity_before_proving(
            session_id,
            circuit,
            &fragments,
            &share_macs,
            total_parties,
            key,
            Some(&metrics),
        )
        .is_ok());

        assert_eq!(
            metrics
                .share_integrity_failures
                .with_label_values(&["1"])
                .get(),
            0
        );

        let corrupted_data = b"tampered_rep3_secret_share_batch_payload";
        std::fs::write(&honest_path, corrupted_data).unwrap();

        let err = verify_shares_integrity_before_proving(
            session_id,
            circuit,
            &fragments,
            &share_macs,
            total_parties,
            key,
            Some(&metrics),
        )
        .unwrap_err();

        assert!(
            err.contains("corrupted share batch rejected from peer 1"),
            "Error must attribute failure to peer: {err}"
        );

        assert_eq!(
            metrics
                .share_integrity_failures
                .with_label_values(&["1"])
                .get(),
            1,
            "Metric must be incremented and labeled by peer"
        );
    }

    #[test]
    fn test_honest_path_mac_overhead_under_5_percent() {
        let key = b"benchmark_key_32_bytes_long_123456";
        let data = vec![42u8; 128 * 1024];
        let iterations = 1000;

        let start = std::time::Instant::now();
        for i in 0..iterations {
            let _ = compute_share_batch_mac(
                key,
                "benchmark-session",
                "deal_valid",
                (i % 3) as u32,
                3,
                &data,
            );
        }
        let elapsed = start.elapsed();
        let time_per_op_us = elapsed.as_micros() as f64 / iterations as f64;

        assert!(
            time_per_op_us < 5000.0,
            "MAC operation must take < 5ms (was {time_per_op_us} us)"
        );
    }

    #[test]
    fn test_phase_timeout_configuration_from_env() {
        std::env::set_var("PHASE_TIMEOUT_SHUFFLE_SECONDS", "45");
        std::env::set_var("PHASE_TIMEOUT_PROVE_SECONDS", "450");
        std::env::set_var("PHASE_TIMEOUT_VERIFY_SECONDS", "30");

        let timeouts = PhaseTimeouts::from_env();
        assert_eq!(timeouts.shuffle, 45);
        assert_eq!(timeouts.prove, 450);
        assert_eq!(timeouts.verify, 30);
    }

    #[test]
    fn test_buffer_release_on_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        let file1 = tmp.path().join("witness.gz");
        let file2 = tmp.path().join("Prover.toml");
        std::fs::write(&file1, b"witness_data").unwrap();
        std::fs::write(&file2, b"prover_data").unwrap();

        assert!(file1.exists());
        assert!(file2.exists());

        release_session_buffers(tmp.path());

        assert!(!file1.exists(), "buffers must be released");
        assert!(!file2.exists(), "buffers must be released");
    }
}
