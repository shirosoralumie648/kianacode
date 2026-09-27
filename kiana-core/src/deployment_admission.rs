//! Read-only Core facade for ready/maintenance/drain admission decisions.

use kiana_domain::{DeploymentAdmissionDecision, DeploymentAdmissionInput};

pub fn evaluate_admission(
    input: &DeploymentAdmissionInput,
) -> Result<DeploymentAdmissionDecision, String> {
    DeploymentAdmissionDecision::evaluate(input)
}

pub fn validate_admission_decision(
    input: &DeploymentAdmissionInput,
    decision: &DeploymentAdmissionDecision,
) -> Result<(), String> {
    decision.validate_against(input)
}
