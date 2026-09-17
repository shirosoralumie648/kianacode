//! Machine-readable RuntimeEvent kind, version and migration contracts.
//!
//! RuntimeEvent remains a compatibility-shaped envelope. This registry is the server-owned
//! interpretation layer: unknown opaque events remain inspectable, while unknown events in a
//! required product family fail closed before a projector can treat them as authority.

use crate::{RuntimeEvent, SchemaVersion, UnknownEventPolicy, UNKNOWN_EVENT_POLICY};
use serde::Serialize;
use serde_json::Value;

pub const RUNTIME_EVENT_SCHEMA: &str = "kiana.runtime-event.v1";
pub const RUNTIME_EVENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

const REQUEST_IDS: &[&str] = &["request_id"];
const RUN_IDS: &[&str] = &["run_id"];
const INVOCATION_IDS: &[&str] = &["run_id", "capability_request_id"];
const APPROVAL_IDS: &[&str] = &["approval_id"];
const ACTION_IDS: &[&str] = &["request_id", "action_digest"];
const COMMUNICATION_IDS: &[&str] = &["message"];
const COMMUNICATION_LIFECYCLE_IDS: &[&str] = &["message_id"];
const SWARM_TRANSITION_IDS: &[&str] = &["swarm_plan_id"];
const QUALITY_IDS: &[&str] = &["request_id"];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EventKindSpec {
    pub kind: &'static str,
    pub schema: &'static str,
    pub version: SchemaVersion,
    pub owner_crate: &'static str,
    pub aggregate_type: &'static str,
    pub required_ids: &'static [&'static str],
    pub allowed_fields: &'static [&'static str],
    pub terminal: bool,
    pub secret_policy: &'static str,
    pub migration: Option<&'static str>,
}

const RUN_FIELDS: &[&str] = &[
    "run_id",
    "session_id",
    "actor_id",
    "project_root",
    "turn_id",
    "turn",
    "text",
    "step",
    "error",
    "reason",
    "outcome",
    "result",
    "snapshot",
    "harness",
    "sandbox",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "invocation_id",
    "capability_request_id",
    "call_id",
    "request_id",
    "capability",
    "operation",
    "risk",
    "action_digest",
    "arguments",
    "cell_id",
    "capability_grant_id",
    "budget_lease_id",
    "execution_scope",
    "cancellation_state",
    "cancellation_reason",
    "cancellation_targets",
    "cancellation_at_unix_ms",
    "stop_confirmed",
    "cancellation_fact",
    "cancel_actor_id",
];
const REQUEST_FIELDS: &[&str] = &[
    "command",
    "run_id",
    "session_id",
    "actor_id",
    "project_root",
    "role_id",
    "department_id",
    "capability",
    "operation",
    "risk",
    "error",
    "reason",
    "action_digest",
    "request_id",
];
const COMMUNICATION_FIELDS: &[&str] = &[
    "message",
    "message_id",
    "lifecycle",
    "accepted",
    "reason",
    "evidence_refs",
    "authority_granted",
    "project_root",
    "actor_id",
    "session_id",
    "request_id",
];
const SWARM_TRANSITION_FIELDS: &[&str] = &[
    "swarm_plan_id",
    "partition_id",
    "attempt_id",
    "child_cell_id",
    "from",
    "to",
    "revision",
    "authority_epoch",
    "correlation_id",
    "causation_event_id",
    "review_complete",
    "event_digest",
];
const QUALITY_FIELDS: &[&str] = &[
    "request_id",
    "command",
    "dataset_id",
    "suite_id",
    "case_id",
    "experiment_id",
    "result_id",
    "candidate_id",
    "gate_id",
    "decision_id",
    "feedback_id",
    "trace_id",
    "source_cursor",
    "source_digest",
    "payload_digest",
    "reason",
    "status",
    "verdict",
    "evidence_refs",
];
const INVOCATION_FIELDS: &[&str] = &[
    "run_id",
    "turn_id",
    "invocation_id",
    "execution_id",
    "capability_request_id",
    "call_id",
    "operation",
    "capability",
    "attempt",
    "action_digest",
    "args_fingerprint",
    "result",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "stop_requested",
    "stop_confirmed",
    "fenced",
    "started",
    "boundary",
    "decision_id",
    "permit",
    "invocation",
];
const MODEL_ATTEMPT_FIELDS: &[&str] = &[
    "run_id",
    "session_id",
    "turn_id",
    "step",
    "step_id",
    "step_identity",
    "model_call_id",
    "model_request_id",
    "model_attempt_id",
    "attempt_identity",
    "attempt",
    "schema",
    "provider_id",
    "model_id",
    "prepared",
    "route_digest",
    "prompt_version",
    "purpose",
    "streaming",
    "budget",
    "reserved_tokens",
    "prompt_sources",
    "usage",
    "usage_complete",
    "attempted",
    "elapsed_ms",
    "finish",
    "retry_class",
    "assistant",
    "error",
    "cache_usage",
];
const APPROVAL_FIELDS: &[&str] = &[
    "approval_id",
    "request_hash",
    "session_id",
    "actor_id",
    "subject_request_id",
    "run_id",
    "operation",
    "expires_at_unix_ms",
    "decision",
    "scope",
    "capability_request_id",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "error",
];
const ACTION_FIELDS: &[&str] = &[
    "request_id",
    "action_digest",
    "authority_version",
    "authority_key",
    "actor_id",
    "run_id",
    "turn_id",
    "invocation_id",
    "execution_id",
    "capability_request_id",
    "call_id",
    "operation",
    "attempt",
    "result",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
];

macro_rules! spec {
    ($kind:literal, $aggregate:literal, $ids:expr, $fields:expr, $terminal:expr, $migration:expr) => {
        EventKindSpec {
            kind: $kind,
            schema: RUNTIME_EVENT_SCHEMA,
            version: RUNTIME_EVENT_SCHEMA_VERSION,
            owner_crate: "kiana-domain",
            aggregate_type: $aggregate,
            required_ids: $ids,
            allowed_fields: $fields,
            terminal: $terminal,
            secret_policy: "redact_event_value_or_reject",
            migration: $migration,
        }
    };
}

pub const EVENT_KIND_SPECS: &[EventKindSpec] = &[
    spec!(
        "request.accepted",
        "request",
        REQUEST_IDS,
        REQUEST_FIELDS,
        false,
        None
    ),
    spec!(
        "request.rejected",
        "request",
        REQUEST_IDS,
        REQUEST_FIELDS,
        true,
        None
    ),
    spec!(
        "communication.chat",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.command",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.handoff",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.decision",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.status_report",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.evidence",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.incident",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.handoff_acknowledged",
        "communication",
        COMMUNICATION_LIFECYCLE_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.handoff_rejected",
        "communication",
        COMMUNICATION_LIFECYCLE_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.incident_escalated",
        "communication",
        COMMUNICATION_LIFECYCLE_IDS,
        COMMUNICATION_FIELDS,
        false,
        None
    ),
    spec!(
        "delegation.swarm_state",
        "swarm",
        SWARM_TRANSITION_IDS,
        SWARM_TRANSITION_FIELDS,
        false,
        None
    ),
    spec!(
        "delegation.partition_state",
        "swarm",
        SWARM_TRANSITION_IDS,
        SWARM_TRANSITION_FIELDS,
        false,
        None
    ),
    spec!(
        "delegation.attempt_state",
        "swarm",
        SWARM_TRANSITION_IDS,
        SWARM_TRANSITION_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.run",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.capture",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.compare",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "quality.feedback",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "quality.promote",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "quality.rollback",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "run.authorized",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.predecessor",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.started",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.queued",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.prompt",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.delta",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.tool_call",
        "run",
        INVOCATION_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.capability_requested",
        "run",
        INVOCATION_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.capability_blocked",
        "run",
        INVOCATION_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.tool_result",
        "run",
        INVOCATION_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.awaiting_approval",
        "run",
        APPROVAL_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.cancelling",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.resume_prepared",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.completed",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.failed",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.cancelled",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.result_unknown",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.model_turn",
        "run",
        RUN_IDS,
        MODEL_ATTEMPT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.snapshot",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.decision",
        "run",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.blocked",
        "run",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.completed",
        "run",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.failed",
        "run",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.cancelled",
        "run",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.result_unknown",
        "run",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "approval.requested",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.activated",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.approved",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.denied",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.expired",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.cancelled",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.consumed",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.continuation_unavailable",
        "approval",
        APPROVAL_IDS,
        APPROVAL_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "invocation.dispatching",
        "execution_permit",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "invocation.executing",
        "execution_permit",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "execution.prepared",
        "execution_permit",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "execution.result_committed",
        "execution_permit",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        true,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "action.authority_pinned",
        "action",
        ACTION_IDS,
        ACTION_FIELDS,
        false,
        Some("legacy_action_event_v0_to_v1")
    ),
    spec!(
        "result.delivery_claimed",
        "result_delivery",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "session.assigned",
        "session_assignment",
        REQUEST_IDS,
        REQUEST_FIELDS,
        false,
        Some("legacy_session_event_v0_to_v1")
    ),
];

pub const EVENT_MIGRATIONS: &[(&str, u32, u32, &str)] = &[
    ("run", 0, 1, "legacy_run_event_v0_to_v1"),
    ("approval", 0, 1, "legacy_approval_event_v0_to_v1"),
    ("invocation", 0, 1, "legacy_invocation_event_v0_to_v1"),
    ("action", 0, 1, "legacy_action_event_v0_to_v1"),
    ("session", 0, 1, "legacy_session_event_v0_to_v1"),
];

pub fn event_kind_spec(kind: &str) -> Option<&'static EventKindSpec> {
    EVENT_KIND_SPECS.iter().find(|spec| spec.kind == kind)
}

pub fn event_kind_is_required(kind: &str) -> bool {
    [
        "request.",
        "run.",
        "capability.",
        "approval.",
        "invocation.",
        "execution.",
        "action.",
        "result.",
        "session.",
        "communication.",
        "eval.",
        "quality.",
    ]
    .iter()
    .any(|prefix| kind.starts_with(prefix))
}

pub fn unknown_event_policy(kind: &str) -> Result<UnknownEventPolicy, String> {
    if event_kind_spec(kind).is_some() {
        return Ok(UNKNOWN_EVENT_POLICY);
    }
    if event_kind_is_required(kind) {
        return Err("unknown_required_event_kind".to_owned());
    }
    Ok(UNKNOWN_EVENT_POLICY)
}

pub fn check_event_schema_version(kind: &str, incoming: &SchemaVersion) -> Result<(), String> {
    let spec = event_kind_spec(kind).ok_or_else(|| {
        unknown_event_policy(kind)
            .err()
            .unwrap_or_else(|| "event_kind_opaque".to_owned())
    })?;
    if !spec.version.is_compatible_with(incoming) {
        return Err("event_schema_version_incompatible".to_owned());
    }
    Ok(())
}

pub fn event_migration(kind: &str, from_major: u32, to_major: u32) -> Option<&'static str> {
    event_kind_spec(kind).and_then(|spec| {
        spec.migration.filter(|_| {
            EVENT_MIGRATIONS.iter().any(|(family, from, to, _)| {
                kind.starts_with(&format!("{family}.")) && *from == from_major && *to == to_major
            })
        })
    })
}

pub fn validate_event_payload(kind: &str, payload: &Value) -> Result<(), String> {
    let Some(spec) = event_kind_spec(kind) else {
        return unknown_event_policy(kind).map(|_| ());
    };
    let object = payload
        .as_object()
        .ok_or_else(|| "event_payload_object_required".to_owned())?;
    for id in spec.required_ids {
        if !object.get(*id).is_some_and(|value| !value.is_null()) {
            return Err(format!("event_required_id_missing:{id}"));
        }
    }
    if object
        .keys()
        .any(|key| !spec.allowed_fields.iter().any(|allowed| allowed == key))
    {
        return Err("event_payload_unknown_field".to_owned());
    }
    Ok(())
}

pub fn validate_runtime_event(event: &RuntimeEvent) -> Result<(), String> {
    unknown_event_policy(&event.kind)?;
    if event_kind_spec(&event.kind).is_some() {
        validate_event_payload(&event.kind, &event.data)?;
    }
    Ok(())
}
