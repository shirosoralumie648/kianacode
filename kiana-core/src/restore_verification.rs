//! PD-23 Core read-only restore verification facade.

use kiana_domain::{validate_restore_verification, RestoreVerificationFact};

pub fn validate_restore_verification_fact(
    fact: &RestoreVerificationFact,
) -> Result<(), &'static str> {
    validate_restore_verification(fact)
}
