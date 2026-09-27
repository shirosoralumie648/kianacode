//! Read-only Core facade for projector/index/queue/lease reconcile decisions.

use kiana_domain::{ReconcileInput, ReconcileReport};

pub fn evaluate_reconcile(input: &ReconcileInput) -> Result<ReconcileReport, String> {
    ReconcileReport::evaluate(input)
}

pub fn validate_reconcile_report(
    input: &ReconcileInput,
    report: &ReconcileReport,
) -> Result<(), String> {
    report.validate_against(input)
}
