//! DaemonHost route for read-only operator status, doctor and preflight diagnostics.

use kiana_domain::{OpsDiagnosticsInput, OpsDiagnosticsMode, OpsDiagnosticsReport};
use kiana_ports::PortError;

pub(crate) fn evaluate(input: &OpsDiagnosticsInput) -> Result<OpsDiagnosticsReport, PortError> {
    kiana_core::evaluate_ops_diagnostics(input).map_err(PortError::Failed)
}

pub(crate) fn evaluate_mode(
    input: &OpsDiagnosticsInput,
    mode: OpsDiagnosticsMode,
) -> Result<OpsDiagnosticsReport, PortError> {
    kiana_core::evaluate_ops_mode(input, mode).map_err(PortError::Failed)
}
