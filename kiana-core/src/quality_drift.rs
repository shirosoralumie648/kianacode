//! EQ-46 read-only ControlPlane adapter for drift metrics and alert evidence.
//!
//! The adapter validates a server-resolved drift input and returns an append-ready event payload.
//! It does not append the event, switch a route, mutate a grant, or dispatch a provider.

use kiana_domain::{evaluate_drift, DriftEvaluation, DriftEvaluationInput};

pub const DRIFT_ALERT_COMMAND: &str = "drift.alerted";

pub fn evaluate_quality_drift(
    input: &DriftEvaluationInput,
) -> Result<DriftEvaluation, &'static str> {
    evaluate_drift(input)
}
