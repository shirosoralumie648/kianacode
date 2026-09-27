//! SW-10 parent/child cancellation, drain and late-result fence evidence.

use crate::{json_digest, AttemptId, RunId};
use serde::{Deserialize, Serialize};

pub const SWARM_CANCEL_SCHEMA: &str = "kiana.swarm-cancel.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmCancelState {
    Requested,
    Draining,
    Cancelled,
    ResultUnknown,
    Fenced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmCancellationFact {
    pub schema: String,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub attempt_id: AttemptId,
    pub cancel_generation: u64,
    pub authority_epoch: u64,
    pub state: SwarmCancelState,
    pub started_effect: bool,
    pub stop_requested: bool,
    pub stop_confirmed: bool,
    pub late_result: bool,
    pub reason: String,
    pub fact_digest: String,
}

impl SwarmCancellationFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_CANCEL_SCHEMA
            || self.parent_run_id == self.child_run_id
            || self.attempt_id.as_uuid().is_nil()
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || self.reason.trim().is_empty()
            || self.stop_confirmed && !self.stop_requested
            || matches!(self.state, SwarmCancelState::Requested) && self.stop_requested
            || matches!(self.state, SwarmCancelState::Cancelled) && !self.stop_confirmed
            || matches!(self.state, SwarmCancelState::ResultUnknown) && !self.started_effect
            || self.late_result && self.state != SwarmCancelState::Fenced
            || !valid_digest(&self.fact_digest)
            || self.fact_digest != self.digest()
        {
            return Err("swarm_cancellation_fact_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent_run_id": self.parent_run_id,
            "child_run_id": self.child_run_id,
            "attempt_id": self.attempt_id,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "started_effect": self.started_effect,
            "stop_requested": self.stop_requested,
            "stop_confirmed": self.stop_confirmed,
            "late_result": self.late_result,
            "reason": self.reason,
        }))
    }
}

pub fn validate_swarm_cancellation(fact: &SwarmCancellationFact) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
