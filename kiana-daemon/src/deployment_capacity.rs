//! DaemonHost route for the pure deployment capacity reducer.

use kiana_domain::{CapacityInput, CapacityReport};
use kiana_ports::PortError;

pub(crate) fn evaluate(input: &CapacityInput) -> Result<CapacityReport, PortError> {
    kiana_core::evaluate_capacity(input).map_err(PortError::Failed)
}
