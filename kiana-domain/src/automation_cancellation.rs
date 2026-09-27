//! AUT-17 automation cancellation generation and late-result fence contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_CANCEL_SCHEMA: &str = "kiana.automation-cancel.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCancelState {
    Requested,
    Stopping,
    Stopped,
    ResultUnknown,
    Fenced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCancellationFact {
    pub schema: String,
    pub execution_id: String,
    pub attempt: u32,
    pub cancel_generation: u64,
    pub authority_epoch: u64,
    pub state: AutomationCancelState,
    pub effect_started: bool,
    pub stop_requested: bool,
    pub stop_confirmed: bool,
    pub late_result: bool,
    pub reason: String,
    pub fact_digest: String,
}

impl AutomationCancellationFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_CANCEL_SCHEMA
            || self.execution_id.trim().is_empty()
            || self.attempt == 0
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || self.reason.trim().is_empty()
            || self.stop_confirmed && !self.stop_requested
            || matches!(self.state, AutomationCancelState::Requested) && self.stop_requested
            || matches!(self.state, AutomationCancelState::Stopped) && !self.stop_confirmed
            || matches!(self.state, AutomationCancelState::ResultUnknown) && !self.effect_started
            || self.late_result && self.state != AutomationCancelState::Fenced
            || !valid_digest(&self.fact_digest)
            || self.fact_digest != self.digest()
        {
            return Err("automation_cancellation_fact_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "effect_started": self.effect_started,
            "stop_requested": self.stop_requested,
            "stop_confirmed": self.stop_confirmed,
            "late_result": self.late_result,
            "reason": self.reason,
        }))
    }
}

pub fn validate_automation_cancellation(
    fact: &AutomationCancellationFact,
) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
