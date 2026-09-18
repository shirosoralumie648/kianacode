//! Versioned durable workflow and trigger contracts. No type here grants execution authority.
use crate::{json_digest, CapabilityRequest, CoreResponse, RequestContext, RequestId, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const AUTOMATION_COMMAND: &str = "workflow.command.v1";
pub const AUTOMATION_SNAPSHOT: &str = "workflow.snapshot.v1";
pub const AUTOMATION_SCHEMA: &str = "kiana.workflow-command.v1";
pub const AUTOMATION_EVENT_SCHEMA: &str = "kiana.workflow-event.v1";
pub const WORKFLOW_DEFINITION_SCHEMA: &str = "kiana.workflow-definition.v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDefinition {
    pub definition_id: String,
    pub version: u64,
    pub input_keys: Vec<String>,
    pub output_keys: Vec<String>,
    pub allowed_roles: Vec<String>,
    pub max_duration_ms: u64,
    pub max_steps: u32,
    pub nodes: BTreeMap<String, WorkflowNode>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, WorkflowArtifact>,
}

impl WorkflowDefinition {
    /// Canonical, version-bound identity used by events and migration checks.  The wire DTO keeps
    /// the historical shape for compatibility; callers must compute this digest before admitting
    /// or dispatching an instance and must never replace an existing `(id, version)` in place.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": WORKFLOW_DEFINITION_SCHEMA,
            "definition_id": self.definition_id,
            "version": self.version,
            "input_keys": self.input_keys,
            "output_keys": self.output_keys,
            "allowed_roles": self.allowed_roles,
            "max_duration_ms": self.max_duration_ms,
            "max_steps": self.max_steps,
            "nodes": self.nodes,
            "artifacts": self.artifacts,
        }))
    }

    pub fn validate_digest(&self, expected: &str) -> Result<(), String> {
        if expected != self.digest() {
            return Err("workflow_definition_digest_mismatch".to_owned());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    pub dependencies: Vec<String>,
    pub kind: WorkflowNodeKind,
    pub timeout_ms: u64,
    #[serde(default)]
    pub retry_limit: u32,
    pub compensation: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowNodeKind {
    Literal {
        values: BTreeMap<String, Value>,
    },
    CopyInput {
        mapping: BTreeMap<String, String>,
    },
    Gate {
        key: String,
        equals: Value,
    },
    AgentTask {
        project_id: String,
        packet_id: String,
        sandbox: Option<String>,
    },
    Capability {
        request: CapabilityRequest,
    },
    Approval {
        role_id: String,
    },
    WaitSignal {
        signal: String,
    },
    FanOut,
    FanIn,
    SubWorkflow {
        definition_id: String,
        version: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowArtifact {
    pub generated_by: String,
    pub requires: Vec<String>,
    pub output_key: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowInstanceStatus {
    Ready,
    Running,
    WaitingApproval,
    WaitingSignal,
    Paused,
    Retrying,
    Compensating,
    CancelRequested,
    Succeeded,
    Failed,
    Cancelled,
    ResultUnknown,
}
impl WorkflowInstanceStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::ResultUnknown
        )
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeStatus {
    Pending,
    Reserved,
    WaitingApproval,
    WaitingSignal,
    Succeeded,
    Failed,
    Cancelled,
    ResultUnknown,
}
impl WorkflowNodeStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::ResultUnknown
        )
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowNodeExecution {
    pub execution_id: RequestId,
    pub session_id: SessionId,
    pub attempt: u32,
    pub status: WorkflowNodeStatus,
    pub started_at: u64,
    pub lease_expires_at: u64,
    pub ended_at: Option<u64>,
    pub input_digest: String,
    pub output: Value,
    pub error_code: Option<String>,
    pub evidence_refs: Vec<String>,
    pub child_instance_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: u64,
    pub owner_id: String,
    pub role_id: String,
    pub created_at: u64,
    pub deadline: u64,
    pub inputs: BTreeMap<String, Value>,
    pub outputs: BTreeMap<String, Value>,
    pub status: WorkflowInstanceStatus,
    pub steps_used: u32,
    #[serde(default)]
    pub error_code: Option<String>,
    pub nodes: BTreeMap<String, WorkflowNodeExecution>,
    pub signals: BTreeMap<String, Value>,
    pub trigger_id: Option<String>,
    pub parent_instance_id: Option<String>,
    #[serde(default)]
    pub selected_nodes: Option<Vec<String>>,
    #[serde(default)]
    pub retry_counts: BTreeMap<String, u32>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerConcurrency {
    Reject,
    Queue,
    Replace,
    Coalesce,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedSchedulePolicy {
    Skip,
    FireOnce,
    CatchUp,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerSchedule {
    Manual,
    Event { kind: String },
    Interval { every_ms: u64, first_at: u64 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDefinition {
    pub trigger_id: String,
    pub definition_id: String,
    pub definition_version: u64,
    pub inputs: BTreeMap<String, Value>,
    pub owner_id: String,
    pub role_id: String,
    pub expires_at: u64,
    pub max_firings: u32,
    pub concurrency: TriggerConcurrency,
    pub missed_schedule: MissedSchedulePolicy,
    pub schedule: TriggerSchedule,
    pub approval_ref: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DurableTrigger {
    pub definition: TriggerDefinition,
    pub enabled: bool,
    pub next_at: Option<u64>,
    pub firings_used: u32,
    pub pending_keys: Vec<String>,
    pub fired: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCommandRequest {
    pub schema: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub command: AutomationCommand,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationCommand {
    RegisterDefinition {
        definition: WorkflowDefinition,
    },
    Start {
        instance_id: String,
        definition_id: String,
        version: u64,
        inputs: BTreeMap<String, Value>,
    },
    Advance {
        instance_id: String,
    },
    Reconcile {
        instance_id: String,
        node_id: String,
    },
    Decide {
        instance_id: String,
        node_id: String,
        approve: bool,
        evidence_ref: String,
    },
    Signal {
        instance_id: String,
        signal: String,
        value: Value,
        evidence_ref: String,
    },
    Pause {
        instance_id: String,
    },
    Resume {
        instance_id: String,
    },
    Cancel {
        instance_id: String,
        reason: String,
    },
    Retry {
        instance_id: String,
        node_id: String,
    },
    Compensate {
        instance_id: String,
        compensation_instance_id: String,
    },
    RegisterTrigger {
        trigger: TriggerDefinition,
    },
    DisableTrigger {
        trigger_id: String,
    },
    Tick {
        trigger_id: String,
    },
    Fire {
        trigger_id: String,
        firing_key: String,
        event_ref: Option<String>,
    },
    /// Only ControlPlane can submit a runtime observation, never a wire caller.
    RecordObservation {
        instance_id: String,
        node_id: String,
    },
}
impl AutomationCommand {
    pub fn instance_id(&self) -> Option<&str> {
        match self {
            Self::Advance { instance_id }
            | Self::Reconcile { instance_id, .. }
            | Self::Decide { instance_id, .. }
            | Self::Signal { instance_id, .. }
            | Self::Pause { instance_id }
            | Self::Resume { instance_id }
            | Self::Cancel { instance_id, .. }
            | Self::Retry { instance_id, .. }
            | Self::Compensate { instance_id, .. }
            | Self::RecordObservation { instance_id, .. } => Some(instance_id),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AutomationState {
    pub revision: u64,
    pub definitions: BTreeMap<String, WorkflowDefinition>,
    pub instances: BTreeMap<String, WorkflowInstance>,
    pub triggers: BTreeMap<String, DurableTrigger>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationAuthority {
    pub context: RequestContext,
    pub now_ms: u64,
    pub execution_id: RequestId,
    pub session_id: SessionId,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AutomationProof {
    pub response: Option<CoreResponse>,
    pub evidence_refs: Vec<String>,
    pub event_kind: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationEvent {
    pub schema: String,
    pub request: AutomationCommandRequest,
    pub authority: AutomationAuthority,
    pub proof: AutomationProof,
}
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowEffect {
    Dispatch {
        instance_id: String,
        node_id: String,
        kind: WorkflowNodeKind,
        execution_id: RequestId,
        session_id: SessionId,
    },
    Cancel {
        instance_id: String,
    },
}

pub type WorkflowValue = serde_json::Value;
/// A deterministic content fingerprint for replay identity, not a cryptographic signature.
pub fn workflow_input_digest(inputs: &BTreeMap<String, Value>) -> String {
    let bytes = serde_json::to_vec(inputs).unwrap_or_default();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}
