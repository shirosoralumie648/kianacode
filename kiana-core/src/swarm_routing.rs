//! SW-08 read-only Core facade for single-spine child routing.

use kiana_domain::{
    validate_swarm_execution_route, SwarmExecutionRouteReceipt, SwarmExecutionRouteRequest,
};

pub fn validate_child_route(
    request: &SwarmExecutionRouteRequest,
) -> Result<SwarmExecutionRouteReceipt, &'static str> {
    validate_swarm_execution_route(request)
}
