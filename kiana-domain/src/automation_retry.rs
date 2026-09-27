//! AUT-16 bounded retry classifier and attempt reservation source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_RETRY_SCHEMA: &str = "kiana.automation-retry.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRetryDecision {
    RetryNewAttempt,
    NoRetry,
    ReconcileUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRetryInput {
    pub schema: String,
    pub execution_id: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub effect_started: bool,
    pub effect_known: bool,
    pub capability_idempotent: bool,
    pub approval_valid: bool,
    pub budget_remaining: u64,
    pub transient_pre_send: bool,
    pub retry_after_ms: Option<u64>,
    pub deadline_remaining_ms: u64,
    pub input_digest: String,
    pub decision_digest: String,
}

impl AutomationRetryInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RETRY_SCHEMA
            || self.execution_id.trim().is_empty()
            || self.attempt == 0
            || self.max_attempts == 0
            || self.attempt > self.max_attempts
            || self.retry_after_ms.is_some_and(|v| v > 86_400_000)
            || !valid_digest(&self.input_digest)
            || !valid_digest(&self.decision_digest)
            || self.decision_digest != self.digest()
        {
            return Err("automation_retry_input_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "max_attempts": self.max_attempts,
            "effect_started": self.effect_started,
            "effect_known": self.effect_known,
            "capability_idempotent": self.capability_idempotent,
            "approval_valid": self.approval_valid,
            "budget_remaining": self.budget_remaining,
            "transient_pre_send": self.transient_pre_send,
            "retry_after_ms": self.retry_after_ms,
            "deadline_remaining_ms": self.deadline_remaining_ms,
            "input_digest": self.input_digest,
        }))
    }
}

pub fn classify_automation_retry(
    input: &AutomationRetryInput,
) -> Result<AutomationRetryDecision, &'static str> {
    input.validate()?;
    if input.effect_started && !input.effect_known {
        return Ok(AutomationRetryDecision::ReconcileUnknown);
    }
    if input.attempt >= input.max_attempts
        || !input.capability_idempotent
        || !input.approval_valid
        || input.budget_remaining == 0
        || input.deadline_remaining_ms == 0
        || !input.transient_pre_send
    {
        return Ok(AutomationRetryDecision::NoRetry);
    }
    if input
        .retry_after_ms
        .is_some_and(|delay| delay >= input.deadline_remaining_ms)
    {
        return Ok(AutomationRetryDecision::NoRetry);
    }
    Ok(AutomationRetryDecision::RetryNewAttempt)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
