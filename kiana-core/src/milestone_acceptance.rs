//! Core adapter for milestone-local acceptance.
use kiana_domain::{MilestoneAcceptanceLedger, MilestoneAcceptanceRequest};
pub(crate) fn record_milestone_acceptance(
    ledger: &mut MilestoneAcceptanceLedger,
    request: MilestoneAcceptanceRequest,
) -> Result<(), &'static str> {
    ledger.record(request)
}
