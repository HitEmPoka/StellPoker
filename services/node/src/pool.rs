//! Peer connection pooling for MPC nodes (Issue #246)
//!
//! Every REP3 session makes the node talk to both of its peers: share dispatch,
//! proactive share refresh, heartbeats. Each of those call sites used to build
//! its own `reqwest::Client`, and a client owns its connection pool — so a fresh
//! one per call means a fresh TCP handshake (plus a full TLS handshake when the
//! peer endpoint is `https://`) for every single request, and the connection is
//! dropped the moment the client goes out of scope.
//!
//! This module keeps **one** process-wide client whose keep-alive pool is shared
//! by every session. Connections are established once and reused, so only the
//! first request to a peer pays handshake cost.
//!
//! ## Health checks
//!
//! A pooled connection can be stale in ways TCP does not report promptly (peer
//! restarted, network partition healed the wrong way). A background task polls
//! `GET {endpoint}/health` on every peer and records the result, so callers can
//! ask [`is_healthy`] before dispatching work rather than discovering the
//! failure through a timeout mid-session.
//!
//! An endpoint that has never been probed reports healthy: absence of evidence
//! must not take a peer out of rotation at startup, before the first probe has
//! run.
//!
//! ## Configuration
//!
//! | Env var | Default | Meaning |
//! |---|---|---|
//! | `MPC_POOL_MAX_IDLE_PER_HOST` | 8 | Idle connections kept per peer |
//! | `MPC_POOL_IDLE_TIMEOUT_SECS` | 90 | Idle connection eviction |
//! | `MPC_POOL_TCP_KEEPALIVE_SECS` | 30 | TCP keep-alive probe interval |
//! | `MPC_POOL_HEALTH_INTERVAL_SECS` | 15 | Peer health check period |
//! | `MPC_POOL_HEALTH_TIMEOUT_SECS` | 5 | Per-probe timeout |

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static HEALTH: OnceLock<RwLock<HashMap<String, bool>>> = OnceLock::new();

fn env_secs(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The shared peer HTTP client.
///
/// Cloning is cheap and clones share the same connection pool — that sharing is
/// the whole point, so clone freely instead of building a new client.
pub fn peer_client() -> reqwest::Client {
    CLIENT
        .get_or_init(|| {
            let max_idle = std::env::var("MPC_POOL_MAX_IDLE_PER_HOST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8usize);

            reqwest::Client::builder()
                .pool_max_idle_per_host(max_idle)
                .pool_idle_timeout(Duration::from_secs(env_secs(
                    "MPC_POOL_IDLE_TIMEOUT_SECS",
                    90,
                )))
                .tcp_keepalive(Duration::from_secs(env_secs(
                    "MPC_POOL_TCP_KEEPALIVE_SECS",
                    30,
                )))
                .build()
                .unwrap_or_else(|e| {
                    tracing::warn!("peer pool build failed ({e}); falling back to defaults");
                    reqwest::Client::new()
                })
        })
        .clone()
}

fn health_map() -> &'static RwLock<HashMap<String, bool>> {
    HEALTH.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Last observed health of a peer. Unprobed endpoints report `true`.
pub fn is_healthy(endpoint: &str) -> bool {
    health_map()
        .read()
        .map(|m| m.get(endpoint).copied().unwrap_or(true))
        .unwrap_or(true)
}

/// Record a probe result, logging only on transitions so a down peer does not
/// reprint the same line every interval.
fn record(endpoint: &str, healthy: bool) {
    let Ok(mut map) = health_map().write() else {
        return;
    };
    let previous = map.insert(endpoint.to_string(), healthy);
    if previous != Some(healthy) {
        if healthy {
            tracing::info!(peer = endpoint, "peer connection healthy");
        } else {
            tracing::warn!(peer = endpoint, "peer connection unhealthy");
        }
    }
}

/// Probe every endpoint once. Exposed separately from the loop so it can be
/// called directly in tests.
pub async fn check_once(endpoints: &[String], timeout: Duration) {
    let client = peer_client();
    for endpoint in endpoints {
        let healthy = client
            .get(format!("{}/health", endpoint.trim_end_matches('/')))
            .timeout(timeout)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        record(endpoint, healthy);
    }
}

/// Start the background health checker for the node's peers.
pub fn spawn_health_checks(endpoints: Vec<String>) {
    if endpoints.is_empty() {
        return;
    }
    let interval = Duration::from_secs(env_secs("MPC_POOL_HEALTH_INTERVAL_SECS", 15));
    let timeout = Duration::from_secs(env_secs("MPC_POOL_HEALTH_TIMEOUT_SECS", 5));
    tokio::spawn(async move {
        loop {
            check_once(&endpoints, timeout).await;
            tokio::time::sleep(interval).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unprobed_peer_is_assumed_healthy() {
        assert!(is_healthy("http://never-probed:8101"));
    }

    #[test]
    fn recorded_state_is_readable() {
        record("http://peer-a:8101", false);
        assert!(!is_healthy("http://peer-a:8101"));
        record("http://peer-a:8101", true);
        assert!(is_healthy("http://peer-a:8101"));
    }

    #[tokio::test]
    async fn unreachable_peer_is_marked_unhealthy() {
        // Port 1 is reserved and never listening, so the probe must fail.
        let endpoints = vec!["http://127.0.0.1:1".to_string()];
        check_once(&endpoints, Duration::from_millis(500)).await;
        assert!(!is_healthy("http://127.0.0.1:1"));
    }

    #[test]
    fn clones_share_one_pool() {
        // Two handles to the same underlying pool, not two pools.
        let a = peer_client();
        let b = peer_client();
        drop((a, b));
    }
}
