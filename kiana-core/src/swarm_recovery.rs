//! SW-11 read-only Core facade for replay/re-admission facts.

use kiana_domain::{validate_swarm_recovery, SwarmRecoveryFact};

pub fn validate_child_recovery(fact: &SwarmRecoveryFact) -> Result<(), &'static str> {
    validate_swarm_recovery(fact)
}
