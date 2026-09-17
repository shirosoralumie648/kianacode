//! Server-owned request security context.
//!
//! The context is a validated snapshot assembled by the ControlPlane from daemon-owned identity,
//! project identity/trust and the current authority stream.  Wire actor, role, department and
//! trust values are treated as assertions and must match the server snapshot; they never widen it.
//! This value does not execute, authorize, or resolve secrets by itself.

use kiana_domain::{
    canonical_journal_bytes, json_digest, AuthenticatedPrincipalRef, ProjectIdentity,
    RequestContext, RoleSpec, SchemaVersion, SecurityContextId, SecurityReasonCode, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SECURITY_CONTEXT_SCHEMA: &str = "kiana.security-context.v1";
pub const SECURITY_CONTEXT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityContext {
    pub schema: String,
    pub version: SchemaVersion,
    pub security_context_id: SecurityContextId,
    pub principal: AuthenticatedPrincipalRef,
    pub project: ProjectIdentity,
    pub session_id: SessionId,
    pub role_id: String,
    pub department_id: String,
    pub project_trusted: bool,
    pub role_descriptor_digest: String,
    pub policy_revision: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub context_digest: String,
}

impl SecurityContext {
    /// Build a context from server-owned values while checking every caller assertion in `request`.
    /// `server_role` is selected by the daemon's trusted role catalog/assignment path; the role in
    /// the request is never used as an authority source.
    #[allow(clippy::too_many_arguments)]
    pub fn from_server(
        request: &RequestContext,
        principal: AuthenticatedPrincipalRef,
        project: ProjectIdentity,
        server_role: &RoleSpec,
        project_trusted: bool,
        policy_revision: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        principal
            .validate()
            .map_err(|_| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        project
            .validate()
            .map_err(|_| SecurityReasonCode::AuthProjectMismatch.as_str().to_owned())?;
        server_role
            .validate()
            .map_err(|_| SecurityReasonCode::AuthRoleMismatch.as_str().to_owned())?;
        let policy_revision = policy_revision.into();
        validate_server_values(&policy_revision, authority_epoch, data_epoch)?;
        let mut context = Self {
            schema: SECURITY_CONTEXT_SCHEMA.to_owned(),
            version: SECURITY_CONTEXT_VERSION,
            security_context_id: SecurityContextId::new(),
            principal,
            project,
            session_id: request.session_id.clone(),
            role_id: server_role.role_id.clone(),
            department_id: server_role.department_id.clone(),
            project_trusted,
            role_descriptor_digest: json_digest(&json!(server_role.descriptor())),
            policy_revision,
            authority_epoch,
            data_epoch,
            context_digest: String::new(),
        };
        context.context_digest = context.digest();
        context.validate_server()?;
        context.validate_request_assertions(request)?;
        Ok(context)
    }

    /// Alias used by adapters that call the operation “resolve”; it retains the same strict rules.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        request: &RequestContext,
        principal: AuthenticatedPrincipalRef,
        project: ProjectIdentity,
        server_role: &RoleSpec,
        project_trusted: bool,
        policy_revision: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        Self::from_server(
            request,
            principal,
            project,
            server_role,
            project_trusted,
            policy_revision,
            authority_epoch,
            data_epoch,
        )
    }

    pub fn validate_server(&self) -> Result<(), String> {
        if self.schema != SECURITY_CONTEXT_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_CONTEXT_VERSION)
            || self.security_context_id.as_uuid().is_nil()
            || self.session_id.is_empty()
            || self.role_id.trim().is_empty()
            || self.department_id.trim().is_empty()
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("security_context_header_invalid".to_owned());
        }
        self.principal
            .validate()
            .map_err(|_| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        self.project
            .validate()
            .map_err(|_| SecurityReasonCode::AuthProjectMismatch.as_str().to_owned())?;
        if RoleSpec::lookup(&self.role_id)
            .is_none_or(|role| role.department_id != self.department_id)
        {
            return Err(SecurityReasonCode::AuthRoleMismatch.as_str().to_owned());
        }
        validate_digest(&self.role_descriptor_digest, "security_context_role_digest")?;
        let role = RoleSpec::lookup(&self.role_id)
            .ok_or_else(|| SecurityReasonCode::AuthRoleMismatch.as_str().to_owned())?;
        if self.role_descriptor_digest != json_digest(&json!(role.descriptor())) {
            return Err("security_context_role_digest_mismatch".to_owned());
        }
        validate_digest(&self.policy_revision, "security_context_policy_revision")?;
        validate_digest(&self.context_digest, "security_context_digest")?;
        if self.context_digest != self.digest() {
            return Err("security_context_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let context: Self = serde_json::from_value(value.clone())
            .map_err(|_| "security_context_decode_failed".to_owned())?;
        context.validate_server()?;
        Ok(context)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "security_context_encode_failed".to_owned())
    }

    /// Reject caller-provided identity fields that differ from this server-owned snapshot.
    pub fn validate_request_assertions(&self, request: &RequestContext) -> Result<(), String> {
        let actor = request
            .actor_id
            .as_deref()
            .filter(|actor| !actor.trim().is_empty())
            .ok_or_else(|| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        if actor != self.principal.principal_id {
            return Err(SecurityReasonCode::AuthCallerUntrusted.as_str().to_owned());
        }
        if request.session_id != self.session_id {
            return Err(SecurityReasonCode::AuthSessionExpired.as_str().to_owned());
        }
        if request.project_root != self.project.root
            && request.project_root != self.project.canonical_root
        {
            return Err(SecurityReasonCode::AuthProjectMismatch.as_str().to_owned());
        }
        if request.role_id != self.role_id {
            return Err(SecurityReasonCode::AuthRoleMismatch.as_str().to_owned());
        }
        if !request.department_id.trim().is_empty() && request.department_id != self.department_id {
            return Err(SecurityReasonCode::AuthRoleMismatch.as_str().to_owned());
        }
        if request.project_trusted && !self.project_trusted {
            return Err(SecurityReasonCode::AuthCallerUntrusted.as_str().to_owned());
        }
        Ok(())
    }

    /// Apply the server snapshot to the legacy RequestContext after assertions have passed.
    pub fn apply_to_request(&self, mut request: RequestContext) -> Result<RequestContext, String> {
        self.validate_request_assertions(&request)?;
        request.actor_id = Some(self.principal.principal_id.clone());
        request.project_root = self.project.root.clone();
        request.project_trusted = self.project_trusted;
        request.role_id = self.role_id.clone();
        request.department_id = self.department_id.clone();
        Ok(request)
    }

    pub fn require_trusted_for_effect(&self) -> Result<(), String> {
        self.project_trusted
            .then_some(())
            .ok_or_else(|| "AUTH_PROJECT_UNTRUSTED".to_owned())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "security_context_id": self.security_context_id,
            "principal": self.principal,
            "project": self.project,
            "session_id": self.session_id,
            "role_id": self.role_id,
            "department_id": self.department_id,
            "project_trusted": self.project_trusted,
            "role_descriptor_digest": self.role_descriptor_digest,
            "policy_revision": self.policy_revision,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
        }))
    }
}

fn validate_server_values(
    policy_revision: &str,
    authority_epoch: u64,
    data_epoch: u64,
) -> Result<(), String> {
    if authority_epoch == 0 || data_epoch == 0 {
        return Err("security_context_epoch_invalid".to_owned());
    }
    validate_digest(policy_revision, "security_context_policy_revision")
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

impl super::ControlPlane {
    /// Resolve a server-owned context for any daemon entrypoint before command dispatch.
    ///
    /// The authority stream is read-only here.  This method never creates a grant, consumes an
    /// approval or calls the Broker; effectful paths still use the existing admission methods.
    pub async fn resolve_security_context(
        &self,
        request: &RequestContext,
        principal: AuthenticatedPrincipalRef,
        project: ProjectIdentity,
        project_trusted: bool,
        server_role: &RoleSpec,
    ) -> Result<SecurityContext, super::CoreError> {
        let authority_epoch = self
            .authority_epoch(&request.project_root)
            .await?
            .unwrap_or(1);
        let authority_revision = self
            .authority_revision(&request.project_root)
            .await?
            .unwrap_or_else(|| "uninitialized".to_owned());
        let policy_revision = json_digest(&json!({
            "authority_revision": authority_revision,
            "role": server_role.descriptor(),
            "project": project.project_id,
        }));
        SecurityContext::from_server(
            request,
            principal,
            project,
            server_role,
            project_trusted,
            policy_revision,
            authority_epoch,
            1,
        )
        .map_err(|reason| kiana_ports::PortError::Failed(reason).into())
    }
}
