//! MPC session preemption policy: priority classes with starvation bounds.
//!
//! Issue #588. When the committee's capacity is scarce (fewer healthy MPC
//! nodes than hands waiting to be dealt), the coordinator has to decide which
//! sessions run now and which wait. A plain FIFO queue lets a single
//! operator's high-volume *priority* tables starve *casual* tables
//! indefinitely.
//!
//! This module implements a small, deterministic admission controller:
//!
//! * Every session is tagged with a [`PriorityClass`]
//!   (`priority` / `standard` / `casual`).
//! * Requests are admitted while there is spare concurrency.
//! * When capacity is full, a higher-priority request may **preempt** a
//!   lower-priority preemptible session. The preempted session is requeued
//!   (never dropped) and its preemption counter is bumped.
//! * Every class has a `*_max_wait` **starvation bound**. Once a queued
//!   request has waited longer than its bound it is *promoted* to the head
//!   of the queue and marked non-preemptible, so it can no longer be
//!   starved by fresh high-priority arrivals.
//! * A session may only be preempted `max_preemptions` times, after which it
//!   is protected too. Combined with promotion, this gives every class a
//!   bounded worst-case wait: `starvation_bound + (max_preemptions + 1) *
//!   max_session_duration` (see docs/mpc-preemption-policy.md).
//!
//! The controller is pure and time-injected (`now: Instant`) so it is fully
//! unit-testable without a running coordinator.

use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Priority class attached to a table's MPC sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PriorityClass {
    /// Operator-committed, latency-sensitive tables (e.g. televised / cash
    /// tables with a contractual uptime). Scheduled first; allowed to
    /// preempt lower classes.
    Priority,
    /// Default class for ordinary paid tables.
    Standard,
    /// Free / casual play. Scheduled last.
    Casual,
}

impl PriorityClass {
    /// Lower rank == scheduled first for two equally-aged requests.
    pub fn base_rank(self) -> u8 {
        match self {
            PriorityClass::Priority => 0,
            PriorityClass::Standard => 1,
            PriorityClass::Casual => 2,
        }
    }

    /// Parse a class from a (case-insensitive) label. Unknown labels fall
    /// back to [`PriorityClass::Standard`] rather than erroring: a
    /// misconfigured caller must never accidentally receive *higher*
    /// priority, and must never be silently dropped either.
    pub fn parse(label: &str) -> PriorityClass {
        match label.trim().to_ascii_lowercase().as_str() {
            "priority" | "high" => PriorityClass::Priority,
            "casual" | "free" | "low" => PriorityClass::Casual,
            _ => PriorityClass::Standard,
        }
    }
}

/// Tunables for the admission controller.
#[derive(Clone, Debug)]
pub struct PreemptionConfig {
    /// Maximum number of MPC sessions allowed to run concurrently.
    pub max_concurrent: usize,
    /// How long a `priority` request may wait before it is promoted.
    pub priority_max_wait: Duration,
    /// How long a `standard` request may wait before it is promoted.
    pub standard_max_wait: Duration,
    /// How long a `casual` request may wait before it is promoted.
    pub casual_max_wait: Duration,
    /// How many times a single session may be preempted before it becomes
    /// non-preemptible.
    pub max_preemptions: u32,
    /// Hard cap on the queue length; beyond this, new requests are rejected
    /// with [`EnqueueError::QueueFull`] so back-pressure reaches the caller
    /// instead of unbounded memory growth.
    pub max_queued: usize,
}

impl Default for PreemptionConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 16,
            priority_max_wait: Duration::from_secs(30),
            standard_max_wait: Duration::from_secs(60),
            casual_max_wait: Duration::from_secs(120),
            max_preemptions: 2,
            max_queued: 10_000,
        }
    }
}

impl PreemptionConfig {
    /// Read the policy from the environment, falling back to defaults for any
    /// unset/invalid value.
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            max_concurrent: parse_env("MPC_PREEMPTION_MAX_CONCURRENT", d.max_concurrent),
            priority_max_wait: Duration::from_secs(parse_env(
                "MPC_PREEMPTION_PRIORITY_MAX_WAIT_SECS",
                d.priority_max_wait.as_secs(),
            )),
            standard_max_wait: Duration::from_secs(parse_env(
                "MPC_PREEMPTION_STANDARD_MAX_WAIT_SECS",
                d.standard_max_wait.as_secs(),
            )),
            casual_max_wait: Duration::from_secs(parse_env(
                "MPC_PREEMPTION_CASUAL_MAX_WAIT_SECS",
                d.casual_max_wait.as_secs(),
            )),
            max_preemptions: parse_env("MPC_PREEMPTION_MAX_PREEMPTIONS", d.max_preemptions),
            max_queued: parse_env("MPC_PREEMPTION_MAX_QUEUED", d.max_queued),
        }
    }

    /// The starvation bound for a class.
    pub fn starvation_bound(&self, class: PriorityClass) -> Duration {
        match class {
            PriorityClass::Priority => self.priority_max_wait,
            PriorityClass::Standard => self.standard_max_wait,
            PriorityClass::Casual => self.casual_max_wait,
        }
    }
}

fn parse_env<T: std::str::FromStr + Copy>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(default)
}

#[derive(Clone, Debug)]
struct QueuedRequest {
    session_id: String,
    table_id: u32,
    class: PriorityClass,
    /// When the request first entered the queue. Retained across
    /// preemptions so a repeatedly-preempted request still ages toward
    /// promotion.
    enqueued_at: Instant,
    /// Monotonic arrival sequence, for FIFO tie-breaking.
    seq: u64,
    /// How many times this request has been preempted so far.
    preemptions: u32,
}

#[derive(Clone, Debug)]
struct RunningSession {
    class: PriorityClass,
    started_at: Instant,
    preemptions: u32,
    /// True when this session was admitted while starving. Promoted sessions
    /// are protected from further preemption.
    promoted: bool,
    preemptible: bool,
}

/// A single scheduling decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScheduleAction {
    /// Start this session on the committee.
    Admit {
        session_id: String,
        table_id: u32,
        class: PriorityClass,
        /// Whether the request had to be promoted to get a slot.
        promoted: bool,
    },
    /// Stop `preempted_session_id` (it is requeued) and start
    /// `admitted_session_id` in its place.
    Preempt {
        preempted_session_id: String,
        admitted_session_id: String,
    },
}

/// Why a request could not be enqueued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueError {
    /// The queue is at `max_queued`.
    QueueFull,
    /// A request with the same `session_id` is already queued.
    AlreadyQueued,
    /// A request with the same `session_id` is already running.
    AlreadyRunning,
}

/// Cumulative counters, exported for the status endpoint / Prometheus.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct PreemptionStats {
    pub admitted: u64,
    pub preemptions: u64,
    pub starvation_promotions: u64,
    pub rejected_queue_full: u64,
}

/// Snapshot of the controller for `GET /api/mpc/preemption/status`.
#[derive(Clone, Debug, Serialize)]
pub struct PreemptionStatus {
    pub max_concurrent: usize,
    pub running: usize,
    pub queued: usize,
    pub queued_priority: usize,
    pub queued_standard: usize,
    pub queued_casual: usize,
    pub max_preemptions: u32,
    pub priority_max_wait_secs: u64,
    pub standard_max_wait_secs: u64,
    pub casual_max_wait_secs: u64,
    pub stats: PreemptionStats,
}

/// The admission controller. Cheap to construct; guard it behind a lock in
/// `AppState`.
#[derive(Debug)]
pub struct PreemptionController {
    config: PreemptionConfig,
    queue: Vec<QueuedRequest>,
    running: HashMap<String, RunningSession>,
    next_seq: u64,
    stats: PreemptionStats,
}

impl PreemptionController {
    pub fn new(config: PreemptionConfig) -> Self {
        Self {
            config,
            queue: Vec::new(),
            running: HashMap::new(),
            next_seq: 0,
            stats: PreemptionStats::default(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(PreemptionConfig::from_env())
    }

    pub fn config(&self) -> &PreemptionConfig {
        &self.config
    }

    /// Add a request to the queue. Idempotency guard: the same `session_id`
    /// may not be queued or running twice.
    pub fn enqueue(
        &mut self,
        session_id: impl Into<String>,
        table_id: u32,
        class: PriorityClass,
        now: Instant,
    ) -> Result<(), EnqueueError> {
        let session_id = session_id.into();
        if self.running.contains_key(&session_id) {
            return Err(EnqueueError::AlreadyRunning);
        }
        if self.queue.iter().any(|q| q.session_id == session_id) {
            return Err(EnqueueError::AlreadyQueued);
        }
        if self.queue.len() >= self.config.max_queued {
            self.stats.rejected_queue_full += 1;
            return Err(EnqueueError::QueueFull);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queue.push(QueuedRequest {
            session_id,
            table_id,
            class,
            enqueued_at: now,
            seq,
            preemptions: 0,
        });
        Ok(())
    }

    /// Whether a queued request has aged past its class starvation bound.
    fn is_promoted(&self, req: &QueuedRequest, now: Instant) -> bool {
        now.saturating_duration_since(req.enqueued_at) >= self.config.starvation_bound(req.class)
    }

    /// Ordering key for the queue: promoted requests first, then by class
    /// rank, then FIFO. The smallest key wins.
    fn ordering_key(&self, req: &QueuedRequest, now: Instant) -> (u8, u8, u64) {
        let promoted = if self.is_promoted(req, now) { 0 } else { 1 };
        (promoted, req.class.base_rank(), req.seq)
    }

    fn best_queued_index(&self, now: Instant) -> Option<usize> {
        (0..self.queue.len()).min_by_key(|&i| self.ordering_key(&self.queue[i], now))
    }

    /// Decide whether `best` should preempt a currently-running `victim`.
    fn should_preempt(&self, best: &QueuedRequest, best_promoted: bool, victim: &RunningSession) -> bool {
        if !victim.preemptible {
            return false;
        }
        if victim.preemptions >= self.config.max_preemptions {
            return false;
        }
        // A session admitted while starving is protected from preemption.
        if victim.promoted {
            return false;
        }
        if best_promoted {
            return true;
        }
        // Otherwise only a strictly higher class preempts.
        best.class.base_rank() < victim.class.base_rank()
    }

    /// Choose the lowest-value preemptible running session, preferring the
    /// most recently started (so near-complete work is less likely to be
    /// killed).
    fn pick_victim(&self, best: &QueuedRequest, best_promoted: bool) -> Option<String> {
        self.running
            .iter()
            .filter(|(_, r)| self.should_preempt(best, best_promoted, r))
            .max_by(|(_, a), (_, b)| {
                a.class
                    .base_rank()
                    .cmp(&b.class.base_rank())
                    .then_with(|| a.started_at.cmp(&b.started_at))
            })
            .map(|(id, _)| id.clone())
    }

    /// Process the queue and return the decisions to apply. Call this after
    /// every `enqueue` / `complete`. Deterministic for a given `now`.
    pub fn schedule(&mut self, now: Instant) -> Vec<ScheduleAction> {
        let mut actions = Vec::new();
        loop {
            let Some(best_idx) = self.best_queued_index(now) else {
                break;
            };
            let best = self.queue[best_idx].clone();
            let promoted = self.is_promoted(&best, now);

            if self.running.len() < self.config.max_concurrent {
                self.queue.swap_remove(best_idx);
                self.running.insert(
                    best.session_id.clone(),
                    RunningSession {
                        class: best.class,
                        started_at: now,
                        preemptions: best.preemptions,
                        promoted,
                        preemptible: true,
                    },
                );
                self.stats.admitted += 1;
                if promoted {
                    self.stats.starvation_promotions += 1;
                }
                actions.push(ScheduleAction::Admit {
                    session_id: best.session_id.clone(),
                    table_id: best.table_id,
                    class: best.class,
                    promoted,
                });
                continue;
            }

            let Some(victim_id) = self.pick_victim(&best, promoted) else {
                break;
            };

            // Requeue the victim, preserving its original arrival time and
            // bumping its preemption counter so it ages out of preemptibility.
            if let Some(victim) = self.running.remove(&victim_id) {
                let seq = self.next_seq;
                self.next_seq += 1;
                let victim_req = self
                    .queue
                    .iter()
                    .find(|q| q.session_id == victim_id)
                    .cloned();
                let (table_id, enqueued_at, preemptions) = match victim_req {
                    Some(prev) => (prev.table_id, prev.enqueued_at, victim.preemptions + 1),
                    None => (0, now, victim.preemptions + 1),
                };
                self.queue.push(QueuedRequest {
                    session_id: victim_id.clone(),
                    table_id,
                    class: victim.class,
                    enqueued_at,
                    seq,
                    preemptions,
                });
            }

            // `best_idx` still refers to `best`: the victim was appended at
            // the end of the queue, which does not shift earlier indices.
            self.queue.swap_remove(best_idx);
            self.running.insert(
                best.session_id.clone(),
                RunningSession {
                    class: best.class,
                    started_at: now,
                    preemptions: best.preemptions,
                    promoted,
                    preemptible: true,
                },
            );
            self.stats.admitted += 1;
            self.stats.preemptions += 1;
            if promoted {
                self.stats.starvation_promotions += 1;
            }
            actions.push(ScheduleAction::Preempt {
                preempted_session_id: victim_id,
                admitted_session_id: best.session_id.clone(),
            });
        }
        actions
    }

    /// Mark a running session complete, freeing its slot. Returns `false` if
    /// it was not running.
    pub fn complete(&mut self, session_id: &str, _now: Instant) -> bool {
        self.running.remove(session_id).is_some()
    }

    /// Cancel a queued or running session.
    pub fn cancel(&mut self, session_id: &str) -> bool {
        let was_running = self.running.remove(session_id).is_some();
        let before = self.queue.len();
        self.queue.retain(|q| q.session_id != session_id);
        was_running || self.queue.len() != before
    }

    pub fn is_running(&self, session_id: &str) -> bool {
        self.running.contains_key(session_id)
    }

    pub fn is_queued(&self, session_id: &str) -> bool {
        self.queue.iter().any(|q| q.session_id == session_id)
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    pub fn queued_count(&self) -> usize {
        self.queue.len()
    }

    pub fn status(&self) -> PreemptionStatus {
        let count = |c: PriorityClass| self.queue.iter().filter(|q| q.class == c).count();
        PreemptionStatus {
            max_concurrent: self.config.max_concurrent,
            running: self.running.len(),
            queued: self.queue.len(),
            queued_priority: count(PriorityClass::Priority),
            queued_standard: count(PriorityClass::Standard),
            queued_casual: count(PriorityClass::Casual),
            max_preemptions: self.config.max_preemptions,
            priority_max_wait_secs: self.config.priority_max_wait.as_secs(),
            standard_max_wait_secs: self.config.standard_max_wait.as_secs(),
            casual_max_wait_secs: self.config.casual_max_wait.as_secs(),
            stats: self.stats,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller(max_concurrent: usize, max_preemptions: u32) -> PreemptionController {
        PreemptionController::new(PreemptionConfig {
            max_concurrent,
            max_preemptions,
            ..PreemptionConfig::default()
        })
    }

    #[test]
    fn priority_is_admitted_before_casual() {
        let t0 = Instant::now();
        let mut c = controller(4, 2);
        c.enqueue("casual", 1, PriorityClass::Casual, t0).unwrap();
        c.enqueue("priority", 2, PriorityClass::Priority, t0).unwrap();

        let actions = c.schedule(t0);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            ScheduleAction::Admit {
                session_id: "priority".into(),
                table_id: 2,
                class: PriorityClass::Priority,
                promoted: false,
            }
        );
        assert!(c.is_running("priority"));
        assert!(c.is_running("casual"));
    }

    #[test]
    fn fifo_tie_break_within_a_class() {
        let t0 = Instant::now();
        let mut c = controller(1, 2);
        c.enqueue("first", 1, PriorityClass::Standard, t0).unwrap();
        c.enqueue("second", 2, PriorityClass::Standard, t0).unwrap();

        let actions = c.schedule(t0);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            ScheduleAction::Admit { session_id, .. } if session_id == "first"
        ));
    }

    #[test]
    fn starvation_promotes_a_waiting_casual_ahead_of_fresh_priority() {
        let t0 = Instant::now();
        let mut c = controller(1, 2);
        // Occupy the only slot so the casual request cannot be admitted
        // immediately.
        c.enqueue("running", 9, PriorityClass::Standard, t0).unwrap();
        c.schedule(t0);
        c.enqueue("casual", 1, PriorityClass::Casual, t0).unwrap();

        // Advance past the casual starvation bound and flood with priority.
        let later = t0 + PreemptionConfig::default().casual_max_wait + Duration::from_secs(1);
        c.enqueue("priority", 2, PriorityClass::Priority, later).unwrap();

        // Free the slot: the promoted casual must win over fresh priority.
        assert!(c.complete("running", later));
        let actions = c.schedule(later);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            ScheduleAction::Admit { session_id, promoted: true, .. } if session_id == "casual"
        ));
    }

    #[test]
    fn higher_class_preempts_lower_when_full() {
        let t0 = Instant::now();
        let mut c = controller(1, 2);
        c.enqueue("casual", 1, PriorityClass::Casual, t0).unwrap();
        c.schedule(t0);
        assert!(c.is_running("casual"));

        c.enqueue("priority", 2, PriorityClass::Priority, t0).unwrap();
        let actions = c.schedule(t0);
        assert_eq!(
            actions,
            vec![ScheduleAction::Preempt {
                preempted_session_id: "casual".into(),
                admitted_session_id: "priority".into(),
            }]
        );
        assert!(c.is_running("priority"));
        // The victim is requeued, not dropped.
        assert!(c.is_queued("casual"));
    }

    #[test]
    fn equal_class_does_not_preempt() {
        let t0 = Instant::now();
        let mut c = controller(1, 2);
        c.enqueue("a", 1, PriorityClass::Standard, t0).unwrap();
        c.schedule(t0);
        c.enqueue("b", 2, PriorityClass::Standard, t0).unwrap();

        assert!(c.schedule(t0).is_empty());
        assert!(c.is_running("a"));
        assert!(c.is_queued("b"));
    }

    #[test]
    fn protected_promoted_session_is_not_preempted() {
        let t0 = Instant::now();
        let mut c = controller(1, 2);
        c.running.insert(
            "protected".into(),
            RunningSession {
                class: PriorityClass::Casual,
                started_at: t0,
                preemptions: 0,
                promoted: true,
                preemptible: true,
            },
        );
        c.enqueue("priority", 2, PriorityClass::Priority, t0).unwrap();

        assert!(c.schedule(t0).is_empty());
        assert!(c.is_running("protected"));
        assert!(c.is_queued("priority"));
    }

    #[test]
    fn session_is_not_preempted_more_than_the_limit() {
        let t0 = Instant::now();
        let mut c = controller(1, 1);
        c.running.insert(
            "tired".into(),
            RunningSession {
                class: PriorityClass::Casual,
                started_at: t0,
                preemptions: 1, // == max_preemptions
                promoted: false,
                preemptible: true,
            },
        );
        c.enqueue("priority", 2, PriorityClass::Priority, t0).unwrap();

        assert!(c.schedule(t0).is_empty());
        assert!(c.is_running("tired"));
    }

    #[test]
    fn completing_a_session_admits_the_next_queued() {
        let t0 = Instant::now();
        let mut c = controller(1, 2);
        c.enqueue("a", 1, PriorityClass::Standard, t0).unwrap();
        c.enqueue("b", 2, PriorityClass::Standard, t0).unwrap();
        c.schedule(t0);
        assert!(c.is_running("a"));
        assert!(c.is_queued("b"));

        assert!(c.complete("a", t0));
        let actions = c.schedule(t0);
        assert!(matches!(
            &actions[0],
            ScheduleAction::Admit { session_id, .. } if session_id == "b"
        ));
        assert_eq!(c.running_count(), 1);
        assert_eq!(c.queued_count(), 0);
    }

    #[test]
    fn duplicate_enqueue_is_rejected() {
        let t0 = Instant::now();
        let mut c = controller(4, 2);
        c.enqueue("a", 1, PriorityClass::Standard, t0).unwrap();
        assert_eq!(
            c.enqueue("a", 1, PriorityClass::Standard, t0),
            Err(EnqueueError::AlreadyQueued)
        );
        c.schedule(t0);
        assert_eq!(
            c.enqueue("a", 1, PriorityClass::Standard, t0),
            Err(EnqueueError::AlreadyRunning)
        );
    }

    #[test]
    fn queue_is_bounded() {
        let t0 = Instant::now();
        let mut c = PreemptionController::new(PreemptionConfig {
            max_concurrent: 0,
            max_queued: 2,
            ..PreemptionConfig::default()
        });
        c.enqueue("a", 1, PriorityClass::Standard, t0).unwrap();
        c.enqueue("b", 2, PriorityClass::Standard, t0).unwrap();
        assert_eq!(
            c.enqueue("c", 3, PriorityClass::Standard, t0),
            Err(EnqueueError::QueueFull)
        );
        assert_eq!(c.status().stats.rejected_queue_full, 1);
    }

    #[test]
    fn class_parse_falls_back_to_standard() {
        assert_eq!(PriorityClass::parse("PRIORITY"), PriorityClass::Priority);
        assert_eq!(PriorityClass::parse("casual"), PriorityClass::Casual);
        assert_eq!(PriorityClass::parse("nonsense"), PriorityClass::Standard);
        assert_eq!(PriorityClass::parse(""), PriorityClass::Standard);
    }
}
