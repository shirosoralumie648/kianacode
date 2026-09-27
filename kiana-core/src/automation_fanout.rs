//! AUT-19 read-only Core facade for bounded fan-out/fan-in plans.

use kiana_domain::{validate_automation_fanout, AutomationFanoutPlan};

pub fn validate_fanout_plan(plan: &AutomationFanoutPlan) -> Result<(), &'static str> {
    validate_automation_fanout(plan)
}
