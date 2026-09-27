//! DaemonHost route for Core-owned deployment admission decisions.

use kiana_domain::{DeploymentAdmissionDecision, DeploymentAdmissionInput};
use kiana_ports::PortError;

pub(crate) fn evaluate(
    input: &DeploymentAdmissionInput,
) -> Result<DeploymentAdmissionDecision, PortError> {
    kiana_core::evaluate_admission(input).map_err(PortError::Failed)
}
