//! Verification-key / circuit rollout policy for multi-circuit deployments
//! (VK registry multi-circuit, adjacent to #47).
//!
//! The on-chain `zk-verifier` contract stores one VK per `CircuitType`, and the
//! coordinator decides which compiled circuit artifact a hand is proved
//! against. This module adds a rollout layer on top of that:
//!
//! * **Pin a version** — each circuit kind has a `pinned` `{version, circuit}`
//!   pair. Without any config the pinned circuit is the kind name itself, so
//!   behaviour is unchanged.
//! * **Canary a percentage** — an optional `canary` pair plus a
//!   `canary_percent`. A deterministic hash of `(circuit kind, table_id)`
//!   decides which channel a table uses, so a canary table stays on the canary
//!   circuit for its whole hand.
//! * **Auto-promote** — with `auto_promote` criteria configured, the canary is
//!   promoted to pinned once it has served `min_probes` proofs with a success
//!   rate of at least `min_success_rate`.
//! * **Metrics per version** — every proof result is recorded in
//!   `coordinator_circuit_proofs_total{circuit,version,result}` and
//!   `coordinator_circuit_proof_latency_seconds{circuit,version}`.
//! * **Rollback switch** — `VK_ROLLBACK=1` (or `VK_ROLLBACK_CIRCUITS=deal_valid,…`)
//!   forces the pinned circuit at startup, and the admin API can roll back or
//!   promote at runtime.
//!
//! Configuration is read from the JSON file named by `VK_ROLLOUT_CONFIG`
//! (default `./vk-rollout.json`; see `vk-rollout.example.json`). The file is
//! optional — a missing or empty file means "everything pinned, no canary".

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use prometheus::{HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RolloutConfig {
    #[serde(default)]
    pub circuits: HashMap<String, CircuitRollout>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircuitRollout {
    pub pinned: VersionRef,
    #[serde(default)]
    pub canary: Option<VersionRef>,
    #[serde(default)]
    pub canary_percent: u8,
    #[serde(default)]
    pub auto_promote: Option<AutoPromote>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRef {
    /// Opaque version label reported in metrics (e.g. `"v1"`).
    pub version: String,
    /// Base name of the compiled circuit artifact to prove against.
    pub circuit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoPromote {
    #[serde(default = "default_min_probes")]
    pub min_probes: u64,
    #[serde(default = "default_min_success_rate")]
    pub min_success_rate: f64,
}

fn default_min_probes() -> u64 {
    50
}

fn default_min_success_rate() -> f64 {
    0.99
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Pinned,
    Canary,
    /// Forced back to pinned by the rollback switch.
    Rollback,
}

/// Result of selecting a circuit for one proof request.
#[derive(Clone, Debug, Serialize)]
pub struct Selection {
    pub kind: String,
    /// Base circuit artifact name (callers may parameterise it by player count).
    pub circuit: String,
    pub version: String,
    pub channel: Channel,
}

/// Aggregated per-(kind, version) statistics used for auto-promotion.
#[derive(Clone, Debug, Default, Serialize)]
pub struct VersionStat {
    pub probes: u64,
    pub successes: u64,
    pub failures: u64,
    pub latency_seconds_sum: f64,
    pub latency_samples: u64,
}

impl VersionStat {
    pub fn success_rate(&self) -> f64 {
        if self.probes == 0 {
            1.0
        } else {
            self.successes as f64 / self.probes as f64
        }
    }

    pub fn avg_latency_seconds(&self) -> f64 {
        if self.latency_samples == 0 {
            0.0
        } else {
            self.latency_seconds_sum / self.latency_samples as f64
        }
    }
}

#[derive(Clone)]
pub struct RolloutMetrics {
    pub proofs: IntCounterVec,
    pub latency: HistogramVec,
    pub promotions: IntCounterVec,
    pub canary_active: IntGaugeVec,
}

impl RolloutMetrics {
    pub fn register(registry: &Registry) -> Self {
        let proofs = IntCounterVec::new(
            Opts::new(
                "coordinator_circuit_proofs_total",
                "MPC proof results by circuit artifact and version.",
            ),
            &["circuit", "version", "result"],
        )
        .unwrap();
        let latency = HistogramVec::new(
            HistogramOpts::new(
                "coordinator_circuit_proof_latency_seconds",
                "MPC proof generation latency by circuit artifact and version.",
            )
            .buckets(vec![
                0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
            ]),
            &["circuit", "version"],
        )
        .unwrap();
        let promotions = IntCounterVec::new(
            Opts::new(
                "coordinator_circuit_rollout_promotions_total",
                "Canary circuits automatically promoted to pinned.",
            ),
            &["circuit"],
        )
        .unwrap();
        let canary_active = IntGaugeVec::new(
            Opts::new(
                "coordinator_circuit_canary_active",
                "1 when a circuit has an active canary (not rolled back).",
            ),
            &["circuit"],
        )
        .unwrap();

        registry.register(Box::new(proofs.clone())).ok();
        registry.register(Box::new(latency.clone())).ok();
        registry.register(Box::new(promotions.clone())).ok();
        registry.register(Box::new(canary_active.clone())).ok();

        Self {
            proofs,
            latency,
            promotions,
            canary_active,
        }
    }
}

struct RuntimeConfig {
    config: RolloutConfig,
    /// Circuit kinds forced back to pinned by the rollback switch.
    rolled_back: HashSet<String>,
    /// Global rollback (`VK_ROLLBACK=1`).
    global_rollback: bool,
    /// `VK_ROLLBACK=0` disables auto-promotion even if configured.
    auto_promote_enabled: bool,
    /// Optional `VK_CANARY_PERCENT` override applied to every circuit.
    canary_percent_override: Option<u8>,
}

impl RuntimeConfig {
    fn is_rolled_back(&self, kind: &str) -> bool {
        self.global_rollback || self.rolled_back.contains(kind)
    }
}

/// Shared rollout state.
#[derive(Clone)]
pub struct RolloutState {
    inner: Arc<RwLock<RuntimeConfig>>,
    stats: Arc<Mutex<HashMap<String, VersionStat>>>,
    pub metrics: RolloutMetrics,
}

fn stats_key(kind: &str, version: &str) -> String {
    format!("{kind}|{version}")
}

/// Deterministic bucket in `0..100` for a `(kind, seed)` pair.
fn canary_bucket(kind: &str, seed: u64) -> u8 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut hasher);
    seed.hash(&mut hasher);
    (hasher.finish() % 100) as u8
}

impl RolloutState {
    pub fn new(registry: &Registry, config: RolloutConfig) -> Self {
        let metrics = RolloutMetrics::register(registry);

        let global_rollback = env_flag("VK_ROLLBACK");
        let auto_promote_enabled = !env_flag_false("VK_AUTO_PROMOTE");
        let canary_percent_override = std::env::var("VK_CANARY_PERCENT")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
            .map(|v| v.min(100));
        let mut rolled_back: HashSet<String> = std::env::var("VK_ROLLBACK_CIRCUITS")
            .ok()
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        if global_rollback {
            rolled_back.extend(config.circuits.keys().cloned());
            tracing::warn!("VK_ROLLBACK=1 — all canary circuits forced to pinned");
        }

        let state = Self {
            inner: Arc::new(RwLock::new(RuntimeConfig {
                config,
                rolled_back,
                global_rollback,
                auto_promote_enabled,
                canary_percent_override,
            })),
            stats: Arc::new(Mutex::new(HashMap::new())),
            metrics,
        };

        // Publish the initial canary gauge for observability.
        if let Ok(rt) = state.inner.try_read() {
            for (kind, rollout) in &rt.config.circuits {
                let active = rollout.canary.is_some() && !rt.is_rolled_back(kind);
                state
                    .metrics
                    .canary_active
                    .with_label_values(&[kind.as_str()])
                    .set(if active { 1 } else { 0 });
            }
        }

        let configured = state
            .inner
            .try_read()
            .map(|rt| rt.config.circuits.len())
            .unwrap_or(0);
        if configured > 0 {
            tracing::info!("Circuit rollout policy loaded for {} circuit kind(s)", configured);
        }

        state
    }

    /// Load configuration from `VK_ROLLOUT_CONFIG` (default `./vk-rollout.json`)
    /// and build the state. `VK_ROLLOUT_ENABLED=false` disables all rollouts.
    pub fn from_env(registry: &Registry) -> Self {
        if env_flag_false("VK_ROLLOUT_ENABLED") {
            tracing::info!("Circuit rollout policy disabled (VK_ROLLOUT_ENABLED=false)");
            return Self::new(registry, RolloutConfig::default());
        }
        let path = std::env::var("VK_ROLLOUT_CONFIG").unwrap_or_else(|_| "./vk-rollout.json".into());
        let config = load_config(Path::new(&path));
        Self::new(registry, config)
    }

    async fn is_rolled_back(rt: &RuntimeConfig, kind: &str) -> bool {
        rt.global_rollback || rt.rolled_back.contains(kind)
    }

    /// Choose the circuit for one proof request. `seed` should be stable for the
    /// lifetime of the hand (the table id) so canary assignment is sticky.
    pub async fn select(&self, kind: &str, seed: u64) -> Selection {
        let rt = self.inner.read().await;
        // No policy for this kind → use the kind itself as the pinned artifact.
        let Some(rollout) = rt.config.circuits.get(kind) else {
            return Selection {
                kind: kind.to_string(),
                circuit: kind.to_string(),
                version: "default".to_string(),
                channel: Channel::Pinned,
            };
        };

        let rolled_back = RuntimeConfig::is_rolled_back(&rt, kind);
        if let Some(canary) = &rollout.canary {
            if !rolled_back {
                let percent = rt
                    .canary_percent_override
                    .unwrap_or(rollout.canary_percent)
                    .min(100);
                if percent > 0 && canary_bucket(kind, seed) < percent {
                    return Selection {
                        kind: kind.to_string(),
                        circuit: canary.circuit.clone(),
                        version: canary.version.clone(),
                        channel: Channel::Canary,
                    };
                }
            }
        }

        Selection {
            kind: kind.to_string(),
            circuit: rollout.pinned.circuit.clone(),
            version: rollout.pinned.version.clone(),
            channel: if rolled_back && rollout.canary.is_some() {
                Channel::Rollback
            } else {
                Channel::Pinned
            },
        }
    }

    /// Record the outcome of a proof for metrics, rollout statistics and
    /// auto-promotion.
    pub async fn record(
        &self,
        kind: &str,
        circuit: &str,
        version: &str,
        success: bool,
        latency_seconds: f64,
    ) {
        self.metrics
            .proofs
            .with_label_values(&[circuit, version, if success { "success" } else { "failure" }])
            .inc();
        self.metrics
            .latency
            .with_label_values(&[circuit, version])
            .observe(latency_seconds);

        {
            let mut stats = self.stats.lock().await;
            let entry = stats.entry(stats_key(kind, version)).or_default();
            entry.probes += 1;
            if success {
                entry.successes += 1;
            } else {
                entry.failures += 1;
            }
            entry.latency_seconds_sum += latency_seconds;
            entry.latency_samples += 1;
        }

        self.maybe_auto_promote(kind).await;
    }

    async fn maybe_auto_promote(&self, kind: &str) {
        let (canary, criteria, enabled) = {
            let rt = self.inner.read().await;
            if !rt.auto_promote_enabled || RuntimeConfig::is_rolled_back(&rt, kind) {
                return;
            }
            let Some(rollout) = rt.config.circuits.get(kind) else {
                return;
            };
            match (&rollout.canary, &rollout.auto_promote) {
                (Some(canary), Some(criteria)) => (canary.clone(), criteria.clone(), true),
                _ => return,
            }
        };
        let _ = enabled;

        let (probes, rate) = {
            let stats = self.stats.lock().await;
            match stats.get(&stats_key(kind, &canary.version)) {
                Some(s) => (s.probes, s.success_rate()),
                None => return,
            }
        };

        if probes >= criteria.min_probes && rate >= criteria.min_success_rate {
            self.promote(kind).await;
        }
    }

    /// Promote the canary to pinned (auto or manual). Returns `true` if a
    /// promotion happened.
    pub async fn promote(&self, kind: &str) -> bool {
        let promoted = {
            let mut rt = self.inner.write().await;
            let Some(rollout) = rt.config.circuits.get(kind).cloned() else {
                return false;
            };
            let Some(canary) = rollout.canary.clone() else {
                return false;
            };
            // Promote: pinned becomes the former canary, canary cleared.
            let mut updated = rollout.clone();
            updated.pinned = canary.clone();
            updated.canary = None;
            rt.config.circuits.insert(kind.to_string(), updated);
            rt.rolled_back.remove(kind);
            (canary, rollout.pinned)
        };

        let (new_pinned, old_pinned) = promoted;
        self.metrics
            .promotions
            .with_label_values(&[kind])
            .inc();
        self.metrics
            .canary_active
            .with_label_values(&[kind])
            .set(0);
        tracing::warn!(
            "Circuit rollout promoted: kind={} {} -> {} (version {} -> {})",
            kind,
            old_pinned.circuit,
            new_pinned.circuit,
            old_pinned.version,
            new_pinned.version
        );
        true
    }

    /// Roll a circuit back to its pinned version by disabling the canary.
    /// Pass `"*"` to roll back every circuit. Returns the kinds affected.
    pub async fn rollback(&self, kind_or_all: &str) -> Vec<String> {
        let mut rt = self.inner.write().await;
        let kinds: Vec<String> = if kind_or_all == "*" {
            rt.config.circuits.keys().cloned().collect()
        } else {
            vec![kind_or_all.to_string()]
        };

        let mut affected = Vec::new();
        for kind in kinds {
            if rt.config.circuits.get(&kind).map(|c| c.canary.is_some()).unwrap_or(false) {
                rt.rolled_back.insert(kind.clone());
                self.metrics
                    .canary_active
                    .with_label_values(&[kind.as_str()])
                    .set(0);
                tracing::warn!("Circuit rollout rolled back: kind={} forced to pinned", kind);
                affected.push(kind);
            } else if kind_or_all == "*" {
                // Nothing to do for circuits without a canary.
            } else {
                // Explicit single-circuit rollback still records the switch so a
                // later config reload cannot re-enable the canary silently.
                rt.rolled_back.insert(kind.clone());
                affected.push(kind);
            }
        }
        affected
    }

    /// Re-arm a previously rolled-back canary (used by the admin API).
    pub async fn clear_rollback(&self, kind: &str) -> bool {
        let mut rt = self.inner.write().await;
        let removed = rt.rolled_back.remove(kind);
        let has_canary = rt.config.circuits.get(kind).map(|c| c.canary.is_some()).unwrap_or(false);
        if has_canary {
            self.metrics
                .canary_active
                .with_label_values(&[kind])
                .set(1);
        }
        removed
    }

    pub async fn status(&self) -> RolloutStatus {
        let rt = self.inner.read().await;
        let stats = self.stats.lock().await;
        let mut circuits = Vec::new();
        // Deterministic ordering keeps the admin response stable for tests.
        let mut kinds: Vec<&String> = rt.config.circuits.keys().collect();
        kinds.sort();
        for kind in kinds {
            let rollout = &rt.config.circuits[kind];
            let rolled_back = RuntimeConfig::is_rolled_back(&rt, kind);
            let mut versions: Vec<VersionStatus> = Vec::new();
            for vref in std::iter::once(&rollout.pinned).chain(rollout.canary.iter()) {
                let stat = stats
                    .get(&stats_key(kind, &vref.version))
                    .cloned()
                    .unwrap_or_default();
                versions.push(VersionStatus {
                    version: vref.version.clone(),
                    circuit: vref.circuit.clone(),
                    role: if Some(vref) == rollout.canary.as_ref() {
                        "canary"
                    } else {
                        "pinned"
                    }
                    .to_string(),
                    stat,
                });
            }
            circuits.push(CircuitStatus {
                kind: kind.clone(),
                canary_percent: rt
                    .canary_percent_override
                    .unwrap_or(rollout.canary_percent)
                    .min(100),
                canary_active: rollout.canary.is_some() && !rolled_back,
                rolled_back,
                auto_promote: rollout.auto_promote.clone(),
                versions,
            });
        }
        RolloutStatus {
            enabled: rt.config.circuits.values().any(|c| c.canary.is_some()),
            global_rollback: rt.global_rollback,
            circuits,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RolloutStatus {
    pub enabled: bool,
    pub global_rollback: bool,
    pub circuits: Vec<CircuitStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CircuitStatus {
    pub kind: String,
    pub canary_percent: u8,
    pub canary_active: bool,
    pub rolled_back: bool,
    pub auto_promote: Option<AutoPromote>,
    pub versions: Vec<VersionStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VersionStatus {
    pub version: String,
    pub circuit: String,
    pub role: String,
    #[serde(flatten)]
    pub stat: VersionStat,
}

/// Load the rollout config from a JSON file. Missing/invalid files fall back to
/// the empty config (everything pinned) and log a warning.
pub fn load_config(path: &Path) -> RolloutConfig {
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<RolloutConfig>(&raw) {
            Ok(config) => config,
            Err(e) => {
                tracing::error!(
                    "Invalid circuit rollout config at {}: {} — using pinned defaults",
                    path.display(),
                    e
                );
                RolloutConfig::default()
            }
        },
        Err(_) => RolloutConfig::default(),
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn env_flag_false(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            matches!(
                v.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        Registry::new()
    }

    fn test_config(percent: u8) -> RolloutConfig {
        let mut circuits = HashMap::new();
        circuits.insert(
            "deal_valid".to_string(),
            CircuitRollout {
                pinned: VersionRef {
                    version: "v1".into(),
                    circuit: "deal_valid".into(),
                },
                canary: Some(VersionRef {
                    version: "v2".into(),
                    circuit: "deal_valid_v2".into(),
                }),
                canary_percent: percent,
                auto_promote: Some(AutoPromote {
                    min_probes: 3,
                    min_success_rate: 0.99,
                }),
            },
        );
        RolloutConfig { circuits }
    }

    #[tokio::test]
    async fn unknown_kind_defaults_to_pinned() {
        let state = RolloutState::new(&registry(), RolloutConfig::default());
        let sel = state.select("showdown_valid", 7).await;
        assert_eq!(sel.circuit, "showdown_valid");
        assert_eq!(sel.version, "default");
        assert_eq!(sel.channel, Channel::Pinned);
    }

    #[tokio::test]
    async fn canary_percentage_is_deterministic_and_respected() {
        // 100% canary always selects the canary; 0% never does.
        let all = RolloutState::new(&registry(), test_config(100));
        for table in 0..50 {
            let sel = all.select("deal_valid", table).await;
            assert_eq!(sel.circuit, "deal_valid_v2", "table {table}");
            assert_eq!(sel.channel, Channel::Canary);
        }

        let none = RolloutState::new(&registry(), test_config(0));
        for table in 0..50 {
            let sel = none.select("deal_valid", table).await;
            assert_eq!(sel.circuit, "deal_valid");
            assert_eq!(sel.channel, Channel::Pinned);
        }

        // Deterministic: same seed → same channel.
        let partial = RolloutState::new(&registry(), test_config(50));
        let a = partial.select("deal_valid", 42).await;
        let b = partial.select("deal_valid", 42).await;
        assert_eq!(a.circuit, b.circuit);
        // And the split is actually mixed across seeds.
        let canary_count = {
            let mut n = 0;
            for table in 0..2000 {
                if partial.select("deal_valid", table).await.channel == Channel::Canary {
                    n += 1;
                }
            }
            n
        };
        assert!(
            (700..1300).contains(&canary_count),
            "50% canary should be roughly balanced, got {canary_count}/2000"
        );
    }

    #[tokio::test]
    async fn auto_promote_on_success_criteria() {
        let state = RolloutState::new(&registry(), test_config(100));
        // Two successful canary probes: below min_probes=3 → no promotion.
        state.record("deal_valid", "deal_valid_v2", "v2", true, 0.5).await;
        state.record("deal_valid", "deal_valid_v2", "v2", true, 0.5).await;
        assert_eq!(state.select("deal_valid", 1).await.channel, Channel::Canary);

        // Third success meets criteria → promoted; new selections are pinned v2.
        state.record("deal_valid", "deal_valid_v2", "v2", true, 0.5).await;
        let sel = state.select("deal_valid", 1).await;
        assert_eq!(sel.circuit, "deal_valid_v2");
        assert_eq!(sel.version, "v2");
        assert_eq!(sel.channel, Channel::Pinned);
    }

    #[tokio::test]
    async fn failure_rate_prevents_auto_promotion() {
        let state = RolloutState::new(&registry(), test_config(100));
        // Interleave failures so the success rate never reaches the 0.99
        // threshold: 3 successes / 5 probes = 0.6.
        state.record("deal_valid", "deal_valid_v2", "v2", true, 0.5).await;
        state.record("deal_valid", "deal_valid_v2", "v2", false, 0.5).await;
        state.record("deal_valid", "deal_valid_v2", "v2", true, 0.5).await;
        state.record("deal_valid", "deal_valid_v2", "v2", false, 0.5).await;
        state.record("deal_valid", "deal_valid_v2", "v2", true, 0.5).await;
        assert_eq!(state.select("deal_valid", 1).await.channel, Channel::Canary);
    }

    #[tokio::test]
    async fn rollback_switch_forces_pinned() {
        let state = RolloutState::new(&registry(), test_config(100));
        assert_eq!(state.select("deal_valid", 5).await.channel, Channel::Canary);

        let affected = state.rollback("deal_valid").await;
        assert_eq!(affected, vec!["deal_valid".to_string()]);
        let sel = state.select("deal_valid", 5).await;
        assert_eq!(sel.circuit, "deal_valid");
        assert_eq!(sel.channel, Channel::Rollback);

        // Auto-promotion must not fire while rolled back.
        for _ in 0..10 {
            state.record("deal_valid", "deal_valid_v2", "v2", true, 0.1).await;
        }
        assert_eq!(state.select("deal_valid", 5).await.channel, Channel::Rollback);

        // Re-arming restores the canary.
        assert!(state.clear_rollback("deal_valid").await);
        assert_eq!(state.select("deal_valid", 5).await.channel, Channel::Canary);
    }

    #[tokio::test]
    async fn rollback_all_affects_every_circuit() {
        let mut cfg = test_config(100);
        cfg.circuits.insert(
            "showdown_valid".to_string(),
            cfg.circuits["deal_valid"].clone(),
        );
        let state = RolloutState::new(&registry(), cfg);
        let affected = state.rollback("*").await;
        assert_eq!(affected.len(), 2);
    }

    #[test]
    fn invalid_config_falls_back_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{ not json").unwrap();
        let cfg = load_config(&path);
        assert!(cfg.circuits.is_empty());
    }

    #[test]
    fn status_reports_versions_and_roles() {
        // sync test wrapping an async call
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let state = RolloutState::new(&registry(), test_config(10));
            state.record("deal_valid", "deal_valid_v2", "v2", true, 0.25).await;
            let status = state.status().await;
            assert_eq!(status.circuits.len(), 1);
            let c = &status.circuits[0];
            assert_eq!(c.kind, "deal_valid");
            assert_eq!(c.versions.len(), 2);
            let canary = c.versions.iter().find(|v| v.role == "canary").unwrap();
            assert_eq!(canary.stat.probes, 1);
            assert_eq!(canary.stat.successes, 1);
        });
    }
}
