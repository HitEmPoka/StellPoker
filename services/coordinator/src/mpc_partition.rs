//! Network partition detection and handling for the MPC cluster.
//!
//! Issue #236: Detect network partitions between MPC nodes using a
//! consensus-based failure detector. On partition, pause affected sessions.
//! Resume when the partition heals or after a timeout.
//!
//! Each MPC node periodically reports which peers it can currently reach.
//! Rather than trusting a single node's view (which could itself be the one
//! that's cut off), the coordinator only declares a node partitioned once a
//! quorum of the *other* nodes independently report it unreachable. Sessions
//! that depend on a partitioned node are paused; they resume automatically
//! once the partition heals (the node is no longer reported unreachable by a
//! quorum) or once `partition_timeout` elapses, whichever comes first.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct PartitionConfig {
    /// Fraction (0.0-1.0] of *other* nodes that must independently report a
    /// node unreachable before it is declared partitioned.
    pub quorum_fraction: f64,
    /// How long a connectivity report remains valid before it's ignored.
    pub report_ttl: Duration,
    /// Auto-resume paused sessions this long after a partition was detected,
    /// even if it hasn't been confirmed healed.
    pub partition_timeout: Duration,
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self {
            quorum_fraction: 0.5,
            report_ttl: Duration::from_secs(30),
            partition_timeout: Duration::from_secs(120),
        }
    }
}

impl PartitionConfig {
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            quorum_fraction: std::env::var("MPC_PARTITION_QUORUM_FRACTION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default.quorum_fraction),
            report_ttl: std::env::var("MPC_PARTITION_REPORT_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs)
                .unwrap_or(default.report_ttl),
            partition_timeout: std::env::var("MPC_PARTITION_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs)
                .unwrap_or(default.partition_timeout),
        }
    }
}

/// A single node's view of which peers it currently cannot reach.
#[derive(Clone, Debug)]
struct ConnectivityReport {
    unreachable: HashSet<String>,
    reported_at: SystemTime,
}

#[derive(Debug)]
pub struct PartitionDetector {
    config: PartitionConfig,
    /// reporter_node_id -> latest report from that node.
    reports: HashMap<String, ConnectivityReport>,
    /// node_id -> when it was first declared partitioned.
    partitioned: HashMap<String, SystemTime>,
    /// table_ids paused because they depend on a partitioned node.
    paused_sessions: HashMap<u32, HashSet<String>>,
}

pub type PartitionStore = Arc<RwLock<PartitionDetector>>;

pub fn new_store(config: PartitionConfig) -> PartitionStore {
    Arc::new(RwLock::new(PartitionDetector {
        config,
        reports: HashMap::new(),
        partitioned: HashMap::new(),
        paused_sessions: HashMap::new(),
    }))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PartitionStatus {
    pub partitioned_nodes: Vec<String>,
    pub paused_sessions: Vec<u32>,
}

impl PartitionDetector {
    /// Record a node's report of which peers it cannot currently reach, then
    /// recompute the partitioned set against the full committee membership.
    pub fn submit_report(
        &mut self,
        reporter_node_id: &str,
        unreachable: HashSet<String>,
        all_node_ids: &[String],
    ) {
        self.reports.insert(
            reporter_node_id.to_string(),
            ConnectivityReport {
                unreachable,
                reported_at: SystemTime::now(),
            },
        );
        self.recompute(all_node_ids);
    }

    /// Recompute which nodes are partitioned based on currently-fresh
    /// reports. A node is partitioned when at least `quorum_fraction` of the
    /// *other* committee members report it unreachable.
    pub fn recompute(&mut self, all_node_ids: &[String]) {
        let now = SystemTime::now();
        self.reports.retain(|_, r| {
            now.duration_since(r.reported_at).unwrap_or_default() <= self.config.report_ttl
        });

        for candidate in all_node_ids {
            let other_reporters: Vec<&ConnectivityReport> = self
                .reports
                .iter()
                .filter(|(reporter, _)| *reporter != candidate)
                .map(|(_, report)| report)
                .collect();

            if other_reporters.is_empty() {
                continue;
            }

            let unreachable_count = other_reporters
                .iter()
                .filter(|r| r.unreachable.contains(candidate))
                .count();
            let quorum_needed =
                ((other_reporters.len() as f64) * self.config.quorum_fraction).ceil() as usize;
            let quorum_needed = quorum_needed.max(1);

            let is_partitioned = unreachable_count >= quorum_needed;

            if is_partitioned {
                self.partitioned.entry(candidate.clone()).or_insert(now);
            } else {
                self.partitioned.remove(candidate);
            }
        }
    }

    pub fn is_partitioned(&self, node_id: &str) -> bool {
        self.partitioned.contains_key(node_id)
    }

    /// Whether a partition detected for `node_id` has exceeded the configured
    /// timeout and should be considered healed for the purposes of resuming
    /// sessions, even without positive confirmation.
    pub fn partition_timed_out(&self, node_id: &str) -> bool {
        match self.partitioned.get(node_id) {
            Some(since) => SystemTime::now()
                .duration_since(*since)
                .map(|d| d >= self.config.partition_timeout)
                .unwrap_or(false),
            None => false,
        }
    }

    /// Pause a session (identified by `table_id`) because it depends on
    /// `node_id`, which is currently partitioned.
    pub fn pause_session(&mut self, table_id: u32, node_id: &str) {
        self.paused_sessions
            .entry(table_id)
            .or_default()
            .insert(node_id.to_string());
    }

    /// Resume a session once none of the nodes it was waiting on are still
    /// partitioned (either the partition healed or it timed out).
    pub fn try_resume_session(&mut self, table_id: u32) -> bool {
        let Some(blocking_nodes) = self.paused_sessions.get(&table_id) else {
            return true;
        };

        let still_blocked: HashSet<String> = blocking_nodes
            .iter()
            .filter(|node_id| self.is_partitioned(node_id) && !self.partition_timed_out(node_id))
            .cloned()
            .collect();

        if still_blocked.is_empty() {
            self.paused_sessions.remove(&table_id);
            true
        } else {
            self.paused_sessions.insert(table_id, still_blocked);
            false
        }
    }

    pub fn is_session_paused(&self, table_id: u32) -> bool {
        self.paused_sessions.contains_key(&table_id)
    }

    pub fn status(&self) -> PartitionStatus {
        PartitionStatus {
            partitioned_nodes: self.partitioned.keys().cloned().collect(),
            paused_sessions: self.paused_sessions.keys().copied().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> Vec<String> {
        vec!["0".into(), "1".into(), "2".into(), "3".into()]
    }

    #[test]
    fn single_node_reporting_unreachable_is_not_enough() {
        let mut d = PartitionDetector {
            config: PartitionConfig::default(),
            reports: HashMap::new(),
            partitioned: HashMap::new(),
            paused_sessions: HashMap::new(),
        };
        d.submit_report("0", HashSet::from(["3".to_string()]), &nodes());
        // Only one report exists at all, so there's nothing to form quorum
        // against node "3" from its peers (1, 2).
        assert!(!d.is_partitioned("3"));
    }

    #[test]
    fn quorum_of_peers_confirms_partition() {
        let mut d = PartitionDetector {
            config: PartitionConfig::default(),
            reports: HashMap::new(),
            partitioned: HashMap::new(),
            paused_sessions: HashMap::new(),
        };
        d.submit_report("0", HashSet::from(["3".to_string()]), &nodes());
        d.submit_report("1", HashSet::from(["3".to_string()]), &nodes());
        d.submit_report("2", HashSet::from(["3".to_string()]), &nodes());
        assert!(d.is_partitioned("3"));
        assert!(!d.is_partitioned("0"));
    }

    #[test]
    fn healed_partition_clears_on_recompute() {
        let mut d = PartitionDetector {
            config: PartitionConfig::default(),
            reports: HashMap::new(),
            partitioned: HashMap::new(),
            paused_sessions: HashMap::new(),
        };
        d.submit_report("0", HashSet::from(["3".to_string()]), &nodes());
        d.submit_report("1", HashSet::from(["3".to_string()]), &nodes());
        d.submit_report("2", HashSet::from(["3".to_string()]), &nodes());
        assert!(d.is_partitioned("3"));

        d.submit_report("0", HashSet::new(), &nodes());
        d.submit_report("1", HashSet::new(), &nodes());
        d.submit_report("2", HashSet::new(), &nodes());
        assert!(!d.is_partitioned("3"));
    }

    #[test]
    fn session_resumes_once_partition_clears() {
        let mut d = PartitionDetector {
            config: PartitionConfig::default(),
            reports: HashMap::new(),
            partitioned: HashMap::new(),
            paused_sessions: HashMap::new(),
        };
        d.partitioned.insert("3".to_string(), SystemTime::now());
        d.pause_session(42, "3");
        assert!(d.is_session_paused(42));
        assert!(!d.try_resume_session(42));

        d.partitioned.remove("3");
        assert!(d.try_resume_session(42));
        assert!(!d.is_session_paused(42));
    }
}
