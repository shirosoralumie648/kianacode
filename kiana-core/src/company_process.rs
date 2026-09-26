//! Core-side Company ProcessManager adapter.
//!
//! This module only delegates to the pure domain planner.  Committing the returned intent and
//! dispatching the existing workflow/Company command remain separate ControlPlane operations.

use kiana_domain::{
    plan_company_process, CompanyProcessEvent, CompanyProcessIntent, CompanyProcessState,
    CompanyProcessTemplate,
};

pub(crate) fn plan_company_intent(
    state: &CompanyProcessState,
    template: &CompanyProcessTemplate,
    event: &CompanyProcessEvent,
) -> Result<(CompanyProcessState, Option<CompanyProcessIntent>), &'static str> {
    plan_company_process(state, template, event)
}
