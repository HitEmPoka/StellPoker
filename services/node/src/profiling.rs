//! On-demand CPU/memory profiling per MPC session phase (issue #244).
//!
//! The actual MPC compute work for a session happens in `co-noir` child
//! processes spawned per phase (`merge_shares`, `witness_generation`,
//! `proof_generation` — see `session::run_proof_generation`), not in this
//! Rust process's own call stack. An in-process sampling profiler (e.g. the
//! `pprof` crate) walks *this* process's call stack, so it would never see
//! the expensive work at all — it would only ever show time spent
//! orchestrating (spawning the child, waiting on it, parsing its output),
//! which is not what "profile a session" means here.
//!
//! Instead, profiling samples each child process's OS-reported CPU% and
//! RSS on a fixed interval for the duration of its phase, aggregated into a
//! peak-memory / average-and-peak-CPU% summary per phase, keyed by
//! session_id. This is exported as JSON rather than the pprof wire format:
//! pprof's call-graph model doesn't apply to an opaque external process
//! whose own internals this node has no visibility into — a flamegraph
//! needs sampled stack traces, which this project's `co-noir` dependency
//! doesn't expose to callers.
//!
//! Profiling is strictly opt-in per session (issue #244's "trigger
//! profiling on demand via API") — sampling only happens for a session
//! whose id has been explicitly enabled via `POST /session/:id/profile`,
//! so a session nobody asked to profile pays zero sampling overhead beyond
//! one registry lookup.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use tokio::sync::RwLock;

/// How often a profiled child process's resource usage is sampled.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(200);

/// Aggregated resource usage for one phase of one session's proof
/// generation run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseProfile {
    /// "merge_shares" | "witness_generation" | "proof_generation". A
    /// retried proof_generation attempt appends another entry with the
    /// same phase name rather than overwriting the previous attempt's.
    pub phase: String,
    pub duration_ms: u64,
    pub peak_memory_bytes: u64,
    pub sample_count: u32,
    pub avg_cpu_percent: f32,
    pub peak_cpu_percent: f32,
}

/// The full profile collected so far for one session.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionProfile {
    pub session_id: String,
    pub phases: Vec<PhaseProfile>,
}

/// Registry of which sessions have profiling enabled and what's been
/// collected for them so far. Cheap to clone (Arc-backed) so it can be
/// threaded into the background proof-generation task alongside the other
/// state `post_generate` already captures.
#[derive(Clone)]
pub struct ProfileRegistry {
    enabled: Arc<RwLock<HashSet<String>>>,
    profiles: Arc<RwLock<HashMap<String, SessionProfile>>>,
}

impl ProfileRegistry {
    pub fn new() -> Self {
        Self {
            enabled: Arc::new(RwLock::new(HashSet::new())),
            profiles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Enable profiling for `session_id`. Idempotent — calling it again on
    /// an already-enabled session does not clear previously collected
    /// phases.
    pub async fn enable(&self, session_id: &str) {
        self.enabled.write().await.insert(session_id.to_string());
        self.profiles
            .write()
            .await
            .entry(session_id.to_string())
            .or_insert_with(|| SessionProfile {
                session_id: session_id.to_string(),
                phases: Vec::new(),
            });
    }

    pub async fn is_enabled(&self, session_id: &str) -> bool {
        self.enabled.read().await.contains(session_id)
    }

    pub async fn get(&self, session_id: &str) -> Option<SessionProfile> {
        self.profiles.read().await.get(session_id).cloned()
    }

    async fn record_phase(&self, session_id: &str, phase: PhaseProfile) {
        let mut profiles = self.profiles.write().await;
        profiles
            .entry(session_id.to_string())
            .or_insert_with(|| SessionProfile {
                session_id: session_id.to_string(),
                phases: Vec::new(),
            })
            .phases
            .push(phase);
    }
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Sample `pid`'s CPU% and memory every [`SAMPLE_INTERVAL`] until the
/// process can no longer be found (i.e. it exited), then record the
/// aggregated [`PhaseProfile`] for `phase` into `registry`.
///
/// Spawned as its own task by `session::run_profiled` so sampling runs
/// concurrently with (not blocking) awaiting the child process itself.
///
/// `duration_ms` is measured from when this task starts sampling, at
/// [`SAMPLE_INTERVAL`] granularity — a phase that finishes faster than one
/// sample interval is reported as taking roughly one interval with zero
/// samples, rather than its true (shorter) wall-clock time. In practice
/// MPC witness/proof generation phases run for seconds to minutes, well
/// above that granularity, so this is a deliberate simplicity/precision
/// tradeoff rather than a correctness gap for the phases this actually
/// profiles.
pub async fn sample_process_until_exit(
    registry: ProfileRegistry,
    session_id: String,
    phase: String,
    pid: u32,
) {
    let start = Instant::now();
    let mut sys = System::new();
    let sysinfo_pid = Pid::from_u32(pid);
    let mut peak_memory_bytes: u64 = 0;
    let mut cpu_samples: Vec<f32> = Vec::new();

    loop {
        tokio::time::sleep(SAMPLE_INTERVAL).await;
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo_pid]), true);
        let Some(process) = sys.process(sysinfo_pid) else {
            break;
        };
        // sysinfo's Process::memory() reports KiB in this version (see
        // main.rs's identical *1024 conversion for the node's own process),
        // not bytes — convert here so PhaseProfile::peak_memory_bytes means
        // what its name says.
        let memory_bytes = process.memory() * 1024;
        peak_memory_bytes = peak_memory_bytes.max(memory_bytes);
        cpu_samples.push(process.cpu_usage());
    }

    let avg_cpu_percent = if cpu_samples.is_empty() {
        0.0
    } else {
        cpu_samples.iter().sum::<f32>() / cpu_samples.len() as f32
    };
    let peak_cpu_percent = cpu_samples.iter().cloned().fold(0.0f32, f32::max);

    registry
        .record_phase(
            &session_id,
            PhaseProfile {
                phase,
                duration_ms: start.elapsed().as_millis() as u64,
                peak_memory_bytes,
                sample_count: cpu_samples.len() as u32,
                avg_cpu_percent,
                peak_cpu_percent,
            },
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enable_then_is_enabled_reports_true_only_for_that_session() {
        let registry = ProfileRegistry::new();
        assert!(!registry.is_enabled("s1").await);

        registry.enable("s1").await;

        assert!(registry.is_enabled("s1").await);
        assert!(!registry.is_enabled("s2").await, "enabling s1 must not affect s2");
    }

    #[tokio::test]
    async fn enable_creates_an_empty_profile_immediately() {
        let registry = ProfileRegistry::new();
        registry.enable("s1").await;

        let profile = registry.get("s1").await.expect("enable must create a profile entry");
        assert_eq!(profile.session_id, "s1");
        assert!(profile.phases.is_empty());
    }

    #[tokio::test]
    async fn get_returns_none_for_a_session_never_enabled() {
        let registry = ProfileRegistry::new();
        assert!(registry.get("never-enabled").await.is_none());
    }

    #[tokio::test]
    async fn enabling_twice_does_not_clear_previously_recorded_phases() {
        let registry = ProfileRegistry::new();
        registry.enable("s1").await;
        registry
            .record_phase(
                "s1",
                PhaseProfile {
                    phase: "merge_shares".to_string(),
                    duration_ms: 10,
                    peak_memory_bytes: 1024,
                    sample_count: 1,
                    avg_cpu_percent: 5.0,
                    peak_cpu_percent: 5.0,
                },
            )
            .await;

        // Re-enabling (e.g. a second POST /session/:id/profile) must not
        // wipe out the phase already recorded.
        registry.enable("s1").await;

        let profile = registry.get("s1").await.unwrap();
        assert_eq!(profile.phases.len(), 1);
        assert_eq!(profile.phases[0].phase, "merge_shares");
    }

    #[tokio::test]
    async fn record_phase_appends_rather_than_overwrites_across_phases() {
        let registry = ProfileRegistry::new();
        registry.enable("s1").await;

        for phase in ["merge_shares", "witness_generation", "proof_generation"] {
            registry
                .record_phase(
                    "s1",
                    PhaseProfile {
                        phase: phase.to_string(),
                        duration_ms: 1,
                        peak_memory_bytes: 0,
                        sample_count: 0,
                        avg_cpu_percent: 0.0,
                        peak_cpu_percent: 0.0,
                    },
                )
                .await;
        }

        let profile = registry.get("s1").await.unwrap();
        let phases: Vec<&str> = profile.phases.iter().map(|p| p.phase.as_str()).collect();
        assert_eq!(phases, vec!["merge_shares", "witness_generation", "proof_generation"]);
    }

    // Spawns a real short-lived child process and confirms the sampler
    // records a phase for it with a non-zero duration once it exits.
    // Unix-only: this whole service stack (co-noir, Docker deployment)
    // targets Linux, so `sh` is a safe assumption for CI here the same way
    // it would be for the production co-noir subprocess calls.
    #[cfg(unix)]
    #[tokio::test]
    async fn sample_process_until_exit_records_a_phase_for_a_real_short_lived_process() {
        let registry = ProfileRegistry::new();
        registry.enable("s1").await;

        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "sleep 0.3"])
            .spawn()
            .expect("failed to spawn sh");
        let pid = child.id().expect("child must have a pid");

        let sampler = tokio::spawn(sample_process_until_exit(
            registry.clone(),
            "s1".to_string(),
            "test_phase".to_string(),
            pid,
        ));

        child.wait().await.expect("child process failed");
        sampler.await.expect("sampler task panicked");

        let profile = registry.get("s1").await.unwrap();
        assert_eq!(profile.phases.len(), 1);
        assert_eq!(profile.phases[0].phase, "test_phase");
        // duration is measured in whole SAMPLE_INTERVAL ticks; a ~300ms
        // sleep should take at least one full interval.
        assert!(profile.phases[0].duration_ms >= SAMPLE_INTERVAL.as_millis() as u64);
    }
}
