//! Daemon-side wake adapter for Company intents.
//!
//! This adapter owns only bounded queue coordination.  It rebuilds from committed source facts
//! and returns typed claims/receipts to the ControlPlane; it never invokes a model, capability or
//! external provider.

use kiana_domain::{CompanyDispatchReceipt, CompanyWake, CompanyWakeLedger};

pub(crate) struct CompanyDispatchAdapter;

impl CompanyDispatchAdapter {
    pub(crate) fn rebuild(
        ledger: &mut CompanyWakeLedger,
        intents: impl IntoIterator<Item = (u64, CompanyWake)>,
    ) -> Result<u64, &'static str> {
        ledger.scan_committed_intents(intents)
    }

    pub(crate) fn consume(
        ledger: &mut CompanyWakeLedger,
        intent_id: &str,
        claim_id: &str,
        receipt: CompanyDispatchReceipt,
    ) -> Result<(), &'static str> {
        ledger.consume(intent_id, claim_id, receipt)
    }
}
