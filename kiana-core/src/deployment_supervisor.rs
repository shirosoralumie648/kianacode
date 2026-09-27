//! Read-only Core facade for lease-fenced supervisor observations.

use kiana_domain::{SupervisorObservation, SupervisorRequest};

pub fn validate_supervisor_observation(
    request: &SupervisorRequest,
    observation: &SupervisorObservation,
) -> Result<(), String> {
    observation.validate_against(request)
}
