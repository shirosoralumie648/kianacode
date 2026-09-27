//! SW-09 bounded child progress ledger and stall escalation evidence.

use crate::{json_digest, AttemptId, RunId};
use serde::{Deserialize, Serialize};

pub const SWARM_PROGRESS_SCHEMA: &str = "kiana.swarm-progress.v1";
pub const SWARM_PROGRESS_CHECKPOINT_SCHEMA: &str = "kiana.swarm-progress-checkpoint.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmProgressBudget {
    pub max_turns: u64,
    pub max_tokens: u64,
    pub max_effects: u64,
    pub max_wall_time_ms: u64,
    pub max_stalls: u32,
}

impl SwarmProgressBudget {
    fn validate(&self) -> Result<(), &'static str> {
        if self.max_turns == 0
            || self.max_tokens == 0
            || self.max_effects == 0
            || self.max_wall_time_ms == 0
            || self.max_stalls == 0
        {
            return Err("swarm_progress_budget_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmProgressStatus {
    Running,
    Stalled,
    Escalated,
    Exhausted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmProgressObservation {
    pub schema: String,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub heartbeat_sequence: u64,
    pub checkpoint_sequence: u64,
    pub turns: u64,
    pub tokens: u64,
    pub effects: u64,
    pub wall_time_ms: u64,
    pub stalls: u32,
    pub budget: SwarmProgressBudget,
    pub status: SwarmProgressStatus,
    pub observation_digest: String,
}

impl SwarmProgressObservation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_PROGRESS_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.heartbeat_sequence == 0
            || self.checkpoint_sequence > self.heartbeat_sequence
            || self.budget.validate().is_err()
            || self.turns > self.budget.max_turns
            || self.tokens > self.budget.max_tokens
            || self.effects > self.budget.max_effects
            || self.wall_time_ms > self.budget.max_wall_time_ms
            || self.stalls > self.budget.max_stalls
            || !valid_digest(&self.observation_digest)
            || self.observation_digest != self.digest()
        {
            return Err("swarm_progress_observation_invalid");
        }
        if self.stalls >= self.budget.max_stalls && self.status == SwarmProgressStatus::Running {
            return Err("swarm_progress_stall_not_escalated");
        }
        if (self.turns == self.budget.max_turns
            || self.tokens == self.budget.max_tokens
            || self.effects == self.budget.max_effects
            || self.wall_time_ms == self.budget.max_wall_time_ms)
            && self.status == SwarmProgressStatus::Running
        {
            return Err("swarm_progress_budget_not_exhausted");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "heartbeat_sequence": self.heartbeat_sequence,
            "checkpoint_sequence": self.checkpoint_sequence,
            "turns": self.turns,
            "tokens": self.tokens,
            "effects": self.effects,
            "wall_time_ms": self.wall_time_ms,
            "stalls": self.stalls,
            "budget": self.budget,
            "status": self.status,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwarmProgressLedger {
    pub observation: Option<SwarmProgressObservation>,
}

impl SwarmProgressLedger {
    pub fn new() -> Self {
        Self { observation: None }
    }

    pub fn record(
        &mut self,
        next: SwarmProgressObservation,
    ) -> Result<SwarmProgressObservation, &'static str> {
        next.validate()?;
        if let Some(previous) = &self.observation {
            if previous.run_id != next.run_id || previous.attempt_id != next.attempt_id {
                return Err("swarm_progress_identity_drift");
            }
            if next.heartbeat_sequence <= previous.heartbeat_sequence
                || next.checkpoint_sequence < previous.checkpoint_sequence
                || next.turns < previous.turns
                || next.tokens < previous.tokens
                || next.effects < previous.effects
                || next.wall_time_ms < previous.wall_time_ms
                || next.stalls < previous.stalls
            {
                return Err("swarm_progress_non_monotonic");
            }
        }
        self.observation = Some(next.clone());
        Ok(next)
    }
}

impl Default for SwarmProgressLedger {
    fn default() -> Self {
        Self::new()
    }
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
