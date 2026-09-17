//! Approval lifecycle facts and prepared mutations for the shared authority transaction.
use crate::{
    canonical_journal_bytes, json_digest, AggregateVersion, ApprovalChallenge, ApprovalDecision,
    ApprovalId, ApprovalState, PendingApproval, RequestId, RuntimeEvent, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APPROVAL_MATERIAL_SCHEMA: &str = "kiana.approval-execution-material.v1";
pub const APPROVAL_MATERIAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

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
}
