//! EQ-48 read-only ControlPlane adapter for report artifact contracts.
//!
//! The adapter validates a report assembled from committed evaluation facts. Rendering and
//! artifact persistence remain outside this boundary; no provider, Broker, Runner or filesystem
//! operation is reachable here.

use kiana_domain::QualityReport;

pub const QUALITY_REPORT_COMMAND: &str = "quality.report";

pub fn validate_quality_report(report: &QualityReport) -> Result<(), String> {
    report.validate()
}
