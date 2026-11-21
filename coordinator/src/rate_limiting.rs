// Issue #575: Session-scoped rate limiting and backpressure on nodes
// Bounded in-flight work with clear backpressure errors

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Error returned when rate limit is exceeded
#[derive(Debug, Clone)]
pub struct RateLimitExceededError {
    pub session_id: String,
    pub current_load: usize,
    pub max_capacity: usize,
    pub retry_after_ms: u64,
}

impl std::fmt::Display for RateLimitExceededError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rate limit exceeded for session {}: {}/{} capacity used. Retry after {}ms",
            self.session_id, self.current_load, self.max_capacity, self.retry_after_ms
        )
    }
}

impl std::error::Error for RateLimitExceededError {}

/// Session rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Maximum in-flight requests per session
    pub max_in_flight: usize,
    /// Maximum requests per second per session
    pub max_requests_per_second: u32,
    /// Time window for rate calculation (seconds)
    pub window_seconds: u32,
    /// Minimum retry delay when rate limited (milliseconds)
    pub min_retry_delay_ms: u64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 100,
            max_requests_per_second: 50,
            window_seconds: 1,
            min_retry_delay_ms: 100,
        }
    }
}

/// Per-session rate limiting state
#[derive(Debug)]
struct SessionLimitState {
    in_flight: usize,
    request_timestamps: Vec<Instant>,
    last_rate_limit: Option<Instant>,
}

impl SessionLimitState {
    fn new() -> Self {
        Self {
            in_flight: 0,
            request_timestamps: Vec::new(),
            last_rate_limit: None,
        }
    }

    /// Prune timestamps outside the rate window
    fn prune_old_timestamps(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        self.request_timestamps.retain(|&ts| ts > cutoff);
    }

    /// Calculate current request rate
    fn current_rate(&self, window: Duration) -> u32 {
        let cutoff = Instant::now() - window;
        self.request_timestamps
            .iter()
            .filter(|&&ts| ts > cutoff)
            .count() as u32
    }
}

/// Session-scoped rate limiter with backpressure
pub struct SessionRateLimiter {
    config: RateLimiterConfig,
    sessions: Arc<RwLock<HashMap<String, SessionLimitState>>>,
}

impl SessionRateLimiter {
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(RateLimiterConfig::default())
    }

    /// Check if request is allowed, apply backpressure if not
    pub async fn check_and_acquire(
        &self,
        session_id: &str,
    ) -> Result<RateLimitGuard, RateLimitExceededError> {
        let mut sessions = self.sessions.write().await;
        let state = sessions
            .entry(session_id.to_string())
            .or_insert_with(SessionLimitState::new);

        let window = Duration::from_secs(self.config.window_seconds as u64);
        state.prune_old_timestamps(window);

        // Check in-flight capacity
        if state.in_flight >= self.config.max_in_flight {
            return Err(RateLimitExceededError {
                session_id: session_id.to_string(),
                current_load: state.in_flight,
                max_capacity: self.config.max_in_flight,
                retry_after_ms: self.config.min_retry_delay_ms,
            });
        }

        // Check request rate
        let current_rate = state.current_rate(window);
        if current_rate >= self.config.max_requests_per_second {
            let retry_after = window.as_millis() as u64;
            state.last_rate_limit = Some(Instant::now());

            return Err(RateLimitExceededError {
                session_id: session_id.to_string(),
                current_load: current_rate as usize,
                max_capacity: self.config.max_requests_per_second as usize,
                retry_after_ms: retry_after,
            });
        }

        // Acquire slot
        state.in_flight += 1;
        state.request_timestamps.push(Instant::now());

        Ok(RateLimitGuard {
            session_id: session_id.to_string(),
            limiter: Arc::clone(&self.sessions),
        })
    }

    /// Get current load for a session
    pub async fn get_session_load(&self, session_id: &str) -> Option<SessionLoadInfo> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|state| {
            let window = Duration::from_secs(self.config.window_seconds as u64);
            SessionLoadInfo {
                in_flight: state.in_flight,
                max_in_flight: self.config.max_in_flight,
                current_rate: state.current_rate(window),
                max_rate: self.config.max_requests_per_second,
            }
        })
    }

    /// Remove session state (call when session ends)
    pub async fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }
}

/// RAII guard that releases in-flight slot on drop
pub struct RateLimitGuard {
    session_id: String,
    limiter: Arc<RwLock<HashMap<String, SessionLimitState>>>,
}

impl Drop for RateLimitGuard {
    fn drop(&mut self) {
        // Release in-flight slot asynchronously
        let session_id = self.session_id.clone();
        let limiter = Arc::clone(&self.limiter);

        tokio::spawn(async move {
            let mut sessions = limiter.write().await;
            if let Some(state) = sessions.get_mut(&session_id) {
                state.in_flight = state.in_flight.saturating_sub(1);
            }
        });
    }
}

/// Session load information
#[derive(Debug, Clone)]
pub struct SessionLoadInfo {
    pub in_flight: usize,
    pub max_in_flight: usize,
    pub current_rate: u32,
    pub max_rate: u32,
}

impl SessionLoadInfo {
    pub fn load_percentage(&self) -> f32 {
        (self.in_flight as f32 / self.max_in_flight as f32) * 100.0
    }

    pub fn rate_percentage(&self) -> f32 {
        (self.current_rate as f32 / self.max_rate as f32) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_within_capacity() {
        let limiter = SessionRateLimiter::with_defaults();
        let guard = limiter.check_and_acquire("session1").await;
        assert!(guard.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_rejects_over_capacity() {
        let config = RateLimiterConfig {
            max_in_flight: 2,
            max_requests_per_second: 100,
            window_seconds: 1,
            min_retry_delay_ms: 100,
        };
        let limiter = SessionRateLimiter::new(config);

        let _guard1 = limiter.check_and_acquire("session1").await.unwrap();
        let _guard2 = limiter.check_and_acquire("session1").await.unwrap();
        let result = limiter.check_and_acquire("session1").await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.current_load, 2);
        assert_eq!(err.max_capacity, 2);
    }

    #[tokio::test]
    async fn test_guard_releases_on_drop() {
        let config = RateLimiterConfig {
            max_in_flight: 1,
            max_requests_per_second: 100,
            window_seconds: 1,
            min_retry_delay_ms: 100,
        };
        let limiter = SessionRateLimiter::new(config);

        {
            let _guard = limiter.check_and_acquire("session1").await.unwrap();
            let result = limiter.check_and_acquire("session1").await;
            assert!(result.is_err());
        }

        // Give drop handler time to execute
        tokio::time::sleep(Duration::from_millis(10)).await;

        let guard = limiter.check_and_acquire("session1").await;
        assert!(guard.is_ok());
    }

    #[tokio::test]
    async fn test_get_session_load() {
        let limiter = SessionRateLimiter::with_defaults();
        let _guard = limiter.check_and_acquire("session1").await.unwrap();

        let load = limiter.get_session_load("session1").await.unwrap();
        assert_eq!(load.in_flight, 1);
        assert!(load.load_percentage() > 0.0);
    }
}
