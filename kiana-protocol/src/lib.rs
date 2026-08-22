//! Versioned wire contracts for Kiana clients and daemons.

use kiana_domain::CoreResponse;
pub use kiana_domain::{
    normalize_role_path, ApprovalChallenge, ApprovalDecision, ApprovalId, ExecutionStatus,
    PermissionProfile, RequestId, RoleSpec, RunId, SessionId, WorkPacket, DEPARTMENT_EXECUTING,
    ROLE_BUILDER, WORK_PACKET_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_SCHEMA: &str = "kiana.protocol.v1";

fn default_role_id() -> String {
    ROLE_BUILDER.to_owned()
}

fn default_department_id() -> String {
    DEPARTMENT_EXECUTING.to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequestMetadata {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub project_root: String,
    pub actor_id: Option<String>,
    pub project_trusted: bool,
    pub permission_profile: PermissionProfile,
    #[serde(default = "default_role_id")]
    pub role_id: String,
    #[serde(default = "default_department_id")]
    pub department_id: String,
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
            role_id: default_role_id(),
            department_id: default_department_id(),
        }
    }

    pub fn assign_role(&mut self, role: &RoleSpec) {
        self.role_id = role.role_id.clone();
        self.department_id = role.department_id.clone();
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

    pub fn continue_run(
        metadata: RequestMetadata,
        prompt: impl Into<String>,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Continue(ContinueRequest {
                prompt: prompt.into(),
                sandbox,
                run_id,
            }),
        }
    }

    pub fn cancel_run(
        metadata: RequestMetadata,
        run_id: Option<RunId>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Cancel(CancelRequest {
                run_id,
                reason: reason.into(),
            }),
        }
    }

    pub fn spawn(metadata: RequestMetadata, packet: WorkPacket, sandbox: Option<String>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Spawn(SpawnRequest { packet, sandbox }),
        }
    }

    pub fn receipt(metadata: RequestMetadata, run_id: Option<RunId>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Receipt(ReceiptRequest { run_id }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "request", rename_all = "snake_case")]
pub enum RequestBody {
    Command(CommandRequest),
    ApprovalDecision(ApprovalDecisionRequest),
    Run(RunRequest),
    Continue(ContinueRequest),
    Cancel(CancelRequest),
    Receipt(ReceiptRequest),
    Spawn(SpawnRequest),
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
pub struct ContinueRequest {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CancelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReceiptRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub packet: WorkPacket,
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
        assert_eq!(decoded.metadata.role_id, ROLE_BUILDER);
        assert_eq!(decoded.metadata.department_id, DEPARTMENT_EXECUTING);
    }

    #[test]
    fn missing_role_fields_default_to_executing_builder() {
        let json = serde_json::json!({
            "schema": PROTOCOL_SCHEMA,
            "metadata": {
                "request_id": RequestId::new(),
                "session_id": "session-1",
                "project_root": "/repo",
                "actor_id": "local-user",
                "project_trusted": true,
                "permission_profile": "safe"
            },
            "body": {
                "type": "run",
                "request": { "prompt": "hello" }
            }
        });
        let decoded: RequestEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.metadata.role_id, ROLE_BUILDER);
        assert_eq!(decoded.metadata.department_id, DEPARTMENT_EXECUTING);
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

    #[test]
    fn continue_and_cancel_envelopes_round_trip() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        let run_id = RunId::new();
        let continue_request =
            RequestEnvelope::continue_run(metadata.clone(), "keep going", None, Some(run_id));
        let encoded = serde_json::to_value(&continue_request).unwrap();
        assert_eq!(encoded["body"]["type"], "continue");
        assert_eq!(encoded["body"]["request"]["prompt"], "keep going");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            continue_request
        );

        let cancel_request = RequestEnvelope::cancel_run(metadata.clone(), Some(run_id), "user");
        let encoded = serde_json::to_value(&cancel_request).unwrap();
        assert_eq!(encoded["body"]["type"], "cancel");
        assert_eq!(encoded["body"]["request"]["reason"], "user");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            cancel_request
        );

        let receipt_request = RequestEnvelope::receipt(metadata, Some(run_id));
        let encoded = serde_json::to_value(&receipt_request).unwrap();
        assert_eq!(encoded["body"]["type"], "receipt");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            receipt_request
        );
    }

    #[test]
    fn spawn_envelope_round_trips_packet_without_transcript() {
        let mut metadata = RequestMetadata::local("builder-1", "/repo");
        metadata.project_trusted = true;
        metadata.assign_role(&RoleSpec::builder());
        let packet = WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt");
        let request =
            RequestEnvelope::spawn(metadata, packet.clone(), Some("workspace-write".to_owned()));
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "spawn");
        assert_eq!(encoded["body"]["request"]["packet"]["id"], "wp-1");
        assert_eq!(
            encoded["body"]["request"]["packet"]["schema"],
            WORK_PACKET_SCHEMA
        );
        assert!(encoded["body"]["request"].get("prompt").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
        assert!(!packet.as_prompt().contains("PLANNER_SECRET_TOKEN"));
    }
}
