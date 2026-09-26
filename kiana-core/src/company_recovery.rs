//! Core adapter for durable-fact Company restart recovery.
//!
//! It only records a validated hydration snapshot or explicit plan; it never reexecutes an
//! intent, dispatch or external effect.

use kiana_domain::{CompanyRecoveryLedger, CompanyRecoveryPlan, CompanyRecoverySnapshot};

pub(crate) fn record_company_recovery_snapshot(
    ledger: &mut CompanyRecoveryLedger,
    snapshot: CompanyRecoverySnapshot,
) -> Result<(), &'static str> {
    ledger.record_snapshot(snapshot)
}

pub(crate) fn record_company_recovery_plan(
    ledger: &mut CompanyRecoveryLedger,
    plan: CompanyRecoveryPlan,
) -> Result<(), &'static str> {
    ledger.record_plan(plan)
}
