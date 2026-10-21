//! Bandwidth estimation, adaptive coNoir protocol selection (Issue #238), and
//! bandwidth-aware session admission control (Issue #589).
//!
//! Every coordinator -> MPC node call already measures round-trip latency
//! (see `node_reliability`). This module additionally tracks payload size per
//! call to estimate the effective throughput to each node, and uses that
//! estimate to pick which coNoir protocol variant a node should run for its
//! next share-preparation call:
//!   - `HighBandwidth`: more round trips / less local computation — faster
//!     when the link to that node is fast.
//!   - `LowBandwidth`: fewer, larger messages / more local computation —
//!     better when bandwidth is the bottleneck.
//!
//! Bandwidth is estimated with an exponential moving average (EMA) of
//! bytes/sec samples per node endpoint. This is process-local, in-memory
//! state; it resets on restart and is not shared across coordinator
//! instances, same as `node_reliability`.
//!
//! ## Admission control (Issue #589)
//!
//! Before admitting a new MPC session the coordinator checks whether the
//! estimated aggregate bandwidth to all required committee nodes meets a
//! configurable minimum threshold.  If any node is below
//! `MPC_ADMISSION_MIN_BPS` (default 512 Kbps) the session is rejected with
//! [`AdmissionDecision::Reject`] so the client can retry later rather than
//! starting a session that will stall or time out.
//!
//! Config env vars:
//! - `MPC_LOW_BANDWIDTH_THRESHOLD_BPS` — threshold below which a node is
//!   considered slow (default 1 Mbps).  Selects `LowBandwidth` protocol.
//! - `MPC_ADMISSION_MIN_BPS` — absolute floor for admission (default 512 Kbps).
//!   Sessions are rejected when *any* required node is below this.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::RwLock;

const DEFAULT_LOW_BANDWIDTH_THRESHOLD_BPS: f64 = 1_000_000.0; // 1 Mbps
const DEFAULT_ADMISSION_MIN_BPS: f64 = 512_000.0; // 512 Kbps
const EMA_ALPHA: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolVariant {
    /// Fewer/larger round trips, more local computation — for constrained links.
    LowBandwidth,
    /// More round trips, less local computation per message — for fast links.
    HighBandwidth,
}

impl ProtocolVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProtocolVariant::HighBandwidth => "high_bandwidth",
            ProtocolVariant::LowBandwidth => "low_bandwidth",
        }
    }
}

#[derive(Default)]
struct Entry {
    estimated_bps: f64,
    samples: u64,
}

type BandwidthStore = RwLock<HashMap<String, Entry>>;

static BANDWIDTH: OnceLock<BandwidthStore> = OnceLock::new();

fn store() -> &'static BandwidthStore {
    BANDWIDTH.get_or_init(|| RwLock::new(HashMap::new()))
}

fn low_bandwidth_threshold_bps() -> f64 {
    std::env::var("MPC_LOW_BANDWIDTH_THRESHOLD_BPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_LOW_BANDWIDTH_THRESHOLD_BPS)
}

fn admission_min_bps() -> f64 {
    std::env::var("MPC_ADMISSION_MIN_BPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ADMISSION_MIN_BPS)
}

/// Record one payload transfer's size and elapsed time toward `endpoint`'s
/// bandwidth estimate. Zero-duration samples are ignored (can't derive a
/// rate from them).
pub async fn record_sample(endpoint: &str, bytes: usize, elapsed: Duration) {
    if elapsed.is_zero() {
        return;
    }
    let sample_bps = bytes as f64 * 8.0 / elapsed.as_secs_f64();

    let mut map = store().write().await;
    let entry = map.entry(endpoint.to_string()).or_default();
    entry.estimated_bps = if entry.samples == 0 {
        sample_bps
    } else {
        EMA_ALPHA * sample_bps + (1.0 - EMA_ALPHA) * entry.estimated_bps
    };
    entry.samples += 1;
}

/// Current estimated bandwidth (bits/sec) to `endpoint`, or `None` if no
/// samples have been recorded yet.
pub async fn estimated_bandwidth_bps(endpoint: &str) -> Option<f64> {
    store().read().await.get(endpoint).map(|e| e.estimated_bps)
}

/// Select the coNoir protocol variant to request from `endpoint` based on its
/// current bandwidth estimate. Defaults to `HighBandwidth` (today's fixed
/// behaviour) until enough samples exist to show the link is constrained.
pub async fn select_protocol_variant(endpoint: &str) -> ProtocolVariant {
    match estimated_bandwidth_bps(endpoint).await {
        Some(bps) if bps < low_bandwidth_threshold_bps() => ProtocolVariant::LowBandwidth,
        _ => ProtocolVariant::HighBandwidth,
    }
}

// ---------------------------------------------------------------------------
// Session admission control (Issue #589)
// ---------------------------------------------------------------------------

/// Result of a bandwidth-based admission check for a new MPC session.
#[derive(Clone, Debug, PartialEq)]
pub enum AdmissionDecision {
    /// All required nodes have sufficient estimated bandwidth — admit the session.
    Admit,
    /// One or more nodes are below the admission threshold — reject the session.
    /// `reason` contains a human-readable explanation and the offending endpoint.
    Reject { reason: String },
    /// No bandwidth samples exist yet for one or more nodes — admit speculatively
    /// (can't make an informed decision without data).
    AdmitSpeculative,
}

/// Per-node bandwidth status snapshot, useful for metrics and admin endpoints.
#[derive(Clone, Debug)]
pub struct NodeBandwidthStatus {
    pub endpoint: String,
    /// `None` if no samples have been recorded yet.
    pub estimated_bps: Option<f64>,
    pub meets_admission_threshold: bool,
}

/// Check whether all `node_endpoints` meet the minimum admission bandwidth.
///
/// Returns [`AdmissionDecision::Admit`] when all nodes have samples above the
/// threshold, [`AdmissionDecision::AdmitSpeculative`] when any node lacks data,
/// and [`AdmissionDecision::Reject`] when any node is measurably below the
/// floor.
pub async fn check_admission(node_endpoints: &[String]) -> AdmissionDecision {
    let min = admission_min_bps();
    let mut any_speculative = false;

    for endpoint in node_endpoints {
        match estimated_bandwidth_bps(endpoint).await {
            None => {
                // No data — can't reject, but flag as speculative.
                any_speculative = true;
            }
            Some(bps) if bps < min => {
                return AdmissionDecision::Reject {
                    reason: format!(
                        "node {} estimated bandwidth {:.0} bps is below admission threshold {:.0} bps",
                        endpoint, bps, min
                    ),
                };
            }
            Some(_) => {}
        }
    }

    if any_speculative {
        AdmissionDecision::AdmitSpeculative
    } else {
        AdmissionDecision::Admit
    }
}

/// Snapshot bandwidth status for all `node_endpoints`. Used by admin/metrics
/// endpoints to expose per-node bandwidth metrics.
pub async fn node_bandwidth_statuses(node_endpoints: &[String]) -> Vec<NodeBandwidthStatus> {
    let min = admission_min_bps();
    let mut statuses = Vec::with_capacity(node_endpoints.len());
    for endpoint in node_endpoints {
        let estimated_bps = estimated_bandwidth_bps(endpoint).await;
        statuses.push(NodeBandwidthStatus {
            endpoint: endpoint.clone(),
            estimated_bps,
            meets_admission_threshold: estimated_bps.map_or(true, |bps| bps >= min),
        });
    }
    statuses
}

#[cfg(test)]
mod tests {
    //! Each test uses a unique endpoint string since the bandwidth store is a
    //! process-wide static shared across parallel test threads.
    use super::*;

    #[tokio::test]
    async fn unknown_endpoint_has_no_estimate() {
        assert!(estimated_bandwidth_bps("http://unknown-bandwidth-test")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn unknown_endpoint_defaults_to_high_bandwidth_variant() {
        let variant = select_protocol_variant("http://unknown-bandwidth-test-2").await;
        assert_eq!(variant, ProtocolVariant::HighBandwidth);
    }

    #[tokio::test]
    async fn fast_transfer_yields_high_bandwidth_variant() {
        let endpoint = "http://fast-bandwidth-test";
        // 10 MB in 10ms => far above the default 1 Mbps threshold.
        record_sample(endpoint, 10_000_000, Duration::from_millis(10)).await;
        assert_eq!(
            select_protocol_variant(endpoint).await,
            ProtocolVariant::HighBandwidth
        );
    }

    #[tokio::test]
    async fn slow_transfer_yields_low_bandwidth_variant() {
        let endpoint = "http://slow-bandwidth-test";
        // 1000 bytes in 1s => 8000 bps, far below the default 1 Mbps threshold.
        record_sample(endpoint, 1000, Duration::from_secs(1)).await;
        assert_eq!(
            select_protocol_variant(endpoint).await,
            ProtocolVariant::LowBandwidth
        );
    }

    #[tokio::test]
    async fn ema_smooths_across_samples() {
        let endpoint = "http://ema-bandwidth-test";
        record_sample(endpoint, 1000, Duration::from_secs(1)).await; // ~8000 bps
        let after_one = estimated_bandwidth_bps(endpoint).await.unwrap();
        record_sample(endpoint, 10_000_000, Duration::from_millis(10)).await; // very high bps
        let after_two = estimated_bandwidth_bps(endpoint).await.unwrap();
        assert!(
            after_two > after_one,
            "EMA should move toward the new, faster sample"
        );
        assert!(
            after_two < 10_000_000.0 * 8.0,
            "EMA should not jump straight to the new sample's raw value"
        );
    }

    #[tokio::test]
    async fn zero_elapsed_sample_is_ignored() {
        let endpoint = "http://zero-elapsed-bandwidth-test";
        record_sample(endpoint, 1000, Duration::from_secs(0)).await;
        assert!(estimated_bandwidth_bps(endpoint).await.is_none());
    }

    // -----------------------------------------------------------------------
    // Admission control tests (Issue #589)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn admission_speculative_when_no_samples() {
        let endpoints = vec![
            "http://admission-speculative-1".to_string(),
            "http://admission-speculative-2".to_string(),
        ];
        let decision = check_admission(&endpoints).await;
        assert_eq!(decision, AdmissionDecision::AdmitSpeculative);
    }

    #[tokio::test]
    async fn admission_admit_when_all_above_threshold() {
        let ep1 = "http://admission-fast-1";
        let ep2 = "http://admission-fast-2";
        // 10 MB in 1s = 80 Mbps >> 512 Kbps floor
        record_sample(ep1, 10_000_000, Duration::from_secs(1)).await;
        record_sample(ep2, 10_000_000, Duration::from_secs(1)).await;

        let decision = check_admission(&[ep1.to_string(), ep2.to_string()]).await;
        assert_eq!(decision, AdmissionDecision::Admit);
    }

    #[tokio::test]
    async fn admission_reject_when_node_below_threshold() {
        let ep_fast = "http://admission-mixed-fast";
        let ep_slow = "http://admission-mixed-slow";
        // Fast node: 80 Mbps
        record_sample(ep_fast, 10_000_000, Duration::from_secs(1)).await;
        // Slow node: 8 bps (1 byte / 1s) — far below 512 Kbps floor
        record_sample(ep_slow, 1, Duration::from_secs(1)).await;

        let decision = check_admission(&[ep_fast.to_string(), ep_slow.to_string()]).await;
        assert!(
            matches!(decision, AdmissionDecision::Reject { .. }),
            "should reject when any node is below admission threshold"
        );
    }

    #[tokio::test]
    async fn admission_speculative_mixed_with_fast_node() {
        let ep_fast = "http://admission-speculative-mixed-fast";
        let ep_unknown = "http://admission-speculative-mixed-unknown";
        record_sample(ep_fast, 10_000_000, Duration::from_secs(1)).await;
        // ep_unknown has no samples

        let decision = check_admission(&[ep_fast.to_string(), ep_unknown.to_string()]).await;
        assert_eq!(
            decision,
            AdmissionDecision::AdmitSpeculative,
            "partial data should be speculative, not rejected"
        );
    }

    #[tokio::test]
    async fn node_bandwidth_statuses_reflects_measurements() {
        let ep = "http://admission-status-test";
        record_sample(ep, 10_000_000, Duration::from_secs(1)).await;

        let statuses = node_bandwidth_statuses(&[ep.to_string()]).await;
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].estimated_bps.is_some());
        assert!(statuses[0].meets_admission_threshold);
    }
}
