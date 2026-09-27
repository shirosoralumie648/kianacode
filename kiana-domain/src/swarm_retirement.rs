//! SW-15 exactly-once swarm release/retire and residual budget evidence contract.

use crate::{json_digest, SwarmPlanId};
use serde::{Deserialize, Serialize};

pub const SWARM_RETIREMENT_SCHEMA: &str = "kiana.swarm-retirement.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmRetirementState {
    Pending,
    Released,
    BlockedUnknown,
    Retired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmRetirementFact {
    pub schema: String,
    pub swarm_plan_id: SwarmPlanId,
    pub source_cursor: u64,
    pub child_count: u32,
    pub terminal_known_count: u32,
    pub unknown_count: u32,
    pub residual_budget_digest: String,
    pub path_lock_release_digest: String,
    pub release_receipt_digest: String,
    pub release_attempt: u32,
    pub state: SwarmRetirementState,
    pub facts_retained: bool,
    pub retirement_digest: String,
}

impl SwarmRetirementFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_RETIREMENT_SCHEMA
            || self.swarm_plan_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.child_count == 0
            || self.terminal_known_count > self.child_count
            || self.unknown_count > self.child_count
            || !valid_digest(&self.residual_budget_digest)
            || !valid_digest(&self.path_lock_release_digest)
            || !valid_digest(&self.release_receipt_digest)
            || self.release_attempt == 0
            || !self.facts_retained
            || !valid_digest(&self.retirement_digest)
            || self.retirement_digest != self.digest()
        {
            return Err("swarm_retirement_fact_invalid");
        }
        if self.terminal_known_count + self.unknown_count > self.child_count {
            return Err("swarm_retirement_child_count_invalid");
        }
        match self.state {
            SwarmRetirementState::Pending => {}
            SwarmRetirementState::BlockedUnknown => {
                if self.unknown_count == 0 {
                    return Err("swarm_retirement_unknown_missing");
                }
            }
            SwarmRetirementState::Released => {
                if self.unknown_count != 0
                    || self.terminal_known_count != self.child_count
                    || self.release_attempt != 1
                {
                    return Err("swarm_retirement_release_not_exactly_once");
                }
            }
            SwarmRetirementState::Retired => {
                if self.unknown_count != 0
                    || self.terminal_known_count != self.child_count
                    || self.release_attempt != 1
                {
                    return Err("swarm_retirement_retire_not_ready");
                }
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "swarm_plan_id": self.swarm_plan_id,
            "source_cursor": self.source_cursor,
            "child_count": self.child_count,
            "terminal_known_count": self.terminal_known_count,
            "unknown_count": self.unknown_count,
            "residual_budget_digest": self.residual_budget_digest,
            "path_lock_release_digest": self.path_lock_release_digest,
            "release_receipt_digest": self.release_receipt_digest,
            "release_attempt": self.release_attempt,
            "state": self.state,
            "facts_retained": self.facts_retained,
        }))
    }
}

pub fn validate_swarm_retirement(fact: &SwarmRetirementFact) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
