//! Read-only Core facade for the unified shutdown reducer.

use kiana_domain::{ShutdownInput, ShutdownReport};

pub fn evaluate_shutdown(input: &ShutdownInput) -> Result<ShutdownReport, String> {
    ShutdownReport::evaluate(input)
}

pub fn validate_shutdown_report(
    input: &ShutdownInput,
    report: &ShutdownReport,
) -> Result<(), String> {
    report.validate_against(input)
}
