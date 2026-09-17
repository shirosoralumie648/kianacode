//! Server-owned bindings for pausing and resuming one Harness invocation.
//!
//! A pending approval is not a permission to run an arbitrary request later. The continuation
//! must point back to the same Run/Turn/Step/Invocation, the same normalized parameters and
//! catalog, and the same owner/authority snapshot. This value contains only typed identities
//! and digests; the executable request remains the protected approval material.

use crate::{
    canonical_action_input_digest, capability_action_catalog_digest, json_digest,
    CapabilityRequest, InvocationId, RequestContext, RequestId, RunId, SchemaVersion, SessionId,
    StepId, TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const INVOCATION_RESUME_BINDING_SCHEMA: &str = "kiana.invocation-resume-binding.v1";
pub const INVOCATION_RESUME_BINDING_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn parse_id_checked<T>(arguments: &Value, key: &str) -> Result<Option<T>, String>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| format!("invocation_resume_{key}_invalid"))
}

/// Immutable identity and authority facts needed to resume a paused invocation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationResumeBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub event_request_id: RequestId,
    pub turn_id: Option<TurnId>,
    pub step_id: Option<StepId>,
    pub invocation_id: InvocationId,
    pub request_id: RequestId,
    pub parameter_digest: String,
    pub catalog_digest: String,
    pub authority_epoch: u64,
    pub owner_id: String,
    pub session_id: SessionId,
    pub project_digest: String,
    pub sandbox_digest: String,
    /// Digest of the declared pending batch at the time this invocation was emitted.
    pub pending_batch_digest: String,
    pub binding_digest: String,
}

impl InvocationResumeBinding {
    /// Build a binding from the server-prepared request. `pending_batch_digest` is supplied by
    /// the Runner because only it owns the ordered remaining queue.
    pub fn from_request(
        run_id: RunId,
        event_request_id: RequestId,
        request: &CapabilityRequest,
        context: &RequestContext,
        sandbox: &str,
        pending_batch_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let owner_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "invocation_resume_owner_missing".to_owned())?;
        let turn_id = parse_id_checked(&request.arguments, "turn_id")?.or_else(|| {
            request
                .execution_scope
                .as_ref()
                .and_then(|scope| scope.turn_id)
        });
        let step_id = parse_id_checked(&request.arguments, "step_id")?;
        let catalog_digest = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.catalog_digest.clone())
            .unwrap_or_else(capability_action_catalog_digest);
        let authority_epoch = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.authority_epoch)
            .unwrap_or_default();
        let parameter_digest = canonical_action_input_digest(request)?;
        let project_digest = json_digest(&json!({
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "role_id": context.role_id,
            "department_id": context.department_id,
        }));
        let sandbox_digest = json_digest(&json!({ "sandbox": sandbox }));
        let pending_batch_digest = pending_batch_digest.into();
        let mut binding = Self {
            schema: INVOCATION_RESUME_BINDING_SCHEMA.to_owned(),
            version: INVOCATION_RESUME_BINDING_VERSION,
            run_id,
            event_request_id,
            turn_id,
            step_id,
            invocation_id: InvocationId::from_uuid(request.request_id.as_uuid()),
            request_id: request.request_id,
            parameter_digest,
            catalog_digest,
            authority_epoch,
            owner_id: owner_id.to_owned(),
            session_id: context.session_id.clone(),
            project_digest,
            sandbox_digest,
            pending_batch_digest,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INVOCATION_RESUME_BINDING_SCHEMA
            || !self
                .version
                .is_compatible_with(&INVOCATION_RESUME_BINDING_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.event_request_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.invocation_id != InvocationId::from_uuid(self.request_id.as_uuid())
            || self.authority_epoch == 0
            || self.owner_id.trim().is_empty()
            || self.owner_id.len() > 256
            || self.owner_id.contains('\0')
            || self.session_id.is_empty()
        {
            return Err("invocation_resume_binding_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.parameter_digest, "invocation_resume_parameter_digest"),
            (&self.catalog_digest, "invocation_resume_catalog_digest"),
            (&self.project_digest, "invocation_resume_project_digest"),
            (&self.sandbox_digest, "invocation_resume_sandbox_digest"),
            (&self.pending_batch_digest, "invocation_resume_batch_digest"),
            (&self.binding_digest, "invocation_resume_binding_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.binding_digest != self.digest() {
            return Err("invocation_resume_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Recompute all values that can be checked at approval resume. The queue digest is not
    /// recomputed here because the Runner owns the checkpointed queue; it is checked when that
    /// checkpoint is restored.
    pub fn validate_against(
        &self,
        run_id: RunId,
        event_request_id: RequestId,
        request: &CapabilityRequest,
        context: &RequestContext,
        sandbox: &str,
    ) -> Result<(), String> {
        self.validate()?;
        let expected_turn = parse_id_checked(&request.arguments, "turn_id")?.or_else(|| {
            request
                .execution_scope
                .as_ref()
                .and_then(|scope| scope.turn_id)
        });
        let expected_step = parse_id_checked(&request.arguments, "step_id")?;
        let expected_catalog = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.catalog_digest.clone())
            .unwrap_or_else(capability_action_catalog_digest);
        let expected_epoch = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.authority_epoch)
            .unwrap_or_default();
        let owner_id = context.actor_id.as_deref().unwrap_or_default();
        let expected_project = json_digest(&json!({
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "role_id": context.role_id,
            "department_id": context.department_id,
        }));
        let expected_sandbox = json_digest(&json!({ "sandbox": sandbox }));
        if self.run_id != run_id
            || self.event_request_id != event_request_id
            || self.request_id != request.request_id
            || self.invocation_id != InvocationId::from_uuid(request.request_id.as_uuid())
            || self.turn_id != expected_turn
            || self.step_id != expected_step
            || self.parameter_digest != canonical_action_input_digest(request)?
            || self.catalog_digest != expected_catalog
            || self.authority_epoch != expected_epoch
            || self.owner_id != owner_id
            || self.session_id != context.session_id
            || self.project_digest != expected_project
            || self.sandbox_digest != expected_sandbox
        {
            return Err("invocation_resume_binding_changed".to_owned());
        }
        Ok(())
    }

    /// Move the continuation cursor to a freshly committed resume command. The invocation,
    /// parameter and authority identities remain unchanged; only the event stream cursor is
    /// intentionally rebased by ControlPlane recovery.
    pub fn rebind_event_request(&mut self, event_request_id: RequestId) -> Result<(), String> {
        if event_request_id.as_uuid().is_nil() {
            return Err("invocation_resume_event_request_invalid".to_owned());
        }
        self.event_request_id = event_request_id;
        self.binding_digest = self.digest();
        self.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "event_request_id": self.event_request_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "invocation_id": self.invocation_id,
            "request_id": self.request_id,
            "parameter_digest": self.parameter_digest,
            "catalog_digest": self.catalog_digest,
            "authority_epoch": self.authority_epoch,
            "owner_id": self.owner_id,
            "session_id": self.session_id,
            "project_digest": self.project_digest,
            "sandbox_digest": self.sandbox_digest,
            "pending_batch_digest": self.pending_batch_digest,
        }))
    }
}
