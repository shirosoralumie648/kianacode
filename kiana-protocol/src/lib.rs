//! Versioned wire contracts for Kiana clients and daemons.

use kiana_domain::CoreResponse;
pub use kiana_domain::{
    ApprovalChallenge, ApprovalDecision, ApprovalId, ExecutionStatus, PermissionProfile, RequestId,
    SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_SCHEMA: &str = "kiana.protocol.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequestMetadata {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub project_root: String,
    pub actor_id: Option<String>,
    pub project_trusted: bool,
    pub permission_profile: PermissionProfile,
}

impl RequestMetadata {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub schema: String,
    pub metadata: RequestMetadata,
    pub body: RequestBody,
}

impl RequestEnvelope {
    pub fn command(metadata: RequestMetadata, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Command(CommandRequest {
                name: name.into(),
                arguments,
            }),
        }
    }

    pub fn approval_decision(
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::ApprovalDecision(ApprovalDecisionRequest {
                approval_id,
                decision,
            }),
        }
    }

    pub fn run(
        metadata: RequestMetadata,
        prompt: impl Into<String>,
        sandbox: Option<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Run(RunRequest {
                prompt: prompt.into(),
                sandbox,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "request", rename_all = "snake_case")]
pub enum RequestBody {
    Command(CommandRequest),
    ApprovalDecision(ApprovalDecisionRequest),
    Run(RunRequest),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecisionRequest {
    pub approval_id: ApprovalId,
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunRequest {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub schema: String,
    pub request_id: RequestId,
    pub status: ExecutionStatus,
    pub output: Value,
    pub error: Option<String>,
}

impl ResponseEnvelope {
    pub fn from_core(response: CoreResponse) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: response.request_id,
            status: response.status,
            output: response.output,
            error: response.error,
        }
    }

    pub fn rejected(request_id: RequestId, reason: impl Into<String>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id,
            status: ExecutionStatus::Blocked,
            output: Value::Null,
            error: Some(reason.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_envelope_round_trip_preserves_security_context_and_arguments() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        metadata.permission_profile = PermissionProfile::Balanced;
        let request = RequestEnvelope::command(
            metadata,
            "system.architecture",
            serde_json::json!({ "format": "json" }),
        );
        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: RequestEnvelope = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
        assert_eq!(decoded.schema, PROTOCOL_SCHEMA);
    }

    #[test]
    fn approval_decision_only_carries_the_daemon_challenge_and_decision() {
        let metadata = RequestMetadata::local("session-1", "/repo");
        let request = RequestEnvelope::approval_decision(
            metadata,
            ApprovalId::new(),
            ApprovalDecision::Approve,
        );
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "approval_decision");
        assert_eq!(encoded["body"]["request"]["decision"], "approve");
        assert!(encoded["body"]["request"].get("arguments").is_none());
        assert!(encoded["body"]["request"].get("request_hash").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn run_envelope_round_trips_prompt_without_capability_payload() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        let request = RequestEnvelope::run(metadata, "map the architecture", None);
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "run");
        assert_eq!(encoded["body"]["request"]["prompt"], "map the architecture");
        assert!(encoded["body"]["request"].get("tools").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }
}
