//! AUT-17 read-only Core facade for cancellation facts.

use kiana_domain::{validate_automation_cancellation, AutomationCancellationFact};

pub fn validate_automation_cancel(fact: &AutomationCancellationFact) -> Result<(), &'static str> {
    validate_automation_cancellation(fact)
}
