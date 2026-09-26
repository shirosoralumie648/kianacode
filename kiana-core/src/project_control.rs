//! Core adapter for project control propagation.
//!
//! The adapter only validates/records the domain plan. ControlPlane remains the only place that
//! can dispatch cancellation, fence a late result, or retire a Cell.

use kiana_domain::{ProjectControlLedger, ProjectControlPlan};

pub(crate) fn record_project_control(
    ledger: &mut ProjectControlLedger,
    plan: ProjectControlPlan,
) -> Result<(), &'static str> {
    ledger.record(plan)
}

pub(crate) fn validate_project_control(plan: &ProjectControlPlan) -> Result<(), &'static str> {
    plan.validate()
}
