//! Coordinator SLO definitions and deterministic error-budget evaluation.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct SloTarget {
    pub name: &'static str,
    pub objective: f64,
}

pub const SESSION_SUCCESS: SloTarget = SloTarget { name: "session_success", objective: 0.995 };
pub const PROOF_LATENCY: SloTarget = SloTarget { name: "proof_latency", objective: 0.99 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum AlertLevel { Healthy, Warning, Critical }

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ErrorBudget {
    pub target: SloTarget,
    pub observed: f64,
    pub remaining: f64,
    pub alert: AlertLevel,
}

pub fn evaluate(target: SloTarget, observed: f64) -> ErrorBudget {
    let observed = observed.clamp(0.0, 1.0);
    let remaining = ((observed - target.objective) / (1.0 - target.objective)).clamp(0.0, 1.0);
    let alert = if remaining <= 0.0 { AlertLevel::Critical } else if remaining < 0.25 { AlertLevel::Warning } else { AlertLevel::Healthy };
    ErrorBudget { target, observed, remaining, alert }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_budget_is_critical() { assert_eq!(evaluate(SESSION_SUCCESS, 0.99).alert, AlertLevel::Critical); }
    #[test]
    fn low_budget_warns_before_breach() { assert_eq!(evaluate(SESSION_SUCCESS, 0.996).alert, AlertLevel::Warning); }
}
