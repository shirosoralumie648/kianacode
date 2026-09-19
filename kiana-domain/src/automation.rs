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
pub const TRIGGER_DEFINITION_SCHEMA: &str = "kiana.trigger-definition.v1";
pub const AUTOMATION_EVENT_ENVELOPE_SCHEMA: &str = "kiana.workflow-event-envelope.v1";
pub const WORKFLOW_PLAN_INTENT_SCHEMA: &str = "kiana.workflow-plan-intent.v1";

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

pub const MAX_INTERVAL_CATCH_UP: u64 = 32;

/// Pure interval cursor result. `next_at` is the first occurrence not consumed by this tick;
/// for CatchUp it may remain due so a later bounded tick can continue the backlog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalDueBatch {
    pub occurrence_keys: Vec<String>,
    pub next_at: u64,
    pub total_due: u64,
    pub emitted: u64,
    pub skipped: u64,
    pub backlog_remaining: u64,
    pub catch_up_limited: bool,
}

/// Calculate one deterministic interval tick without reading time, state or performing I/O.
/// Every emitted key is tied to its exact scheduled timestamp, and cursor arithmetic fails closed
/// on overflow instead of wrapping into a duplicate or distant future occurrence.
pub fn plan_interval_due(
    next_at: u64,
    every_ms: u64,
    now_ms: u64,
    policy: MissedSchedulePolicy,
) -> Result<IntervalDueBatch, String> {
    if next_at == 0 || every_ms < 1_000 || now_ms == 0 {
        return Err("trigger_interval_cursor_invalid".to_owned());
    }
    if now_ms < next_at {
        return Ok(IntervalDueBatch {
            occurrence_keys: Vec::new(),
            next_at,
            total_due: 0,
            emitted: 0,
            skipped: 0,
            backlog_remaining: 0,
            catch_up_limited: false,
        });
    }
    let total_due = (now_ms - next_at)
        .checked_div(every_ms)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?;
    let emitted = match policy {
        MissedSchedulePolicy::Skip if total_due > 1 => 0,
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => 1,
        MissedSchedulePolicy::CatchUp => total_due.min(MAX_INTERVAL_CATCH_UP),
    };
    let advance = match policy {
        MissedSchedulePolicy::CatchUp => emitted,
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => total_due,
    };
    let first_due = next_at;
    let next_at = next_at
        .checked_add(
            every_ms
                .checked_mul(advance)
                .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?,
        )
        .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?;
    let occurrence_keys = (0..emitted)
        .map(|offset| {
            let step = every_ms
                .checked_mul(offset)
                .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?;
            first_due
                .checked_add(step)
                .map(|scheduled_at| format!("schedule:{scheduled_at}"))
                .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let skipped = match policy {
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => total_due - emitted,
        MissedSchedulePolicy::CatchUp => 0,
    };
    let backlog_remaining = match policy {
        MissedSchedulePolicy::CatchUp => total_due - emitted,
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => 0,
    };
    Ok(IntervalDueBatch {
        occurrence_keys,
        next_at,
        total_due,
        emitted,
        skipped,
        backlog_remaining,
        catch_up_limited: policy == MissedSchedulePolicy::CatchUp && backlog_remaining > 0,
    })
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

impl TriggerDefinition {
    /// Validate trigger shape independently from a definition/project store.  The planner adds
    /// owner, approval, role and definition checks; keeping this part in domain prevents a wire
    /// adapter from creating an unbounded or ambiguous occurrence.
    pub fn validate_shape(&self, now_ms: u64) -> Result<(), String> {
        fn bounded(value: &str, max: usize) -> bool {
            !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
        }
        if !bounded(&self.trigger_id, 128)
            || !bounded(&self.definition_id, 128)
            || self.definition_version == 0
            || !bounded(&self.owner_id, 256)
            || !bounded(&self.role_id, 128)
            || !bounded(&self.approval_ref, 512)
            || !self.approval_ref.starts_with("event:")
            || now_ms == 0
            || self.expires_at <= now_ms
            || self.expires_at - now_ms > 31_536_000_000
            || !(1..=1_024).contains(&self.max_firings)
            || self.inputs.len() > 256
            || serde_json::to_vec(&self.inputs).map_or(true, |bytes| bytes.len() > 64 * 1024)
        {
            return Err("trigger_definition_shape_invalid".to_owned());
        }
        for key in self.inputs.keys() {
            if !bounded(key, 128) {
                return Err("trigger_input_key_invalid".to_owned());
            }
        }
        match &self.schedule {
            TriggerSchedule::Manual => {}
            TriggerSchedule::Event { kind } => {
                if !bounded(kind, 128) {
                    return Err("trigger_event_kind_invalid".to_owned());
                }
            }
            TriggerSchedule::Interval { every_ms, first_at } => {
                if *every_ms < 1_000 || *first_at == 0 {
                    return Err("trigger_schedule_invalid".to_owned());
                }
                self.expires_at
                    .checked_add(*every_ms)
                    .ok_or_else(|| "trigger_schedule_overflow".to_owned())?;
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": TRIGGER_DEFINITION_SCHEMA,
            "trigger_id": self.trigger_id,
            "definition_id": self.definition_id,
            "definition_version": self.definition_version,
            "inputs": self.inputs,
            "owner_id": self.owner_id,
            "role_id": self.role_id,
            "expires_at": self.expires_at,
            "max_firings": self.max_firings,
            "concurrency": self.concurrency,
            "missed_schedule": self.missed_schedule,
            "schedule": self.schedule,
            "approval_ref": self.approval_ref,
        }))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DurableTrigger {
    pub definition: TriggerDefinition,
    pub enabled: bool,
    pub next_at: Option<u64>,
    pub firings_used: u32,
    pub pending_keys: Vec<String>,
    #[serde(default)]
    pub pending_digests: BTreeMap<String, String>,
    pub fired: BTreeMap<String, String>,
    #[serde(default)]
    pub fired_digests: BTreeMap<String, String>,
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

/// Canonical envelope for a committed workflow command fact.  RuntimeEvent remains the source of
/// truth; this envelope makes aggregate/CAS/idempotency/cursor joins explicit and replayable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEventEnvelope {
    pub schema: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub stream_version: u64,
    pub source_cursor: u64,
    pub request_id: RequestId,
    pub idempotency_key: String,
    pub command_digest: String,
    pub payload_digest: String,
    pub envelope_digest: String,
}

impl AutomationEventEnvelope {
    pub fn new(
        aggregate_id: impl Into<String>,
        stream_version: u64,
        request_id: RequestId,
        idempotency_key: impl Into<String>,
        command_digest: impl Into<String>,
        payload_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut envelope = Self {
            schema: AUTOMATION_EVENT_ENVELOPE_SCHEMA.to_owned(),
            aggregate_type: "workflow".to_owned(),
            aggregate_id: aggregate_id.into(),
            stream_version,
            source_cursor: stream_version,
            request_id,
            idempotency_key: idempotency_key.into(),
            command_digest: command_digest.into(),
            payload_digest: payload_digest.into(),
            envelope_digest: String::new(),
        };
        envelope.envelope_digest = envelope.digest();
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), String> {
        fn digest(value: &str) -> bool {
            value
                .strip_prefix("sha256:")
                .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        }
        if self.schema != AUTOMATION_EVENT_ENVELOPE_SCHEMA
            || self.aggregate_type != "workflow"
            || self.aggregate_id.trim().is_empty()
            || self.aggregate_id.len() > 512
            || self.stream_version == 0
            || self.source_cursor != self.stream_version
            || self.request_id.as_uuid().is_nil()
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 256
            || !digest(&self.command_digest)
            || !digest(&self.payload_digest)
            || !digest(&self.envelope_digest)
            || self.envelope_digest != self.digest()
        {
            return Err("automation_event_envelope_invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        aggregate_id: &str,
        stream_version: u64,
        request_id: RequestId,
        idempotency_key: &str,
        command_digest: &str,
        payload_digest: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.aggregate_id != aggregate_id
            || self.stream_version != stream_version
            || self.request_id != request_id
            || self.idempotency_key != idempotency_key
            || self.command_digest != command_digest
            || self.payload_digest != payload_digest
        {
            return Err("automation_event_envelope_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "aggregate_type": self.aggregate_type,
            "aggregate_id": self.aggregate_id,
            "stream_version": self.stream_version,
            "source_cursor": self.source_cursor,
            "request_id": self.request_id,
            "idempotency_key": self.idempotency_key,
            "command_digest": self.command_digest,
            "payload_digest": self.payload_digest,
        }))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationEvent {
    pub schema: String,
    pub request: AutomationCommandRequest,
    pub authority: AutomationAuthority,
    pub proof: AutomationProof,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<AutomationEventEnvelope>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowIntentKind {
    Dispatch,
    Reserve,
    Wait,
    Terminal,
    Cancel,
    Noop,
}

/// Pure planner output.  It is an intent/projection only; ControlPlane still commits a fact and
/// owns every dispatch.  The digest binds the input/definition/authority snapshots used to plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlanIntent {
    pub schema: String,
    pub kind: WorkflowIntentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub expected_revision: u64,
    pub next_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_key: Option<String>,
    pub authority_digest: String,
    pub intent_digest: String,
}

impl WorkflowPlanIntent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_PLAN_INTENT_SCHEMA
            || self.next_revision != self.expected_revision.saturating_add(1)
            || self.expected_revision == u64::MAX
            || self.authority_digest.strip_prefix("sha256:").is_none()
            || self.authority_digest.len() != 71
            || !self.authority_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.intent_digest != self.digest()
        {
            return Err("workflow_plan_intent_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "instance_id": self.instance_id,
            "node_id": self.node_id,
            "expected_revision": self.expected_revision,
            "next_revision": self.next_revision,
            "definition_digest": self.definition_digest,
            "input_digest": self.input_digest,
            "queue_key": self.queue_key,
            "authority_digest": self.authority_digest,
        }))
    }
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
