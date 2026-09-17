//! Immutable local execution permits. The journal, rather than this DTO, grants authority.
use crate::{
    AggregateVersion, ApprovalId, CapabilityRequest, ExecutionId, InvocationId, RequestContext,
    RequestId, RunId, SchemaVersion, TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const DISPATCH_PERMIT_SCHEMA: &str = "kiana.dispatch-permit.v1";
pub const DISPATCH_PERMIT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchPermit {
    pub schema: String,
    pub version: SchemaVersion,
    pub execution_id: ExecutionId,
    pub invocation_id: InvocationId,
    pub request_id: RequestId,
    pub run_id: Option<RunId>,
    pub turn_id: Option<TurnId>,
    pub decision_id: String,
    pub approval_id: Option<ApprovalId>,
    pub context: RequestContext,
    pub action_digest: String,
    pub project_identity: serde_json::Value,
    pub authority_versions: Vec<AggregateVersion>,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub permit_digest: String,
}

impl DispatchPermit {
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let permit: Self = serde_json::from_value(value.clone())
            .map_err(|_| "dispatch_permit_decode_failed".to_owned())?;
        permit.validate()?;
        Ok(permit)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "dispatch_permit_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DISPATCH_PERMIT_SCHEMA
            || !self.version.is_compatible_with(&DISPATCH_PERMIT_VERSION)
            || self.execution_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.decision_id.trim().is_empty()
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.run_id.is_some() != self.turn_id.is_some()
            || self.authority_versions.is_empty()
        {
            return Err("dispatch_permit_header_invalid".to_owned());
        }
        if self.context.request_id.as_uuid().is_nil()
            || self.context.session_id.is_empty()
            || self.context.project_root.trim().is_empty()
            || !self.project_identity.is_object()
        {
            return Err("dispatch_permit_context_invalid".to_owned());
        }
        validate_digest(&self.action_digest, "dispatch_permit_action_digest")?;
        let mut seen = std::collections::BTreeSet::new();
        for version in &self.authority_versions {
            version.validate().map_err(str::to_owned)?;
            if !seen.insert((
                version.aggregate_type.as_str(),
                version.aggregate_id.as_str(),
            )) || (version.aggregate_type == "authority" && version.version == 0)
            {
                return Err("dispatch_permit_authority_versions_invalid".to_owned());
            }
        }
        validate_digest(&self.permit_digest, "dispatch_permit_digest")?;
        if self.permit_digest != self.digest() {
            return Err("dispatch_permit_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_request(
        &self,
        request: &CapabilityRequest,
        project_identity: &Value,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.request_id != request.request_id
            || self.action_digest != crate::capability_action_digest(request)
            || &self.project_identity != project_identity
            || now_unix_ms < self.issued_at_unix_ms
            || now_unix_ms >= self.expires_at_unix_ms
        {
            return Err("dispatch_permit_scope_or_expiry_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        crate::json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "execution_id": self.execution_id,
            "invocation_id": self.invocation_id,
            "request_id": self.request_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "decision_id": self.decision_id,
            "approval_id": self.approval_id,
            "context": self.context,
            "action_digest": self.action_digest,
            "project_identity": self.project_identity,
            "authority_versions": self.authority_versions,
            "issued_at_unix_ms": self.issued_at_unix_ms,
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

/// Purpose-separated stable command IDs make network retries and recovery unambiguous.
pub fn derived_request_id(purpose: &str, subject: &str) -> RequestId {
    let mut hash = Sha256::new();
    hash.update(purpose.as_bytes());
    hash.update([0]);
    hash.update(subject.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    RequestId::from_uuid(uuid::Uuid::from_bytes(bytes))
}
