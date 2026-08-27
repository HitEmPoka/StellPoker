//! MPC node version negotiation protocol.
//!
//! Issue #233: Add a version handshake between coordinator and MPC nodes.
//! Nodes report supported circuit versions and protocol versions. Coordinator
//! selects a compatible version for each session.
//!
//! Each MPC node advertises the protocol versions it speaks and, per circuit
//! (deal/reveal/showdown), which circuit versions it has compiled. Before a
//! session starts, the coordinator negotiates a single protocol version and a
//! version per circuit that every selected node supports, choosing the
//! highest mutually-supported version so the committee always runs the most
//! capable version all nodes agree on.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

/// Capabilities reported by a single MPC node during the version handshake.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NodeCapabilities {
    pub node_id: String,
    /// Protocol (wire format / session orchestration) versions this node supports.
    pub protocol_versions: Vec<u32>,
    /// Circuit name -> list of ACIR/circuit versions this node has compiled.
    pub circuit_versions: HashMap<String, Vec<u32>>,
    #[serde(skip_deserializing)]
    pub reported_at: Option<SystemTime>,
}

/// In-memory registry of the most recently reported capabilities per node.
pub type VersionRegistry = Arc<RwLock<HashMap<String, NodeCapabilities>>>;

pub fn new_registry() -> VersionRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Record (or replace) a node's reported capabilities.
pub async fn register_capabilities(registry: &VersionRegistry, mut caps: NodeCapabilities) {
    caps.reported_at = Some(SystemTime::now());
    let mut guard = registry.write().await;
    guard.insert(caps.node_id.clone(), caps);
}

/// Result of negotiating a session's protocol + circuit versions across a set
/// of nodes.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SessionVersionPlan {
    pub protocol_version: u32,
    pub circuit_versions: HashMap<String, u32>,
    pub participating_nodes: Vec<String>,
}

/// Pick the highest version present in every set among `sets`. Returns `None`
/// if there is no version common to all of them (or `sets` is empty).
fn highest_common_version(sets: &[HashSet<u32>]) -> Option<u32> {
    let mut iter = sets.iter();
    let first = iter.next()?;
    let mut common: HashSet<u32> = first.clone();
    for s in iter {
        common = common.intersection(s).copied().collect();
        if common.is_empty() {
            return None;
        }
    }
    common.into_iter().max()
}

/// Negotiate a protocol version compatible with all `node_ids`.
pub async fn negotiate_protocol_version(
    registry: &VersionRegistry,
    node_ids: &[String],
) -> Result<u32, String> {
    let guard = registry.read().await;
    let mut sets = Vec::with_capacity(node_ids.len());
    for id in node_ids {
        let caps = guard
            .get(id)
            .ok_or_else(|| format!("no version handshake on file for node {}", id))?;
        sets.push(caps.protocol_versions.iter().copied().collect());
    }
    highest_common_version(&sets)
        .ok_or_else(|| format!("no protocol version compatible across nodes {:?}", node_ids))
}

/// Negotiate a circuit version compatible with all `node_ids` for `circuit_name`.
pub async fn negotiate_circuit_version(
    registry: &VersionRegistry,
    node_ids: &[String],
    circuit_name: &str,
) -> Result<u32, String> {
    let guard = registry.read().await;
    let mut sets = Vec::with_capacity(node_ids.len());
    for id in node_ids {
        let caps = guard
            .get(id)
            .ok_or_else(|| format!("no version handshake on file for node {}", id))?;
        let versions = caps
            .circuit_versions
            .get(circuit_name)
            .ok_or_else(|| format!("node {} did not report versions for circuit {}", id, circuit_name))?;
        sets.push(versions.iter().copied().collect());
    }
    highest_common_version(&sets).ok_or_else(|| {
        format!(
            "no version of circuit '{}' compatible across nodes {:?}",
            circuit_name, node_ids
        )
    })
}

/// Full negotiation for a session: protocol version plus a version for each
/// circuit in `circuit_names`.
pub async fn negotiate_session(
    registry: &VersionRegistry,
    node_ids: &[String],
    circuit_names: &[&str],
) -> Result<SessionVersionPlan, String> {
    let protocol_version = negotiate_protocol_version(registry, node_ids).await?;

    let mut circuit_versions = HashMap::new();
    for circuit_name in circuit_names {
        let version = negotiate_circuit_version(registry, node_ids, circuit_name).await?;
        circuit_versions.insert(circuit_name.to_string(), version);
    }

    Ok(SessionVersionPlan {
        protocol_version,
        circuit_versions,
        participating_nodes: node_ids.to_vec(),
    })
}

/// Snapshot of every node's last-reported capabilities, for diagnostics.
pub async fn list_capabilities(registry: &VersionRegistry) -> Vec<NodeCapabilities> {
    registry.read().await.values().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_highest_common_version() {
        let sets = vec![
            [1u32, 2, 3].into_iter().collect(),
            [2u32, 3, 4].into_iter().collect(),
            [2u32, 3].into_iter().collect(),
        ];
        assert_eq!(highest_common_version(&sets), Some(3));
    }

    #[test]
    fn no_common_version_returns_none() {
        let sets = vec![
            [1u32].into_iter().collect(),
            [2u32].into_iter().collect(),
        ];
        assert_eq!(highest_common_version(&sets), None);
    }

    #[tokio::test]
    async fn negotiates_protocol_and_circuit_versions() {
        let registry = new_registry();
        register_capabilities(
            &registry,
            NodeCapabilities {
                node_id: "0".into(),
                protocol_versions: vec![1, 2],
                circuit_versions: HashMap::from([("deal".to_string(), vec![1, 2, 3])]),
                reported_at: None,
            },
        )
        .await;
        register_capabilities(
            &registry,
            NodeCapabilities {
                node_id: "1".into(),
                protocol_versions: vec![2, 3],
                circuit_versions: HashMap::from([("deal".to_string(), vec![2, 3])]),
                reported_at: None,
            },
        )
        .await;

        let node_ids = vec!["0".to_string(), "1".to_string()];
        let plan = negotiate_session(&registry, &node_ids, &["deal"])
            .await
            .expect("negotiation succeeds");
        assert_eq!(plan.protocol_version, 2);
        assert_eq!(plan.circuit_versions.get("deal"), Some(&2));
    }

    #[tokio::test]
    async fn missing_handshake_errors() {
        let registry = new_registry();
        let node_ids = vec!["ghost".to_string()];
        assert!(negotiate_protocol_version(&registry, &node_ids).await.is_err());
    }
}
