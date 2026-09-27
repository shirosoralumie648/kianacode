//! SW-17 Core read-only swarm projection facade.

use kiana_domain::{validate_swarm_projection_event, SwarmProjectionEvent};

pub fn validate_swarm_projection_event_fact(event: &SwarmProjectionEvent) -> Result<(), String> {
    validate_swarm_projection_event(event)
}
