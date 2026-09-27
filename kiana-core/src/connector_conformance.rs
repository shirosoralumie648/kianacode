//! INT-30 Core read-only connector conformance report facade.

use kiana_domain::{validate_connector_conformance, ConnectorConformanceReport};

pub fn validate_connector_conformance_report(
    report: &ConnectorConformanceReport,
) -> Result<(), String> {
    validate_connector_conformance(report)
}
