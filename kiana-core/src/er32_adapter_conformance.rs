//! Core read-only facade for ER-32 adapter conformance evidence.

use kiana_domain::Er32ConformanceReport;

pub fn validate_er32_conformance_report(report: &Er32ConformanceReport) -> Result<(), String> {
    report.validate()
}
