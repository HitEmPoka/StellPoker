//! Bandwidth estimation and adaptive coNoir protocol selection (Issue #238).
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

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::RwLock;

const DEFAULT_LOW_BANDWIDTH_THRESHOLD_BPS: f64 = 1_000_000.0; // 1 Mbps
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
}
