//! Redacted approval plan projection.
//!
//! This is a display/read DTO only. It binds the final prepared request and scope digests but
//! never carries an executable secret or grants authority; approval execution still reloads the
//! protected material and re-runs ControlPlane admission.

use crate::{
    json_digest, redact_value, validate_json_limits, ApprovalId, CapabilityKind, PendingApproval,
    RequestContext, RequestId, RiskLevel, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const APPROVAL_PLAN_PREVIEW_SCHEMA: &str = "kiana.approval-plan-preview.v1";
pub const APPROVAL_PLAN_PREVIEW_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_PREVIEW_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalPlanPreview {
    pub schema: String,
    pub version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub request_id: RequestId,
    pub capability: CapabilityKind,
    pub operation: String,
    pub risk: RiskLevel,
    pub payload_digest: String,
    pub preview_digest: String,
    pub scope_digest: String,
    pub environment_digest: String,
    pub expires_at_unix_ms: u64,
    pub payload_available: bool,
    pub preview: Value,
    pub plan_digest: String,
}

impl ApprovalPlanPreview {
    pub fn from_pending(
        pending: &PendingApproval,
        context: &RequestContext,
        payload_available: bool,
    ) -> Result<Self, String> {
        let request_value = serde_json::to_value(&pending.request)
            .map_err(|_| "approval_plan_preview_encode_failed".to_owned())?;
        let preview = redact_value(&request_value);
        let payload_digest = json_digest(&request_value);
        let preview_digest = json_digest(&preview);
        let scope_digest = json_digest(&json!({
            "session_id": context.session_id,
            "actor_id": context.actor_id,
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "permission_profile": context.permission_profile,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "work_packet_id": context.work_packet_id,
            "cell_id": context.cell_id,
            "path_allow": context.path_allow,
        }));
        let environment_digest = json_digest(&json!({
            "sandbox": pending.request.arguments.get("sandbox"),
            "workdir": pending.request.arguments.get("workdir"),
            "path_allow": context.path_allow,
            "mcp_snapshot": pending.request.arguments.get("mcp_snapshot"),
            "memory_snapshot": pending.request.arguments.get("memory_snapshot"),
        }));
        let mut projection = Self {
            schema: APPROVAL_PLAN_PREVIEW_SCHEMA.to_owned(),
            version: APPROVAL_PLAN_PREVIEW_VERSION,
            approval_id: pending.challenge.approval_id,
            request_id: pending.request.request_id,
            capability: pending.request.capability.clone(),
            operation: pending.request.operation.clone(),
            risk: pending.request.risk,
            payload_digest,
            preview_digest,
            scope_digest,
            environment_digest,
            expires_at_unix_ms: pending.challenge.expires_at_unix_ms,
            payload_available,
            preview,
            plan_digest: String::new(),
        };
        projection.plan_digest = projection.digest();
        projection.validate()?;
        Ok(projection)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let projection: Self = serde_json::from_value(value.clone())
            .map_err(|_| "approval_plan_preview_decode_failed".to_owned())?;
        projection.validate()?;
        Ok(projection)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "approval_plan_preview_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != APPROVAL_PLAN_PREVIEW_SCHEMA
            || !self
                .version
                .is_compatible_with(&APPROVAL_PLAN_PREVIEW_VERSION)
            || self.approval_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.operation.trim().is_empty()
            || self.operation.len() > 256
            || self.expires_at_unix_ms == 0
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.preview_digest)
            || !valid_digest(&self.scope_digest)
            || !valid_digest(&self.environment_digest)
            || !valid_digest(&self.plan_digest)
            || !self.preview.is_object()
            || redact_value(&self.preview) != self.preview
            || serde_json::to_vec(&self.preview)
                .map_err(|_| "approval_plan_preview_encode_failed".to_owned())?
                .len()
                > MAX_PREVIEW_BYTES
            || self.preview_digest != json_digest(&self.preview)
        {
            return Err("approval_plan_preview_invalid".to_owned());
        }
        validate_json_limits(&self.preview)
            .map_err(|_| "approval_plan_preview_invalid".to_owned())?;
        if self.plan_digest != self.digest() {
            return Err("approval_plan_preview_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "approval_id": self.approval_id,
            "request_id": self.request_id,
            "capability": self.capability,
            "operation": self.operation,
            "risk": self.risk,
            "payload_digest": self.payload_digest,
            "preview_digest": self.preview_digest,
            "scope_digest": self.scope_digest,
            "environment_digest": self.environment_digest,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "payload_available": self.payload_available,
            "preview": self.preview,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
