/**
 * Per-Peer Rate Limiter
 *
 * Implements rate limiting for MPC node connections to protect against
 * compromised coordinator or malicious peers.
 */
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

/// Configuration for rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum messages per second from a single peer
    pub max_messages_per_second: u64,
    /// Maximum bytes per second from a single peer
    pub max_bytes_per_second: u64,
    /// Burst capacity (messages)
    pub burst_capacity: u64,
    /// Window size for rate calculation
    pub window_duration: Duration,
    /// Action when limit exceeded
    pub on_exceeded: ExceededAction,
    /// Cooldown period after disconnect
    pub cooldown_duration: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_messages_per_second: 100,
            max_bytes_per_second: 1024 * 1024, // 1 MB
            burst_capacity: 200,
            window_duration: Duration::from_secs(1),
            on_exceeded: ExceededAction::Disconnect,
            cooldown_duration: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExceededAction {
    Disconnect,
    Throttle,
}

/// State tracking for a single peer
#[derive(Debug)]
struct PeerState {
    /// Message timestamps in current window
    message_timestamps: Vec<Instant>,
    /// Bytes transferred in current window
    bytes_in_window: u64,
    /// Window start time
    window_start: Instant,
    /// Whether peer is currently in cooldown
    in_cooldown: bool,
    /// Cooldown expiry time
    cooldown_until: Option<Instant>,
}

/// Rate limiter for per-peer message tracking
pub struct PeerRateLimiter {
    config: RateLimitConfig,
    peers: Arc<RwLock<HashMap<String, PeerState>>>,
}

impl PeerRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a message from a peer is allowed
    pub async fn check_message(&self, peer_id: &str, message_size: usize) -> RateLimitResult {
        let mut peers = self.peers.write().await;
        let now = Instant::now();

        let state = peers.entry(peer_id.to_string()).or_insert_with(|| PeerState {
            message_timestamps: Vec::new(),
            bytes_in_window: 0,
            window_start: now,
            in_cooldown: false,
            cooldown_until: None,
        });

        // Check cooldown
        if state.in_cooldown {
            if let Some(cooldown_end) = state.cooldown_until {
                if now < cooldown_end {
                    return RateLimitResult::Rejected {
                        reason: "Peer in cooldown".to_string(),
                        retry_after: cooldown_end.duration_since(now),
                    };
                }
                state.in_cooldown = false;
                state.cooldown_until = None;
            }
        }

        // Reset window if needed
        if now.duration_since(state.window_start) > self.config.window_duration {
            state.message_timestamps.clear();
            state.bytes_in_window = 0;
            state.window_start = now;
        }

        // Check message rate
        if state.message_timestamps.len() as u64 >= self.config.max_messages_per_second {
            self.trigger_cooldown(state, peer_id);
            return RateLimitResult::Rejected {
                reason: format!(
                    "Message rate limit exceeded: {} msg/s (limit: {})",
                    state.message_timestamps.len(),
                    self.config.max_messages_per_second
                ),
                retry_after: self.config.window_duration,
            };
        }

        // Check byte rate
        if state.bytes_in_window + message_size as u64 > self.config.max_bytes_per_second {
            self.trigger_cooldown(state, peer_id);
            return RateLimitResult::Rejected {
                reason: format!(
                    "Byte rate limit exceeded: {} bytes (limit: {})",
                    state.bytes_in_window + message_size as u64,
                    self.config.max_bytes_per_second
                ),
                retry_after: self.config.window_duration,
            };
        }

        // Allow and record
        state.message_timestamps.push(now);
        state.bytes_in_window += message_size as u64;

        RateLimitResult::Allowed
    }

    fn trigger_cooldown(&self, state: &mut PeerState, _peer_id: &str) {
        if self.config.on_exceeded == ExceededAction::Disconnect {
            state.in_cooldown = true;
            state.cooldown_until = Some(Instant::now() + self.config.cooldown_duration);
        }
    }

    /// Get current metrics for a peer
    pub async fn get_peer_metrics(&self, peer_id: &str) -> PeerMetrics {
        let peers = self.peers.read().await;
        let state = peers.get(peer_id);

        match state {
            Some(s) => PeerMetrics {
                messages_in_window: s.message_timestamps.len() as u64,
                bytes_in_window: s.bytes_in_window,
                in_cooldown: s.in_cooldown,
            },
            None => PeerMetrics::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RateLimitResult {
    Allowed,
    Rejected { reason: String, retry_after: Duration },
}

#[derive(Debug, Clone, Default)]
pub struct PeerMetrics {
    pub messages_in_window: u64,
    pub bytes_in_window: u64,
    pub in_cooldown: bool,
}
