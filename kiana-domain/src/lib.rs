//! Stable domain contracts for the Kiana control plane.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(RequestId);
uuid_id!(RunId);
uuid_id!(EventId);
uuid_id!(ApprovalId);

pub const APPROVAL_CHALLENGE_SCHEMA: &str = "kiana.approval-challenge.v1";

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    #[default]
    Safe,
    Balanced,
    Autonomous,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequestContext {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub project_root: String,
    pub actor_id: Option<String>,
    pub project_trusted: bool,
    pub permission_profile: PermissionProfile,
}

impl RequestContext {
    pub fn local(session_id: impl Into<String>, project_root: impl Into<String>) -> Self {
        Self {
            request_id: RequestId::new(),
            session_id: SessionId::new(session_id),
            project_root: project_root.into(),
            actor_id: Some("local-user".to_owned()),
            project_trusted: false,
            permission_profile: PermissionProfile::Safe,
        }
    }
}

pub const ROLE_BUILDER: &str = "builder";
pub const DEPARTMENT_EXECUTING: &str = "executing";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoleSpec {
    pub role_id: String,
    pub department_id: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub knowledge_grants: Vec<String>,
}

impl RoleSpec {
    pub fn builder() -> Self {
        Self {
            role_id: ROLE_BUILDER.to_owned(),
            department_id: DEPARTMENT_EXECUTING.to_owned(),
            prompt: "You are Kiana's executing Builder. Use only the provided tools `shell` and `apply_patch`. Never request danger-full-access.".to_owned(),
            tools: vec!["shell".to_owned(), "apply_patch".to_owned()],
            knowledge_grants: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DepartmentSpec {
    pub department_id: String,
    pub roles: Vec<String>,
}

impl DepartmentSpec {
    pub fn executing() -> Self {
        Self {
            department_id: DEPARTMENT_EXECUTING.to_owned(),
            roles: vec![ROLE_BUILDER.to_owned()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandIntent {
    pub name: String,
    pub arguments: Value,
}

impl CommandIntent {
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Query,
    Filesystem,
    Process,
    Network,
    Model,
    Secret,
    Sandbox,
    Computer,
    Tool,
    Other(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    ReadOnly,
    LocalWrite,
    ExternalSideEffect,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    pub request_id: RequestId,
    pub capability: CapabilityKind,
    pub operation: String,
    pub arguments: Value,
    pub risk: RiskLevel,
}

impl CapabilityRequest {
    pub fn new(
        request_id: RequestId,
        capability: CapabilityKind,
        operation: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self {
            request_id,
            capability,
            operation: operation.into(),
            arguments,
            risk: RiskLevel::ReadOnly,
        }
    }

    pub fn with_risk(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalChallenge {
    pub schema: String,
    pub approval_id: ApprovalId,
    pub request_id: RequestId,
    pub request_hash: String,
    pub expires_at_unix_ms: u64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingApproval {
    pub challenge: ApprovalChallenge,
    pub request: CapabilityRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthorizedCapabilityRequest {
    pub authorization_id: String,
    pub request: CapabilityRequest,
}

impl AuthorizedCapabilityRequest {
    pub fn new(
        authorization_id: impl Into<String>,
        request: CapabilityRequest,
    ) -> Result<Self, DomainError> {
        let authorization_id = authorization_id.into();
        if authorization_id.trim().is_empty() {
            return Err(DomainError::EmptyAuthorizationId);
        }
        Ok(Self {
            authorization_id,
            request,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityResult {
    pub request_id: RequestId,
    pub success: bool,
    pub output: Value,
    pub evidence_refs: Vec<String>,
}

impl CapabilityResult {
    pub fn success(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            success: true,
            output,
            evidence_refs: Vec::new(),
        }
    }

    pub fn failure(request_id: RequestId, error: impl Into<String>) -> Self {
        Self {
            request_id,
            success: false,
            output: serde_json::json!({ "error": error.into() }),
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow { authorization_id: String },
    Ask { reason: String },
    Deny { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum GateDecision {
    Allowed { authorization_id: String },
    AwaitingApproval { reason: String },
    Denied { reason: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Accepted,
    Denied,
    AwaitingApproval,
    Running,
    Completed,
    Failed,
    ResultUnknown,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub event_id: EventId,
    pub request_id: RequestId,
    pub sequence: u64,
    pub kind: String,
    pub data: Value,
}

impl RuntimeEvent {
    pub fn new(
        request_id: RequestId,
        sequence: u64,
        kind: impl Into<String>,
        data: Value,
    ) -> Result<Self, DomainError> {
        if sequence == 0 {
            return Err(DomainError::InvalidEventSequence);
        }
        Ok(Self {
            event_id: EventId::new(),
            request_id,
            sequence,
            kind: kind.into(),
            data,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoreResponse {
    pub request_id: RequestId,
    pub status: ExecutionStatus,
    pub output: Value,
    pub error: Option<String>,
}

impl CoreResponse {
    pub fn completed(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            status: ExecutionStatus::Completed,
            output,
            error: None,
        }
    }

    pub fn blocked(request_id: RequestId, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            request_id,
            status: ExecutionStatus::Blocked,
            output: Value::Null,
            error: Some(reason),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DomainError {
    #[error("authorization_id_required")]
    EmptyAuthorizationId,
    #[error("event_sequence_must_be_positive")]
    InvalidEventSequence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_context_defaults_to_untrusted() {
        let context = RequestContext::local("session-1", "/repo");
        assert!(!context.project_trusted);
        assert_eq!(context.permission_profile, PermissionProfile::Safe);
    }

    #[test]
    fn v0_2_worker_is_executing_builder() {
        let role = RoleSpec::builder();
        let department = DepartmentSpec::executing();
        assert_eq!(role.role_id, ROLE_BUILDER);
        assert_eq!(role.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(role.tools, ["shell", "apply_patch"]);
        assert_eq!(department.department_id, DEPARTMENT_EXECUTING);
        assert_eq!(department.roles, [ROLE_BUILDER]);
    }

    #[test]
    fn capability_request_serialization_contains_reference_not_secret_value() {
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Secret,
            "resolve",
            serde_json::json!({ "secret_ref": "provider/anthropic" }),
        );
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("secret_ref"));
        assert!(!json.contains("secret_value"));
    }

    #[test]
    fn authorization_and_event_invariants_fail_closed() {
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "search",
            Value::Null,
        );
        assert_eq!(
            AuthorizedCapabilityRequest::new("", request).unwrap_err(),
            DomainError::EmptyAuthorizationId
        );
        assert_eq!(
            RuntimeEvent::new(RequestId::new(), 0, "invalid", Value::Null).unwrap_err(),
            DomainError::InvalidEventSequence
        );
    }
}
