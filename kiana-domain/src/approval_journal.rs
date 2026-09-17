//! Approval lifecycle facts and prepared mutations for the shared authority transaction.
use crate::{
    canonical_journal_bytes, json_digest, AggregateVersion, ApprovalChallenge, ApprovalDecision,
    ApprovalId, ApprovalState, PendingApproval, RequestId, RuntimeEvent, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APPROVAL_MATERIAL_SCHEMA: &str = "kiana.approval-execution-material.v1";
pub const APPROVAL_MATERIAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const APPROVAL_DECISION_FACT_SCHEMA: &str = "kiana.approval-decision-fact.v1";
pub const APPROVAL_CONSUMPTION_FACT_SCHEMA: &str = "kiana.approval-consumption-fact.v1";
pub const APPROVAL_FACT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMaterialState {
    InlineRedacted,
    VolatileProtected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalExecutionMaterial {
    pub schema: String,
    pub version: SchemaVersion,
    pub payload_digest: String,
    pub preview_digest: String,
    pub state: ApprovalMaterialState,
    pub expires_at_unix_ms: u64,
    pub material_digest: String,
}

impl Default for ApprovalExecutionMaterial {
    fn default() -> Self {
        Self {
            schema: String::new(),
            version: SchemaVersion::new(0, 0),
            payload_digest: String::new(),
            preview_digest: String::new(),
            state: ApprovalMaterialState::InlineRedacted,
            expires_at_unix_ms: 0,
            material_digest: String::new(),
        }
    }
}

impl ApprovalExecutionMaterial {
    pub fn new(
        payload_digest: impl Into<String>,
        preview_digest: impl Into<String>,
        state: ApprovalMaterialState,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut material = Self {
            schema: APPROVAL_MATERIAL_SCHEMA.to_owned(),
            version: APPROVAL_MATERIAL_VERSION,
            payload_digest: payload_digest.into(),
            preview_digest: preview_digest.into(),
            state,
            expires_at_unix_ms,
            material_digest: String::new(),
        };
        material.material_digest = material.digest();
        material.validate()?;
        Ok(material)
    }

    pub fn from_payloads(
        payload: &Value,
        preview: &Value,
        state: ApprovalMaterialState,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let payload_digest = format!(
            "sha256:{}",
            crate::journal_sha256(&canonical_journal_bytes(payload)?)
        );
        let preview_digest = format!(
            "sha256:{}",
            crate::journal_sha256(&canonical_journal_bytes(preview)?)
        );
        Self::new(payload_digest, preview_digest, state, expires_at_unix_ms)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let material: Self = serde_json::from_value(value.clone())
            .map_err(|_| "approval_material_decode_failed".to_owned())?;
        material.validate()?;
        Ok(material)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "approval_material_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != APPROVAL_MATERIAL_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_MATERIAL_VERSION)
            || self.expires_at_unix_ms == 0
        {
            return Err("approval_material_header_invalid".to_owned());
        }
        for (digest, field) in [
            (&self.payload_digest, "approval_material_payload_digest"),
            (&self.preview_digest, "approval_material_preview_digest"),
            (&self.material_digest, "approval_material_digest"),
        ] {
            validate_digest(digest, field)?;
        }
        if self.material_digest != self.digest() {
            return Err("approval_material_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn matches_payloads(&self, payload: &Value, preview: &Value) -> Result<bool, String> {
        self.validate()?;
        let expected = Self::from_payloads(payload, preview, self.state, self.expires_at_unix_ms)?;
        Ok(expected.payload_digest == self.payload_digest
            && expected.preview_digest == self.preview_digest)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "payload_digest": self.payload_digest,
            "preview_digest": self.preview_digest,
            "state": self.state,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// The immutable, server-committed decision fact for one approval subject.
///
/// It intentionally carries only typed identities and digests.  In particular, the nonce and
/// payload never need to be copied into a decision event: the staged subject remains the sole
/// source for proof/material validation and `command_id` makes retries idempotent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDecisionFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub subject_request_id: RequestId,
    pub request_hash: String,
    pub decision: ApprovalDecision,
    pub actor_id: String,
    pub command_id: RequestId,
    pub expected_version: u64,
    pub authority_version: AggregateVersion,
    pub decided_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub decision_digest: String,
}

impl ApprovalDecisionFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: ApprovalId,
        subject_request_id: RequestId,
        request_hash: impl Into<String>,
        decision: ApprovalDecision,
        actor_id: impl Into<String>,
        command_id: RequestId,
        expected_version: u64,
        authority_version: AggregateVersion,
        decided_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: APPROVAL_DECISION_FACT_SCHEMA.to_owned(),
            version: APPROVAL_FACT_VERSION,
            approval_id,
            subject_request_id,
            request_hash: request_hash.into(),
            decision,
            actor_id: actor_id.into(),
            command_id,
            expected_version,
            authority_version,
            decided_at_unix_ms,
            expires_at_unix_ms,
            decision_digest: String::new(),
        };
        fact.decision_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != APPROVAL_DECISION_FACT_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_FACT_VERSION)
            || self.approval_id.as_uuid().is_nil()
            || self.subject_request_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.expected_version == 0
            || self.decided_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.decided_at_unix_ms
            || self.actor_id.trim().is_empty()
            || self.actor_id.len() > 256
            || self.actor_id.contains('\0')
        {
            return Err("approval_decision_fact_header_invalid".to_owned());
        }
        validate_digest(&self.request_hash, "approval_decision_fact_request_hash")?;
        if self.authority_version.aggregate_type != "authority"
            || self.authority_version.version == 0
        {
            return Err("approval_decision_fact_authority_invalid".to_owned());
        }
        self.authority_version.validate().map_err(str::to_owned)?;
        validate_digest(&self.decision_digest, "approval_decision_fact_digest")?;
        if self.decision_digest != self.digest() {
            return Err("approval_decision_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "approval_id": self.approval_id,
            "subject_request_id": self.subject_request_id,
            "request_hash": self.request_hash,
            "decision": self.decision,
            "actor_id": self.actor_id,
            "command_id": self.command_id,
            "expected_version": self.expected_version,
            "authority_version": self.authority_version,
            "decided_at_unix_ms": self.decided_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// The separate Approved -> Consumed fact.  A decision never implies that a Broker dispatch has
/// started; only this fact, committed in the dispatch read-set, spends once authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalConsumptionFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub subject_request_id: RequestId,
    pub request_hash: String,
    pub decision_command_id: RequestId,
    pub dispatch_command_id: RequestId,
    pub expected_version: u64,
    pub authority_version: AggregateVersion,
    pub consumed_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub consumption_digest: String,
}

impl ApprovalConsumptionFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: ApprovalId,
        subject_request_id: RequestId,
        request_hash: impl Into<String>,
        decision_command_id: RequestId,
        dispatch_command_id: RequestId,
        expected_version: u64,
        authority_version: AggregateVersion,
        consumed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: APPROVAL_CONSUMPTION_FACT_SCHEMA.to_owned(),
            version: APPROVAL_FACT_VERSION,
            approval_id,
            subject_request_id,
            request_hash: request_hash.into(),
            decision_command_id,
            dispatch_command_id,
            expected_version,
            authority_version,
            consumed_at_unix_ms,
            expires_at_unix_ms,
            consumption_digest: String::new(),
        };
        fact.consumption_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != APPROVAL_CONSUMPTION_FACT_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_FACT_VERSION)
            || self.approval_id.as_uuid().is_nil()
            || self.subject_request_id.as_uuid().is_nil()
            || self.decision_command_id.as_uuid().is_nil()
            || self.dispatch_command_id.as_uuid().is_nil()
            || self.expected_version == 0
            || self.consumed_at_unix_ms == 0
            || self.expires_at_unix_ms < self.consumed_at_unix_ms
        {
            return Err("approval_consumption_fact_header_invalid".to_owned());
        }
        validate_digest(&self.request_hash, "approval_consumption_fact_request_hash")?;
        if self.authority_version.aggregate_type != "authority"
            || self.authority_version.version == 0
        {
            return Err("approval_consumption_fact_authority_invalid".to_owned());
        }
        self.authority_version.validate().map_err(str::to_owned)?;
        validate_digest(&self.consumption_digest, "approval_consumption_fact_digest")?;
        if self.consumption_digest != self.digest() {
            return Err("approval_consumption_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_after_decision(&self, decision: &ApprovalDecisionFact) -> Result<(), String> {
        self.validate()?;
        decision.validate()?;
        if decision.decision != ApprovalDecision::Approve
            || self.approval_id != decision.approval_id
            || self.subject_request_id != decision.subject_request_id
            || self.request_hash != decision.request_hash
            || self.decision_command_id != decision.command_id
            || self.authority_version != decision.authority_version
            || self.expires_at_unix_ms != decision.expires_at_unix_ms
            || self.expected_version != decision.expected_version.saturating_add(1)
            || self.consumed_at_unix_ms < decision.decided_at_unix_ms
        {
            return Err("approval_consumption_fact_decision_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "approval_id": self.approval_id,
            "subject_request_id": self.subject_request_id,
            "request_hash": self.request_hash,
            "decision_command_id": self.decision_command_id,
            "dispatch_command_id": self.dispatch_command_id,
            "expected_version": self.expected_version,
            "authority_version": self.authority_version,
            "consumed_at_unix_ms": self.consumed_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// A prepared event is inert until core commits it together with the dispatch permit or pause.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedApprovalConsumption {
    pub expected_version: AggregateVersion,
    pub authority_versions: Vec<AggregateVersion>,
    pub event: RuntimeEvent,
    pub pending: PendingApproval,
}

/// Read projection for decision retries. Approved and Consumed never mean "execute again".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecisionRecord {
    pub approval_id: ApprovalId,
    pub state: ApprovalState,
    pub challenge: ApprovalChallenge,
    pub decision: Option<ApprovalDecision>,
    pub decision_command_id: Option<RequestId>,
    pub decided_by: Option<String>,
    pub dispatch_command_id: Option<RequestId>,
    pub payload_available: bool,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumption_digest: Option<String>,
}
