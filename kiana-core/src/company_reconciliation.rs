//! Core adapter for Company risk/incident reconciliation facts.
//!
//! This module records validated domain successors only. It does not query a provider, retry an
//! effect, close a Run or mutate a Delivery result.

use kiana_domain::{CompanyReconciliationCase, CompanyReconciliationLedger};

pub(crate) fn record_company_reconciliation(
    ledger: &mut CompanyReconciliationLedger,
    case: CompanyReconciliationCase,
) -> Result<(), &'static str> {
    ledger.record_case(case)
}

pub(crate) fn validate_company_reconciliation(
    case: &CompanyReconciliationCase,
) -> Result<(), &'static str> {
    case.validate()
}
