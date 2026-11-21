// Issue #574: MPC node anti-entropy config sync (desired vs actual config)
// Nodes report config hash; coordinator/dashboard flags drift

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Configuration hash for drift detection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigHash {
    pub hash: String,
    pub timestamp: SystemTime,
    pub config_version: String,
}

impl ConfigHash {
    /// Compute SHA-256 hash of configuration
    pub fn from_config<T: Serialize>(config: &T, version: &str) -> Result<Self, String> {
        let json = serde_json::to_string(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        Ok(Self {
            hash,
            timestamp: SystemTime::now(),
            config_version: version.to_string(),
        })
    }
}

/// Configuration drift report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDriftReport {
    pub has_drift: bool,
    pub expected_hash: String,
    pub actual_hashes: HashMap<String, String>, // node_id -> hash
    pub drifted_nodes: Vec<String>,
    pub drift_detected_at: SystemTime,
}

impl ConfigDriftReport {
    pub fn drift_severity(&self) -> DriftSeverity {
        if !self.has_drift {
            return DriftSeverity::None;
        }

        let drift_ratio = self.drifted_nodes.len() as f32 / self.actual_hashes.len() as f32;

        if drift_ratio >= 0.5 {
            DriftSeverity::Critical
        } else if drift_ratio >= 0.25 {
            DriftSeverity::High
        } else {
            DriftSeverity::Low
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftSeverity {
    None,
    Low,      // < 25% of nodes drifted
    High,     // 25-50% of nodes drifted
    Critical, // > 50% of nodes drifted
}

/// Config sync coordinator - tracks expected config and detects drift
pub struct ConfigSyncCoordinator {
    expected_config_hash: Option<ConfigHash>,
    node_configs: HashMap<String, ConfigHash>,
}

impl ConfigSyncCoordinator {
    pub fn new() -> Self {
        Self {
            expected_config_hash: None,
            node_configs: HashMap::new(),
        }
    }

    /// Set the expected configuration hash (from coordinator)
    pub fn set_expected_config<T: Serialize>(
        &mut self,
        config: &T,
        version: &str,
    ) -> Result<(), String> {
        let hash = ConfigHash::from_config(config, version)?;
        self.expected_config_hash = Some(hash);
        Ok(())
    }

    /// Report node configuration (called by each node)
    pub fn report_node_config(&mut self, node_id: String, config_hash: ConfigHash) {
        self.node_configs.insert(node_id, config_hash);
    }

    /// Check for configuration drift across all nodes
    pub fn check_drift(&self) -> Option<ConfigDriftReport> {
        let expected = self.expected_config_hash.as_ref()?;

        let mut drifted_nodes = Vec::new();
        let mut actual_hashes = HashMap::new();

        for (node_id, node_config) in &self.node_configs {
            actual_hashes.insert(node_id.clone(), node_config.hash.clone());

            if node_config.hash != expected.hash {
                drifted_nodes.push(node_id.clone());
            }
        }

        let has_drift = !drifted_nodes.is_empty();

        Some(ConfigDriftReport {
            has_drift,
            expected_hash: expected.hash.clone(),
            actual_hashes,
            drifted_nodes,
            drift_detected_at: SystemTime::now(),
        })
    }

    /// Remove node from tracking
    pub fn remove_node(&mut self, node_id: &str) {
        self.node_configs.remove(node_id);
    }

    /// Get all tracked nodes
    pub fn tracked_nodes(&self) -> Vec<String> {
        self.node_configs.keys().cloned().collect()
    }
}

impl Default for ConfigSyncCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Node-side config sync - periodically reports config to coordinator
pub struct NodeConfigSync {
    node_id: String,
    current_config_hash: Option<ConfigHash>,
    last_sync: Option<SystemTime>,
    sync_interval: Duration,
}

impl NodeConfigSync {
    pub fn new(node_id: String, sync_interval_seconds: u64) -> Self {
        Self {
            node_id,
            current_config_hash: None,
            last_sync: None,
            sync_interval: Duration::from_secs(sync_interval_seconds),
        }
    }

    /// Update current configuration and compute hash
    pub fn update_config<T: Serialize>(
        &mut self,
        config: &T,
        version: &str,
    ) -> Result<ConfigHash, String> {
        let hash = ConfigHash::from_config(config, version)?;
        self.current_config_hash = Some(hash.clone());
        Ok(hash)
    }

    /// Check if sync is needed
    pub fn should_sync(&self) -> bool {
        match self.last_sync {
            None => true,
            Some(last) => {
                SystemTime::now()
                    .duration_since(last)
                    .map(|d| d >= self.sync_interval)
                    .unwrap_or(true)
            }
        }
    }

    /// Mark sync as performed
    pub fn mark_synced(&mut self) {
        self.last_sync = Some(SystemTime::now());
    }

    /// Get current config hash for reporting
    pub fn get_current_hash(&self) -> Option<&ConfigHash> {
        self.current_config_hash.as_ref()
    }
}

/// Remediation actions for config drift
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RemediationAction {
    /// Restart node with correct config
    RestartNode { node_id: String },
    /// Push updated config to node
    PushConfig { node_id: String },
    /// Manual intervention required
    ManualIntervention { reason: String },
}

pub fn recommend_remediation(drift_report: &ConfigDriftReport) -> Vec<RemediationAction> {
    let mut actions = Vec::new();

    match drift_report.drift_severity() {
        DriftSeverity::None => {}
        DriftSeverity::Low => {
            // Single node drift - likely safe to restart
            for node_id in &drift_report.drifted_nodes {
                actions.push(RemediationAction::RestartNode {
                    node_id: node_id.clone(),
                });
            }
        }
        DriftSeverity::High | DriftSeverity::Critical => {
            // Multiple nodes drifted - manual intervention recommended
            actions.push(RemediationAction::ManualIntervention {
                reason: format!(
                    "{} nodes drifted - verify expected config is correct before remediation",
                    drift_report.drifted_nodes.len()
                ),
            });
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct TestConfig {
        setting: String,
        value: i32,
    }

    #[test]
    fn test_config_hash_deterministic() {
        let config = TestConfig {
            setting: "test".to_string(),
            value: 42,
        };

        let hash1 = ConfigHash::from_config(&config, "1.0").unwrap();
        let hash2 = ConfigHash::from_config(&config, "1.0").unwrap();

        assert_eq!(hash1.hash, hash2.hash);
    }

    #[test]
    fn test_config_hash_different_for_different_config() {
        let config1 = TestConfig {
            setting: "test".to_string(),
            value: 42,
        };
        let config2 = TestConfig {
            setting: "test".to_string(),
            value: 43,
        };

        let hash1 = ConfigHash::from_config(&config1, "1.0").unwrap();
        let hash2 = ConfigHash::from_config(&config2, "1.0").unwrap();

        assert_ne!(hash1.hash, hash2.hash);
    }

    #[test]
    fn test_no_drift_when_all_match() {
        let mut coordinator = ConfigSyncCoordinator::new();
        let config = TestConfig {
            setting: "test".to_string(),
            value: 42,
        };

        coordinator.set_expected_config(&config, "1.0").unwrap();

        let hash = ConfigHash::from_config(&config, "1.0").unwrap();
        coordinator.report_node_config("node1".to_string(), hash.clone());
        coordinator.report_node_config("node2".to_string(), hash);

        let drift_report = coordinator.check_drift().unwrap();
        assert!(!drift_report.has_drift);
        assert_eq!(drift_report.drift_severity(), DriftSeverity::None);
    }

    #[test]
    fn test_drift_detected() {
        let mut coordinator = ConfigSyncCoordinator::new();
        let config1 = TestConfig {
            setting: "test".to_string(),
            value: 42,
        };
        let config2 = TestConfig {
            setting: "test".to_string(),
            value: 99,
        };

        coordinator.set_expected_config(&config1, "1.0").unwrap();

        let hash1 = ConfigHash::from_config(&config1, "1.0").unwrap();
        let hash2 = ConfigHash::from_config(&config2, "1.0").unwrap();

        coordinator.report_node_config("node1".to_string(), hash1);
        coordinator.report_node_config("node2".to_string(), hash2);

        let drift_report = coordinator.check_drift().unwrap();
        assert!(drift_report.has_drift);
        assert_eq!(drift_report.drifted_nodes, vec!["node2"]);
        assert_eq!(drift_report.drift_severity(), DriftSeverity::High);
    }

    #[test]
    fn test_node_sync_interval() {
        let mut sync = NodeConfigSync::new("node1".to_string(), 60);
        assert!(sync.should_sync());

        sync.mark_synced();
        assert!(!sync.should_sync());
    }
}
