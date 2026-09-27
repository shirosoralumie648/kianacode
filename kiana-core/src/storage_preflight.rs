//! Read-only Core facade for storage root and trust preflight.

use kiana_domain::{StoragePreflightReport, StoragePreflightRequest};

pub fn evaluate_storage_preflight(
    request: &StoragePreflightRequest,
) -> Result<StoragePreflightReport, String> {
    StoragePreflightReport::evaluate(request)
}

pub fn validate_storage_preflight_report(report: &StoragePreflightReport) -> Result<(), String> {
    report.validate()
}
