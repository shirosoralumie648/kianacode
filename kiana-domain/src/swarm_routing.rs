//! SW-08 single-spine child execution routing and correlation evidence.

use crate::{json_digest, AttemptId, ChildCellId, EventId, RunId, SessionId};
use serde::{Deserialize, Serialize};

pub const SWARM_ROUTE_SCHEMA: &str = "kiana.swarm-route.v1";
pub const SWARM_ROUTE_RECEIPT_SCHEMA: &str = "kiana.swarm-route-receipt.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmExecutionRouteRequest {
    pub schema: String,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub child_session_id: SessionId,
    pub child_cell_id: ChildCellId,
    pub attempt_id: AttemptId,
    pub partition_key: String,
    pub correlation_id: String,
    pub causation_event_id: EventId,
    pub daemon_host_route: String,
    pub control_plane_route: String,
    pub broker_route: String,
    pub harness_route: String,
    pub direct_runner_route: bool,
    pub direct_provider_route: bool,
    pub authority_epoch: u64,
    pub route_digest: String,
}

impl SwarmExecutionRouteRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_ROUTE_SCHEMA
            || self.parent_run_id == self.child_run_id
            || self.child_session_id.is_empty()
            || self.child_cell_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || !valid_text(&self.partition_key)
            || !valid_text(&self.correlation_id)
            || self.causation_event_id.as_uuid().is_nil()
            || self.daemon_host_route != "DaemonHost"
            || self.control_plane_route != "ControlPlane"
            || self.broker_route != "CapabilityBroker"
            || self.harness_route != "KianaHarness"
            || self.direct_runner_route
            || self.direct_provider_route
            || self.authority_epoch == 0
            || !valid_digest(&self.route_digest)
            || self.route_digest != self.canonical_digest()
        {
            return Err("swarm_execution_route_invalid");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent_run_id": self.parent_run_id,
            "child_run_id": self.child_run_id,
            "child_session_id": self.child_session_id,
            "child_cell_id": self.child_cell_id,
            "attempt_id": self.attempt_id,
            "partition_key": self.partition_key,
            "correlation_id": self.correlation_id,
            "causation_event_id": self.causation_event_id,
            "daemon_host_route": self.daemon_host_route,
            "control_plane_route": self.control_plane_route,
            "broker_route": self.broker_route,
            "harness_route": self.harness_route,
            "direct_runner_route": self.direct_runner_route,
            "direct_provider_route": self.direct_provider_route,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmExecutionRouteReceipt {
    pub schema: String,
    pub route_digest: String,
    pub correlation_id: String,
    pub causation_event_id: EventId,
    pub child_run_id: RunId,
    pub child_session_id: SessionId,
    pub authority_epoch: u64,
    pub effect_dispatched: bool,
    pub receipt_digest: String,
}

impl SwarmExecutionRouteReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_ROUTE_RECEIPT_SCHEMA
            || !valid_digest(&self.route_digest)
            || !valid_text(&self.correlation_id)
            || self.causation_event_id.as_uuid().is_nil()
            || self.child_run_id.as_uuid().is_nil()
            || self.child_session_id.is_empty()
            || self.authority_epoch == 0
            || self.effect_dispatched
            || !valid_digest(&self.receipt_digest)
            || self.receipt_digest != self.digest()
        {
            return Err("swarm_execution_route_receipt_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "route_digest": self.route_digest,
            "correlation_id": self.correlation_id,
            "causation_event_id": self.causation_event_id,
            "child_run_id": self.child_run_id,
            "child_session_id": self.child_session_id,
            "authority_epoch": self.authority_epoch,
            "effect_dispatched": self.effect_dispatched,
        }))
    }
}

pub fn validate_swarm_execution_route(
    request: &SwarmExecutionRouteRequest,
) -> Result<SwarmExecutionRouteReceipt, &'static str> {
    request.validate()?;
    let mut receipt = SwarmExecutionRouteReceipt {
        schema: SWARM_ROUTE_RECEIPT_SCHEMA.to_owned(),
        route_digest: request.route_digest.clone(),
        correlation_id: request.correlation_id.clone(),
        causation_event_id: request.causation_event_id,
        child_run_id: request.child_run_id,
        child_session_id: request.child_session_id.clone(),
        authority_epoch: request.authority_epoch,
        effect_dispatched: false,
        receipt_digest: String::new(),
    };
    receipt.receipt_digest = receipt.digest();
    receipt.validate()?;
    Ok(receipt)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
