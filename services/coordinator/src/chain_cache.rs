//! Multi-wallet batch state reads with TTL caching layer.
//!
//! Implements issue #600: Batch resolver + TTL cache for chain reads to reduce
//! lobby load times by avoiding redundant Soroban RPC calls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// Cached table state with expiration timestamp.
#[derive(Clone, Debug)]
struct CachedTableState {
    data: String,
    cached_at: Instant,
}

/// Configuration for the chain state cache.
#[derive(Clone, Debug)]
pub struct ChainCacheConfig {
    /// Time-to-live for cached entries (default: 5 seconds)
    pub ttl: Duration,
    /// Batch window duration for aggregating queries (default: 50 milliseconds)
    pub batch_window: Duration,
}

impl Default for ChainCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(
                std::env::var("CHAIN_CACHE_TTL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5),
            ),
            batch_window: Duration::from_millis(
                std::env::var("CHAIN_CACHE_BATCH_WINDOW_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(50),
            ),
        }
    }
}

/// Pending batch request for a table ID.
struct PendingRequest {
    table_id: u32,
    response_tx: tokio::sync::oneshot::Sender<Result<String, String>>,
}

/// Chain state cache with batch resolution and TTL expiration.
pub struct ChainCache {
    cache: Arc<RwLock<HashMap<u32, CachedTableState>>>,
    config: ChainCacheConfig,
    pending_batch: Arc<Mutex<Vec<PendingRequest>>>,
    batch_processor_running: Arc<Mutex<bool>>,
}

impl ChainCache {
    /// Create a new chain cache with default configuration.
    pub fn new() -> Self {
        Self::with_config(ChainCacheConfig::default())
    }

    /// Create a new chain cache with custom configuration.
    pub fn with_config(config: ChainCacheConfig) -> Self {
        tracing::info!(
            "Chain cache initialized: TTL={}s, batch_window={}ms",
            config.ttl.as_secs(),
            config.batch_window.as_millis()
        );
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
            pending_batch: Arc::new(Mutex::new(Vec::new())),
            batch_processor_running: Arc::new(Mutex::new(false)),
        }
    }

    /// Get table state, using cache if available and not expired.
    /// If not cached, queue for batch resolution.
    pub async fn get_table_state<F, Fut>(
        &self,
        table_id: u32,
        fetch_fn: F,
    ) -> Result<String, String>
    where
        F: FnOnce(Vec<u32>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = HashMap<u32, Result<String, String>>> + Send,
    {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&table_id) {
                if cached.cached_at.elapsed() < self.config.ttl {
                    tracing::debug!("Cache hit for table {}", table_id);
                    return Ok(cached.data.clone());
                }
                tracing::debug!("Cache expired for table {}", table_id);
            }
        }

        // Cache miss - add to pending batch
        let (tx, rx) = tokio::sync::oneshot::channel();

        {
            let mut pending = self.pending_batch.lock().await;
            pending.push(PendingRequest {
                table_id,
                response_tx: tx,
            });
        }

        // Spawn batch processor if not already running
        self.spawn_batch_processor(fetch_fn).await;

        // Wait for batch result
        rx.await
            .map_err(|_| "batch processor dropped".to_string())?
    }

    /// Spawn batch processor task if not already running.
    async fn spawn_batch_processor<F, Fut>(&self, fetch_fn: F)
    where
        F: FnOnce(Vec<u32>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = HashMap<u32, Result<String, String>>> + Send,
    {
        let mut running = self.batch_processor_running.lock().await;
        if *running {
            return;
        }
        *running = true;

        let pending_batch = Arc::clone(&self.pending_batch);
        let cache = Arc::clone(&self.cache);
        let batch_window = self.config.batch_window;
        let processor_running = Arc::clone(&self.batch_processor_running);

        tokio::spawn(async move {
            // Wait for batch window to collect requests
            tokio::time::sleep(batch_window).await;

            // Collect all pending requests
            let requests = {
                let mut pending = pending_batch.lock().await;
                std::mem::take(&mut *pending)
            };

            if requests.is_empty() {
                *processor_running.lock().await = false;
                return;
            }

            // Extract unique table IDs
            let table_ids: Vec<u32> = requests
                .iter()
                .map(|req| req.table_id)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            tracing::debug!(
                "Processing batch of {} requests for {} unique tables",
                requests.len(),
                table_ids.len()
            );

            // Fetch all states in batch
            let results = fetch_fn(table_ids).await;

            // Update cache and respond to requests
            let now = Instant::now();
            {
                let mut cache_guard = cache.write().await;
                for req in requests {
                    if let Some(result) = results.get(&req.table_id) {
                        if let Ok(data) = result {
                            // Cache successful results
                            cache_guard.insert(
                                req.table_id,
                                CachedTableState {
                                    data: data.clone(),
                                    cached_at: now,
                                },
                            );
                        }
                        // Send result to waiter
                        let _ = req.response_tx.send(result.clone());
                    } else {
                        // No result for this table ID (shouldn't happen)
                        let _ = req
                            .response_tx
                            .send(Err(format!("No result for table {}", req.table_id)));
                    }
                }
            }

            *processor_running.lock().await = false;
        });
    }

    /// Invalidate cache entry for a specific table.
    pub async fn invalidate(&self, table_id: u32) {
        let mut cache = self.cache.write().await;
        if cache.remove(&table_id).is_some() {
            tracing::debug!("Cache invalidated for table {}", table_id);
        }
    }

    /// Invalidate all cache entries.
    pub async fn invalidate_all(&self) {
        let mut cache = self.cache.write().await;
        let count = cache.len();
        cache.clear();
        tracing::info!("Cache invalidated: {} entries cleared", count);
    }

    /// Get cache statistics for monitoring.
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let now = Instant::now();
        let mut expired = 0usize;
        let mut valid = 0usize;

        for entry in cache.values() {
            if now.duration_since(entry.cached_at) >= self.config.ttl {
                expired += 1;
            } else {
                valid += 1;
            }
        }

        CacheStats {
            total_entries: cache.len(),
            valid_entries: valid,
            expired_entries: expired,
            ttl_seconds: self.config.ttl.as_secs(),
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub expired_entries: usize,
    pub ttl_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_hit_returns_cached_data() {
        let cache = ChainCache::new();

        // Populate cache
        {
            let mut guard = cache.cache.write().await;
            guard.insert(
                1,
                CachedTableState {
                    data: "test_data".to_string(),
                    cached_at: Instant::now(),
                },
            );
        }

        // Should return cached data without calling fetch_fn
        let result = cache
            .get_table_state(1, |_| async { HashMap::new() })
            .await
            .unwrap();
        assert_eq!(result, "test_data");
    }

    #[tokio::test]
    async fn cache_miss_triggers_fetch() {
        let cache = ChainCache::with_config(ChainCacheConfig {
            ttl: Duration::from_secs(5),
            batch_window: Duration::from_millis(10),
        });

        let result = cache
            .get_table_state(1, |table_ids| async move {
                let mut results = HashMap::new();
                for id in table_ids {
                    results.insert(id, Ok(format!("data_{}", id)));
                }
                results
            })
            .await
            .unwrap();

        assert_eq!(result, "data_1");

        // Verify it's now cached
        let cached_result = cache
            .get_table_state(1, |_| async { HashMap::new() })
            .await
            .unwrap();
        assert_eq!(cached_result, "data_1");
    }

    #[tokio::test]
    async fn invalidate_removes_cache_entry() {
        let cache = ChainCache::new();

        // Populate cache
        {
            let mut guard = cache.cache.write().await;
            guard.insert(
                1,
                CachedTableState {
                    data: "test_data".to_string(),
                    cached_at: Instant::now(),
                },
            );
        }

        cache.invalidate(1).await;

        // Cache should be empty
        let cache_guard = cache.cache.read().await;
        assert!(cache_guard.get(&1).is_none());
    }

    #[tokio::test]
    async fn expired_entry_triggers_refetch() {
        let cache = ChainCache::with_config(ChainCacheConfig {
            ttl: Duration::from_millis(50),
            batch_window: Duration::from_millis(10),
        });

        // Populate with expired entry
        {
            let mut guard = cache.cache.write().await;
            guard.insert(
                1,
                CachedTableState {
                    data: "old_data".to_string(),
                    cached_at: Instant::now() - Duration::from_millis(100),
                },
            );
        }

        // Should trigger fetch due to expiration
        let result = cache
            .get_table_state(1, |table_ids| async move {
                let mut results = HashMap::new();
                for id in table_ids {
                    results.insert(id, Ok("fresh_data".to_string()));
                }
                results
            })
            .await
            .unwrap();

        assert_eq!(result, "fresh_data");
    }

    #[tokio::test]
    async fn stats_reports_cache_state() {
        let cache = ChainCache::new();

        // Populate with valid and expired entries
        {
            let mut guard = cache.cache.write().await;
            guard.insert(
                1,
                CachedTableState {
                    data: "valid".to_string(),
                    cached_at: Instant::now(),
                },
            );
            guard.insert(
                2,
                CachedTableState {
                    data: "expired".to_string(),
                    cached_at: Instant::now() - Duration::from_secs(100),
                },
            );
        }

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.valid_entries, 1);
        assert_eq!(stats.expired_entries, 1);
    }
}
