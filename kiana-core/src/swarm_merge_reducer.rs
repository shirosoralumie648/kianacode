//! SW-13 Core read-only swarm merge reducer facade.

use kiana_domain::{validate_swarm_merge_decision, SwarmMergeDecision};

pub fn validate_swarm_merge(decision: &SwarmMergeDecision) -> Result<(), String> {
    validate_swarm_merge_decision(decision)
}
