//! Core adapter for Company bounded parallel plan/settlement facts.

use kiana_domain::{CompanyParallelPlan, CompanyParallelSettlement};

pub(crate) fn validate_company_parallel_plan(
    plan: &CompanyParallelPlan,
) -> Result<(), &'static str> {
    plan.validate()
}

pub(crate) fn validate_company_parallel_settlement(
    settlement: &CompanyParallelSettlement,
    plan: &CompanyParallelPlan,
) -> Result<(), &'static str> {
    settlement.validate_against(plan)
}
