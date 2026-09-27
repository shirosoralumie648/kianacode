//! AUT-18 read-only Core facade for signal/checkpoint facts.

use kiana_domain::{validate_automation_signal, AutomationSignalFact};

pub fn validate_automation_signal_fact(fact: &AutomationSignalFact) -> Result<(), &'static str> {
    validate_automation_signal(fact)
}
