//! AUT-15 worker dispatch/observation separation source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_DISPATCH_SCHEMA: &str = "kiana.automation-dispatch-intent.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDispatchState {
    Prepared,
    Claimed,
    Observed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchIntent {
    pub schema: String,
    pub execution_id: String,
    pub reservation_id: String,
    pub action_digest: String,
    pub worker_id: Option<String>,
    pub attempt: u32,
    pub authority_epoch: u64,
    pub state: AutomationDispatchState,
    pub observation_digest: Option<String>,
    pub intent_digest: String,
}

impl AutomationDispatchIntent {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_DISPATCH_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.reservation_id)
            || !valid_digest(&self.action_digest)
            || self.worker_id.as_deref().is_some_and(|v| !valid_text(v))
            || self.attempt == 0
            || self.authority_epoch == 0
            || self
                .observation_digest
                .as_deref()
                .is_some_and(|v| !valid_digest(v))
            || !valid_digest(&self.intent_digest)
            || self.intent_digest != self.digest()
        {
            return Err("automation_dispatch_intent_invalid");
        }
        match self.state {
            AutomationDispatchState::Prepared if self.worker_id.is_some() => {
                Err("automation_dispatch_prepared_worker_invalid")
            }
            AutomationDispatchState::Claimed if self.worker_id.is_none() => {
                Err("automation_dispatch_claim_worker_required")
            }
            AutomationDispatchState::Observed | AutomationDispatchState::Unknown
                if self.observation_digest.is_none() =>
            {
                Err("automation_dispatch_observation_required")
            }
            _ => Ok(()),
        }
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "reservation_id": self.reservation_id,
            "action_digest": self.action_digest,
            "worker_id": self.worker_id,
            "attempt": self.attempt,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "observation_digest": self.observation_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationObservation {
    pub execution_id: String,
    pub reservation_id: String,
    pub action_digest: String,
    pub worker_id: String,
    pub authority_epoch: u64,
    pub outcome: AutomationObservationOutcome,
    pub result_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationObservationOutcome {
    Succeeded,
    Failed,
    Unknown,
}

impl AutomationObservation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.execution_id)
            || !valid_text(&self.reservation_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.worker_id)
            || self.authority_epoch == 0
            || !valid_digest(&self.result_digest)
        {
            return Err("automation_observation_invalid");
        }
        Ok(())
    }
}

pub fn validate_automation_observation(
    intent: &AutomationDispatchIntent,
    observation: &AutomationObservation,
) -> Result<AutomationDispatchIntent, &'static str> {
    intent.validate()?;
    observation.validate()?;
    if intent.state != AutomationDispatchState::Claimed
        || intent.worker_id.as_deref() != Some(observation.worker_id.as_str())
        || intent.execution_id != observation.execution_id
        || intent.reservation_id != observation.reservation_id
        || intent.action_digest != observation.action_digest
        || intent.authority_epoch != observation.authority_epoch
    {
        return Err("automation_observation_binding_drift");
    }
    let mut next = intent.clone();
    next.state = match observation.outcome {
        AutomationObservationOutcome::Succeeded | AutomationObservationOutcome::Failed => {
            AutomationDispatchState::Observed
        }
        AutomationObservationOutcome::Unknown => AutomationDispatchState::Unknown,
    };
    next.observation_digest = Some(observation.result_digest.clone());
    next.intent_digest = next.digest();
    next.validate()?;
    Ok(next)
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
