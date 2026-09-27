//! AUT-20 compensation workflow and fresh authorization source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_COMPENSATION_SCHEMA: &str = "kiana.automation-compensation.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCompensationState {
    Planned,
    Authorized,
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompensationPlan {
    pub schema: String,
    pub original_execution_id: String,
    pub original_attempt: u32,
    pub original_action_digest: String,
    pub compensation_action_digest: String,
    pub compensation_execution_id: String,
    pub authority_epoch: u64,
    pub fresh_authorization_digest: String,
    pub reused_original_permit: bool,
    pub state: AutomationCompensationState,
    pub plan_digest: String,
}

impl AutomationCompensationPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_COMPENSATION_SCHEMA
            || !valid_text(&self.original_execution_id)
            || self.original_attempt == 0
            || !valid_digest(&self.original_action_digest)
            || !valid_digest(&self.compensation_action_digest)
            || self.original_action_digest == self.compensation_action_digest
            || !valid_text(&self.compensation_execution_id)
            || self.compensation_execution_id == self.original_execution_id
            || self.authority_epoch == 0
            || !valid_digest(&self.fresh_authorization_digest)
            || self.reused_original_permit
            || !valid_digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("automation_compensation_plan_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "original_execution_id": self.original_execution_id,
            "original_attempt": self.original_attempt,
            "original_action_digest": self.original_action_digest,
            "compensation_action_digest": self.compensation_action_digest,
            "compensation_execution_id": self.compensation_execution_id,
            "authority_epoch": self.authority_epoch,
            "fresh_authorization_digest": self.fresh_authorization_digest,
            "reused_original_permit": self.reused_original_permit,
            "state": self.state,
        }))
    }
}

pub fn validate_automation_compensation(
    plan: &AutomationCompensationPlan,
) -> Result<(), &'static str> {
    plan.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
