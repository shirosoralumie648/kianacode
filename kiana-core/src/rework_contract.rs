//! Core adapter for bounded rework provenance; actual Company command CAS remains authoritative.
use kiana_domain::{ReworkLedger, ReworkProvenance};
pub(crate) fn record_rework(
    ledger: &mut ReworkLedger,
    provenance: ReworkProvenance,
) -> Result<(), &'static str> {
    ledger.record(provenance)
}
