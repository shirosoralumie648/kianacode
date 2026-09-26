//! Core adapter for fixed-base Company integration and MergeReceipt validation.

use kiana_domain::{CompanyIntegrationPlan, CompanyMergeReceipt};

pub(crate) fn validate_company_integration_plan(
    plan: &CompanyIntegrationPlan,
) -> Result<(), &'static str> {
    plan.validate()
}

pub(crate) fn validate_company_merge_receipt(
    receipt: &CompanyMergeReceipt,
    plan: &CompanyIntegrationPlan,
) -> Result<(), &'static str> {
    receipt.validate_against(plan)
}
