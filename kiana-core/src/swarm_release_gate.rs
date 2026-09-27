//! SW-18 Core read-only swarm release gate facade.

use kiana_domain::{validate_swarm_release_gate, SwarmReleaseGate};

pub fn validate_swarm_release_evidence(gate: &SwarmReleaseGate) -> Result<(), String> {
    validate_swarm_release_gate(gate)
}
