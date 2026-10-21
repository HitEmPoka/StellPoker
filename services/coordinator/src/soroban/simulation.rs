//! Pre-submission transaction simulation (Issue #137).
//!
//! Every state-changing `stellar contract invoke` issued by the coordinator
//! is first run with `--send no`, which only calls the RPC's
//! `simulateTransaction`. The real submission happens only if the
//! simulation succeeds, so a call the contract would reject never reaches the
//! network (and never burns a fee or a sequence number).
//!
//! - Failed simulations are logged with the contract function and the
//!   simulation error, and returned to the caller exactly as a failed invoke
//!   would be.
//! - Results are cached for a short TTL keyed on the full invocation, so
//!   the retry loops in `soroban/mod.rs` don't re-simulate the same call.
//!   Transient failures (timeouts, resets, `ResourceLimitExceeded`) are never
//!   cached so a retry really retries.
//! - Read-only calls (`get_*`, `is_*`, `has_*`) are never submitted by the CLI
//!   anyway, so they skip the extra round trip.
//!
//! Configuration:
//! - `SOROBAN_PRESIMULATE=false` disables the pre-simulation step.
//! - `SOROBAN_SIMULATION_CACHE_TTL_SECS` (default 10, `0` disables caching).

use std::collections::HashMap;
use std::future::Future;
use std::process::Output;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::process::Command;

use super::is_transient_invoke_error;

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(10);
const MAX_CACHE_ENTRIES: usize = 1024;

pub(crate) type SimulationKey = [u8; 32];

/// Short-lived cache of simulation results keyed on the full invocation.
pub(crate) struct SimulationCache {
    ttl: Duration,
    entries: Mutex<HashMap<SimulationKey, (Instant, Output)>>,
}

impl SimulationCache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn get(&self, key: &SimulationKey, now: Instant) -> Option<Output> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        match entries.get(key) {
            Some((at, output)) if now.duration_since(*at) < self.ttl => Some(output.clone()),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    pub(crate) fn insert(&self, key: SimulationKey, output: Output, now: Instant) {
        if self.ttl.is_zero() {
            return;
        }
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() >= MAX_CACHE_ENTRIES {
            let ttl = self.ttl;
            entries.retain(|_, (at, _)| now.duration_since(*at) < ttl);
            if entries.len() >= MAX_CACHE_ENTRIES {
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(k, _)| *k)
                {
                    entries.remove(&oldest);
                }
            }
        }
        entries.insert(key, (now, output));
    }
}

static SIMULATION_CACHE: LazyLock<SimulationCache> = LazyLock::new(|| {
    let ttl = std::env::var("SOROBAN_SIMULATION_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_CACHE_TTL);
    SimulationCache::new(ttl)
});

fn presimulation_enabled() -> bool {
    !matches!(
        std::env::var("SOROBAN_PRESIMULATE")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// Contract function name: the first argument after `--`.
pub(crate) fn contract_function(args: &[String]) -> &str {
    args.iter()
        .position(|a| a == "--")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("")
}

pub(crate) fn is_read_only_function(function: &str) -> bool {
    ["get_", "is_", "has_"]
        .iter()
        .any(|prefix| function.starts_with(prefix))
}

/// The same invocation with `--send no` inserted before the contract
/// arguments, so the CLI only simulates.
pub(crate) fn simulation_args(args: &[String]) -> Vec<String> {
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let mut out = Vec::with_capacity(args.len() + 2);
    out.extend_from_slice(&args[..split]);
    out.push("--send".to_string());
    out.push("no".to_string());
    out.extend_from_slice(&args[split..]);
    out
}

/// Cache key: SHA-256 over every argument. The source secret is part of the
/// arguments, but only its digest is kept.
pub(crate) fn simulation_key(args: &[String]) -> SimulationKey {
    let mut hasher = Sha256::new();
    for arg in args {
        hasher.update((arg.len() as u64).to_be_bytes());
        hasher.update(arg.as_bytes());
    }
    hasher.finalize().into()
}

async fn run_stellar(args: &[String]) -> Result<Output, String> {
    Command::new("stellar")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to invoke stellar CLI: {}", e))
}

/// Core gate: simulate (or reuse a cached simulation), and submit only if the
/// simulation succeeded. On a failed simulation the failed output is returned
/// without submitting.
pub(crate) async fn simulate_then_submit<Sim, SimFut, Sub, SubFut>(
    cache: &SimulationCache,
    key: SimulationKey,
    function: &str,
    simulate: Sim,
    submit: Sub,
) -> Result<Output, String>
where
    Sim: FnOnce() -> SimFut,
    SimFut: Future<Output = Result<Output, String>>,
    Sub: FnOnce() -> SubFut,
    SubFut: Future<Output = Result<Output, String>>,
{
    let simulation = match cache.get(&key, Instant::now()) {
        Some(cached) => {
            tracing::debug!(function, "soroban simulation cache hit");
            cached
        }
        None => {
            let output = simulate().await?;
            if output.status.success() || !is_transient_invoke_error(&output) {
                cache.insert(key, output.clone(), Instant::now());
            }
            output
        }
    };

    if simulation.status.success() {
        return submit().await;
    }

    tracing::warn!(
        function,
        error = %String::from_utf8_lossy(&simulation.stderr).trim(),
        "soroban simulation failed; transaction not submitted"
    );
    Ok(simulation)
}

/// Run a full `stellar contract invoke ...` argument list, pre-simulating
/// state-changing calls. Drop-in replacement for running the CLI directly.
pub(crate) async fn run_invoke(args: Vec<String>) -> Result<Output, String> {
    let function = contract_function(&args).to_string();
    if !presimulation_enabled() || is_read_only_function(&function) {
        return run_stellar(&args).await;
    }

    let sim_args = simulation_args(&args);
    let key = simulation_key(&args);
    simulate_then_submit(
        &SIMULATION_CACHE,
        key,
        &function,
        || async move { run_stellar(&sim_args).await },
        || async move { run_stellar(&args).await },
    )
    .await
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn output(code: i32, stderr: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: b"sim".to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn args(function: &str) -> Vec<String> {
        [
            "contract",
            "invoke",
            "--id",
            "CABC",
            "--source",
            "S123",
            "--",
            function,
            "--table_id",
            "1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    struct Calls {
        sims: AtomicUsize,
        submits: AtomicUsize,
    }

    impl Calls {
        fn new() -> Self {
            Self {
                sims: AtomicUsize::new(0),
                submits: AtomicUsize::new(0),
            }
        }
    }

    async fn gate(cache: &SimulationCache, calls: &Calls, sim: Output) -> Output {
        simulate_then_submit(
            cache,
            simulation_key(&args("player_action")),
            "player_action",
            || async {
                calls.sims.fetch_add(1, Ordering::SeqCst);
                Ok(sim)
            },
            || async {
                calls.submits.fetch_add(1, Ordering::SeqCst);
                Ok(output(0, ""))
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn submits_only_after_successful_simulation() {
        let cache = SimulationCache::new(DEFAULT_CACHE_TTL);
        let calls = Calls::new();
        let out = gate(&cache, &calls, output(0, "")).await;
        assert!(out.status.success());
        assert_eq!(calls.sims.load(Ordering::SeqCst), 1);
        assert_eq!(calls.submits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_simulation_is_not_submitted() {
        let cache = SimulationCache::new(DEFAULT_CACHE_TTL);
        let calls = Calls::new();
        let out = gate(&cache, &calls, output(1, "HostError: Error(Contract, #5)")).await;
        assert!(!out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "HostError: Error(Contract, #5)"
        );
        assert_eq!(calls.submits.load(Ordering::SeqCst), 0);
        // The failure feeds the existing error path.
        assert!(super::super::parse_tx_result(out).is_err());
    }

    #[tokio::test]
    async fn cached_results_skip_resimulation() {
        let cache = SimulationCache::new(DEFAULT_CACHE_TTL);
        let calls = Calls::new();
        gate(&cache, &calls, output(0, "")).await;
        gate(&cache, &calls, output(0, "")).await;
        assert_eq!(calls.sims.load(Ordering::SeqCst), 1);
        assert_eq!(calls.submits.load(Ordering::SeqCst), 2);

        // A cached deterministic failure also short-circuits.
        let failing = SimulationCache::new(DEFAULT_CACHE_TTL);
        let calls = Calls::new();
        gate(&failing, &calls, output(1, "Error(Contract, #3)")).await;
        gate(&failing, &calls, output(0, "")).await;
        assert_eq!(calls.sims.load(Ordering::SeqCst), 1);
        assert_eq!(calls.submits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn transient_failures_are_not_cached() {
        let cache = SimulationCache::new(DEFAULT_CACHE_TTL);
        let calls = Calls::new();
        gate(&cache, &calls, output(1, "request timeout after 30s")).await;
        gate(&cache, &calls, output(0, "")).await;
        assert_eq!(calls.sims.load(Ordering::SeqCst), 2);
        assert_eq!(calls.submits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_entries_expire() {
        let cache = SimulationCache::new(Duration::from_secs(5));
        let key = simulation_key(&args("player_action"));
        let t0 = Instant::now();
        cache.insert(key, output(0, ""), t0);
        assert!(cache.get(&key, t0 + Duration::from_secs(4)).is_some());
        assert!(cache.get(&key, t0 + Duration::from_secs(5)).is_none());

        let disabled = SimulationCache::new(Duration::ZERO);
        disabled.insert(key, output(0, ""), t0);
        assert!(disabled.get(&key, t0).is_none());
    }

    #[test]
    fn cache_is_bounded() {
        let cache = SimulationCache::new(Duration::from_secs(60));
        let t0 = Instant::now();
        for i in 0..(MAX_CACHE_ENTRIES + 10) {
            cache.insert(
                simulation_key(&[i.to_string()]),
                output(0, ""),
                t0 + Duration::from_millis(i as u64),
            );
        }
        assert_eq!(cache.entries.lock().unwrap().len(), MAX_CACHE_ENTRIES);
    }

    #[test]
    fn simulation_args_insert_send_no_before_contract_args() {
        let sim = simulation_args(&args("player_action"));
        let dashdash = sim.iter().position(|a| a == "--").unwrap();
        assert_eq!(&sim[dashdash - 2..dashdash], ["--send", "no"]);
        assert_eq!(sim[dashdash + 1], "player_action");
    }

    #[test]
    fn keys_distinguish_invocations() {
        assert_eq!(simulation_key(&args("a")), simulation_key(&args("a")));
        assert_ne!(simulation_key(&args("a")), simulation_key(&args("b")));
        // Length-prefixing prevents ["ab","c"] colliding with ["a","bc"].
        assert_ne!(
            simulation_key(&["ab".into(), "c".into()]),
            simulation_key(&["a".into(), "bc".into()])
        );
    }

    #[test]
    fn read_only_calls_are_recognised() {
        assert_eq!(contract_function(&args("get_table")), "get_table");
        assert!(is_read_only_function("get_table"));
        assert!(is_read_only_function("is_committee_member"));
        assert!(!is_read_only_function("player_action"));
        assert_eq!(contract_function(&["contract".to_string()]), "");
    }
}
