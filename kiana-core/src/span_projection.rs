//! Event-derived lifecycle spans.
//!
//! The projector is deliberately read-only.  It folds committed `RuntimeEvent` facts into
//! bounded `SpanLifecycleRecord` rows and never appends a terminal event, grants authority, or
//! treats a runner/UI notification as evidence.  A caller can therefore rebuild the same rows
//! after a restart from the EventLog alone.

use kiana_domain::{
    json_digest, redact_text, CapabilityErrorCode, EventId, ExecutionId, InvocationId, RequestId,
    RunId, RuntimeEvent, SpanEntityKind, SpanId, SpanLifecyclePhase, SpanLifecycleRecord, TraceId,
    TraceStatus, TurnId,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::{ControlPlane, CoreError};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SpanProjectionError {
    #[error("span_event_run_id_invalid")]
    EventRunIdInvalid,
    #[error("span_event_run_id_conflict")]
    EventRunIdConflict,
    #[error("span_event_field_invalid:{0}")]
    EventFieldInvalid(&'static str),
    #[error("span_event_cursor_overflow")]
    CursorOverflow,
    #[error("span_terminal_conflict:{key}:{previous}:{next}")]
    TerminalConflict {
        key: String,
        previous: String,
        next: String,
    },
    #[error("span_record_invalid:{0}")]
    RecordInvalid(String),
}

#[derive(Clone, Debug)]
struct SpanSlot {
    entity: SpanEntityKind,
    run_id: RunId,
    turn_id: Option<TurnId>,
    invocation_id: Option<InvocationId>,
    execution_id: Option<ExecutionId>,
    attempt: Option<u32>,
    latest_attempt: u32,
    terminal: Option<TraceStatus>,
    last_transition: Option<String>,
    last_phase: Option<SpanLifecyclePhase>,
}

impl SpanSlot {
    fn new(
        entity: SpanEntityKind,
        run_id: RunId,
        turn_id: Option<TurnId>,
        invocation_id: Option<InvocationId>,
        execution_id: Option<ExecutionId>,
        attempt: Option<u32>,
    ) -> Self {
        Self {
            entity,
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            attempt,
            latest_attempt: attempt.unwrap_or(0),
            terminal: None,
            last_transition: None,
            last_phase: None,
        }
    }
}

fn event_run_id(event: &RuntimeEvent) -> Result<Option<RunId>, SpanProjectionError> {
    let aggregate = match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(value)) => {
            Some(RunId::parse_str(value).ok_or(SpanProjectionError::EventRunIdInvalid)?)
        }
        (Some("run"), None) => return Err(SpanProjectionError::EventRunIdInvalid),
        _ => None,
    };
    let payload = match event.data.get("run_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            Some(RunId::parse_str(value).ok_or(SpanProjectionError::EventRunIdInvalid)?)
        }
        Some(_) => return Err(SpanProjectionError::EventRunIdInvalid),
    };
    if let (Some(payload), Some(aggregate)) = (payload, aggregate) {
        if payload != aggregate {
            return Err(SpanProjectionError::EventRunIdConflict);
        }
    }
    Ok(payload.or(aggregate))
}

fn optional_field<T: DeserializeOwned>(
    event: &RuntimeEvent,
    name: &'static str,
) -> Result<Option<T>, SpanProjectionError> {
    match event.data.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| SpanProjectionError::EventFieldInvalid(name)),
    }
}

fn request_id_for(event: &RuntimeEvent) -> Result<Option<RequestId>, SpanProjectionError> {
    for name in ["capability_request_id", "request_id", "subject_request_id"] {
        if event.data.get(name).is_some() {
            return optional_field(event, name);
        }
    }
    Ok(None)
}

fn turn_id_for(
    event: &RuntimeEvent,
    current: Option<TurnId>,
) -> Result<Option<TurnId>, SpanProjectionError> {
    if let Some(turn_id) = optional_field(event, "turn_id")? {
        return Ok(Some(turn_id));
    }
    if matches!(
        event.kind.as_str(),
        "run.authorized" | "run.predecessor" | "run.prompt"
    ) {
        return Ok(Some(TurnId::from_uuid(event.request_id.as_uuid())));
    }
    Ok(current)
}

fn invocation_id_for(
    event: &RuntimeEvent,
) -> Result<Option<(InvocationId, Option<RequestId>, bool)>, SpanProjectionError> {
    let request_id = request_id_for(event)?;
    if let Some(invocation_id) = optional_field(event, "invocation_id")? {
        return Ok(Some((invocation_id, request_id, true)));
    }
    if let Some(request_id) = request_id {
        return Ok(Some((
            InvocationId::from_uuid(request_id.as_uuid()),
            Some(request_id),
            false,
        )));
    }
    if let Some(execution_id) = optional_field::<ExecutionId>(event, "execution_id")? {
        return Ok(Some((
            InvocationId::from_uuid(execution_id.as_uuid()),
            None,
            false,
        )));
    }
    Ok(None)
}

fn promote_invocation_alias(
    records: &mut [SpanLifecycleRecord],
    slots: &mut HashMap<String, SpanSlot>,
    run_id: RunId,
    previous: InvocationId,
    replacement: InvocationId,
) -> Result<(), SpanProjectionError> {
    if previous == replacement {
        return Ok(());
    }
    let old_keys = slots
        .iter()
        .filter(|(_, slot)| slot.run_id == run_id && slot.invocation_id == Some(previous))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for old_key in old_keys {
        let Some(mut slot) = slots.remove(&old_key) else {
            continue;
        };
        slot.invocation_id = Some(replacement);
        let new_key = span_key(
            SpanEntityKind::Invocation,
            run_id,
            slot.turn_id,
            Some(replacement),
        );
        if let Some(existing) = slots.get_mut(&new_key) {
            if existing.terminal.is_some()
                && slot.terminal.is_some()
                && existing.terminal != slot.terminal
            {
                return Err(SpanProjectionError::TerminalConflict {
                    key: new_key,
                    previous: format!("{:?}", existing.terminal),
                    next: format!("{:?}", slot.terminal),
                });
            }
            existing.latest_attempt = existing.latest_attempt.max(slot.latest_attempt);
            existing.attempt = existing.attempt.or(slot.attempt);
            existing.execution_id = existing.execution_id.or(slot.execution_id);
            existing.terminal = existing.terminal.or(slot.terminal);
            existing.last_transition = existing.last_transition.take().or(slot.last_transition);
            existing.last_phase = existing.last_phase.or(slot.last_phase);
        } else {
            slots.insert(new_key, slot);
        }
    }
    for record in records
        .iter_mut()
        .filter(|record| record.run_id == run_id && record.invocation_id == Some(previous))
    {
        record.invocation_id = Some(replacement);
        let key = span_key(
            SpanEntityKind::Invocation,
            run_id,
            record.turn_id,
            Some(replacement),
        );
        record.span_id = stable_span_id(&key)?;
        record.record_digest = record.digest();
        record
            .validate()
            .map_err(SpanProjectionError::RecordInvalid)?;
    }
    Ok(())
}

fn execution_id_for(event: &RuntimeEvent) -> Result<Option<ExecutionId>, SpanProjectionError> {
    optional_field(event, "execution_id")
}

fn attempt_for(event: &RuntimeEvent) -> Result<Option<u32>, SpanProjectionError> {
    let Some(value) = event.data.get("attempt") else {
        return Ok(None);
    };
    let Some(value) = value.as_u64() else {
        return Err(SpanProjectionError::EventFieldInvalid("attempt"));
    };
    let value =
        u32::try_from(value).map_err(|_| SpanProjectionError::EventFieldInvalid("attempt"))?;
    Ok(Some(value))
}

fn stable_trace_id(run_id: RunId) -> Result<TraceId, SpanProjectionError> {
    let digest = json_digest(&json!({"run_id": run_id, "kind": "kiana.trace"}));
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or(SpanProjectionError::EventFieldInvalid("trace_id"))?;
    TraceId::parse(&hex[..32]).map_err(SpanProjectionError::RecordInvalid)
}

fn stable_span_id(key: &str) -> Result<SpanId, SpanProjectionError> {
    let digest = json_digest(&json!({"span_key": key, "kind": "kiana.span"}));
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or(SpanProjectionError::EventFieldInvalid("span_id"))?;
    SpanId::parse(&hex[..16]).map_err(SpanProjectionError::RecordInvalid)
}

fn span_key(
    entity: SpanEntityKind,
    run_id: RunId,
    turn_id: Option<TurnId>,
    invocation_id: Option<InvocationId>,
) -> String {
    match entity {
        SpanEntityKind::Run => format!("run:{run_id}"),
        SpanEntityKind::Turn => format!(
            "turn:{run_id}:{}",
            turn_id.map(|id| id.to_string()).unwrap_or_default()
        ),
        SpanEntityKind::Invocation => format!(
            "invocation:{run_id}:{}",
            invocation_id.map(|id| id.to_string()).unwrap_or_default()
        ),
    }
}

fn bounded_error(event: &RuntimeEvent, fallback: Option<&str>) -> Option<String> {
    let candidate = event
        .data
        .get("error")
        .or_else(|| event.data.get("reason"))
        .and_then(Value::as_str)
        .or(fallback);
    let candidate = candidate?.trim();
    if candidate.is_empty() {
        return None;
    }
    let redacted = redact_text(candidate);
    if redacted.as_bytes().contains(&0) {
        return fallback.map(str::to_owned);
    }
    let mut value = redacted.chars().take(256).collect::<String>();
    if redacted.chars().count() > 256 {
        while value.len() > 253 {
            value.pop();
        }
        value.push('…');
    }
    Some(value)
}

fn transition_attributes(event: &RuntimeEvent) -> std::collections::BTreeMap<String, String> {
    let mut attributes = std::collections::BTreeMap::new();
    for name in [
        "tokens_before",
        "tokens_after",
        "summary_present",
        "stop_confirmed",
        "cancellation_state",
    ] {
        let Some(value) = event.data.get(name) else {
            continue;
        };
        if value.is_boolean() || value.is_number() {
            attributes.insert(name.to_owned(), value.to_string());
        } else if name == "cancellation_state" {
            if let Some(value) = value.as_str() {
                let value = redact_text(value);
                if !value.is_empty() && value.len() <= 64 && !value.contains('\0') {
                    attributes.insert(name.to_owned(), value);
                }
            }
        }
    }
    attributes
}

fn transition_signature(
    phase: SpanLifecyclePhase,
    status: TraceStatus,
    attempt: Option<u32>,
    event: &RuntimeEvent,
    attributes: &std::collections::BTreeMap<String, String>,
) -> String {
    // Start/pause/resume are state boundaries, so equivalent runner/ControlPlane envelopes must
    // not create duplicate rows. Checkpoints retain the event kind because two compactions with
    // different token counts are distinct lifecycle facts; terminal duplicates are handled by
    // the slot's terminal status before this signature is consulted.
    let event_kind = (phase == SpanLifecyclePhase::Checkpointed).then_some(event.kind.as_str());
    json_digest(&json!({
        "phase": phase,
        "status": status,
        "attempt": attempt,
        "event_kind": event_kind,
        "attributes": attributes,
    }))
}

#[allow(clippy::too_many_arguments)]
fn append_transition(
    records: &mut Vec<SpanLifecycleRecord>,
    slots: &mut HashMap<String, SpanSlot>,
    trace_id: &TraceId,
    entity: SpanEntityKind,
    run_id: RunId,
    turn_id: Option<TurnId>,
    invocation_id: Option<InvocationId>,
    execution_id: Option<ExecutionId>,
    attempt: Option<u32>,
    phase: SpanLifecyclePhase,
    status: TraceStatus,
    event: &RuntimeEvent,
    source_cursor: u64,
    error_code: Option<String>,
    attributes: std::collections::BTreeMap<String, String>,
) -> Result<bool, SpanProjectionError> {
    let key = span_key(entity, run_id, turn_id, invocation_id);
    let slot = slots.entry(key.clone()).or_insert_with(|| {
        SpanSlot::new(
            entity,
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            attempt,
        )
    });
    if slot.turn_id.is_none() {
        slot.turn_id = turn_id;
    }
    if slot.invocation_id.is_none() {
        slot.invocation_id = invocation_id;
    }
    if execution_id.is_some() {
        slot.execution_id = execution_id;
    }
    if attempt.is_some() {
        slot.attempt = attempt;
    }
    if phase == SpanLifecyclePhase::Ended {
        if let Some(previous) = slot.terminal {
            if previous != status {
                return Err(SpanProjectionError::TerminalConflict {
                    key,
                    previous: format!("{previous:?}"),
                    next: format!("{status:?}"),
                });
            }
            return Ok(false);
        }
    } else if slot.terminal.is_some() {
        // A late delta, approval or dispatch cannot reopen a terminal span. A new attempt is
        // explicitly reopened by the invocation branch below before reaching this helper.
        return Ok(false);
    }
    let signature = transition_signature(phase, status, attempt, event, &attributes);
    if slot.last_transition.as_deref() == Some(signature.as_str()) {
        return Ok(false);
    }
    let span_id = stable_span_id(&key)?;
    let mut record = SpanLifecycleRecord::new(
        trace_id.clone(),
        span_id,
        entity,
        run_id,
        slot.turn_id,
        slot.invocation_id,
        slot.execution_id,
        slot.attempt,
        phase,
        status,
        event.kind.clone(),
        source_cursor,
        vec![event.event_id],
        error_code,
    )
    .map_err(SpanProjectionError::RecordInvalid)?;
    record.attributes = attributes;
    record.record_digest = record.digest();
    record
        .validate()
        .map_err(SpanProjectionError::RecordInvalid)?;
    records.push(record);
    slot.last_transition = Some(signature);
    slot.last_phase = Some(phase);
    if phase == SpanLifecyclePhase::Ended {
        slot.terminal = Some(status);
    }
    if let Some(attempt) = attempt {
        slot.latest_attempt = slot.latest_attempt.max(attempt);
    }
    Ok(true)
}

fn is_run_terminal(kind: &str) -> bool {
    matches!(
        kind,
        "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
    )
}

fn run_terminal_status(kind: &str) -> TraceStatus {
    match kind {
        "run.completed" => TraceStatus::Ok,
        "run.failed" => TraceStatus::Error,
        "run.cancelled" => TraceStatus::Degraded,
        "run.result_unknown" => TraceStatus::Unknown,
        _ => TraceStatus::Unknown,
    }
}

fn invocation_event_kind(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_call"
            | "run.capability_requested"
            | "capability.decision"
            | "approval.requested"
            | "run.awaiting_approval"
            | "approval.approved"
            | "approval.denied"
            | "run.capability_blocked"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.result_committed"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result"
    )
}

fn invocation_transition(
    event: &RuntimeEvent,
    new_attempt: bool,
) -> Option<(SpanLifecyclePhase, TraceStatus, Option<&'static str>)> {
    if new_attempt
        || matches!(
            event.kind.as_str(),
            "run.tool_call" | "run.capability_requested"
        )
    {
        return Some((SpanLifecyclePhase::Started, TraceStatus::Ok, None));
    }
    match event.kind.as_str() {
        "capability.decision" => match event.data["gate"]["decision"].as_str() {
            Some("awaiting_approval") => Some((
                SpanLifecyclePhase::Paused,
                TraceStatus::Degraded,
                Some("approval_required"),
            )),
            Some("denied") => Some((
                SpanLifecyclePhase::Ended,
                TraceStatus::Error,
                Some("policy_denied"),
            )),
            _ => Some((SpanLifecyclePhase::Resumed, TraceStatus::Ok, None)),
        },
        "approval.requested" | "run.awaiting_approval" => Some((
            SpanLifecyclePhase::Paused,
            TraceStatus::Degraded,
            Some("approval_required"),
        )),
        "approval.approved" => Some((SpanLifecyclePhase::Resumed, TraceStatus::Ok, None)),
        "approval.denied" => Some((
            SpanLifecyclePhase::Ended,
            TraceStatus::Error,
            Some("approval_denied"),
        )),
        "run.capability_blocked" => Some((
            SpanLifecyclePhase::Ended,
            TraceStatus::Error,
            Some("capability_blocked"),
        )),
        "invocation.dispatching" | "invocation.executing" => {
            Some((SpanLifecyclePhase::Resumed, TraceStatus::Ok, None))
        }
        "execution.result_committed" => {
            let effect_known = event
                .data
                .get("effect_known")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !effect_known {
                return Some((
                    SpanLifecyclePhase::Ended,
                    TraceStatus::Unknown,
                    Some("result_unknown"),
                ));
            }
            let result = event.data.get("result").unwrap_or(&Value::Null);
            if result["output"]["cancelled"] == true || result["cancelled"] == true {
                let stop_confirmed = event
                    .data
                    .get("stop_confirmed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || result["output"]["stop_confirmed"] == true;
                return Some((
                    SpanLifecyclePhase::Ended,
                    if stop_confirmed {
                        TraceStatus::Degraded
                    } else {
                        TraceStatus::Unknown
                    },
                    Some(if stop_confirmed {
                        "cancelled"
                    } else {
                        "cancel_stop_unconfirmed"
                    }),
                ));
            }
            match result.get("success").and_then(Value::as_bool) {
                Some(true) => Some((SpanLifecyclePhase::Ended, TraceStatus::Ok, None)),
                Some(false) => Some((
                    SpanLifecyclePhase::Ended,
                    TraceStatus::Error,
                    Some("execution_failed"),
                )),
                None => Some((
                    SpanLifecyclePhase::Ended,
                    TraceStatus::Unknown,
                    Some("result_malformed"),
                )),
            }
        }
        "capability.completed" => Some((SpanLifecyclePhase::Ended, TraceStatus::Ok, None)),
        "capability.failed" => Some((
            SpanLifecyclePhase::Ended,
            TraceStatus::Error,
            Some("capability_failed"),
        )),
        "capability.cancelled" => Some((
            SpanLifecyclePhase::Ended,
            TraceStatus::Degraded,
            Some("cancelled"),
        )),
        "capability.result_unknown" => Some((
            SpanLifecyclePhase::Ended,
            TraceStatus::Unknown,
            Some("result_unknown"),
        )),
        "run.tool_result" => {
            let result_error_code = event
                .data
                .get("result")
                .and_then(|result| result.get("error"))
                .and_then(Value::as_str)
                .map(CapabilityErrorCode::from_reason);
            let unknown = event.data.get("effect_known") == Some(&Value::Bool(false))
                || result_error_code.is_some_and(|code| {
                    matches!(
                        code,
                        CapabilityErrorCode::ResultUnknown
                            | CapabilityErrorCode::CompensationRequired
                    )
                });
            if unknown {
                Some((
                    SpanLifecyclePhase::Ended,
                    TraceStatus::Unknown,
                    Some("result_unknown"),
                ))
            } else if event.data["cancelled"] == true
                || result_error_code == Some(CapabilityErrorCode::Cancelled)
            {
                Some((
                    SpanLifecyclePhase::Ended,
                    TraceStatus::Degraded,
                    Some("cancelled"),
                ))
            } else if event.data["result"]["error"].is_string() {
                Some((
                    SpanLifecyclePhase::Ended,
                    TraceStatus::Error,
                    Some("tool_failed"),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn ensure_turn_started(
    records: &mut Vec<SpanLifecycleRecord>,
    slots: &mut HashMap<String, SpanSlot>,
    trace_id: &TraceId,
    run_id: RunId,
    turn_id: TurnId,
    event: &RuntimeEvent,
    source_cursor: u64,
) -> Result<(), SpanProjectionError> {
    let key = span_key(SpanEntityKind::Turn, run_id, Some(turn_id), None);
    if slots.contains_key(&key) {
        return Ok(());
    }
    append_transition(
        records,
        slots,
        trace_id,
        SpanEntityKind::Turn,
        run_id,
        Some(turn_id),
        None,
        None,
        None,
        SpanLifecyclePhase::Started,
        TraceStatus::Ok,
        event,
        source_cursor,
        None,
        transition_attributes(event),
    )?;
    Ok(())
}

/// Fold committed events into deterministic run/turn/invocation lifecycle span records.
pub fn project_span_lifecycle(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<SpanLifecycleRecord>, SpanProjectionError> {
    let trace_id = stable_trace_id(run_id)?;
    let mut records = Vec::new();
    let mut slots: HashMap<String, SpanSlot> = HashMap::new();
    let mut seen_event_ids: HashSet<EventId> = HashSet::new();
    let mut invocation_aliases: HashMap<RequestId, InvocationId> = HashMap::new();
    let mut current_turn = None;

    for (index, event) in events.iter().enumerate() {
        if !seen_event_ids.insert(event.event_id) {
            continue;
        }
        if event_run_id(event)? != Some(run_id) {
            continue;
        }
        let source_cursor = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(SpanProjectionError::CursorOverflow)?;
        let event_turn = turn_id_for(event, current_turn)?;
        if matches!(
            event.kind.as_str(),
            "run.authorized" | "run.predecessor" | "run.started" | "run.prompt"
        ) {
            current_turn =
                event_turn.or_else(|| Some(TurnId::from_uuid(event.request_id.as_uuid())));
        } else if event_turn.is_some() {
            current_turn = event_turn;
        }

        let run_key = span_key(SpanEntityKind::Run, run_id, None, None);
        if event.kind == "run.prompt" {
            let was_terminal = slots.get(&run_key).and_then(|slot| slot.terminal).is_some();
            if was_terminal {
                if let Some(slot) = slots.get_mut(&run_key) {
                    slot.terminal = None;
                }
            }
            append_transition(
                &mut records,
                &mut slots,
                &trace_id,
                SpanEntityKind::Run,
                run_id,
                None,
                None,
                None,
                None,
                if was_terminal {
                    SpanLifecyclePhase::Resumed
                } else {
                    SpanLifecyclePhase::Started
                },
                TraceStatus::Ok,
                event,
                source_cursor,
                None,
                transition_attributes(event),
            )?;
            if let Some(turn_id) = current_turn {
                ensure_turn_started(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    run_id,
                    turn_id,
                    event,
                    source_cursor,
                )?;
            }
            continue;
        }

        if is_run_terminal(&event.kind) {
            let status = run_terminal_status(&event.kind);
            let inserted = append_transition(
                &mut records,
                &mut slots,
                &trace_id,
                SpanEntityKind::Run,
                run_id,
                None,
                None,
                None,
                None,
                SpanLifecyclePhase::Ended,
                status,
                event,
                source_cursor,
                bounded_error(event, None),
                transition_attributes(event),
            )?;
            if !inserted {
                continue;
            }
            if let Some(turn_id) = current_turn {
                append_transition(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    SpanEntityKind::Turn,
                    run_id,
                    Some(turn_id),
                    None,
                    None,
                    None,
                    SpanLifecyclePhase::Ended,
                    status,
                    event,
                    source_cursor,
                    bounded_error(event, None),
                    transition_attributes(event),
                )?;
            }
            let active = slots
                .values()
                .filter(|slot| slot.entity == SpanEntityKind::Invocation && slot.terminal.is_none())
                .cloned()
                .collect::<Vec<_>>();
            for slot in active {
                append_transition(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    SpanEntityKind::Invocation,
                    slot.run_id,
                    slot.turn_id,
                    slot.invocation_id,
                    slot.execution_id,
                    slot.attempt,
                    SpanLifecyclePhase::Ended,
                    if status == TraceStatus::Degraded {
                        TraceStatus::Degraded
                    } else {
                        TraceStatus::Unknown
                    },
                    event,
                    source_cursor,
                    Some(if status == TraceStatus::Degraded {
                        "cancelled_before_result".to_owned()
                    } else {
                        "run_ended_before_result".to_owned()
                    }),
                    transition_attributes(event),
                )?;
            }
            continue;
        }

        // Once the run span is terminal, late deltas/approval/dispatch facts are retained in the
        // EventLog but cannot mutate this projection. A later explicit prompt is the only reopen.
        if slots.get(&run_key).and_then(|slot| slot.terminal).is_some() {
            continue;
        }

        match event.kind.as_str() {
            "run.authorized" | "run.started" => {
                append_transition(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    SpanEntityKind::Run,
                    run_id,
                    None,
                    None,
                    None,
                    None,
                    SpanLifecyclePhase::Started,
                    TraceStatus::Ok,
                    event,
                    source_cursor,
                    None,
                    transition_attributes(event),
                )?;
                if let Some(turn_id) = current_turn {
                    ensure_turn_started(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        run_id,
                        turn_id,
                        event,
                        source_cursor,
                    )?;
                }
            }
            "run.cancelling" => {
                append_transition(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    SpanEntityKind::Run,
                    run_id,
                    None,
                    None,
                    None,
                    None,
                    SpanLifecyclePhase::Paused,
                    TraceStatus::Degraded,
                    event,
                    source_cursor,
                    bounded_error(event, Some("cancellation_requested")),
                    transition_attributes(event),
                )?;
                if let Some(turn_id) = current_turn {
                    ensure_turn_started(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        run_id,
                        turn_id,
                        event,
                        source_cursor,
                    )?;
                    append_transition(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        SpanEntityKind::Turn,
                        run_id,
                        Some(turn_id),
                        None,
                        None,
                        None,
                        SpanLifecyclePhase::Paused,
                        TraceStatus::Degraded,
                        event,
                        source_cursor,
                        bounded_error(event, Some("cancellation_requested")),
                        transition_attributes(event),
                    )?;
                }
            }
            "run.compacted" => {
                append_transition(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    SpanEntityKind::Run,
                    run_id,
                    None,
                    None,
                    None,
                    None,
                    SpanLifecyclePhase::Checkpointed,
                    TraceStatus::Ok,
                    event,
                    source_cursor,
                    None,
                    transition_attributes(event),
                )?;
                if let Some(turn_id) = current_turn {
                    ensure_turn_started(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        run_id,
                        turn_id,
                        event,
                        source_cursor,
                    )?;
                    append_transition(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        SpanEntityKind::Turn,
                        run_id,
                        Some(turn_id),
                        None,
                        None,
                        None,
                        SpanLifecyclePhase::Checkpointed,
                        TraceStatus::Ok,
                        event,
                        source_cursor,
                        None,
                        transition_attributes(event),
                    )?;
                }
            }
            "run.model_turn" => {
                if let Some(turn_id) = current_turn {
                    ensure_turn_started(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        run_id,
                        turn_id,
                        event,
                        source_cursor,
                    )?;
                    append_transition(
                        &mut records,
                        &mut slots,
                        &trace_id,
                        SpanEntityKind::Turn,
                        run_id,
                        Some(turn_id),
                        None,
                        None,
                        None,
                        SpanLifecyclePhase::Resumed,
                        TraceStatus::Ok,
                        event,
                        source_cursor,
                        None,
                        transition_attributes(event),
                    )?;
                }
            }
            kind if invocation_event_kind(kind) => {
                let Some((candidate_id, request_id, explicit_id)) = invocation_id_for(event)?
                else {
                    // A capability/approval event with no stable invocation reference is not
                    // guessed into a span. The opaque EventLog fact remains queryable.
                    continue;
                };
                let invocation_id = if let Some(request_id) = request_id {
                    if let Some(previous_id) = invocation_aliases.get(&request_id).copied() {
                        if explicit_id && previous_id != candidate_id {
                            // Older events may only carry capability_request_id. Once a later
                            // committed dispatch reveals the canonical InvocationId, migrate the
                            // already-built rows so one request cannot fork into two spans.
                            promote_invocation_alias(
                                &mut records,
                                &mut slots,
                                run_id,
                                previous_id,
                                candidate_id,
                            )?;
                            invocation_aliases.insert(request_id, candidate_id);
                            candidate_id
                        } else {
                            previous_id
                        }
                    } else {
                        invocation_aliases.insert(request_id, candidate_id);
                        candidate_id
                    }
                } else {
                    candidate_id
                };
                let turn_id = event_turn
                    .or(current_turn)
                    .or_else(|| Some(TurnId::from_uuid(event.request_id.as_uuid())));
                let Some(turn_id) = turn_id else {
                    continue;
                };
                current_turn = Some(turn_id);
                ensure_turn_started(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    run_id,
                    turn_id,
                    event,
                    source_cursor,
                )?;
                let execution_id = execution_id_for(event)?;
                let key = span_key(
                    SpanEntityKind::Invocation,
                    run_id,
                    Some(turn_id),
                    Some(invocation_id),
                );
                let prior_attempt = slots.get(&key).map(|slot| slot.latest_attempt).unwrap_or(0);
                let attempt = attempt_for(event)?
                    .or_else(|| (prior_attempt > 0).then_some(prior_attempt))
                    .or(Some(1));
                if let Some(attempt) = attempt {
                    if attempt < prior_attempt {
                        continue;
                    }
                    if attempt > prior_attempt {
                        if let Some(slot) = slots.get_mut(&key) {
                            slot.terminal = None;
                            slot.last_transition = None;
                        }
                    }
                }
                let new_attempt = attempt.is_some_and(|attempt| attempt > prior_attempt);
                let Some((phase, status, fallback)) = invocation_transition(event, new_attempt)
                else {
                    continue;
                };
                append_transition(
                    &mut records,
                    &mut slots,
                    &trace_id,
                    SpanEntityKind::Invocation,
                    run_id,
                    Some(turn_id),
                    Some(invocation_id),
                    execution_id,
                    attempt,
                    phase,
                    status,
                    event,
                    source_cursor,
                    bounded_error(event, fallback),
                    transition_attributes(event),
                )?;
            }
            _ => {}
        }
    }
    Ok(records)
}

/// Compatibility alias used by callers that refer to the output as a span projection.
pub fn project_spans(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<SpanLifecycleRecord>, SpanProjectionError> {
    project_span_lifecycle(run_id, events)
}

impl ControlPlane {
    /// Rebuild lifecycle spans from the EventLog without consulting a runner or exporter.
    pub async fn span_lifecycle(
        &self,
        run_id: RunId,
    ) -> Result<Vec<SpanLifecycleRecord>, CoreError> {
        let events = self.events_for_persisted_run(run_id).await?;
        if events.is_empty() {
            return Err(kiana_ports::PortError::Failed("run_not_found".to_owned()).into());
        }
        project_span_lifecycle(run_id, &events)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    /// Alias for read paths that expose projections as state snapshots.
    pub async fn span_state(&self, run_id: RunId) -> Result<Vec<SpanLifecycleRecord>, CoreError> {
        self.span_lifecycle(run_id).await
    }
}
