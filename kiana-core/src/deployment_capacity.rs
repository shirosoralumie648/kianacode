//! Read-only Core facade for deployment capacity and shutdown-limit decisions.

use kiana_domain::{CapacityInput, CapacityReport};

pub fn evaluate_capacity(input: &CapacityInput) -> Result<CapacityReport, String> {
    CapacityReport::evaluate(input)
}

pub fn validate_capacity_report(
    input: &CapacityInput,
    report: &CapacityReport,
) -> Result<(), String> {
    report.validate_against(input)
}
