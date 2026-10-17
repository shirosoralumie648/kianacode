//! Versioned, redacted read-model receipts.
//!
//! These DTOs carry only identities, digests and bounded status dimensions. They are projections
//! of committed facts and never contain raw prompts, arguments, secrets or handler output.

use crate::{
    json_digest, CapabilityExecutionState, EventCursor, EventId, ExecutionId, ExecutionStatus,
    InvocationId, RequestId, RunId, SchemaVersion, SessionId, MAX_SOURCE_EVENT_IDS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const RUN_RECEIPT_SCHEMA: &str = "kiana.run-receipt.v1";
pub const EXECUTION_RECEIPT_SCHEMA: &str = "kiana.execution-receipt.v1";
pub const RECEIPT_CONTRACT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_RECEIPT_DIGESTS: usize = 1_024;
pub const MAX_RECEIPT_LIMITATIONS: usize = 16;
const MAX_RECEIPT_LABEL_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    pub project_digest: String,
    pub status: ExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    pub source_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub redaction_profile: String,
    pub feature_status: String,
    pub proof_level: String,
    pub result_digest: String,
    #[serde(default)]
    pub execution_receipt_digests: Vec<String>,
    pub receipt_digest: String,
}

impl RunReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        session_id: SessionId,
        owner_actor_id: Option<String>,
        project_digest: impl Into<String>,
        status: ExecutionStatus,
        terminal_reason: Option<String>,
        source_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        redaction_profile: impl Into<String>,
        feature_status: impl Into<String>,
        proof_level: impl Into<String>,
        result_digest: impl Into<String>,
        mut execution_receipt_digests: Vec<String>,
    ) -> Result<Self, String> {
        execution_receipt_digests.sort();
        execution_receipt_digests.dedup();
        let mut receipt = Self {
            schema: RUN_RECEIPT_SCHEMA.to_owned(),
            version: RECEIPT_CONTRACT_VERSION,
            run_id,
            session_id,
            owner_actor_id,
            project_digest: project_digest.into(),
            status,
            terminal_reason,
            source_cursor,
            source_event_ids,
            redaction_profile: redaction_profile.into(),
            feature_status: feature_status.into(),
            proof_level: proof_level.into(),
            result_digest: result_digest.into(),
            execution_receipt_digests,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let receipt: Self = serde_json::from_value(value.clone())
            .map_err(|_| "run_receipt_decode_failed".to_owned())?;
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "run_receipt_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RUN_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&RECEIPT_CONTRACT_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.session_id.is_empty()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || !unique_ids(&self.source_event_ids)
            || self.owner_actor_id.as_ref().is_some_and(|actor| {
                actor.trim().is_empty() || actor.len() > 256 || actor.contains('\0')
            })
            || !valid_digest(&self.project_digest)
            || !valid_digest(&self.redaction_profile)
            || !valid_digest(&self.result_digest)
            || !valid_feature_status(&self.feature_status)
            || !valid_proof_level(&self.proof_level)
            || self.execution_receipt_digests.len() > MAX_RECEIPT_DIGESTS
            || self
                .execution_receipt_digests
                .iter()
                .any(|digest| !valid_digest(digest))
            || !valid_digest(&self.receipt_digest)
        {
            return Err("run_receipt_header_invalid".to_owned());
        }
        if self.terminal_reason.as_ref().is_some_and(|reason| {
            reason.is_empty() || reason.len() > 4_096 || reason.contains('\0')
        }) {
            return Err("run_receipt_terminal_reason_invalid".to_owned());
        }
        if self.receipt_digest != self.digest() {
            return Err("run_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "session_id": self.session_id,
            "owner_actor_id": self.owner_actor_id,
            "project_digest": self.project_digest,
            "status": self.status,
            "terminal_reason": self.terminal_reason,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "redaction_profile": self.redaction_profile,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "result_digest": self.result_digest,
            "execution_receipt_digests": self.execution_receipt_digests,
        }))
    }
}

/// Server-derived plan/effect evidence attached to an [`ExecutionReceipt`].
///
/// The evidence is a projection of committed facts (descriptor identity, prepared permit,
/// scope/input digests, terminal result payload). Every field is optional because a legacy or
/// denied attempt may genuinely have no plan; a missing field is never filled with a wildcard.
/// This type carries no authority and grants nothing by itself.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionReceiptEvidence {
    pub descriptor_id: Option<String>,
    pub descriptor_digest: Option<String>,
    pub input_digest: Option<String>,
    pub scope_digest: Option<String>,
    pub plan_digest: Option<String>,
    pub approval_id: Option<String>,
    pub backend: Option<String>,
    pub resource_usage_digest: Option<String>,
    pub changes_digest: Option<String>,
    pub result_digest: Option<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<ExecutionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<InvocationId>,
    pub attempt: u32,
    pub action_digest: String,
    pub status: CapabilityExecutionState,
    pub effect_known: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_confirmed: Option<bool>,
    pub fenced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    /// Descriptor/binding identity observed for this attempt, e.g. `shell.exec@kiana.handler-binding.v1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_usage_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes_digest: Option<String>,
    /// Bounded, stable limitations of this receipt projection; never raw diagnostics.
    #[serde(default)]
    pub limitations: Vec<String>,
    pub source_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub redaction_profile: String,
    pub feature_status: String,
    pub proof_level: String,
    pub receipt_digest: String,
}

impl ExecutionReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: RequestId,
        execution_id: Option<ExecutionId>,
        invocation_id: Option<InvocationId>,
        attempt: u32,
        action_digest: impl Into<String>,
        status: CapabilityExecutionState,
        effect_known: bool,
        stop_confirmed: Option<bool>,
        fenced: bool,
        result_digest: Option<String>,
        source_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        redaction_profile: impl Into<String>,
        feature_status: impl Into<String>,
        proof_level: impl Into<String>,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: EXECUTION_RECEIPT_SCHEMA.to_owned(),
            version: RECEIPT_CONTRACT_VERSION,
            request_id,
            execution_id,
            invocation_id,
            attempt,
            action_digest: action_digest.into(),
            status,
            effect_known,
            stop_confirmed,
            fenced,
            result_digest,
            descriptor_id: None,
            descriptor_digest: None,
            input_digest: None,
            scope_digest: None,
            plan_digest: None,
            approval_id: None,
            backend: None,
            resource_usage_digest: None,
            changes_digest: None,
            limitations: Vec::new(),
            source_cursor,
            source_event_ids,
            redaction_profile: redaction_profile.into(),
            feature_status: feature_status.into(),
            proof_level: proof_level.into(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    /// Attach the server-derived plan/effect evidence and rebind the receipt digest.
    ///
    /// Evidence fields that were already set by the constructor are preserved unless the evidence
    /// supplies a value; a revision without evidence cannot silently erase a bound digest. The
    /// combined receipt is re-validated before it is returned, so an invalid digest, unknown
    /// effect or inconsistent limitation fails closed instead of entering the read model.
    pub fn with_evidence(mut self, evidence: ExecutionReceiptEvidence) -> Result<Self, String> {
        self.descriptor_id = evidence.descriptor_id.or(self.descriptor_id);
        self.descriptor_digest = evidence.descriptor_digest.or(self.descriptor_digest);
        self.input_digest = evidence.input_digest.or(self.input_digest);
        self.scope_digest = evidence.scope_digest.or(self.scope_digest);
        self.plan_digest = evidence.plan_digest.or(self.plan_digest);
        self.approval_id = evidence.approval_id.or(self.approval_id);
        self.backend = evidence.backend.or(self.backend);
        self.resource_usage_digest = evidence
            .resource_usage_digest
            .or(self.resource_usage_digest);
        self.changes_digest = evidence.changes_digest.or(self.changes_digest);
        self.result_digest = evidence.result_digest.or(self.result_digest);
        self.limitations = evidence.limitations;
        self.receipt_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let receipt: Self = serde_json::from_value(value.clone())
            .map_err(|_| "execution_receipt_decode_failed".to_owned())?;
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "execution_receipt_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&RECEIPT_CONTRACT_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.execution_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.invocation_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.attempt == 0
            || !valid_digest(&self.action_digest)
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || !unique_ids(&self.source_event_ids)
            || !valid_digest(&self.redaction_profile)
            || !valid_feature_status(&self.feature_status)
            || !valid_proof_level(&self.proof_level)
            || self
                .result_digest
                .as_ref()
                .is_some_and(|digest| !valid_digest(digest))
            || [
                self.descriptor_digest.as_deref(),
                self.input_digest.as_deref(),
                self.scope_digest.as_deref(),
                self.plan_digest.as_deref(),
                self.resource_usage_digest.as_deref(),
                self.changes_digest.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|digest| !valid_digest(digest))
            || [
                self.descriptor_id.as_deref(),
                self.approval_id.as_deref(),
                self.backend.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|label| !valid_label(label))
            || self.limitations.len() > MAX_RECEIPT_LIMITATIONS
            || self
                .limitations
                .iter()
                .any(|limitation| !valid_label(limitation))
            || !valid_digest(&self.receipt_digest)
            || (matches!(self.status, CapabilityExecutionState::Unknown)
                && (self.effect_known || !self.fenced))
            || matches!(self.status, CapabilityExecutionState::Succeeded) && !self.effect_known
            || self.stop_confirmed == Some(false) && !self.fenced
        {
            return Err("execution_receipt_header_invalid".to_owned());
        }
        if self.receipt_digest != self.digest() {
            return Err("execution_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "execution_id": self.execution_id,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "action_digest": self.action_digest,
            "status": self.status,
            "effect_known": self.effect_known,
            "stop_confirmed": self.stop_confirmed,
            "fenced": self.fenced,
            "result_digest": self.result_digest,
            "descriptor_id": self.descriptor_id,
            "descriptor_digest": self.descriptor_digest,
            "input_digest": self.input_digest,
            "scope_digest": self.scope_digest,
            "plan_digest": self.plan_digest,
            "approval_id": self.approval_id,
            "backend": self.backend,
            "resource_usage_digest": self.resource_usage_digest,
            "changes_digest": self.changes_digest,
            "limitations": self.limitations,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "redaction_profile": self.redaction_profile,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
        }))
    }
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RECEIPT_LABEL_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'-' | b'/' | b'@')
        })
}

fn unique_ids(values: &[EventId]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    values
        .iter()
        .all(|id| !id.as_uuid().is_nil() && seen.insert(*id))
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_feature_status(value: &str) -> bool {
    matches!(
        value,
        "implemented" | "partial" | "target" | "deferred" | "not_supported"
    )
}

fn valid_proof_level(value: &str) -> bool {
    matches!(
        value,
        "source" | "local_behavior" | "durable" | "live" | "physical"
    )
}
