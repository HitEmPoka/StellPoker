//! Per-node resource limits guarding against exhaustion and DoS via session flooding.
//!
//! See issue #315. Three knobs are enforced per node:
//!   * max concurrent sessions  — reject new proof sessions once the node is saturated
//!   * max memory per session    — cap the address space of each co-noir subprocess
//!   * max CPU time per session  — cap CPU seconds (and wall-clock) of each subprocess
//!
//! Without these limits, an attacker (or a buggy coordinator) could flood a node
//! with sessions or trigger pathological proof generations and drive it out of
//! memory / CPU, taking the whole MPC committee offline.
//!
//! All limits are configurable via environment variables and default to sane values.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::session::{MpcSessionState, SessionStatus};

/// Resource ceilings applied per MPC node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Maximum number of concurrent (non-terminal) sessions a node will admit.
    /// `0` disables the limit.
    pub max_concurrent_sessions: usize,
    /// Maximum address space (virtual memory) a proof-generation subprocess may
    /// map, in bytes. `0` disables the limit. Enforced on Linux via `RLIMIT_AS`.
    pub max_session_memory_bytes: u64,
    /// Maximum CPU time a proof-generation subprocess may consume, in seconds.
    /// `0` disables the limit. Enforced on Unix via `RLIMIT_CPU`.
    pub max_session_cpu_seconds: u64,
    /// Wall-clock timeout for a single proof generation run, in seconds.
    /// `0` disables the limit. Guards against hangs that don't burn CPU.
    pub max_session_wall_seconds: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: 64,
            max_session_memory_bytes: 4096 * 1024 * 1024, // 4 GiB
            max_session_cpu_seconds: 300,                 // 5 min CPU
            max_session_wall_seconds: 600,                // 10 min wall clock
        }
    }
}

impl ResourceLimits {
    /// Build limits from environment variables, falling back to defaults.
    ///
    /// * `MAX_CONCURRENT_SESSIONS`
    /// * `SESSION_MAX_MEMORY_MB`
    /// * `SESSION_MAX_CPU_SECONDS`
    /// * `SESSION_MAX_WALL_SECONDS`
    pub fn from_env() -> Self {
        let d = Self::default();
        let memory_mb_default = d.max_session_memory_bytes / (1024 * 1024);
        Self {
            max_concurrent_sessions: parse_env(
                "MAX_CONCURRENT_SESSIONS",
                d.max_concurrent_sessions,
            ),
            max_session_memory_bytes: parse_env::<u64>("SESSION_MAX_MEMORY_MB", memory_mb_default)
                .saturating_mul(1024 * 1024),
            max_session_cpu_seconds: parse_env(
                "SESSION_MAX_CPU_SECONDS",
                d.max_session_cpu_seconds,
            ),
            max_session_wall_seconds: parse_env(
                "SESSION_MAX_WALL_SECONDS",
                d.max_session_wall_seconds,
            ),
        }
    }

    /// Apply per-subprocess limits to a co-noir command before it is spawned.
    ///
    /// Sets `kill_on_drop` on all platforms (so a wall-clock timeout actually
    /// reaps the child), plus `RLIMIT_CPU`/`RLIMIT_AS` via a `pre_exec` hook on
    /// Unix. The rlimit is applied in the forked child before `exec`, so it
    /// bounds the co-noir process itself, not this node.
    pub fn apply_to_command(&self, cmd: &mut tokio::process::Command) {
        cmd.kill_on_drop(true);

        #[cfg(unix)]
        {
            let cpu = self.max_session_cpu_seconds;
            let mem = self.max_session_memory_bytes;
            // SAFETY: the closure runs in the forked child between fork and exec.
            // It only calls async-signal-safe `setrlimit` and touches no shared
            // state, which is sound in that context.
            unsafe {
                cmd.pre_exec(move || {
                    if cpu > 0 {
                        let rlim = libc::rlimit {
                            rlim_cur: cpu as libc::rlim_t,
                            rlim_max: cpu as libc::rlim_t,
                        };
                        libc::setrlimit(libc::RLIMIT_CPU, &rlim);
                    }
                    // RLIMIT_AS (address-space cap) is reliable on Linux; macOS
                    // does not enforce it meaningfully, so restrict to Linux.
                    #[cfg(target_os = "linux")]
                    if mem > 0 {
                        let rlim = libc::rlimit {
                            rlim_cur: mem as libc::rlim_t,
                            rlim_max: mem as libc::rlim_t,
                        };
                        libc::setrlimit(libc::RLIMIT_AS, &rlim);
                    }
                    let _ = mem; // unused on non-Linux unix targets
                    Ok(())
                });
            }
        }
    }
}

/// Generic `KEY=value` env parse with a default fallback.
fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// A session is "active" until it reaches a terminal state (complete or failed).
/// Only active sessions count against the concurrency limit.
pub fn is_active(status: &SessionStatus) -> bool {
    !matches!(status, SessionStatus::Complete | SessionStatus::Failed(_))
}

/// Count sessions that are still occupying node resources.
pub async fn count_active_sessions(
    sessions: &HashMap<String, Arc<RwLock<MpcSessionState>>>,
) -> usize {
    let mut count = 0;
    for lock in sessions.values() {
        if is_active(&lock.read().await.status) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn session_with_status(id: &str, status: SessionStatus) -> Arc<RwLock<MpcSessionState>> {
        let mut s = MpcSessionState::new(id.to_string(), "deal".to_string(), PathBuf::from("/tmp"));
        s.status = status;
        Arc::new(RwLock::new(s))
    }

    #[test]
    fn defaults_are_sane() {
        let d = ResourceLimits::default();
        assert_eq!(d.max_concurrent_sessions, 64);
        assert_eq!(d.max_session_memory_bytes, 4096 * 1024 * 1024);
        assert_eq!(d.max_session_cpu_seconds, 300);
        assert_eq!(d.max_session_wall_seconds, 600);
    }

    #[test]
    fn from_env_uses_defaults_when_unset() {
        // These vars are not set in the test environment.
        let l = ResourceLimits::from_env();
        assert_eq!(l, ResourceLimits::default());
    }

    #[test]
    fn parse_env_falls_back_on_garbage() {
        assert_eq!(parse_env::<usize>("NONEXISTENT_LIMIT_VAR_315", 7), 7);
    }

    #[test]
    fn is_active_classifies_terminal_states() {
        assert!(is_active(&SessionStatus::SharesReceived));
        assert!(is_active(&SessionStatus::WitnessGenerating));
        assert!(is_active(&SessionStatus::ProofGenerating));
        assert!(!is_active(&SessionStatus::Complete));
        assert!(!is_active(&SessionStatus::Failed("boom".to_string())));
    }

    #[tokio::test]
    async fn count_active_sessions_ignores_terminal() {
        let mut map: HashMap<String, Arc<RwLock<MpcSessionState>>> = HashMap::new();
        map.insert(
            "a".into(),
            session_with_status("a", SessionStatus::SharesReceived),
        );
        map.insert(
            "b".into(),
            session_with_status("b", SessionStatus::ProofGenerating),
        );
        map.insert(
            "c".into(),
            session_with_status("c", SessionStatus::Complete),
        );
        map.insert(
            "d".into(),
            session_with_status("d", SessionStatus::Failed("x".into())),
        );

        assert_eq!(count_active_sessions(&map).await, 2);
    }
}
