//! SW-12 typed child result/failure delegation facts.

use crate::{json_digest, AttemptId, RunId};
use serde::{Deserialize, Serialize};

pub const SWARM_CHILD_RESULT_SCHEMA: &str = "kiana.swarm-child-result.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypedChildResultState {
    Succeeded,
    Failed,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedChildResult {
    pub schema: String,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub attempt_id: AttemptId,
    pub partition_key: String,
    pub state: TypedChildResultState,
    pub output_digest: Option<String>,
    pub artifact_refs: Vec<String>,
    pub failure_reason: Option<String>,
    pub source_cursor: u64,
    pub independent_review_required: bool,
    pub result_digest: String,
}

impl TypedChildResult {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_CHILD_RESULT_SCHEMA
            || self.parent_run_id == self.child_run_id
            || self.attempt_id.as_uuid().is_nil()
            || self.partition_key.trim().is_empty()
            || self.source_cursor == 0
            || self.artifact_refs.iter().any(|value| !valid_text(value))
            || self
                .output_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self
                .failure_reason
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_digest(&self.result_digest)
            || self.result_digest != self.digest()
        {
            return Err("swarm_child_result_invalid");
        }
        match self.state {
            TypedChildResultState::Succeeded
                if self.output_digest.is_none()
                    || self.artifact_refs.is_empty()
                    || self.failure_reason.is_some()
                    || !self.independent_review_required =>
            {
                return Err("swarm_child_result_success_contract_invalid")
            }
            TypedChildResultState::Failed
                if self.failure_reason.is_none() || self.output_digest.is_some() =>
            {
                return Err("swarm_child_result_failure_contract_invalid")
            }
            TypedChildResultState::ResultUnknown
                if self.output_digest.is_some() || !self.independent_review_required =>
            {
                return Err("swarm_child_result_unknown_contract_invalid")
            }
            _ => {}
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent_run_id": self.parent_run_id,
            "child_run_id": self.child_run_id,
            "attempt_id": self.attempt_id,
            "partition_key": self.partition_key,
            "state": self.state,
            "output_digest": self.output_digest,
            "artifact_refs": self.artifact_refs,
            "failure_reason": self.failure_reason,
            "source_cursor": self.source_cursor,
            "independent_review_required": self.independent_review_required,
        }))
    }
}

pub fn validate_typed_child_result(result: &TypedChildResult) -> Result<(), &'static str> {
    result.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
