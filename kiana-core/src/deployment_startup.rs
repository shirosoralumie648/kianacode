//! Read-only Core facade for the deterministic deployment startup coordinator.

use kiana_domain::{StartupCoordinatorReport, StartupCoordinatorRequest};

pub fn evaluate_startup(
    request: &StartupCoordinatorRequest,
) -> Result<StartupCoordinatorReport, String> {
    StartupCoordinatorReport::evaluate(request)
}

pub fn validate_startup_report(
    request: &StartupCoordinatorRequest,
    report: &StartupCoordinatorReport,
) -> Result<(), String> {
    report.validate_against(request)
}
