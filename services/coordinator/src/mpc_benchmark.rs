//! MPC performance benchmarking for deal phase.
//!
//! Tracks latency breakdown by operation phase:
//! - Secret sharing: time to distribute shares across nodes
//! - Shuffling: time for deck shuffling via MPC
//! - Commitment: time to compute hand commitments
//! - Proving: time to generate ZK proofs
//!
//! Supports testing with 2-6 players.
//!
//! Each benchmark also carries a peak resident-memory figure (#886). Six-player
//! showdowns build the largest witness, so a regression there shows up as an
//! out-of-memory kill on modest nodes before it shows up as latency. Set
//! `MPC_BENCH_MAX_RSS_MIB` to turn the figure into a pass/fail budget with
//! [`check_peak_memory`].

// The coordinator binary only reads the benchmark store; sessions, the budget
// check and the memory helpers are driven by the benchmark harness and tests.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

/// Environment variable holding the peak-RSS budget, in MiB, enforced by
/// [`budget_kib_from_env`] / [`check_peak_memory`]. Unset means no budget.
pub const MAX_RSS_ENV: &str = "MPC_BENCH_MAX_RSS_MIB";

/// Parses the `VmHWM` ("peak resident set size") line out of the contents of
/// `/proc/<pid>/status`, in KiB.
pub fn parse_vm_hwm_kib(status: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmHWM:")?;
        rest.trim().strip_suffix("kB")?.trim().parse().ok()
    })
}

/// Peak resident set size of this process so far, in KiB.
///
/// This is the kernel's high-water mark (`VmHWM`), so it only ever grows: it
/// answers "how large did this process get", not "how large is it now". It is
/// `None` where `/proc` is unavailable (non-Linux).
pub fn peak_rss_kib() -> Option<u64> {
    parse_vm_hwm_kib(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// Parses a MiB budget such as `"4096"` into KiB. Empty, zero or unparseable
/// values mean "no budget" rather than a budget of nothing.
pub fn parse_budget_mib(raw: Option<&str>) -> Option<u64> {
    let mib: u64 = raw?.trim().parse().ok()?;
    if mib == 0 {
        None
    } else {
        Some(mib.saturating_mul(1024))
    }
}

/// The budget configured through [`MAX_RSS_ENV`], in KiB.
pub fn budget_kib_from_env() -> Option<u64> {
    parse_budget_mib(std::env::var(MAX_RSS_ENV).ok().as_deref())
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PhaseTiming {
    pub phase: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DealBenchmark {
    pub table_id: u32,
    pub player_count: usize,
    pub total_duration_ms: u64,
    pub phase_timings: Vec<PhaseTiming>,
    /// Highest resident memory seen for this deal, in KiB (#886): the larger of
    /// this process's own high-water mark and anything folded in with
    /// [`BenchmarkSession::record_peak_rss_kib`]. `None` when it could not be
    /// measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss_kib: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct BenchmarkSession {
    pub table_id: u32,
    pub player_count: usize,
    pub start_time: Instant,
    pub phase_starts: HashMap<String, Instant>,
    pub phase_durations: HashMap<String, u64>,
    /// Peak memory reported by other processes working on this deal, in KiB.
    pub reported_peak_rss_kib: Option<u64>,
}

impl BenchmarkSession {
    pub fn new(table_id: u32, player_count: usize) -> Self {
        BenchmarkSession {
            table_id,
            player_count,
            start_time: Instant::now(),
            phase_starts: HashMap::new(),
            phase_durations: HashMap::new(),
            reported_peak_rss_kib: None,
        }
    }

    /// Fold in a peak-memory figure measured elsewhere, in KiB.
    ///
    /// Witness generation and proving run in `co-noir` child processes on the
    /// MPC nodes, not in the coordinator, so the coordinator's own high-water
    /// mark does not see them. The node's per-phase profile
    /// (`PhaseProfile::peak_memory_bytes`) is the figure to pass here. Only the
    /// largest value reported is kept.
    pub fn record_peak_rss_kib(&mut self, kib: u64) {
        self.reported_peak_rss_kib = Some(self.reported_peak_rss_kib.map_or(kib, |c| c.max(kib)));
    }

    pub fn start_phase(&mut self, phase_name: &str) {
        self.phase_starts
            .insert(phase_name.to_string(), Instant::now());
    }

    pub fn end_phase(&mut self, phase_name: &str) {
        if let Some(start) = self.phase_starts.remove(phase_name) {
            let duration_ms = start.elapsed().as_millis() as u64;
            self.phase_durations
                .insert(phase_name.to_string(), duration_ms);
        }
    }

    pub fn finalize(self) -> DealBenchmark {
        let total_duration_ms = self.start_time.elapsed().as_millis() as u64;
        let mut phase_timings: Vec<PhaseTiming> = self
            .phase_durations
            .into_iter()
            .map(|(phase, duration_ms)| PhaseTiming { phase, duration_ms })
            .collect();

        phase_timings.sort_by(|a, b| a.phase.cmp(&b.phase));

        DealBenchmark {
            table_id: self.table_id,
            player_count: self.player_count,
            total_duration_ms,
            phase_timings,
            peak_rss_kib: match (peak_rss_kib(), self.reported_peak_rss_kib) {
                (Some(own), Some(reported)) => Some(own.max(reported)),
                (own, reported) => own.or(reported),
            },
        }
    }
}

/// A deal whose peak memory went over its budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeakMemoryExceeded {
    pub table_id: u32,
    pub player_count: usize,
    pub peak_rss_kib: u64,
    pub budget_kib: u64,
}

impl fmt::Display for PeakMemoryExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "peak memory {} MiB exceeds the {} MiB budget for the {}-player deal at table {}",
            self.peak_rss_kib / 1024,
            self.budget_kib / 1024,
            self.player_count,
            self.table_id
        )
    }
}

impl std::error::Error for PeakMemoryExceeded {}

/// Checks a finished benchmark against a peak-RSS budget in KiB.
///
/// A benchmark whose memory could not be measured passes: the check must not
/// fail on platforms that cannot report it. Use [`assert_peak_memory`] in
/// benchmark runs that must fail loudly.
pub fn check_peak_memory(
    benchmark: &DealBenchmark,
    budget_kib: u64,
) -> Result<(), PeakMemoryExceeded> {
    match benchmark.peak_rss_kib {
        Some(peak) if peak > budget_kib => Err(PeakMemoryExceeded {
            table_id: benchmark.table_id,
            player_count: benchmark.player_count,
            peak_rss_kib: peak,
            budget_kib,
        }),
        _ => Ok(()),
    }
}

/// Panics if `benchmark` went over `budget_kib`, for use as the peak-memory
/// assertion in the benchmark suite.
pub fn assert_peak_memory(benchmark: &DealBenchmark, budget_kib: u64) {
    if let Err(exceeded) = check_peak_memory(benchmark, budget_kib) {
        panic!("{exceeded}");
    }
}

use std::sync::{Arc, Mutex};

pub type BenchmarkStore = Arc<Mutex<Vec<DealBenchmark>>>;

pub fn new_store() -> BenchmarkStore {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn record_benchmark(store: &BenchmarkStore, benchmark: DealBenchmark) {
    if let Ok(mut benchmarks) = store.lock() {
        benchmarks.push(benchmark);
        if benchmarks.len() > 1000 {
            benchmarks.remove(0);
        }
    }
}

pub fn get_benchmarks(store: &BenchmarkStore, table_id: Option<u32>) -> Vec<DealBenchmark> {
    if let Ok(benchmarks) = store.lock() {
        match table_id {
            Some(id) => benchmarks
                .iter()
                .filter(|b| b.table_id == id)
                .cloned()
                .collect(),
            None => benchmarks.clone(),
        }
    } else {
        Vec::new()
    }
}

pub fn get_benchmark_stats(benchmarks: &[DealBenchmark]) -> serde_json::Value {
    if benchmarks.is_empty() {
        return serde_json::json!({ "count": 0 });
    }

    let mut by_player_count: HashMap<usize, Vec<u64>> = HashMap::new();
    let mut peak_rss_by_player_count: HashMap<usize, u64> = HashMap::new();
    for benchmark in benchmarks {
        by_player_count
            .entry(benchmark.player_count)
            .or_default()
            .push(benchmark.total_duration_ms);
        if let Some(peak) = benchmark.peak_rss_kib {
            let highest = peak_rss_by_player_count
                .entry(benchmark.player_count)
                .or_insert(0);
            *highest = (*highest).max(peak);
        }
    }

    let mut stats = serde_json::Map::new();
    stats.insert(
        "total_samples".to_string(),
        serde_json::json!(benchmarks.len()),
    );

    for (player_count, durations) in by_player_count {
        let avg = durations.iter().sum::<u64>() / durations.len() as u64;
        let min = durations.iter().min().copied().unwrap_or(0);
        let max = durations.iter().max().copied().unwrap_or(0);

        stats.insert(
            format!("players_{}_count", player_count),
            serde_json::json!(durations.len()),
        );
        stats.insert(
            format!("players_{}_avg_ms", player_count),
            serde_json::json!(avg),
        );
        stats.insert(
            format!("players_{}_min_ms", player_count),
            serde_json::json!(min),
        );
        stats.insert(
            format!("players_{}_max_ms", player_count),
            serde_json::json!(max),
        );
        if let Some(peak) = peak_rss_by_player_count.get(&player_count) {
            stats.insert(
                format!("players_{}_peak_rss_kib", player_count),
                serde_json::json!(peak),
            );
        }
    }

    serde_json::Value::Object(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn benchmark(table_id: u32, player_count: usize, peak: Option<u64>) -> DealBenchmark {
        DealBenchmark {
            table_id,
            player_count,
            total_duration_ms: 10,
            phase_timings: vec![],
            peak_rss_kib: peak,
        }
    }

    #[test]
    fn parses_vm_hwm_from_proc_status() {
        let status = "Name:\tcoordinator\nVmPeak:\t  900000 kB\nVmHWM:\t  123456 kB\nVmRSS:\t  100000 kB\n";
        assert_eq!(parse_vm_hwm_kib(status), Some(123_456));
    }

    #[test]
    fn vm_hwm_is_none_when_absent_or_malformed() {
        assert_eq!(parse_vm_hwm_kib("Name:\tx\nVmRSS:\t5 kB\n"), None);
        assert_eq!(parse_vm_hwm_kib("VmHWM:\tlots kB\n"), None);
        assert_eq!(parse_vm_hwm_kib("VmHWM:\t12\n"), None);
        assert_eq!(parse_vm_hwm_kib(""), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn this_process_reports_a_peak_rss_on_linux() {
        let peak = peak_rss_kib().expect("VmHWM is available on Linux");
        assert!(peak > 0);
    }

    #[test]
    fn budget_is_read_in_mib_and_converted_to_kib() {
        assert_eq!(parse_budget_mib(Some("4096")), Some(4096 * 1024));
        assert_eq!(parse_budget_mib(Some(" 512 ")), Some(512 * 1024));
    }

    #[test]
    fn a_missing_zero_or_invalid_budget_means_no_budget() {
        assert_eq!(parse_budget_mib(None), None);
        assert_eq!(parse_budget_mib(Some("")), None);
        assert_eq!(parse_budget_mib(Some("0")), None);
        assert_eq!(parse_budget_mib(Some("4 GiB")), None);
        assert_eq!(parse_budget_mib(Some("-1")), None);
    }

    #[test]
    fn a_finished_session_carries_a_peak_memory_figure() {
        let mut session = BenchmarkSession::new(1, 6);
        session.start_phase("proving");
        session.end_phase("proving");
        let benchmark = session.finalize();
        if cfg!(target_os = "linux") {
            assert!(benchmark.peak_rss_kib.is_some_and(|kib| kib > 0));
        }
    }

    #[test]
    fn reported_peaks_keep_the_largest_value() {
        let mut session = BenchmarkSession::new(1, 6);
        session.record_peak_rss_kib(5_000);
        session.record_peak_rss_kib(9_000);
        session.record_peak_rss_kib(7_000);
        assert_eq!(session.reported_peak_rss_kib, Some(9_000));
    }

    #[test]
    fn a_reported_peak_above_this_process_wins() {
        let mut session = BenchmarkSession::new(1, 6);
        let huge = u64::MAX / 2;
        session.record_peak_rss_kib(huge);
        assert_eq!(session.finalize().peak_rss_kib, Some(huge));
    }

    #[test]
    fn a_peak_within_budget_passes() {
        let b = benchmark(1, 6, Some(1_000));
        assert_eq!(check_peak_memory(&b, 1_000), Ok(()));
        assert_eq!(check_peak_memory(&b, 2_000), Ok(()));
    }

    #[test]
    fn a_peak_over_budget_is_reported_with_the_deal_it_came_from() {
        let b = benchmark(7, 6, Some(4_096 * 1024 + 1));
        let err = check_peak_memory(&b, 4_096 * 1024).unwrap_err();
        assert_eq!(err.table_id, 7);
        assert_eq!(err.player_count, 6);
        assert_eq!(err.peak_rss_kib, 4_096 * 1024 + 1);
        assert!(err.to_string().contains("6-player"), "{err}");
        assert!(err.to_string().contains("table 7"), "{err}");
    }

    #[test]
    fn an_unmeasured_peak_does_not_fail_the_budget() {
        assert_eq!(check_peak_memory(&benchmark(1, 6, None), 1), Ok(()));
    }

    #[test]
    #[should_panic(expected = "exceeds the")]
    fn assert_peak_memory_panics_over_budget() {
        assert_peak_memory(&benchmark(1, 6, Some(9 * 1024)), 8 * 1024);
    }

    #[test]
    fn assert_peak_memory_is_silent_within_budget() {
        assert_peak_memory(&benchmark(1, 6, Some(8 * 1024)), 8 * 1024);
    }

    #[test]
    fn stats_report_the_highest_peak_per_player_count() {
        let stats = get_benchmark_stats(&[
            benchmark(1, 2, Some(100)),
            benchmark(2, 6, Some(900)),
            benchmark(3, 6, Some(1_200)),
            benchmark(4, 6, None),
        ]);
        assert_eq!(stats["players_2_peak_rss_kib"], 100);
        assert_eq!(stats["players_6_peak_rss_kib"], 1_200);
    }

    #[test]
    fn stats_omit_the_peak_when_nothing_was_measured() {
        let stats = get_benchmark_stats(&[benchmark(1, 3, None)]);
        assert!(stats.get("players_3_peak_rss_kib").is_none());
        assert_eq!(stats["players_3_count"], 1);
    }

    #[test]
    fn the_peak_is_left_out_of_the_json_when_unmeasured() {
        let measured = serde_json::to_value(benchmark(1, 3, Some(42))).unwrap();
        let unmeasured = serde_json::to_value(benchmark(1, 3, None)).unwrap();
        assert_eq!(measured["peak_rss_kib"], 42);
        assert!(unmeasured.get("peak_rss_kib").is_none());
    }
}
