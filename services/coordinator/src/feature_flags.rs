//! Feature flag service for gradual rollouts.
//!
//! Flags are loaded from environment variables at startup:
//!   `FEATURE_FLAG_<UPPER_NAME>=1|true|yes|on`
//!
//! Each flag can be scoped at three levels (most-specific wins):
//!   - Global        — applies everywhere unless overridden
//!   - Per-table     — key: `<flag>.table.<table_id>`
//!   - Per-player    — key: `<flag>.player.<address>`
//!
//! Runtime overrides are supported via the admin API endpoints:
//!   `GET  /api/flags`         — list all current flag values
//!   `POST /api/flags/:key`    — set a flag value (body: `{"enabled": bool}`)
//!
//! # Supported flags
//!
//! | Environment variable              | Flag key           | Purpose                         |
//! |-----------------------------------|--------------------|---------------------------------|
//! | `FEATURE_FLAG_NEW_CIRCUITS`       | `new_circuits`     | Enable experimental circuits    |
//! | `FEATURE_FLAG_CONTRACT_UPGRADE`   | `contract_upgrade` | Gate new contract function calls|
//! | `FEATURE_FLAG_EXPERIMENTAL_UI`    | `experimental_ui`  | Signal UI to use new components |
//! | `FEATURE_FLAG_CHAT_ENABLED`       | `chat_enabled`     | Enable in-table chat            |
//! | `FEATURE_FLAG_SOLO_MODE`          | `solo_mode`        | Allow solo/bot table creation   |

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Canonical key names for every supported feature flag.
pub mod keys {
    pub const NEW_CIRCUITS: &str = "new_circuits";
    pub const CONTRACT_UPGRADE: &str = "contract_upgrade";
    pub const EXPERIMENTAL_UI: &str = "experimental_ui";
    pub const CHAT_ENABLED: &str = "chat_enabled";
    pub const SOLO_MODE: &str = "solo_mode";

    /// All known global flag keys, in a stable order.
    pub const ALL: &[&str] = &[
        NEW_CIRCUITS,
        CONTRACT_UPGRADE,
        EXPERIMENTAL_UI,
        CHAT_ENABLED,
        SOLO_MODE,
    ];
}

/// Scope variants determine the lookup key used when querying a flag.
///
/// Resolution order (most-specific wins):
/// `PerPlayer` > `PerTable` > `Global`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagScope {
    /// Apply to all tables and players unless a narrower scope overrides it.
    Global,
    /// Apply to a specific table (key suffix: `.table.<id>`).
    PerTable(u32),
    /// Apply to a specific player address (key suffix: `.player.<address>`).
    PerPlayer(String),
}

/// Thread-safe store of feature flags backed by a `HashMap<String, bool>`.
///
/// Cloning is cheap — all clones share the same underlying `Arc<RwLock<…>>`.
#[derive(Clone)]
pub struct FeatureFlagStore {
    inner: Arc<RwLock<HashMap<String, bool>>>,
}

impl FeatureFlagStore {
    /// Build an empty store with all known flags defaulting to `false`.
    pub fn new() -> Self {
        let mut map = HashMap::new();
        for &key in keys::ALL {
            map.insert(key.to_string(), false);
        }
        Self {
            inner: Arc::new(RwLock::new(map)),
        }
    }

    /// Build a store populated from `FEATURE_FLAG_<UPPER_KEY>` environment variables.
    ///
    /// Unknown env vars with the prefix are ignored; known flags not present in the
    /// environment default to `false`.
    pub fn from_env() -> Self {
        let store = Self::new();
        let mut map = HashMap::new();

        // Seed defaults.
        for &key in keys::ALL {
            map.insert(key.to_string(), false);
        }

        // Override from env.
        for &key in keys::ALL {
            let env_key = format!("FEATURE_FLAG_{}", key.to_uppercase().replace('-', "_"));
            if let Ok(val) = std::env::var(&env_key) {
                let enabled = parse_bool_env(&val);
                map.insert(key.to_string(), enabled);
                if enabled {
                    tracing::info!("Feature flag '{}' enabled via {}", key, env_key);
                }
            }
        }

        // Also scan for per-table / per-player overrides:
        // e.g. FEATURE_FLAG_SOLO_MODE_TABLE_3=1  → key "solo_mode.table.3"
        //      FEATURE_FLAG_CHAT_ENABLED_PLAYER_GABC...=0 → key "chat_enabled.player.GABC..."
        for (env_key, env_val) in std::env::vars() {
            let Some(rest) = env_key.strip_prefix("FEATURE_FLAG_") else {
                continue;
            };
            // Try to match pattern: <FLAG_UPPER>_TABLE_<id> or <FLAG_UPPER>_PLAYER_<addr>
            for &flag_key in keys::ALL {
                let flag_upper = flag_key.to_uppercase().replace('-', "_");
                if let Some(scope_part) = rest.strip_prefix(&flag_upper) {
                    let scoped_key = if let Some(table_id) = scope_part.strip_prefix("_TABLE_") {
                        Some(format!("{}.table.{}", flag_key, table_id.to_lowercase()))
                    } else if let Some(player) = scope_part.strip_prefix("_PLAYER_") {
                        Some(format!("{}.player.{}", flag_key, player))
                    } else {
                        None
                    };
                    if let Some(k) = scoped_key {
                        let enabled = parse_bool_env(&env_val);
                        tracing::info!(
                            "Feature flag scoped override '{}' = {} (from {})",
                            k,
                            enabled,
                            env_key
                        );
                        map.insert(k, enabled);
                    }
                }
            }
        }

        *store.inner.blocking_write() = map;
        store
    }

    /// Returns `true` if the flag is enabled for the given scope.
    ///
    /// Resolution order:
    /// 1. Per-player key (if `scope` is `PerPlayer`)
    /// 2. Per-table key  (if `scope` is `PerTable` or `PerPlayer` — player implies a table)
    /// 3. Global key
    pub async fn is_enabled(&self, flag: &str, scope: &FlagScope) -> bool {
        let map = self.inner.read().await;
        match scope {
            FlagScope::PerPlayer(address) => {
                let player_key = format!("{}.player.{}", flag, address);
                if let Some(&v) = map.get(&player_key) {
                    return v;
                }
                // Fall through to global.
                map.get(flag).copied().unwrap_or(false)
            }
            FlagScope::PerTable(table_id) => {
                let table_key = format!("{}.table.{}", flag, table_id);
                if let Some(&v) = map.get(&table_key) {
                    return v;
                }
                map.get(flag).copied().unwrap_or(false)
            }
            FlagScope::Global => map.get(flag).copied().unwrap_or(false),
        }
    }

    /// Synchronous variant for use in non-async contexts (e.g. startup checks).
    pub fn is_enabled_sync(&self, flag: &str) -> bool {
        let map = self.inner.blocking_read();
        map.get(flag).copied().unwrap_or(false)
    }

    /// Override (or insert) a flag value at runtime.
    pub async fn set_flag(&self, key: &str, enabled: bool) {
        let mut map = self.inner.write().await;
        map.insert(key.to_string(), enabled);
        tracing::info!("Feature flag '{}' set to {} via runtime API", key, enabled);
    }

    /// Return a snapshot of all current flag values (global + any scoped overrides).
    pub async fn snapshot(&self) -> HashMap<String, bool> {
        self.inner.read().await.clone()
    }
}

impl Default for FeatureFlagStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse common truthy / falsy env-var strings.
fn parse_bool_env(val: &str) -> bool {
    matches!(
        val.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a store with explicit initial values without touching env.
    fn store_with(pairs: &[(&str, bool)]) -> FeatureFlagStore {
        let store = FeatureFlagStore::new();
        let mut map = store.inner.blocking_write();
        for &(k, v) in pairs {
            map.insert(k.to_string(), v);
        }
        drop(map);
        store
    }

    #[test]
    fn test_default_store_all_flags_false() {
        let store = FeatureFlagStore::new();
        let map = store.inner.blocking_read();
        for &key in keys::ALL {
            assert_eq!(
                map.get(key).copied(),
                Some(false),
                "Flag '{}' should default to false",
                key
            );
        }
    }

    #[test]
    fn test_parse_bool_env_truthy() {
        for &v in &["1", "true", "True", "TRUE", "yes", "YES", "on", "ON"] {
            assert!(parse_bool_env(v), "Expected true for '{}'", v);
        }
    }

    #[test]
    fn test_parse_bool_env_falsy() {
        for &v in &["0", "false", "False", "FALSE", "no", "NO", "off", "OFF", ""] {
            assert!(!parse_bool_env(v), "Expected false for '{}'", v);
        }
    }

    #[tokio::test]
    async fn test_is_enabled_global_false_by_default() {
        let store = FeatureFlagStore::new();
        assert!(!store.is_enabled(keys::SOLO_MODE, &FlagScope::Global).await);
    }

    #[tokio::test]
    async fn test_is_enabled_global_true_when_set() {
        let store = store_with(&[(keys::SOLO_MODE, true)]);
        assert!(store.is_enabled(keys::SOLO_MODE, &FlagScope::Global).await);
    }

    #[tokio::test]
    async fn test_is_enabled_unknown_flag_returns_false() {
        let store = FeatureFlagStore::new();
        assert!(!store.is_enabled("totally_unknown_flag", &FlagScope::Global).await);
    }

    #[tokio::test]
    async fn test_per_table_override_wins_over_global() {
        let store = store_with(&[
            (keys::CHAT_ENABLED, false),             // global: off
            ("chat_enabled.table.7", true),          // table 7: on
        ]);
        // Table 7 sees the flag as enabled.
        assert!(store.is_enabled(keys::CHAT_ENABLED, &FlagScope::PerTable(7)).await);
        // Table 8 falls back to global (false).
        assert!(!store.is_enabled(keys::CHAT_ENABLED, &FlagScope::PerTable(8)).await);
    }

    #[tokio::test]
    async fn test_per_player_override_wins_over_global() {
        let player = "GBADXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
        let store = store_with(&[
            (keys::EXPERIMENTAL_UI, false),
            (&format!("experimental_ui.player.{}", player), true),
        ]);
        assert!(
            store
                .is_enabled(keys::EXPERIMENTAL_UI, &FlagScope::PerPlayer(player.to_string()))
                .await
        );
        // A different player falls back to global.
        assert!(
            !store
                .is_enabled(
                    keys::EXPERIMENTAL_UI,
                    &FlagScope::PerPlayer("GCOTHER".to_string())
                )
                .await
        );
    }

    #[tokio::test]
    async fn test_set_flag_mutates_store() {
        let store = FeatureFlagStore::new();
        assert!(!store.is_enabled(keys::NEW_CIRCUITS, &FlagScope::Global).await);

        store.set_flag(keys::NEW_CIRCUITS, true).await;
        assert!(store.is_enabled(keys::NEW_CIRCUITS, &FlagScope::Global).await);

        store.set_flag(keys::NEW_CIRCUITS, false).await;
        assert!(!store.is_enabled(keys::NEW_CIRCUITS, &FlagScope::Global).await);
    }

    #[tokio::test]
    async fn test_set_flag_arbitrary_key() {
        let store = FeatureFlagStore::new();
        store.set_flag("solo_mode.table.42", true).await;
        assert!(store.is_enabled(keys::SOLO_MODE, &FlagScope::PerTable(42)).await);
        // Global still false.
        assert!(!store.is_enabled(keys::SOLO_MODE, &FlagScope::Global).await);
    }

    #[tokio::test]
    async fn test_snapshot_returns_current_values() {
        let store = store_with(&[(keys::SOLO_MODE, true)]);
        let snap = store.snapshot().await;
        assert_eq!(snap.get(keys::SOLO_MODE), Some(&true));
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let store = FeatureFlagStore::new();
        let clone = store.clone();

        store.set_flag(keys::SOLO_MODE, true).await;
        // The clone should see the same value because they share the Arc.
        assert!(clone.is_enabled(keys::SOLO_MODE, &FlagScope::Global).await);
    }

    /// Integration tests that require environment variable manipulation or
    /// live network access are skipped in CI via `#[ignore]`.
    #[test]
    #[ignore = "reads/writes FEATURE_FLAG_* env vars; run manually"]
    fn test_from_env_reads_environment() {
        // Set a known env var, rebuild store, confirm it's picked up.
        std::env::set_var("FEATURE_FLAG_SOLO_MODE", "1");
        let store = FeatureFlagStore::from_env();
        assert!(store.is_enabled_sync(keys::SOLO_MODE));
        std::env::remove_var("FEATURE_FLAG_SOLO_MODE");
    }
}
