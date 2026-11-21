//! Secure multi-party reshuffle when a committee node is decommissioned.
//!
//! Issue #585. A node leaving the committee must not leak or freeze ongoing
//! privacy: shares held by the departing node must never be copied to a
//! replacement, and hands already in flight must either finish on the
//! original committee or be restarted from a *fresh* shuffle on the new one.
//!
//! ## Lifecycle
//!
//! ```text
//!   Active ──begin──▶ Draining ──all pending done──▶ Decommissioned
//!                        │
//!                        └──drain timeout with pending sessions──▶ Reshuffling
//!                                                                    │
//!                                                             enact  ▼
//!                                                            Decommissioned
//! ```
//!
//! Two safe exit paths are supported, matching the issue's acceptance
//! criteria:
//!
//! 1. **Drain-and-wait** — stop assigning new sessions to the node and wait
//!    for every in-flight session to finish. If they all finish within
//!    `drain_timeout` the node exits with no reshuffle at all. This is the
//!    preferred path because no session is interrupted.
//! 2. **Reshuffle** — if the drain times out with sessions still pending,
//!    the coordinator replaces the node with a standby and, for each pending
//!    session, either
//!      * *resumes* it, when the replacement is **identity-equivalent**
//!        (holds the same REP3 party identity — a warm standby, see
//!        `scripts/promote-mpc-standby.sh`), so it can take over the exact
//!        same share-holder role without any shares moving; or
//!      * *aborts* it, when the replacement has a different identity. The
//!        session is then re-dealt from a brand new shuffle on the new
//!        committee. Aborting never reveals old shares: the previous shares
//!        are simply discarded by their holders.
//!
//! In both cases the departing node's share material is never transmitted to
//! the replacement; a replacement only ever receives *fresh* shares from a
//! new shuffle, or already held identical shares in the identity-equivalent
//! case.
//!
//! The manager is time-injected (`now: Instant`) and pure, so it is fully
//! unit-testable without a running cluster.

use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

/// Where a decommissioning node is in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecommissionPhase {
    /// Normal operation; may accept new sessions.
    Active,
    /// No new sessions; waiting for in-flight sessions to finish.
    Draining,
    /// A replacement has been chosen; pending sessions are being
    /// resumed/aborted onto the new committee.
    Reshuffling,
    /// Fully removed from the committee.
    Decommissioned,
}

impl DecommissionPhase {
    pub fn accepts_new_sessions(self) -> bool {
        matches!(self, DecommissionPhase::Active)
    }
}

#[derive(Clone, Debug)]
pub struct DecommissionConfig {
    /// How long to wait for pending sessions to drain before falling back to
    /// a reshuffle.
    pub drain_timeout: Duration,
    /// When true, a reshuffle without an identity-equivalent replacement is
    /// refused outright instead of falling back to aborting sessions. Set
    /// this on an operator that would rather keep a node up than abort hands.
    pub require_identity_equivalent_standby: bool,
}

impl Default for DecommissionConfig {
    fn default() -> Self {
        Self {
            drain_timeout: Duration::from_secs(120),
            require_identity_equivalent_standby: false,
        }
    }
}

impl DecommissionConfig {
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            drain_timeout: Duration::from_secs(
                std::env::var("MPC_DECOMMISSION_DRAIN_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(d.drain_timeout.as_secs()),
            ),
            require_identity_equivalent_standby: std::env::var(
                "MPC_DECOMMISSION_REQUIRE_IDENTITY_STANDBY",
            )
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(d.require_identity_equivalent_standby),
        }
    }
}

/// Per-node decommission state.
#[derive(Clone, Debug)]
pub struct NodeDecommission {
    pub node_id: String,
    pub phase: DecommissionPhase,
    /// When draining began.
    pub drain_started_at: Option<Instant>,
    /// Sessions still in flight on this node.
    pub pending_sessions: BTreeSet<String>,
    /// The standby chosen to replace this node, if any.
    pub replacement: Option<String>,
}

/// Result of polling the drain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrainOutcome {
    /// Sessions are still running but the drain window has not elapsed.
    InProgress { pending: Vec<String> },
    /// Every pending session finished; the node can exit cleanly.
    Drained,
    /// The drain window elapsed with sessions still pending; a reshuffle is
    /// now required.
    TimedOut { pending: Vec<String> },
}

/// What a reshuffle does with one pending session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDisposition {
    /// The replacement holds the same party identity; the session continues.
    Resume,
    /// The replacement has a different identity; the session must be re-dealt
    /// from a fresh shuffle.
    Abort,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionPlan {
    pub session_id: String,
    pub disposition: SessionDisposition,
}

/// The concrete plan produced by a reshuffle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReshufflePlan {
    pub decommissioned_node: String,
    pub replacement_node: String,
    /// True when the replacement can take over the exact REP3 party identity
    /// (no shares move).
    pub identity_preserved: bool,
    pub sessions: Vec<SessionPlan>,
}

impl ReshufflePlan {
    pub fn resume_sessions(&self) -> Vec<&str> {
        self.sessions
            .iter()
            .filter(|s| s.disposition == SessionDisposition::Resume)
            .map(|s| s.session_id.as_str())
            .collect()
    }

    pub fn abort_sessions(&self) -> Vec<&str> {
        self.sessions
            .iter()
            .filter(|s| s.disposition == SessionDisposition::Abort)
            .map(|s| s.session_id.as_str())
            .collect()
    }
}

/// Errors from the decommission state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecommissionError {
    UnknownNode(String),
    AlreadyDecommissioning(String),
    NotDraining(String),
    DrainInProgress,
    NoIdentityEquivalentStandby,
    NotReshuffling(String),
    SessionNotTracked(String),
    InvalidReplacement(String),
}

impl std::fmt::Display for DecommissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecommissionError::UnknownNode(n) => write!(f, "node {n} is not tracked"),
            DecommissionError::AlreadyDecommissioning(n) => {
                write!(f, "node {n} is already decommissioning")
            }
            DecommissionError::NotDraining(n) => write!(f, "node {n} is not draining"),
            DecommissionError::DrainInProgress => {
                write!(f, "drain window has not elapsed; await completion or timeout")
            }
            DecommissionError::NoIdentityEquivalentStandby => write!(
                f,
                "no identity-equivalent standby available and config requires one"
            ),
            DecommissionError::NotReshuffling(n) => write!(f, "node {n} is not reshuffling"),
            DecommissionError::SessionNotTracked(s) => {
                write!(f, "session {s} is not tracked on this node")
            }
            DecommissionError::InvalidReplacement(r) => {
                write!(f, "replacement {r} is not a distinct node")
            }
        }
    }
}

/// Serializable view for `GET /api/mpc/decommission/status`.
#[derive(Clone, Debug, Serialize)]
pub struct NodeDecommissionStatus {
    pub node_id: String,
    pub phase: DecommissionPhase,
    pub pending_sessions: usize,
    pub replacement: Option<String>,
}

/// Manages decommission/reshuffle across committee nodes.
#[derive(Debug)]
pub struct DecommissionManager {
    config: DecommissionConfig,
    nodes: HashMap<String, NodeDecommission>,
    last_plan: Option<ReshufflePlan>,
}

impl DecommissionManager {
    pub fn new(config: DecommissionConfig) -> Self {
        Self {
            config,
            nodes: HashMap::new(),
            last_plan: None,
        }
    }

    pub fn from_env() -> Self {
        Self::new(DecommissionConfig::from_env())
    }

    pub fn config(&self) -> &DecommissionConfig {
        &self.config
    }

    /// Begin decommissioning `node_id`. New sessions are refused from this
    /// point on; existing ones are tracked until they complete.
    pub fn begin(&mut self, node_id: impl Into<String>, now: Instant) -> Result<(), DecommissionError> {
        let node_id = node_id.into();
        let entry = self.nodes.entry(node_id.clone()).or_insert_with(|| NodeDecommission {
            node_id: node_id.clone(),
            phase: DecommissionPhase::Active,
            drain_started_at: None,
            pending_sessions: BTreeSet::new(),
            replacement: None,
        });
        if !entry.phase.accepts_new_sessions() {
            return Err(DecommissionError::AlreadyDecommissioning(node_id));
        }
        entry.phase = DecommissionPhase::Draining;
        entry.drain_started_at = Some(now);
        Ok(())
    }

    fn entry_mut(&mut self, node_id: &str) -> Result<&mut NodeDecommission, DecommissionError> {
        self.nodes
            .get_mut(node_id)
            .ok_or_else(|| DecommissionError::UnknownNode(node_id.to_string()))
    }

    fn entry(&self, node_id: &str) -> Result<&NodeDecommission, DecommissionError> {
        self.nodes
            .get(node_id)
            .ok_or_else(|| DecommissionError::UnknownNode(node_id.to_string()))
    }

    /// Record that a session is now running on `node_id`. Refuses to assign a
    /// new session to a node that is draining or beyond. An unknown node is
    /// first created in the `Active` phase.
    pub fn track_session(
        &mut self,
        node_id: &str,
        session_id: impl Into<String>,
    ) -> Result<(), DecommissionError> {
        let entry = self
            .nodes
            .entry(node_id.to_string())
            .or_insert_with(|| NodeDecommission {
                node_id: node_id.to_string(),
                phase: DecommissionPhase::Active,
                drain_started_at: None,
                pending_sessions: BTreeSet::new(),
                replacement: None,
            });
        if !entry.phase.accepts_new_sessions() {
            return Err(DecommissionError::NotDraining(node_id.to_string()));
        }
        entry.pending_sessions.insert(session_id.into());
        Ok(())
    }

    /// Mark a session complete, freeing it from the pending set.
    pub fn complete_session(&mut self, node_id: &str, session_id: &str) -> bool {
        match self.nodes.get_mut(node_id) {
            Some(entry) => entry.pending_sessions.remove(session_id),
            None => false,
        }
    }

    /// Poll the drain state for a draining node.
    pub fn drain_status(
        &self,
        node_id: &str,
        now: Instant,
    ) -> Result<DrainOutcome, DecommissionError> {
        let entry = self.entry(node_id)?;
        if entry.phase != DecommissionPhase::Draining {
            return Err(DecommissionError::NotDraining(node_id.to_string()));
        }
        let pending: Vec<String> = entry.pending_sessions.iter().cloned().collect();
        if pending.is_empty() {
            return Ok(DrainOutcome::Drained);
        }
        let elapsed = entry
            .drain_started_at
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or_default();
        if elapsed >= self.config.drain_timeout {
            Ok(DrainOutcome::TimedOut { pending })
        } else {
            Ok(DrainOutcome::InProgress { pending })
        }
    }

    /// Plan a reshuffle of `node_id` onto `replacement`.
    ///
    /// `identity_preserved` must be set by the caller only after verifying
    /// that `replacement` genuinely holds the same REP3 party identity (e.g.
    /// a warm standby launched from the same `PARTY_CONFIG`). It is the one
    /// piece of information this module cannot infer, and it decides whether
    /// pending sessions resume or must be re-dealt.
    pub fn plan_reshuffle(
        &mut self,
        node_id: &str,
        replacement: impl Into<String>,
        identity_preserved: bool,
        now: Instant,
    ) -> Result<ReshufflePlan, DecommissionError> {
        let replacement = replacement.into();
        if replacement == node_id {
            return Err(DecommissionError::InvalidReplacement(replacement));
        }

        // Validate the drain first (immutable borrow ends here).
        match self.drain_status(node_id, now)? {
            DrainOutcome::InProgress { .. } => return Err(DecommissionError::DrainInProgress),
            DrainOutcome::Drained | DrainOutcome::TimedOut { .. } => {}
        }

        if !identity_preserved && self.config.require_identity_equivalent_standby {
            return Err(DecommissionError::NoIdentityEquivalentStandby);
        }

        let pending: Vec<String> = self
            .entry(node_id)?
            .pending_sessions
            .iter()
            .cloned()
            .collect();

        let disposition = if identity_preserved {
            SessionDisposition::Resume
        } else {
            SessionDisposition::Abort
        };

        let entry = self.entry_mut(node_id)?;
        entry.replacement = Some(replacement.clone());
        entry.phase = DecommissionPhase::Reshuffling;

        let plan = ReshufflePlan {
            decommissioned_node: node_id.to_string(),
            replacement_node: replacement,
            identity_preserved,
            sessions: pending
                .into_iter()
                .map(|session_id| SessionPlan {
                    session_id,
                    disposition,
                })
                .collect(),
        };
        self.last_plan = Some(plan.clone());
        Ok(plan)
    }

    /// Finalize a planned reshuffle: pending sessions are handed off (resumed
    /// or aborted) and the node becomes `Decommissioned`. Returns the plan
    /// that was enacted, or the most recent plan if the node already moved on.
    pub fn enact_reshuffle(&mut self, node_id: &str) -> Result<ReshufflePlan, DecommissionError> {
        let plan = self
            .last_plan
            .clone()
            .filter(|p| p.decommissioned_node == node_id)
            .ok_or_else(|| DecommissionError::NotReshuffling(node_id.to_string()))?;

        let entry = self.entry_mut(node_id)?;
        if entry.phase != DecommissionPhase::Reshuffling {
            return Err(DecommissionError::NotReshuffling(node_id.to_string()));
        }

        // The sessions have been handed off; drop them from this node's
        // pending set so nothing is left dangling.
        entry.pending_sessions.clear();
        entry.phase = DecommissionPhase::Decommissioned;
        Ok(plan)
    }

    /// Cancel a decommission (e.g. the node was repaired) and allow it to
    /// accept sessions again.
    pub fn cancel(&mut self, node_id: &str) -> Result<(), DecommissionError> {
        let entry = self.entry_mut(node_id)?;
        if entry.phase == DecommissionPhase::Decommissioned {
            return Err(DecommissionError::AlreadyDecommissioning(node_id.to_string()));
        }
        entry.phase = DecommissionPhase::Active;
        entry.drain_started_at = None;
        entry.replacement = None;
        Ok(())
    }

    /// Whether `node_id` may be assigned a new session. Unknown nodes are
    /// treated as active (they have not been decommissioned).
    pub fn can_assign(&self, node_id: &str) -> bool {
        self.nodes
            .get(node_id)
            .map(|n| n.phase.accepts_new_sessions())
            .unwrap_or(true)
    }

    pub fn phase(&self, node_id: &str) -> Option<DecommissionPhase> {
        self.nodes.get(node_id).map(|n| n.phase)
    }

    pub fn is_decommissioning(&self, node_id: &str) -> bool {
        self.nodes
            .get(node_id)
            .map(|n| n.phase != DecommissionPhase::Active)
            .unwrap_or(false)
    }

    /// Filter a candidate node list down to those that may be assigned new
    /// work. Used by the coordinator's node-selection path.
    pub fn filter_assignable<'a>(&self, candidates: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        candidates
            .into_iter()
            .filter(|id| self.can_assign(id))
            .cloned()
            .collect()
    }

    pub fn status(&self) -> Vec<NodeDecommissionStatus> {
        self.nodes
            .values()
            .map(|n| NodeDecommissionStatus {
                node_id: n.node_id.clone(),
                phase: n.phase,
                pending_sessions: n.pending_sessions.len(),
                replacement: n.replacement.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> DecommissionManager {
        DecommissionManager::new(DecommissionConfig {
            drain_timeout: Duration::from_secs(60),
            require_identity_equivalent_standby: false,
        })
    }

    #[test]
    fn draining_refuses_new_sessions_and_keeps_pending() {
        let t0 = Instant::now();
        let mut m = manager();
        m.track_session("0", "s1").unwrap();
        m.begin("0", t0).unwrap();
        assert!(!m.can_assign("0"));
        assert_eq!(
            m.track_session("0", "s2"),
            Err(DecommissionError::NotDraining("0".into()))
        );
        assert_eq!(m.phase("0"), Some(DecommissionPhase::Draining));
    }

    #[test]
    fn drain_completes_when_sessions_finish() {
        let t0 = Instant::now();
        let mut m = manager();
        m.begin("0", t0).unwrap();
        // No sessions were tracked, so it drains immediately.
        assert_eq!(m.drain_status("0", t0), Ok(DrainOutcome::Drained));

        m.cancel("0").unwrap();
        m.track_session("0", "s1").unwrap();
        m.begin("0", t0).unwrap();
        assert!(m.complete_session("0", "s1"));
        assert_eq!(m.drain_status("0", t0), Ok(DrainOutcome::Drained));
    }

    #[test]
    fn drain_times_out_with_pending_sessions() {
        let t0 = Instant::now();
        let mut m = manager();
        m.track_session("0", "s1").unwrap();
        m.begin("0", t0).unwrap();

        let early = t0 + Duration::from_secs(10);
        assert_eq!(
            m.drain_status("0", early),
            Ok(DrainOutcome::InProgress {
                pending: vec!["s1".into()]
            })
        );

        let late = t0 + Duration::from_secs(61);
        assert_eq!(
            m.drain_status("0", late),
            Ok(DrainOutcome::TimedOut {
                pending: vec!["s1".into()]
            })
        );
    }

    #[test]
    fn identity_preserving_reshuffle_resumes_sessions() {
        let t0 = Instant::now();
        let mut m = manager();
        m.track_session("0", "s1").unwrap();
        m.track_session("0", "s2").unwrap();
        m.begin("0", t0).unwrap();

        let plan = m
            .plan_reshuffle("0", "3", true, t0 + Duration::from_secs(61))
            .unwrap();
        assert!(plan.identity_preserved);
        assert_eq!(plan.resume_sessions(), vec!["s1", "s2"]);
        assert!(plan.abort_sessions().is_empty());
        assert_eq!(m.phase("0"), Some(DecommissionPhase::Reshuffling));

        let enacted = m.enact_reshuffle("0").unwrap();
        assert_eq!(enacted, plan);
        assert_eq!(m.phase("0"), Some(DecommissionPhase::Decommissioned));
    }

    #[test]
    fn identity_change_reshuffle_aborts_sessions() {
        let t0 = Instant::now();
        let mut m = manager();
        m.track_session("0", "s1").unwrap();
        m.begin("0", t0).unwrap();

        let plan = m
            .plan_reshuffle("0", "3", false, t0 + Duration::from_secs(61))
            .unwrap();
        assert!(!plan.identity_preserved);
        assert!(plan.resume_sessions().is_empty());
        assert_eq!(plan.abort_sessions(), vec!["s1"]);
    }

    #[test]
    fn plan_before_timeout_is_refused_while_pending() {
        let t0 = Instant::now();
        let mut m = manager();
        m.track_session("0", "s1").unwrap();
        m.begin("0", t0).unwrap();
        assert_eq!(
            m.plan_reshuffle("0", "3", true, t0),
            Err(DecommissionError::DrainInProgress)
        );
    }

    #[test]
    fn strict_mode_refuses_non_identity_reshuffle() {
        let t0 = Instant::now();
        let mut m = DecommissionManager::new(DecommissionConfig {
            drain_timeout: Duration::from_secs(1),
            require_identity_equivalent_standby: true,
        });
        m.track_session("0", "s1").unwrap();
        m.begin("0", t0).unwrap();
        assert_eq!(
            m.plan_reshuffle("0", "3", false, t0 + Duration::from_secs(2)),
            Err(DecommissionError::NoIdentityEquivalentStandby)
        );
    }

    #[test]
    fn self_replacement_is_rejected() {
        let t0 = Instant::now();
        let mut m = manager();
        m.begin("0", t0).unwrap();
        assert_eq!(
            m.plan_reshuffle("0", "0", true, t0),
            Err(DecommissionError::InvalidReplacement("0".into()))
        );
    }

    #[test]
    fn cancel_restores_active() {
        let t0 = Instant::now();
        let mut m = manager();
        m.begin("0", t0).unwrap();
        assert!(!m.can_assign("0"));
        m.cancel("0").unwrap();
        assert!(m.can_assign("0"));
        assert_eq!(m.phase("0"), Some(DecommissionPhase::Active));
    }

    #[test]
    fn filter_assignable_excludes_draining_nodes() {
        let t0 = Instant::now();
        let mut m = manager();
        m.begin("1", t0).unwrap();
        let candidates = vec!["0".to_string(), "1".to_string(), "2".to_string()];
        assert_eq!(m.filter_assignable(candidates.iter()), vec!["0", "2"]);
    }
}
