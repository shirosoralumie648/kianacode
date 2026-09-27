//! SW-16 Core read-only directed swarm handoff facade.

use kiana_domain::{validate_swarm_handoff, SwarmHandoffRecord};

pub fn validate_swarm_handoff_record(record: &SwarmHandoffRecord) -> Result<(), &'static str> {
    validate_swarm_handoff(record)
}
