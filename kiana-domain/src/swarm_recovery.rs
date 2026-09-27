//! SW-11 replay, pending-write and explicit re-admission evidence.

use crate::{json_digest, AttemptId, EventId, RunId};
use serde::{Deserialize, Serialize};

pub const SWARM_RECOVERY_SCHEMA: &str = "kiana.swarm-recovery.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmRecoveryState {
    PendingWrite,
    Replaying,
    ReAdmissionRequired,
    Recovered,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmRecoveryFact {
    pub schema: String,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub source_cursor: u64,
    pub last_committed_cursor: u64,
    pub pending_write_digest: String,
    pub source_event_ids: Vec<EventId>,
    pub authority_epoch: u64,
    pub state: SwarmRecoveryState,
    pub effect_known: bool,
    pub re_admission_authorized: bool,
    pub recovery_digest: String,
}

impl SwarmRecoveryFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_RECOVERY_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.last_committed_cursor > self.source_cursor
            || !valid_digest(&self.pending_write_digest)
            || self.source_event_ids.is_empty()
            || self.authority_epoch == 0
            || matches!(self.state, SwarmRecoveryState::Recovered)
                && (!self.effect_known || !self.re_admission_authorized)
            || matches!(self.state, SwarmRecoveryState::ResultUnknown) && self.effect_known
            || matches!(self.state, SwarmRecoveryState::PendingWrite)
                && self.last_committed_cursor >= self.source_cursor
            || !valid_digest(&self.recovery_digest)
            || self.recovery_digest != self.digest()
        {
            return Err("swarm_recovery_fact_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "source_cursor": self.source_cursor,
            "last_committed_cursor": self.last_committed_cursor,
            "pending_write_digest": self.pending_write_digest,
            "source_event_ids": self.source_event_ids,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "effect_known": self.effect_known,
            "re_admission_authorized": self.re_admission_authorized,
        }))
    }
}

pub fn validate_swarm_recovery(fact: &SwarmRecoveryFact) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
