//! DaemonHost route for the Core-owned unified shutdown decision.

use kiana_domain::{ShutdownInput, ShutdownReport};
use kiana_ports::PortError;

pub(crate) fn evaluate(input: &ShutdownInput) -> Result<ShutdownReport, PortError> {
    kiana_core::evaluate_shutdown(input).map_err(PortError::Failed)
}
