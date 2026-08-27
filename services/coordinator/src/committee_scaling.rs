//! Committee auto-scaling for MPC session demand (Issue #229)
//!
//! ## Why this scales committees, not nodes
//!
//! The issue asks to "provision additional MPC node instances when active
//! session count exceeds a threshold". That is not possible for this protocol,
//! and the Helm chart already says so:
//!
//! ```text
//! # The MPC protocol requires exactly 3 nodes (REP3). Do not change replicas
//! # without a corresponding party-config update and re-key ceremony.
//! replicaCount: 3
//! ```
//!
//! REP3 is a three-party replicated secret sharing scheme. The share layout,
//! the party indices, and every protocol message are defined for exactly three
//! parties. A fourth replica holds no share, cannot participate, and would sit
//! idle at best — at worst it is a live endpoint advertising itself as a party.
//! Raising `maxReplicas` would not add capacity; it would add confusion.
//!
//! What *does* scale is the number of independent committees. Each committee is
//! its own set of three nodes with its own key material, and sessions are
//! assigned to a committee at creation. Adding capacity means standing up
//! another whole committee — three nodes plus a key ceremony — and routing new
//! sessions to it.
//!
//! So this module answers "how many committees does current demand require, and
//! is it safe to change that number right now", and deliberately never answers
//! "how many nodes should a committee have".
//!
//! ## What makes this different from a CPU-based HPA
//!
//! Two things a generic autoscaler gets wrong here:
//!
//! - **A committee cannot be drained instantly.** Sessions hold key shares for
//!   their lifetime. Scaling down means refusing *new* sessions on a committee
//!   and waiting for the running ones to finish — never terminating pods with
//!   live hands on them.
//! - **Scaling up is expensive and slow.** A new committee needs a distributed
//!   key generation ceremony before it can take a session. Reacting to a
//!   thirty-second spike would start a ceremony that finishes after the spike
//!   has passed, so scale-up is deliberately damped.

use serde::{Deserialize, Serialize};

/// Nodes per committee. Fixed by REP3 — this is a protocol constant, not a
/// tunable, and is exposed so callers do not hardcode `3` at each site.
pub const NODES_PER_COMMITTEE: u32 = 3;

/// Policy for how demand maps onto committee count.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScalingPolicy {
    /// Sessions one committee is provisioned to carry.
    pub sessions_per_committee: u32,
    /// Utilisation above which another committee is warranted, 0..1.
    pub scale_up_threshold: f64,
    /// Utilisation below which one can be retired, 0..1.
    pub scale_down_threshold: f64,
    /// Never drop below this many committees.
    pub min_committees: u32,
    /// Never exceed this many, regardless of demand.
    pub max_committees: u32,
    /// Consecutive observations above the threshold before scaling up.
    pub scale_up_samples: u32,
    /// Consecutive observations below the threshold before scaling down.
    pub scale_down_samples: u32,
}

impl Default for ScalingPolicy {
    fn default() -> Self {
        Self {
            sessions_per_committee: 50,
            scale_up_threshold: 0.80,
            // A wide gap from the scale-up threshold on purpose. Thresholds
            // close together flap: the committee added at 80% drops utilisation
            // to just under the scale-down line, which retires it, which pushes
            // utilisation back over 80%.
            scale_down_threshold: 0.40,
            min_committees: 1,
            max_committees: 10,
            // Asymmetric by design. Scaling up costs a key ceremony, so a brief
            // spike should not trigger one; scaling down strands capacity, so
            // it waits even longer to be sure demand has really gone.
            scale_up_samples: 3,
            scale_down_samples: 10,
        }
    }
}

/// One observation of cluster demand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DemandSample {
    pub active_sessions: u32,
    pub ready_committees: u32,
    /// Committees mid-ceremony. Counted as capacity for scale-up decisions so
    /// a ceremony already under way does not trigger a second one.
    pub provisioning_committees: u32,
    /// Committees refusing new sessions while they drain.
    pub draining_committees: u32,
    pub observed_at_secs: u64,
}

/// What the scaler decided to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum ScalingAction {
    /// Demand is within band, or the run of samples is not long enough yet.
    Hold,
    /// Begin a key ceremony for `count` more committees.
    ScaleUp { count: u32 },
    /// Stop routing new sessions to `count` committees and let them drain.
    /// Deliberately not "terminate": running sessions hold key shares.
    DrainDown { count: u32 },
}

/// A decision plus the reasoning, so an operator can see why capacity moved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingDecision {
    pub action: ScalingAction,
    pub reason: String,
    pub utilisation: f64,
    pub desired_committees: u32,
    pub current_committees: u32,
    /// Pods this implies, purely for reporting. Always a multiple of three.
    pub desired_nodes: u32,
}

/// Utilisation of the ready capacity, or `None` when there is none to divide by.
///
/// Returns `None` rather than 0.0 for zero capacity: no committees with
/// sessions queued is a very different situation from no committees and no
/// demand, and collapsing both to zero would make the scaler hold when it most
/// needs to act.
pub fn utilisation(sample: &DemandSample, policy: &ScalingPolicy) -> Option<f64> {
    let capacity = sample
        .ready_committees
        .saturating_mul(policy.sessions_per_committee);
    if capacity == 0 {
        return None;
    }
    Some(f64::from(sample.active_sessions) / f64::from(capacity))
}

/// Committees required to hold `active_sessions` at the scale-up threshold.
///
/// Sized against the threshold rather than raw capacity, so the result already
/// includes headroom instead of provisioning to exactly 100% full.
pub fn desired_committees(sample: &DemandSample, policy: &ScalingPolicy) -> u32 {
    if policy.sessions_per_committee == 0 {
        return policy.min_committees;
    }

    let effective_capacity = f64::from(policy.sessions_per_committee) * policy.scale_up_threshold;
    if effective_capacity <= 0.0 {
        return policy.max_committees;
    }

    let needed = (f64::from(sample.active_sessions) / effective_capacity).ceil() as u32;
    needed.clamp(policy.min_committees, policy.max_committees)
}

/// Decide from a run of consecutive samples.
///
/// Takes the whole run rather than one reading because the sustained-demand
/// requirement is the entire point: a single sample cannot distinguish a real
/// capacity shortfall from a thirty-second burst, and acting on the latter
/// starts a key ceremony that completes after the burst is over.
pub fn decide(samples: &[DemandSample], policy: &ScalingPolicy) -> ScalingDecision {
    let Some(latest) = samples.last() else {
        return ScalingDecision {
            action: ScalingAction::Hold,
            reason: "no demand samples".to_string(),
            utilisation: 0.0,
            desired_committees: policy.min_committees,
            current_committees: 0,
            desired_nodes: policy.min_committees * NODES_PER_COMMITTEE,
        };
    };

    let current = latest.ready_committees;
    let desired = desired_committees(latest, policy);
    let util = utilisation(latest, policy);

    let decision = |action: ScalingAction, reason: String| ScalingDecision {
        action,
        reason,
        utilisation: util.unwrap_or(0.0),
        desired_committees: desired,
        current_committees: current,
        desired_nodes: desired * NODES_PER_COMMITTEE,
    };

    // No ready capacity at all. If there is demand this is urgent and skips the
    // sustained-run requirement — there is nothing to wait for, every session
    // is already unservable.
    if util.is_none() {
        if latest.active_sessions > 0 && latest.provisioning_committees == 0 {
            return decision(
                ScalingAction::ScaleUp {
                    count: desired.max(policy.min_committees),
                },
                "no ready committees while sessions are waiting".to_string(),
            );
        }
        return decision(
            ScalingAction::Hold,
            "no ready committees and no waiting sessions".to_string(),
        );
    }

    let util = util.unwrap_or(0.0);

    // A ceremony already running is capacity on the way. Starting another
    // because it has not landed yet is how a scaler overshoots.
    if latest.provisioning_committees > 0 {
        return decision(
            ScalingAction::Hold,
            format!(
                "{} committee(s) already provisioning",
                latest.provisioning_committees
            ),
        );
    }

    // ── Scale up ────────────────────────────────────────────────────────────
    if util > policy.scale_up_threshold {
        if current >= policy.max_committees {
            return decision(
                ScalingAction::Hold,
                format!("at the {} committee ceiling", policy.max_committees),
            );
        }
        if !sustained(
            samples,
            policy,
            |u| u > policy.scale_up_threshold,
            policy.scale_up_samples,
        ) {
            return decision(
                ScalingAction::Hold,
                format!(
                    "utilisation {:.0}% is high but not yet sustained over {} samples",
                    util * 100.0,
                    policy.scale_up_samples
                ),
            );
        }

        let count = desired.saturating_sub(current).max(1);
        return decision(
            ScalingAction::ScaleUp { count },
            format!("utilisation {:.0}% sustained above threshold", util * 100.0),
        );
    }

    // ── Scale down ──────────────────────────────────────────────────────────
    if util < policy.scale_down_threshold {
        if current <= policy.min_committees {
            return decision(
                ScalingAction::Hold,
                format!("at the {} committee floor", policy.min_committees),
            );
        }
        if latest.draining_committees > 0 {
            return decision(
                ScalingAction::Hold,
                format!(
                    "{} committee(s) already draining",
                    latest.draining_committees
                ),
            );
        }
        if !sustained(
            samples,
            policy,
            |u| u < policy.scale_down_threshold,
            policy.scale_down_samples,
        ) {
            return decision(
                ScalingAction::Hold,
                format!(
                    "utilisation {:.0}% is low but not yet sustained over {} samples",
                    util * 100.0,
                    policy.scale_down_samples
                ),
            );
        }

        // One at a time. Removing several committees at once can push the
        // remaining ones straight past the scale-up threshold, and each drain
        // is already slow.
        return decision(
            ScalingAction::DrainDown { count: 1 },
            format!("utilisation {:.0}% sustained below threshold", util * 100.0),
        );
    }

    decision(
        ScalingAction::Hold,
        format!("utilisation {:.0}% is within band", util * 100.0),
    )
}

/// Whether the last `required` samples all satisfy `predicate`.
///
/// The run must be unbroken: one sample back inside the band means demand is
/// not actually sustained, and the clock restarts.
fn sustained(
    samples: &[DemandSample],
    policy: &ScalingPolicy,
    predicate: impl Fn(f64) -> bool,
    required: u32,
) -> bool {
    let required = required.max(1) as usize;
    if samples.len() < required {
        return false;
    }

    samples[samples.len() - required..]
        .iter()
        .all(|s| utilisation(s, policy).map(&predicate).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ScalingPolicy {
        ScalingPolicy::default()
    }

    fn sample(active: u32, ready: u32) -> DemandSample {
        DemandSample {
            active_sessions: active,
            ready_committees: ready,
            provisioning_committees: 0,
            draining_committees: 0,
            observed_at_secs: 0,
        }
    }

    /// `n` identical samples, for exercising the sustained-run requirement.
    fn run(active: u32, ready: u32, n: usize) -> Vec<DemandSample> {
        vec![sample(active, ready); n]
    }

    // ── The REP3 constraint ─────────────────────────────────────────────────

    #[test]
    fn capacity_is_always_provisioned_in_whole_committees() {
        // The scaler must never emit a node count that would leave a partial
        // committee — three parties or none.
        let p = policy();
        for active in [0, 1, 49, 50, 120, 500, 5_000] {
            let d = decide(&run(active, 2, 12), &p);
            assert_eq!(
                d.desired_nodes % NODES_PER_COMMITTEE,
                0,
                "desired_nodes must be a multiple of the fixed committee size"
            );
            assert_eq!(d.desired_nodes, d.desired_committees * NODES_PER_COMMITTEE);
        }
    }

    #[test]
    fn scaling_never_expresses_itself_as_nodes_within_a_committee() {
        // Guards the property the whole module is shaped around: the only
        // actions are whole-committee ones.
        let p = policy();
        let d = decide(&run(200, 1, 5), &p);
        assert!(matches!(
            d.action,
            ScalingAction::ScaleUp { .. } | ScalingAction::Hold
        ));
    }

    // ── Utilisation ─────────────────────────────────────────────────────────

    #[test]
    fn utilisation_is_sessions_over_committee_capacity() {
        let p = policy();
        // 40 sessions against one 50-session committee.
        assert_eq!(utilisation(&sample(40, 1), &p), Some(0.8));
    }

    #[test]
    fn zero_capacity_reports_none_rather_than_zero() {
        // Collapsing "no committees, sessions waiting" to 0.0 would read as
        // idle and make the scaler hold exactly when it must act.
        let p = policy();
        assert_eq!(utilisation(&sample(10, 0), &p), None);
    }

    // ── Scale up ────────────────────────────────────────────────────────────

    #[test]
    fn sustained_high_demand_scales_up() {
        let p = policy();
        // 45/50 = 90%, above the 80% threshold, held for the full run.
        let d = decide(&run(45, 1, p.scale_up_samples as usize), &p);

        assert!(matches!(d.action, ScalingAction::ScaleUp { .. }));
        assert!(d.reason.contains("sustained"));
    }

    #[test]
    fn a_brief_spike_does_not_start_a_ceremony() {
        // The reason scale-up is damped: a ceremony started for a 30-second
        // burst completes after the burst is over.
        let p = policy();
        let mut samples = run(10, 1, 5); // quiet
        samples.push(sample(45, 1)); // one spike

        assert_eq!(decide(&samples, &p).action, ScalingAction::Hold);
    }

    #[test]
    fn an_in_flight_ceremony_suppresses_a_second_one() {
        // Capacity already on the way; starting another is how a scaler
        // overshoots.
        let p = policy();
        let mut samples = run(45, 1, 5);
        if let Some(last) = samples.last_mut() {
            last.provisioning_committees = 1;
        }

        let d = decide(&samples, &p);
        assert_eq!(d.action, ScalingAction::Hold);
        assert!(d.reason.contains("provisioning"));
    }

    #[test]
    fn no_ready_committees_with_waiting_sessions_scales_up_immediately() {
        // Urgent: every session is unservable, so there is nothing to wait for.
        let p = policy();
        let d = decide(&[sample(5, 0)], &p);

        assert!(matches!(d.action, ScalingAction::ScaleUp { .. }));
        assert!(d.reason.contains("no ready committees"));
    }

    #[test]
    fn no_committees_and_no_demand_holds() {
        let p = policy();
        assert_eq!(decide(&[sample(0, 0)], &p).action, ScalingAction::Hold);
    }

    #[test]
    fn the_ceiling_is_respected() {
        let p = ScalingPolicy {
            max_committees: 2,
            ..policy()
        };
        let d = decide(&run(500, 2, 10), &p);

        assert_eq!(d.action, ScalingAction::Hold);
        assert!(d.reason.contains("ceiling"));
    }

    // ── Scale down ──────────────────────────────────────────────────────────

    #[test]
    fn sustained_low_demand_drains_rather_than_terminating() {
        // Sessions hold key shares for their lifetime; pods with live hands on
        // them must never be killed.
        let p = policy();
        // 10 sessions over 3 committees = 6.7%, well under the 40% floor.
        let d = decide(&run(10, 3, p.scale_down_samples as usize), &p);

        assert_eq!(d.action, ScalingAction::DrainDown { count: 1 });
    }

    #[test]
    fn scale_down_retires_one_committee_at_a_time() {
        // Removing several at once can push the remainder straight back over
        // the scale-up threshold.
        let p = policy();
        let d = decide(&run(5, 8, p.scale_down_samples as usize), &p);

        assert_eq!(d.action, ScalingAction::DrainDown { count: 1 });
    }

    #[test]
    fn scale_down_needs_a_longer_run_than_scale_up() {
        // Asymmetric on purpose: stranding capacity is worse than briefly
        // holding it.
        let p = policy();
        assert!(p.scale_down_samples > p.scale_up_samples);

        // A run long enough to scale up, but not to scale down.
        let d = decide(&run(10, 3, p.scale_up_samples as usize), &p);
        assert_eq!(d.action, ScalingAction::Hold);
    }

    #[test]
    fn the_floor_is_respected() {
        let p = policy();
        let d = decide(&run(0, 1, 20), &p);

        assert_eq!(d.action, ScalingAction::Hold);
        assert!(d.reason.contains("floor"));
    }

    #[test]
    fn an_in_flight_drain_suppresses_a_second_one() {
        let p = policy();
        let mut samples = run(5, 4, p.scale_down_samples as usize);
        if let Some(last) = samples.last_mut() {
            last.draining_committees = 1;
        }

        let d = decide(&samples, &p);
        assert_eq!(d.action, ScalingAction::Hold);
        assert!(d.reason.contains("draining"));
    }

    // ── Stability ───────────────────────────────────────────────────────────

    #[test]
    fn the_thresholds_leave_a_gap_that_prevents_flapping() {
        // Thresholds close together oscillate: the committee added at 80%
        // drops utilisation under the scale-down line, which retires it, which
        // pushes utilisation back over 80%.
        let p = policy();
        assert!(p.scale_up_threshold - p.scale_down_threshold >= 0.2);
    }

    #[test]
    fn utilisation_inside_the_band_holds() {
        let p = policy();
        // 30/50 = 60%, between the 40% and 80% thresholds.
        let d = decide(&run(30, 1, 20), &p);

        assert_eq!(d.action, ScalingAction::Hold);
        assert!(d.reason.contains("within band"));
    }

    #[test]
    fn adding_the_recommended_committees_brings_utilisation_into_the_band() {
        // The scaler must not recommend a target that immediately re-triggers.
        let p = policy();
        let overloaded = sample(180, 2); // 180%
        let desired = desired_committees(&overloaded, &p);

        let after = sample(180, desired);
        let util = utilisation(&after, &p).expect("capacity after scaling");

        assert!(util <= p.scale_up_threshold, "still over threshold: {util}");
        assert!(
            util >= p.scale_down_threshold,
            "overshot into scale-down: {util}"
        );
    }

    #[test]
    fn a_broken_run_restarts_the_clock() {
        // One sample back inside the band means demand is not sustained.
        let p = policy();
        let mut samples = run(45, 1, p.scale_up_samples as usize);
        samples.insert(samples.len() - 1, sample(10, 1));

        assert_eq!(decide(&samples, &p).action, ScalingAction::Hold);
    }

    #[test]
    fn no_samples_holds_rather_than_guessing() {
        let p = policy();
        let d = decide(&[], &p);

        assert_eq!(d.action, ScalingAction::Hold);
        assert!(d.reason.contains("no demand samples"));
    }

    #[test]
    fn desired_committees_is_clamped_to_the_configured_range() {
        let p = ScalingPolicy {
            min_committees: 2,
            max_committees: 4,
            ..policy()
        };

        assert_eq!(desired_committees(&sample(0, 1), &p), 2);
        assert_eq!(desired_committees(&sample(100_000, 1), &p), 4);
    }

    #[test]
    fn a_zero_sessions_per_committee_policy_does_not_divide_by_zero() {
        let p = ScalingPolicy {
            sessions_per_committee: 0,
            ..policy()
        };

        assert_eq!(desired_committees(&sample(100, 1), &p), p.min_committees);
    }
}
