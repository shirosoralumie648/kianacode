//! AUT-21 read-only Core facade for boot recovery facts.

use kiana_domain::{validate_automation_boot_recovery, AutomationBootRecoveryFact};

pub fn validate_boot_recovery(fact: &AutomationBootRecoveryFact) -> Result<(), &'static str> {
    validate_automation_boot_recovery(fact)
}
