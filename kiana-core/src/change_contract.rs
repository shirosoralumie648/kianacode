//! Core adapter for atomic baseline publication; existing ChangeRequest CAS remains authoritative.
use kiana_domain::{BaselinePublication, ChangeImpact, ChangePublicationLedger};
pub(crate) fn publish_change(
    ledger: &mut ChangePublicationLedger,
    impact: ChangeImpact,
    publication: BaselinePublication,
) -> Result<(), &'static str> {
    ledger.publish(impact, publication)
}
