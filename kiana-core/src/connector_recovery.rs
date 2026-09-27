//! INT-29 Core read-only connector recovery validation facade.

use kiana_domain::{validate_connector_recovery, ConnectorRecoveryFact};

pub fn validate_connector_recovery_fact(fact: &ConnectorRecoveryFact) -> Result<(), &'static str> {
    validate_connector_recovery(fact)
}
