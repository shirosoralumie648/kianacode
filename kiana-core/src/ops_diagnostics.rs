//! Read-only Core facade for operator status, doctor and preflight diagnostics.

use kiana_domain::{OpsDiagnosticsInput, OpsDiagnosticsMode, OpsDiagnosticsReport};

pub fn evaluate_ops_diagnostics(
    input: &OpsDiagnosticsInput,
) -> Result<OpsDiagnosticsReport, String> {
    OpsDiagnosticsReport::evaluate(input)
}

pub fn evaluate_ops_mode(
    input: &OpsDiagnosticsInput,
    mode: OpsDiagnosticsMode,
) -> Result<OpsDiagnosticsReport, String> {
    if input.mode != mode {
        return Err("ops_diagnostics_mode_mismatch".to_owned());
    }
    evaluate_ops_diagnostics(input)
}

pub fn validate_ops_diagnostics(
    input: &OpsDiagnosticsInput,
    report: &OpsDiagnosticsReport,
) -> Result<(), String> {
    report.validate_against(input)
}
