//! Read-only Core facade for deployment incident lifecycle decisions.

use kiana_domain::{IncidentInput, IncidentReport};

pub fn evaluate_incident(input: &IncidentInput) -> Result<IncidentReport, String> {
    IncidentReport::evaluate(input)
}

pub fn validate_incident_report(
    input: &IncidentInput,
    report: &IncidentReport,
) -> Result<(), String> {
    report.validate_against(input)
}
