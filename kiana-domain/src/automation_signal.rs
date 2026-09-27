//! AUT-18 approval/signal pause-resume and checkpoint source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_SIGNAL_SCHEMA: &str = "kiana.automation-signal.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSignalState {
    Paused,
    Resumed,
    Consumed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalFact {
    pub schema: String,
    pub execution_id: String,
    pub signal_id: String,
    pub owner_id: String,
    pub action_digest: String,
    pub path_scope_digest: String,
    pub authority_epoch: u64,
    pub checkpoint_sequence: u64,
    pub state: AutomationSignalState,
    pub consumed: bool,
    pub signal_digest: String,
}

impl AutomationSignalFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_SIGNAL_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.signal_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.path_scope_digest)
            || self.authority_epoch == 0
            || self.checkpoint_sequence == 0
            || self.consumed != matches!(self.state, AutomationSignalState::Consumed)
            || !valid_digest(&self.signal_digest)
            || self.signal_digest != self.digest()
        {
            return Err("automation_signal_fact_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "signal_id": self.signal_id,
            "owner_id": self.owner_id,
            "action_digest": self.action_digest,
            "path_scope_digest": self.path_scope_digest,
            "authority_epoch": self.authority_epoch,
            "checkpoint_sequence": self.checkpoint_sequence,
            "state": self.state,
            "consumed": self.consumed,
        }))
    }
}

pub fn validate_automation_signal(fact: &AutomationSignalFact) -> Result<(), &'static str> {
    fact.validate()
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
