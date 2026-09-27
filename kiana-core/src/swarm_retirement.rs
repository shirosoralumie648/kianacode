//! SW-15 Core read-only swarm retirement validation facade.

use kiana_domain::{validate_swarm_retirement, SwarmRetirementFact};

pub fn validate_swarm_retirement_fact(fact: &SwarmRetirementFact) -> Result<(), &'static str> {
    validate_swarm_retirement(fact)
}
