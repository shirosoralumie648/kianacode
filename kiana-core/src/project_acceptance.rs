//! Core adapter for project-level acceptance facts.
use kiana_domain::{ProjectAcceptanceLedger, ProjectAcceptanceRequest};
pub(crate) fn record_project_acceptance(
    ledger: &mut ProjectAcceptanceLedger,
    request: ProjectAcceptanceRequest,
) -> Result<(), &'static str> {
    ledger.record(request)
}
