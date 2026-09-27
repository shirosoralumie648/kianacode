//! SW-07 fresh child materialization and delegation evidence contract.

use crate::{json_digest, AttemptId, ChildCellId, RunId, SessionId};
use serde::{Deserialize, Serialize};

pub const SWARM_CHILD_INPUT_SCHEMA: &str = "kiana.swarm-child-input.v1";
pub const SWARM_CHILD_RECEIPT_SCHEMA: &str = "kiana.swarm-child-receipt.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmChildMaterializationRequest {
    pub schema: String,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub child_session_id: SessionId,
    pub child_run_id: RunId,
    pub child_attempt_id: AttemptId,
    pub child_cell_id: ChildCellId,
    pub partition_key: String,
    pub input_refs: Vec<String>,
    pub authorized_input_refs: Vec<String>,
    pub parent_scope_digest: String,
    pub child_scope_digest: String,
    pub child_scope_is_subset: bool,
    pub parent_private_history_included: bool,
    pub authority_epoch: u64,
    pub template_revision: String,
    pub policy_revision: String,
}

impl SwarmChildMaterializationRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_CHILD_INPUT_SCHEMA
            || self.parent_session_id.is_empty()
            || self.child_session_id.is_empty()
            || self.parent_session_id == self.child_session_id
            || self.parent_run_id == self.child_run_id
            || self.parent_session_id.is_empty()
            || self.partition_key.trim().is_empty()
            || self.input_refs.is_empty()
            || self.authorized_input_refs != self.input_refs
            || !valid_digest(&self.parent_scope_digest)
            || !valid_digest(&self.child_scope_digest)
            || !self.child_scope_is_subset
            || self.parent_private_history_included
            || self.authority_epoch == 0
            || self.template_revision.trim().is_empty()
            || self.policy_revision.trim().is_empty()
        {
            return Err("swarm_child_materialization_invalid");
        }
        Ok(())
    }

    pub fn request_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent_session_id": self.parent_session_id,
            "parent_run_id": self.parent_run_id,
            "child_session_id": self.child_session_id,
            "child_run_id": self.child_run_id,
            "child_attempt_id": self.child_attempt_id,
            "child_cell_id": self.child_cell_id,
            "partition_key": self.partition_key,
            "input_refs": self.input_refs,
            "authorized_input_refs": self.authorized_input_refs,
            "parent_scope_digest": self.parent_scope_digest,
            "child_scope_digest": self.child_scope_digest,
            "authority_epoch": self.authority_epoch,
            "template_revision": self.template_revision,
            "policy_revision": self.policy_revision,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmChildMaterializationReceipt {
    pub schema: String,
    pub child_session_id: SessionId,
    pub child_run_id: RunId,
    pub child_attempt_id: AttemptId,
    pub child_cell_id: ChildCellId,
    pub request_digest: String,
    pub authority_epoch: u64,
    pub fresh_context: bool,
    pub private_history_excluded: bool,
    pub receipt_digest: String,
}

impl SwarmChildMaterializationReceipt {
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "child_session_id": self.child_session_id,
            "child_run_id": self.child_run_id,
            "child_attempt_id": self.child_attempt_id,
            "child_cell_id": self.child_cell_id,
            "request_digest": self.request_digest,
            "authority_epoch": self.authority_epoch,
            "fresh_context": self.fresh_context,
            "private_history_excluded": self.private_history_excluded,
        }))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_CHILD_RECEIPT_SCHEMA
            || self.child_session_id.is_empty()
            || self.child_run_id.as_uuid().is_nil()
            || self.child_attempt_id.as_uuid().is_nil()
            || self.child_cell_id.as_uuid().is_nil()
            || !valid_digest(&self.request_digest)
            || self.authority_epoch == 0
            || !self.fresh_context
            || !self.private_history_excluded
            || self.receipt_digest != self.digest()
        {
            return Err("swarm_child_materialization_receipt_invalid");
        }
        Ok(())
    }
}

pub fn materialize_swarm_child(
    request: &SwarmChildMaterializationRequest,
) -> Result<SwarmChildMaterializationReceipt, &'static str> {
    request.validate()?;
    let mut receipt = SwarmChildMaterializationReceipt {
        schema: SWARM_CHILD_RECEIPT_SCHEMA.to_owned(),
        child_session_id: request.child_session_id.clone(),
        child_run_id: request.child_run_id,
        child_attempt_id: request.child_attempt_id,
        child_cell_id: request.child_cell_id,
        request_digest: request.request_digest(),
        authority_epoch: request.authority_epoch,
        fresh_context: true,
        private_history_excluded: true,
        receipt_digest: String::new(),
    };
    receipt.receipt_digest = receipt.digest();
    receipt.validate()?;
    Ok(receipt)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
