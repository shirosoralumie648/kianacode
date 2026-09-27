//! AUT-20 read-only Core facade for compensation plans.

use kiana_domain::{validate_automation_compensation, AutomationCompensationPlan};

pub fn validate_compensation_plan(plan: &AutomationCompensationPlan) -> Result<(), &'static str> {
    validate_automation_compensation(plan)
}
