//! SW-10 read-only Core facade for cancellation/fence facts.

use kiana_domain::{validate_swarm_cancellation, SwarmCancellationFact};

pub fn validate_child_cancellation(fact: &SwarmCancellationFact) -> Result<(), &'static str> {
    validate_swarm_cancellation(fact)
}
