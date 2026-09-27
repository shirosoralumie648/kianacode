//! DaemonHost route for the pure deployment incident reducer.

use kiana_domain::{IncidentInput, IncidentReport};
use kiana_ports::PortError;

pub(crate) fn evaluate(input: &IncidentInput) -> Result<IncidentReport, PortError> {
    kiana_core::evaluate_incident(input).map_err(PortError::Failed)
}
