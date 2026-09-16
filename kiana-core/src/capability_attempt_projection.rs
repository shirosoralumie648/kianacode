//! Event-derived capability admission/effect observations.
//!
//! The reducer consumes committed request, approval, permit, dispatch, execution, result and
//! cancellation facts.  It never calls the Broker or a handler and it never turns a diagnostic
//! observation into authority.  An effect or stop that cannot be proven remains fenced and
//! `unknown` after restart.

use kiana_domain::{
    json_digest, redact_text, CapabilityAdmissionState, CapabilityApprovalState,
    CapabilityAttemptRecord, CapabilityEffectState, CapabilityErrorCode, CapabilityKind,
    CapabilityStopState, EventId, ExecutionId, InvocationId, RequestId, RunId, RuntimeEvent,
    SpanId, TraceId, TraceStatus, TurnId,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::{ControlPlane, CoreError};

const MAX_OPERATION_BYTES: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CapabilityAttemptProjectionError {
    #[error("capability_attempt_event_run_id_invalid")]
    EventRunIdInvalid,
    #[error("capability_attempt_event_run_id_conflict")]
    EventRunIdConflict,
    #[error("capability_attempt_event_field_invalid:{0}")]
    EventFieldInvalid(&'static str),
    #[error("capability_attempt_cursor_overflow")]
    CursorOverflow,
    #[error("capability_attempt_request_id_missing")]
    RequestIdMissing,
    #[error("capability_attempt_foreign_result:{0}")]
    ForeignAttemptResult(String),
    #[error("capability_attempt_terminal_conflict:{0}")]
    TerminalConflict(String),
    #[error("capability_attempt_record_invalid:{0}")]
    RecordInvalid(String),
}

fn recognized(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_call"
            | "run.capability_requested"
            | "capability.decision"
            | "approval.requested"
            | "run.awaiting_approval"
            | "approval.activated"
            | "approval.approved"
            | "approval.denied"
            | "approval.expired"
            | "approval.cancelled"
            | "approval.consumed"
            | "run.capability_blocked"
            | "capability.blocked"
            | "execution.prepared"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.result_committed"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result"
            | "run.cancelling"
            | "run.cancelled"
            | "run.result_unknown"
    )
}

fn is_terminal_result_event(kind: &str) -> bool {
    matches!(
        kind,
        "execution.result_committed"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result"
    )
}

fn event_run_id(event: &RuntimeEvent) -> Result<Option<RunId>, CapabilityAttemptProjectionError> {
    let aggregate = match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(value)) => Some(
            RunId::parse_str(value).ok_or(CapabilityAttemptProjectionError::EventRunIdInvalid)?,
        ),
        (Some("run"), None) => return Err(CapabilityAttemptProjectionError::EventRunIdInvalid),
        _ => None,
    };
    let payload = match event.data.get("run_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(
            RunId::parse_str(value).ok_or(CapabilityAttemptProjectionError::EventRunIdInvalid)?,
        ),
        Some(_) => {
            return Err(CapabilityAttemptProjectionError::EventFieldInvalid(
                "run_id",
            ))
        }
    };
    if let (Some(payload), Some(aggregate)) = (payload, aggregate) {
        if payload != aggregate {
            return Err(CapabilityAttemptProjectionError::EventRunIdConflict);
        }
    }
    Ok(payload.or(aggregate))
}

fn id_field<T: DeserializeOwned>(value: Option<&Value>) -> Option<T> {
    value.and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn id_from_event<T: DeserializeOwned>(event: &RuntimeEvent, names: &[&str]) -> Option<T> {
    names
        .iter()
        .find_map(|name| id_field(event.data.get(*name)))
}

fn permit_field<T: DeserializeOwned>(event: &RuntimeEvent, name: &str) -> Option<T> {
    event
        .data
        .get("permit")
        .and_then(|permit| permit.get(name))
        .and_then(|value| id_field(Some(value)))
}

fn request_id_for(event: &RuntimeEvent) -> Option<RequestId> {
    id_from_event(
        event,
        &["capability_request_id", "request_id", "subject_request_id"],
    )
    .or_else(|| permit_field(event, "request_id"))
    .or_else(|| {
        matches!(
            event.kind.as_str(),
            "run.tool_call" | "run.capability_requested"
        )
        .then_some(event.request_id)
    })
}

fn invocation_id_for(event: &RuntimeEvent) -> Option<InvocationId> {
    id_from_event(event, &["invocation_id"])
        .or_else(|| permit_field(event, "invocation_id"))
        .or_else(|| request_id_for(event).map(|id| InvocationId::from_uuid(id.as_uuid())))
}

fn explicit_invocation_id_for(event: &RuntimeEvent) -> Option<InvocationId> {
    id_from_event(event, &["invocation_id"]).or_else(|| permit_field(event, "invocation_id"))
}

fn execution_id_for(event: &RuntimeEvent) -> Option<ExecutionId> {
    id_from_event(event, &["execution_id"]).or_else(|| permit_field(event, "execution_id"))
}

fn explicit_execution_id_for(event: &RuntimeEvent) -> Option<ExecutionId> {
    execution_id_for(event)
}

fn turn_id_for(event: &RuntimeEvent) -> Option<TurnId> {
    id_from_event(event, &["turn_id"]).or_else(|| permit_field(event, "turn_id"))
}

fn result_error_code(event: &RuntimeEvent) -> Option<CapabilityErrorCode> {
    let values = [
        event.data.get("error"),
        event.data.get("reason"),
        event
            .data
            .get("result")
            .and_then(|result| result.get("error")),
        event
            .data
            .get("result")
            .and_then(|result| result.get("output"))
            .and_then(|output| output.get("error")),
    ];
    let codes = values
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(CapabilityErrorCode::from_reason))
        .collect::<Vec<_>>();
    codes
        .iter()
        .copied()
        .find(|code| {
            matches!(
                code,
                CapabilityErrorCode::ResultUnknown | CapabilityErrorCode::CompensationRequired
            )
        })
        .or_else(|| codes.into_iter().next())
}

fn approval_id_for(event: &RuntimeEvent) -> Option<String> {
    event
        .data
        .get("approval_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn safe_label(value: Option<&Value>, fallback: &str, max: usize) -> (String, bool) {
    let Some(value) = value.and_then(Value::as_str).map(str::trim) else {
        return (fallback.to_owned(), true);
    };
    if value.is_empty() || value.len() > max {
        return (fallback.to_owned(), true);
    }
    let value = redact_text(value);
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || ".:_/-".contains(byte as char))
    {
        return (fallback.to_owned(), true);
    }
    (value, false)
}

fn capability_label(value: Option<&Value>) -> (String, bool) {
    if let Some(value) = value {
        if let Ok(kind) = serde_json::from_value::<CapabilityKind>(value.clone()) {
            return match kind {
                CapabilityKind::Other(value) => {
                    safe_label(Some(&Value::String(value)), "unknown", 128)
                }
                CapabilityKind::Query => ("query".to_owned(), false),
                CapabilityKind::Filesystem => ("filesystem".to_owned(), false),
                CapabilityKind::Process => ("process".to_owned(), false),
                CapabilityKind::Network => ("network".to_owned(), false),
                CapabilityKind::Model => ("model".to_owned(), false),
                CapabilityKind::Secret => ("secret".to_owned(), false),
                CapabilityKind::Sandbox => ("sandbox".to_owned(), false),
                CapabilityKind::Computer => ("computer".to_owned(), false),
                CapabilityKind::Tool => ("tool".to_owned(), false),
            };
        }
    }
    ("unknown".to_owned(), true)
}

fn operation_for(event: &RuntimeEvent) -> Option<&str> {
    event
        .data
        .get("operation")
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .data
                .get("permit")
                .and_then(|permit| permit.get("operation"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn digest_for(
    event: &RuntimeEvent,
    capability: &str,
    operation: &str,
    request_id: RequestId,
) -> (String, bool) {
    for name in ["action_digest", "args_fingerprint"] {
        if let Some(value) = event.data.get(name).and_then(Value::as_str).map(str::trim) {
            if let Some(hex) = value.strip_prefix("sha256:") {
                if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return (value.to_owned(), false);
                }
            }
        }
    }
    if let Some(value) = event
        .data
        .get("permit")
        .and_then(|permit| permit.get("action_digest"))
        .and_then(Value::as_str)
    {
        if let Some(hex) = value.strip_prefix("sha256:") {
            if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return (value.to_owned(), false);
            }
        }
    }
    (
        json_digest(
            &json!({"capability":capability,"operation":operation,"request_id":request_id}),
        ),
        true,
    )
}

fn parse_attempt(event: &RuntimeEvent) -> (u32, bool) {
    match event.data.get("attempt").and_then(Value::as_u64) {
        Some(value) if value > 0 && value <= u64::from(u32::MAX) => (value as u32, false),
        None => (1, false),
        Some(_) => (1, true),
    }
}

fn bool_field(event: &RuntimeEvent, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| event.data.get(*name).and_then(Value::as_bool))
}

fn safe_error_code(event: &RuntimeEvent) -> Option<String> {
    let raw = [
        event.data.get("error"),
        event.data.get("reason"),
        event
            .data
            .get("result")
            .and_then(|result| result.get("error")),
        event
            .data
            .get("result")
            .and_then(|result| result.get("output"))
            .and_then(|output| output.get("error")),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.get("code").and_then(Value::as_str).map(str::to_owned))
    })?;
    let raw = raw.trim();
    let raw = raw.strip_prefix("result_unknown:").unwrap_or(raw);
    let raw = raw.strip_prefix("cancelled:").unwrap_or(raw);
    let code = raw.split(':').next().unwrap_or(raw).trim();
    if code.is_empty()
        || code.len() > 128
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || "._-".contains(byte as char))
    {
        Some("unclassified".to_owned())
    } else {
        Some(code.to_owned())
    }
}

fn result_success(event: &RuntimeEvent) -> Option<bool> {
    if event.kind == "capability.completed" {
        return Some(true);
    }
    if matches!(
        event.kind.as_str(),
        "capability.failed" | "capability.cancelled"
    ) {
        return Some(false);
    }
    let result = event
        .data
        .get("result")
        .or_else(|| (event.kind.starts_with("capability.")).then_some(&event.data))?;
    result.get("success").and_then(Value::as_bool).or_else(|| {
        (event.kind == "run.tool_result")
            .then_some(())
            .and_then(|_| {
                if result.get("error").is_some() {
                    Some(false)
                } else {
                    result
                        .get("exit_code")
                        .and_then(Value::as_i64)
                        .map(|code| code == 0)
                }
            })
    })
}

fn result_unknown(event: &RuntimeEvent) -> bool {
    event.kind == "capability.result_unknown"
        || event
            .data
            .get("effect_known")
            .and_then(Value::as_bool)
            .is_some_and(|known| !known)
        || result_error_code(event).is_some_and(|code| {
            matches!(
                code,
                CapabilityErrorCode::ResultUnknown | CapabilityErrorCode::CompensationRequired
            )
        })
}

fn terminal_effect(event: &RuntimeEvent) -> CapabilityEffectState {
    if result_unknown(event) {
        CapabilityEffectState::Unknown
    } else if result_success(event) == Some(true) {
        CapabilityEffectState::Succeeded
    } else if result_success(event) == Some(false)
        || matches!(
            event.kind.as_str(),
            "capability.failed" | "capability.cancelled"
        )
    {
        CapabilityEffectState::Failed
    } else {
        CapabilityEffectState::Unknown
    }
}

fn stable_trace_id(run_id: RunId) -> Result<TraceId, CapabilityAttemptProjectionError> {
    let digest = json_digest(&json!({"run_id":run_id,"kind":"kiana.trace"}));
    let hex = digest.strip_prefix("sha256:").ok_or(
        CapabilityAttemptProjectionError::EventFieldInvalid("trace_id"),
    )?;
    TraceId::parse(&hex[..32]).map_err(CapabilityAttemptProjectionError::RecordInvalid)
}

fn stable_span_id(
    run_id: RunId,
    request_id: RequestId,
    attempt: u32,
) -> Result<SpanId, CapabilityAttemptProjectionError> {
    let digest = json_digest(&json!({
        "run_id": run_id,
        "request_id": request_id,
        "attempt": attempt,
        "kind": "kiana.capability-attempt",
    }));
    let hex = digest.strip_prefix("sha256:").ok_or(
        CapabilityAttemptProjectionError::EventFieldInvalid("span_id"),
    )?;
    SpanId::parse(&hex[..16]).map_err(CapabilityAttemptProjectionError::RecordInvalid)
}

#[derive(Clone, Debug)]
struct AttemptState {
    run_id: RunId,
    turn_id: Option<TurnId>,
    invocation_id: Option<InvocationId>,
    execution_id: Option<ExecutionId>,
    request_id: RequestId,
    attempt: u32,
    capability_id: String,
    operation: String,
    action_digest: String,
    admission: CapabilityAdmissionState,
    approval: CapabilityApprovalState,
    effect: CapabilityEffectState,
    stop: CapabilityStopState,
    effect_known: bool,
    stop_confirmed: Option<bool>,
    fenced: bool,
    error_code: Option<String>,
    source_cursor: u64,
    source_event_ids: Vec<EventId>,
    attributes: BTreeMap<String, String>,
    malformed: bool,
    terminal_digest: Option<String>,
}

impl AttemptState {
    fn new(
        run_id: RunId,
        request_id: RequestId,
        attempt: u32,
        capability: String,
        operation: String,
        digest: String,
    ) -> Self {
        Self {
            run_id,
            turn_id: None,
            // A request-id fallback is useful for legacy records, but it is not an identity
            // claim. Keep the slot empty until a committed invocation fact can bind it.
            invocation_id: None,
            execution_id: None,
            request_id,
            attempt,
            capability_id: capability,
            operation,
            action_digest: digest,
            admission: CapabilityAdmissionState::Unknown,
            approval: CapabilityApprovalState::NotRequired,
            effect: CapabilityEffectState::NotStarted,
            stop: CapabilityStopState::NotRequested,
            effect_known: true,
            stop_confirmed: None,
            fenced: false,
            error_code: None,
            source_cursor: 0,
            source_event_ids: Vec::new(),
            attributes: BTreeMap::new(),
            malformed: false,
            terminal_digest: None,
        }
    }

    fn observe(
        &mut self,
        event: &RuntimeEvent,
        cursor: u64,
    ) -> Result<(), CapabilityAttemptProjectionError> {
        let incoming_turn = turn_id_for(event);
        let incoming_invocation = explicit_invocation_id_for(event);
        let incoming_execution = explicit_execution_id_for(event);
        if incoming_turn.is_some_and(|value| self.turn_id.is_some_and(|previous| previous != value))
            || incoming_invocation
                .is_some_and(|value| self.invocation_id.is_some_and(|previous| previous != value))
            || incoming_execution
                .is_some_and(|value| self.execution_id.is_some_and(|previous| previous != value))
        {
            return Err(CapabilityAttemptProjectionError::ForeignAttemptResult(
                self.request_id.to_string(),
            ));
        }
        self.source_cursor = self.source_cursor.max(cursor);
        if !self.source_event_ids.contains(&event.event_id) {
            self.source_event_ids.push(event.event_id);
        }
        self.turn_id = self.turn_id.or(incoming_turn);
        self.invocation_id = self.invocation_id.or(incoming_invocation);
        self.execution_id = self.execution_id.or(incoming_execution);
        let (attempt, attempt_malformed) = parse_attempt(event);
        if attempt != self.attempt {
            if attempt > self.attempt {
                self.attempt = attempt;
                self.effect = CapabilityEffectState::NotStarted;
                self.effect_known = true;
                self.stop = CapabilityStopState::NotRequested;
                self.stop_confirmed = None;
                self.terminal_digest = None;
            } else if attempt < self.attempt {
                return Ok(());
            }
        }
        self.malformed |= attempt_malformed;
        if is_terminal_result_event(&event.kind)
            && matches!(
                self.effect,
                CapabilityEffectState::Succeeded
                    | CapabilityEffectState::Failed
                    | CapabilityEffectState::Unknown
            )
            && self.effect != terminal_effect(event)
        {
            return Err(CapabilityAttemptProjectionError::TerminalConflict(format!(
                "effect_transition:{}",
                self.request_id
            )));
        }
        if let Some(operation) = operation_for(event) {
            let (operation, malformed) = safe_label(
                Some(&Value::String(operation.to_owned())),
                "unknown",
                MAX_OPERATION_BYTES,
            );
            if self.operation == "unknown" || self.operation.is_empty() {
                self.operation = operation;
            } else if self.operation != operation {
                return Err(CapabilityAttemptProjectionError::TerminalConflict(
                    "operation_conflict".to_owned(),
                ));
            }
            self.malformed |= malformed;
        }
        if let Some(value) = event.data.get("capability") {
            let (capability, malformed) = capability_label(Some(value));
            if self.capability_id == "unknown" {
                self.capability_id = capability;
            } else if self.capability_id != capability {
                return Err(CapabilityAttemptProjectionError::TerminalConflict(
                    "capability_conflict".to_owned(),
                ));
            }
            self.malformed |= malformed;
        }
        if let Some(digest) = ["action_digest", "args_fingerprint"]
            .iter()
            .find_map(|name| event.data.get(*name).and_then(Value::as_str))
        {
            if digest.starts_with("sha256:") && digest != self.action_digest {
                if self.action_digest.starts_with("sha256:") && self.action_digest != digest {
                    return Err(CapabilityAttemptProjectionError::TerminalConflict(
                        "action_digest_conflict".to_owned(),
                    ));
                }
                self.action_digest = digest.to_owned();
            }
        }
        match event.kind.as_str() {
            "run.capability_requested" | "run.tool_call" => {
                // The request fact only establishes the attempt identity; policy owns the
                // admission decision recorded by the following event.
            }
            "capability.decision" => {
                let decision = event
                    .data
                    .get("gate")
                    .and_then(|gate| gate.get("decision"))
                    .and_then(Value::as_str)
                    .or_else(|| event.data.get("decision").and_then(Value::as_str));
                match decision.map(str::to_ascii_lowercase).as_deref() {
                    Some("allowed") | Some("allow") | Some("authorized") => {
                        self.admission = CapabilityAdmissionState::Allowed;
                    }
                    Some("awaiting_approval") | Some("ask") | Some("staged") => {
                        self.approval = CapabilityApprovalState::Pending;
                    }
                    Some("denied") | Some("deny") | Some("blocked") => {
                        self.admission = CapabilityAdmissionState::Denied;
                        self.effect = CapabilityEffectState::NotStarted;
                        self.effect_known = true;
                    }
                    _ => self.malformed = true,
                }
            }
            "approval.requested" | "run.awaiting_approval" | "approval.activated" => {
                self.approval = CapabilityApprovalState::Pending;
            }
            "approval.approved" | "approval.consumed" => {
                self.approval = CapabilityApprovalState::Approved;
                self.admission = CapabilityAdmissionState::Allowed;
            }
            "approval.denied" => {
                self.approval = CapabilityApprovalState::Denied;
                self.admission = CapabilityAdmissionState::Denied;
                self.effect = CapabilityEffectState::NotStarted;
                self.effect_known = true;
            }
            "approval.expired" => {
                self.approval = CapabilityApprovalState::Expired;
                self.admission = CapabilityAdmissionState::Denied;
                self.effect = CapabilityEffectState::NotStarted;
                self.effect_known = true;
            }
            "approval.cancelled" => {
                self.approval = CapabilityApprovalState::Cancelled;
                self.admission = CapabilityAdmissionState::Denied;
                self.effect = CapabilityEffectState::NotStarted;
                self.effect_known = true;
            }
            "run.capability_blocked" | "capability.blocked" => {
                self.admission = CapabilityAdmissionState::Denied;
                self.effect = CapabilityEffectState::NotStarted;
                self.effect_known = true;
                self.error_code = safe_error_code(event);
            }
            "execution.prepared" => {
                self.admission = CapabilityAdmissionState::Allowed;
                self.fenced = true;
                self.execution_id = self
                    .execution_id
                    .or_else(|| permit_field(event, "execution_id"));
                self.invocation_id = self
                    .invocation_id
                    .or_else(|| permit_field(event, "invocation_id"));
                self.turn_id = self.turn_id.or_else(|| permit_field(event, "turn_id"));
                if let Some(digest) = permit_field::<String>(event, "action_digest") {
                    if digest.starts_with("sha256:") {
                        self.action_digest = digest;
                    } else {
                        self.malformed = true;
                    }
                }
            }
            "invocation.dispatching" => {
                self.admission = CapabilityAdmissionState::Allowed;
                self.fenced = true;
                if bool_field(event, &["started", "effect_started"]) == Some(true) {
                    self.effect = CapabilityEffectState::Started;
                    self.effect_known = true;
                }
            }
            "invocation.executing" => {
                self.admission = CapabilityAdmissionState::Allowed;
                self.effect = CapabilityEffectState::Started;
                self.effect_known = true;
                self.fenced = true;
            }
            "execution.result_committed" => {
                self.admission = CapabilityAdmissionState::Allowed;
                self.fenced = true;
                let known = event
                    .data
                    .get("effect_known")
                    .and_then(Value::as_bool)
                    .ok_or(CapabilityAttemptProjectionError::EventFieldInvalid(
                        "effect_known",
                    ))?;
                let signature = json_digest(
                    &json!({"effect_known":known,"result":event.data.get("result"),"stop_confirmed":event.data.get("stop_confirmed")}),
                );
                if let Some(previous) = &self.terminal_digest {
                    if previous != &signature {
                        return Err(CapabilityAttemptProjectionError::TerminalConflict(
                            self.request_id.to_string(),
                        ));
                    }
                }
                self.terminal_digest = Some(signature);
                if !known || result_unknown(event) {
                    self.effect = CapabilityEffectState::Unknown;
                    self.effect_known = false;
                    self.fenced = true;
                } else if result_success(event) == Some(true) {
                    self.effect = CapabilityEffectState::Succeeded;
                    self.effect_known = true;
                } else if result_success(event) == Some(false) {
                    self.effect = CapabilityEffectState::Failed;
                    self.effect_known = true;
                } else {
                    self.effect = CapabilityEffectState::Unknown;
                    self.effect_known = false;
                    self.fenced = true;
                    self.malformed = true;
                }
                if let Some(stopped) = bool_field(event, &["stop_confirmed"]) {
                    self.stop_confirmed = Some(stopped);
                    self.stop = if stopped {
                        CapabilityStopState::Confirmed
                    } else {
                        CapabilityStopState::Unconfirmed
                    };
                }
                self.error_code = safe_error_code(event);
            }
            "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result" => {
                if event.kind == "capability.result_unknown" || result_unknown(event) {
                    self.effect = CapabilityEffectState::Unknown;
                    self.effect_known = false;
                    self.fenced = true;
                } else if result_success(event) == Some(true) {
                    self.effect = CapabilityEffectState::Succeeded;
                    self.effect_known = true;
                } else if result_success(event) == Some(false) || event.kind == "capability.failed"
                {
                    self.effect = CapabilityEffectState::Failed;
                    self.effect_known = true;
                } else if event.kind == "capability.cancelled" || event.data["cancelled"] == true {
                    self.effect = CapabilityEffectState::Failed;
                    self.effect_known = true;
                } else {
                    self.effect = CapabilityEffectState::Unknown;
                    self.effect_known = false;
                    self.fenced = true;
                }
                if let Some(stopped) = bool_field(event, &["stop_confirmed"]) {
                    self.stop_confirmed = Some(stopped);
                    self.stop = if stopped {
                        CapabilityStopState::Confirmed
                    } else {
                        CapabilityStopState::Unconfirmed
                    };
                }
                if event.data["cancelled"] == true
                    || safe_error_code(event).as_deref() == Some("user")
                {
                    self.stop = match self.stop_confirmed {
                        Some(true) => CapabilityStopState::Confirmed,
                        Some(false) => CapabilityStopState::Unconfirmed,
                        None => CapabilityStopState::Unknown,
                    };
                }
                self.error_code = safe_error_code(event);
            }
            "run.cancelling" => {
                if self.effect == CapabilityEffectState::Started {
                    self.stop = CapabilityStopState::Requested;
                    self.fenced = true;
                }
            }
            "run.cancelled" | "run.result_unknown" => {
                if matches!(
                    self.effect,
                    CapabilityEffectState::Started | CapabilityEffectState::Unknown
                ) {
                    self.stop = match self.stop_confirmed {
                        Some(true) => CapabilityStopState::Confirmed,
                        Some(false) => CapabilityStopState::Unconfirmed,
                        None => CapabilityStopState::Unknown,
                    };
                    self.fenced = true;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn status(&self) -> TraceStatus {
        if self.admission == CapabilityAdmissionState::Denied {
            return TraceStatus::Error;
        }
        if self.effect == CapabilityEffectState::Unknown
            || matches!(
                self.stop,
                CapabilityStopState::Unconfirmed | CapabilityStopState::Unknown
            )
        {
            return TraceStatus::Unknown;
        }
        match self.effect {
            CapabilityEffectState::Succeeded
                if self.admission == CapabilityAdmissionState::Allowed
                    && self.effect_known
                    && !self.malformed
                    && self.error_code.is_none() =>
            {
                TraceStatus::Ok
            }
            CapabilityEffectState::Failed => TraceStatus::Error,
            CapabilityEffectState::Started => TraceStatus::Unknown,
            CapabilityEffectState::NotStarted
                if self.approval == CapabilityApprovalState::Pending =>
            {
                TraceStatus::Degraded
            }
            CapabilityEffectState::NotStarted => TraceStatus::Unknown,
            CapabilityEffectState::Succeeded => TraceStatus::Degraded,
            CapabilityEffectState::Unknown => TraceStatus::Unknown,
        }
    }

    fn record(self) -> Result<CapabilityAttemptRecord, CapabilityAttemptProjectionError> {
        let zero_effect = self.effect_known && self.effect == CapabilityEffectState::NotStarted;
        let status = self.status();
        let mut attributes = self.attributes;
        attributes.insert(
            "admission".to_owned(),
            format!("{:?}", self.admission).to_ascii_lowercase(),
        );
        attributes.insert(
            "approval".to_owned(),
            format!("{:?}", self.approval).to_ascii_lowercase(),
        );
        attributes.insert(
            "effect".to_owned(),
            format!("{:?}", self.effect).to_ascii_lowercase(),
        );
        attributes.insert(
            "stop".to_owned(),
            format!("{:?}", self.stop).to_ascii_lowercase(),
        );
        attributes.insert("fenced".to_owned(), self.fenced.to_string());
        attributes.insert("zero_effect".to_owned(), zero_effect.to_string());
        if self.malformed {
            attributes.insert(
                "metadata_quality".to_owned(),
                "malformed_or_untrusted".to_owned(),
            );
        }
        let span_id = stable_span_id(self.run_id, self.request_id, self.attempt)?;
        CapabilityAttemptRecord::new(
            stable_trace_id(self.run_id)?,
            span_id,
            Some(self.run_id),
            self.turn_id,
            self.invocation_id
                .or_else(|| Some(InvocationId::from_uuid(self.request_id.as_uuid()))),
            self.execution_id,
            self.request_id,
            self.attempt,
            self.capability_id,
            self.operation,
            self.action_digest,
            self.admission,
            self.approval,
            self.effect,
            self.stop,
            self.effect_known,
            self.stop_confirmed,
            self.fenced,
            zero_effect,
            status,
            self.source_cursor,
            self.source_event_ids,
            self.error_code,
            attributes,
        )
        .map_err(CapabilityAttemptProjectionError::RecordInvalid)
    }
}

/// Fold committed capability facts for one run into one record per `(request_id, attempt)`.
pub fn project_capability_attempts(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<CapabilityAttemptRecord>, CapabilityAttemptProjectionError> {
    let mut run_events = Vec::new();
    let mut approvals = HashSet::new();
    let mut approval_requests = HashMap::new();
    let mut requests = HashSet::new();
    let mut invocations = HashSet::new();
    let mut executions = HashSet::new();
    let mut run_cancelling = false;
    for (index, event) in events.iter().enumerate() {
        if !recognized(&event.kind) {
            continue;
        }
        let event_run = event_run_id(event)?;
        if event_run == Some(run_id) {
            if matches!(
                event.kind.as_str(),
                "run.cancelling" | "run.cancelled" | "run.result_unknown"
            ) {
                run_cancelling = true;
            }
            if let Some(id) = approval_id_for(event) {
                approvals.insert(id.clone());
                if let Some(request_id) = request_id_for(event) {
                    approval_requests.insert(id, request_id);
                }
            }
            if let Some(id) = request_id_for(event) {
                requests.insert(id.to_string());
            }
            if let Some(id) = invocation_id_for(event) {
                invocations.insert(id.to_string());
            }
            if let Some(id) = execution_id_for(event) {
                executions.insert(id.to_string());
            }
            run_events.push((index, event));
        }
    }
    for (index, event) in events.iter().enumerate() {
        if !recognized(&event.kind) || event_run_id(event)?.is_some() {
            continue;
        }
        let linked = approval_id_for(event).is_some_and(|id| approvals.contains(&id))
            || request_id_for(event).is_some_and(|id| requests.contains(&id.to_string()))
            || invocation_id_for(event).is_some_and(|id| invocations.contains(&id.to_string()))
            || execution_id_for(event).is_some_and(|id| executions.contains(&id.to_string()));
        if linked {
            run_events.push((index, event));
        }
    }
    run_events.sort_by_key(|(index, _)| *index);
    let mut seen = HashSet::new();
    let mut states: HashMap<(RequestId, u32), AttemptState> = HashMap::new();
    for (index, event) in run_events {
        if !seen.insert(event.event_id) {
            continue;
        }
        let request_id = request_id_for(event).or_else(|| {
            approval_id_for(event)
                .and_then(|approval_id| approval_requests.get(&approval_id).copied())
        });
        let Some(request_id) = request_id else {
            // Run-level cancellation/terminal facts are applied to existing attempts below;
            // they do not identify a new capability attempt by themselves.
            if matches!(
                event.kind.as_str(),
                "run.cancelling" | "run.cancelled" | "run.result_unknown"
            ) {
                continue;
            }
            return Err(CapabilityAttemptProjectionError::RequestIdMissing);
        };
        let (attempt, _) = parse_attempt(event);
        let (capability_id, capability_malformed) = capability_label(event.data.get("capability"));
        let operation = operation_for(event).unwrap_or("unknown").to_owned();
        let (operation, operation_malformed) = safe_label(
            Some(&Value::String(operation)),
            "unknown",
            MAX_OPERATION_BYTES,
        );
        let (action_digest, digest_malformed) =
            digest_for(event, &capability_id, &operation, request_id);
        if is_terminal_result_event(&event.kind) && !states.contains_key(&(request_id, attempt)) {
            return Err(CapabilityAttemptProjectionError::ForeignAttemptResult(
                format!("{}:{attempt}", request_id),
            ));
        }
        let state = states.entry((request_id, attempt)).or_insert_with(|| {
            AttemptState::new(
                run_id,
                request_id,
                attempt,
                capability_id.clone(),
                operation.clone(),
                action_digest.clone(),
            )
        });
        state.malformed |= capability_malformed || operation_malformed || digest_malformed;
        state.observe(
            event,
            u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or(CapabilityAttemptProjectionError::CursorOverflow)?,
        )?;
    }
    if run_cancelling {
        for state in states.values_mut() {
            if state.effect == CapabilityEffectState::Started
                && state.stop == CapabilityStopState::NotRequested
            {
                state.stop = CapabilityStopState::Requested;
                state.fenced = true;
            }
        }
    }
    states.into_values().map(AttemptState::record).collect()
}

/// Alias for callers that use invocation/effect terminology.
pub fn project_effect_attempts(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<CapabilityAttemptRecord>, CapabilityAttemptProjectionError> {
    project_capability_attempts(run_id, events)
}

impl ControlPlane {
    /// Rebuild broker/effect/stop observations from the persisted EventLog only.
    pub async fn capability_attempts(
        &self,
        run_id: RunId,
    ) -> Result<Vec<CapabilityAttemptRecord>, CoreError> {
        let events = match self.read_all_events().await? {
            Some(events) => events,
            None => self.events.read_stream("run", &run_id.to_string()).await?,
        };
        if !events
            .iter()
            .any(|event| event_run_id(event).ok() == Some(Some(run_id)))
        {
            return Err(kiana_ports::PortError::Failed("run_not_found".to_owned()).into());
        }
        project_capability_attempts(run_id, &events)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    /// Compatibility alias for effect-focused query paths.
    pub async fn effect_attempts(
        &self,
        run_id: RunId,
    ) -> Result<Vec<CapabilityAttemptRecord>, CoreError> {
        self.capability_attempts(run_id).await
    }
}
