//! EQ-28 runtime correctness evaluation over a canonical, caller-supplied trace.
//!
//! This module deliberately evaluates an already materialized `CanonicalEventTrace`.  It does
//! not normalize events, read the EventLog, start a runner, contact a provider, or authorize a
//! retry/cancel/approval.  A finding is a diagnostic quality result; it is never a ControlPlane
//! decision.

use crate::{
    CanonicalEventTrace, DeterministicEvaluator, EvaluatorError, Finding,
    CANONICAL_NORMALIZATION_VERSION, MAX_FINDINGS,
};
use kiana_domain::event_kind_spec;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Versioned input envelope for the runtime correctness evaluator.
pub const RUNTIME_CORRECTNESS_INPUT_SCHEMA: &str = "kiana.quality-runtime-correctness-input.v1";
/// Stable registry identifier for the runtime correctness evaluator.
pub const RUNTIME_CORRECTNESS_EVALUATOR_ID: &str = "runtime-correctness";
const MAX_RUNTIME_EVENTS: usize = 4_096;
const MAX_IDENTIFIER_BYTES: usize = 256;

/// A canonical trace selected and redacted by EQ-17/EQ-18.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCorrectnessInput {
    pub schema: String,
    pub trace: CanonicalEventTrace,
}

impl RuntimeCorrectnessInput {
    pub fn new(trace: CanonicalEventTrace) -> Self {
        Self {
            schema: RUNTIME_CORRECTNESS_INPUT_SCHEMA.to_owned(),
            trace,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != RUNTIME_CORRECTNESS_INPUT_SCHEMA {
            return Err("runtime_correctness_input_schema_invalid");
        }
        if self.trace.normalization_version != CANONICAL_NORMALIZATION_VERSION {
            return Err("runtime_correctness_input_normalization_invalid");
        }
        if self.trace.events.len() > MAX_RUNTIME_EVENTS {
            return Err("runtime_correctness_input_event_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("runtime correctness input is serializable")
    }
}

/// Pure runtime correctness evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeCorrectnessEvaluator;

impl DeterministicEvaluator for RuntimeCorrectnessEvaluator {
    fn evaluator_id(&self) -> &str {
        RUNTIME_CORRECTNESS_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: RuntimeCorrectnessInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("runtime_correctness"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_runtime_correctness(&decoded)
    }
}

/// Evaluate event ordering and runtime lifecycle invariants without executing anything.
pub fn evaluate_runtime_correctness(
    input: &RuntimeCorrectnessInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let trace = &input.trace;
    let mut findings = Vec::new();

    if trace.events.is_empty() {
        emit(
            &mut findings,
            "runtime.trace_empty",
            Value::String("one_or_more_events".to_owned()),
            Value::from(0),
            None,
        )?;
        return Ok(findings);
    }

    let expected_correlation = trace.correlation_id.to_string();
    let mut previous_cursor = None;
    let mut request_sequences: BTreeMap<String, u64> = BTreeMap::new();
    let mut terminal_streams: BTreeMap<String, TerminalObservation> = BTreeMap::new();
    let mut observed_terminal_indexes = Vec::new();
    let mut invocations = Vec::new();
    let mut invocation_aliases: BTreeMap<String, usize> = BTreeMap::new();
    let mut approvals: BTreeMap<String, ApprovalState> = BTreeMap::new();
    let mut retry_attempts: BTreeMap<String, u64> = BTreeMap::new();
    let mut unknown_scopes = BTreeSet::new();
    let mut cancelling_runs = BTreeSet::new();
    let mut cancelled_runs = BTreeSet::new();

    for (index, event) in trace.events.iter().enumerate() {
        let cursor = event.source_cursor;
        let evidence_cursor = (cursor != 0).then_some(cursor);
        if cursor == 0 {
            emit(
                &mut findings,
                "runtime.event_order_invalid",
                Value::String("positive_source_cursor".to_owned()),
                Value::from(cursor),
                evidence_cursor,
            )?;
        }
        if let Some(previous) = previous_cursor {
            if cursor <= previous {
                emit(
                    &mut findings,
                    "runtime.event_order_invalid",
                    Value::from(previous + 1),
                    Value::from(cursor),
                    evidence_cursor,
                )?;
            }
        }
        previous_cursor = Some(cursor);

        let Some(value) = event.value.as_object() else {
            emit(
                &mut findings,
                "runtime.event_shape_invalid",
                Value::String("canonical_event_object".to_owned()),
                Value::String("non_object".to_owned()),
                evidence_cursor,
            )?;
            continue;
        };
        let data = value.get("data").and_then(Value::as_object);
        let kind = event.kind.as_str();

        if value.get("kind").and_then(Value::as_str) != Some(kind) {
            emit(
                &mut findings,
                "runtime.event_shape_invalid",
                Value::String(kind.to_owned()),
                value.get("kind").cloned().unwrap_or(Value::Null),
                evidence_cursor,
            )?;
        }
        if value.get("source_cursor").and_then(Value::as_u64) != Some(cursor) {
            emit(
                &mut findings,
                "runtime.event_shape_invalid",
                Value::from(cursor),
                value.get("source_cursor").cloned().unwrap_or(Value::Null),
                evidence_cursor,
            )?;
        }

        let request_id = identifier(value, data, "request_id");
        let sequence = value.get("sequence").and_then(Value::as_u64);
        match (request_id.as_deref(), sequence) {
            (Some(request), Some(sequence)) if sequence > 0 => {
                if let Some(previous) = request_sequences.insert(request.to_owned(), sequence) {
                    if sequence <= previous {
                        emit(
                            &mut findings,
                            "runtime.event_order_invalid",
                            Value::from(previous + 1),
                            Value::from(sequence),
                            evidence_cursor,
                        )?;
                    }
                }
            }
            _ => emit(
                &mut findings,
                "runtime.event_shape_invalid",
                Value::String("request_id_and_positive_sequence".to_owned()),
                json_object_fields(value, &["request_id", "sequence"]),
                evidence_cursor,
            )?,
        }

        let correlation = identifier(value, None, "correlation_id");
        if correlation.as_deref() != Some(expected_correlation.as_str()) {
            emit(
                &mut findings,
                "runtime.correlation_mismatch",
                Value::String(expected_correlation.clone()),
                correlation.map(Value::String).unwrap_or(Value::Null),
                evidence_cursor,
            )?;
        }

        let terminal = event_kind_spec(kind).is_some_and(|spec| spec.terminal);
        if terminal {
            observed_terminal_indexes.push(index);
        }
        if let Some(stream_key) = stream_key(value, data, kind) {
            if terminal {
                if let Some(previous) = terminal_streams.get(&stream_key) {
                    emit(
                        &mut findings,
                        "runtime.terminal_duplicate",
                        Value::String("one_terminal_per_stream".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?;
                    if previous.kind != kind {
                        emit(
                            &mut findings,
                            "runtime.terminal_conflict",
                            Value::String(previous.kind.clone()),
                            Value::String(kind.to_owned()),
                            evidence_cursor,
                        )?;
                    }
                } else {
                    terminal_streams.insert(
                        stream_key.clone(),
                        TerminalObservation {
                            kind: kind.to_owned(),
                            cursor,
                        },
                    );
                }
            } else if terminal_streams.contains_key(&stream_key) {
                emit(
                    &mut findings,
                    "runtime.event_after_terminal",
                    Value::String("no_events_after_terminal".to_owned()),
                    Value::String(kind.to_owned()),
                    evidence_cursor,
                )?;
            }
        }

        let unknown = is_unknown_kind(kind);
        let invocation_related = is_invocation_related(kind);
        let invocation_key = if invocation_related {
            let aliases = invocation_aliases_for(value, data);
            if aliases.is_empty() {
                emit(
                    &mut findings,
                    "runtime.invocation_correlation_missing",
                    Value::String("invocation_or_capability_identity".to_owned()),
                    Value::String(kind.to_owned()),
                    evidence_cursor,
                )?;
                None
            } else {
                let state_index = match aliases
                    .iter()
                    .filter_map(|alias| invocation_aliases.get(alias).copied())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .next()
                {
                    Some(index) => index,
                    None => {
                        let index = invocations.len();
                        invocations.push(InvocationState::default());
                        index
                    }
                };
                let existing = aliases
                    .iter()
                    .filter_map(|alias| invocation_aliases.get(alias).copied())
                    .collect::<BTreeSet<_>>();
                if existing.len() > 1 {
                    emit(
                        &mut findings,
                        "runtime.invocation_correlation_mismatch",
                        Value::String("one_invocation_identity".to_owned()),
                        Value::String("multiple_lifecycle_identities".to_owned()),
                        evidence_cursor,
                    )?;
                }
                for alias in aliases {
                    if let Some(previous) = invocation_aliases.insert(alias, state_index) {
                        if previous != state_index {
                            emit(
                                &mut findings,
                                "runtime.invocation_correlation_mismatch",
                                Value::String("stable_invocation_alias".to_owned()),
                                Value::String("alias_rebound".to_owned()),
                                evidence_cursor,
                            )?;
                        }
                    }
                }
                Some(state_index)
            }
        } else {
            None
        };

        if let Some(state_index) = invocation_key {
            let state = &mut invocations[state_index];
            if state.terminal {
                emit(
                    &mut findings,
                    "runtime.event_after_terminal",
                    Value::String("no_invocation_events_after_terminal".to_owned()),
                    Value::String(kind.to_owned()),
                    evidence_cursor,
                )?;
            }
            if unknown && state.unknown {
                emit(
                    &mut findings,
                    "runtime.unknown_duplicate",
                    Value::String("one_unknown_outcome".to_owned()),
                    Value::String(kind.to_owned()),
                    evidence_cursor,
                )?;
            }
            if state.unknown && is_invocation_start(kind) {
                emit(
                    &mut findings,
                    "runtime.unknown_retry_forbidden",
                    Value::String("reconcile_before_new_attempt".to_owned()),
                    Value::String(kind.to_owned()),
                    evidence_cursor,
                )?;
            }
            if is_invocation_terminal(kind) {
                if !state.seen {
                    emit(
                        &mut findings,
                        "runtime.invocation_unmatched",
                        Value::String("prior_invocation_event".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?;
                }
                if state.terminal {
                    emit(
                        &mut findings,
                        "runtime.invocation_terminal_duplicate",
                        Value::String("one_terminal_per_invocation".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?;
                }
                state.terminal = true;
                state.unknown = unknown;
            } else {
                state.seen = true;
            }
            if unknown {
                state.unknown = true;
            }
        }

        process_approval(
            &mut approvals,
            &mut findings,
            kind,
            value,
            data,
            evidence_cursor,
        )?;
        check_approval_binding(
            &approvals,
            &mut findings,
            kind,
            value,
            data,
            evidence_cursor,
        )?;

        if kind == "model.retry_scheduled" {
            let retry_key = retry_key(value, data);
            let retry = data.and_then(|fields| fields.get("retry"));
            let retry_object = retry.and_then(Value::as_object);
            let ordinal = retry_object
                .and_then(|fields| fields.get("retry_ordinal"))
                .and_then(Value::as_u64);
            let next_attempt = retry_object
                .and_then(|fields| fields.get("next_attempt"))
                .and_then(Value::as_u64);
            if retry_key.is_none() || ordinal.is_none() || next_attempt.is_none() {
                emit(
                    &mut findings,
                    "runtime.retry_metadata_invalid",
                    Value::String("retry_key_ordinal_next_attempt".to_owned()),
                    json_object_fields(
                        retry_object.unwrap_or(&Map::new()),
                        &["retry_ordinal", "next_attempt"],
                    ),
                    evidence_cursor,
                )?;
            } else if let (Some(key), Some(ordinal), Some(next_attempt)) =
                (retry_key, ordinal, next_attempt)
            {
                if ordinal == 0 || next_attempt == 0 {
                    emit(
                        &mut findings,
                        "runtime.retry_metadata_invalid",
                        Value::String("positive_retry_ordinal_and_next_attempt".to_owned()),
                        serde_json::json!({
                            "retry_ordinal": ordinal,
                            "next_attempt": next_attempt,
                        }),
                        evidence_cursor,
                    )?;
                }
                if unknown_scopes.contains(&key) {
                    emit(
                        &mut findings,
                        "runtime.unknown_retry_forbidden",
                        Value::String("reconcile_before_retry".to_owned()),
                        Value::String("retry_scheduled".to_owned()),
                        evidence_cursor,
                    )?;
                }
                if let Some(previous) = retry_attempts.insert(key, ordinal) {
                    if ordinal <= previous {
                        emit(
                            &mut findings,
                            "runtime.retry_order_invalid",
                            Value::from(previous + 1),
                            Value::from(ordinal),
                            evidence_cursor,
                        )?;
                    }
                }
            }
        }

        if unknown {
            if let Some(key) = lifecycle_key(value, data, kind) {
                if !unknown_scopes.insert(key) {
                    emit(
                        &mut findings,
                        "runtime.unknown_duplicate",
                        Value::String("one_unknown_outcome".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?;
                }
            }
            if let Some(key) = retry_key(value, data) {
                unknown_scopes.insert(key);
            }
        } else if is_retry_after_unknown(kind) {
            if let Some(key) = lifecycle_key(value, data, kind) {
                if unknown_scopes.contains(&key) {
                    emit(
                        &mut findings,
                        "runtime.unknown_retry_forbidden",
                        Value::String("reconcile_before_new_attempt".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?;
                }
            }
        }

        let run_identity = run_key(value, data);
        if kind == "run.cancelling" {
            if let Some(key) = run_identity.clone() {
                cancelling_runs.insert(key);
            } else {
                emit(
                    &mut findings,
                    "runtime.cancel_correlation_missing",
                    Value::String("run_id".to_owned()),
                    Value::String(kind.to_owned()),
                    evidence_cursor,
                )?;
            }
        } else if matches!(kind, "run.cancelled" | "run.result_unknown") {
            if let Some(key) = run_identity.clone() {
                cancelled_runs.insert(key.clone());
            }
            if kind == "run.cancelled" {
                match run_identity {
                    Some(key) if cancelling_runs.contains(&key) => {}
                    Some(_) => emit(
                        &mut findings,
                        "runtime.cancel_unfenced",
                        Value::String("run.cancelling_before_run.cancelled".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?,
                    None => emit(
                        &mut findings,
                        "runtime.cancel_correlation_missing",
                        Value::String("run_id".to_owned()),
                        Value::String(kind.to_owned()),
                        evidence_cursor,
                    )?,
                }
            }
        }

        if findings.len() > MAX_FINDINGS {
            return Err(EvaluatorError::FindingLimitExceeded);
        }

        let _ = index;
    }

    if trace.source_cursor_start != trace.events[0].source_cursor {
        emit(
            &mut findings,
            "runtime.trace_cursor_invalid",
            Value::from(trace.events[0].source_cursor),
            Value::from(trace.source_cursor_start),
            None,
        )?;
    }
    if trace.source_cursor_end != trace.events[trace.events.len() - 1].source_cursor {
        emit(
            &mut findings,
            "runtime.trace_cursor_invalid",
            Value::from(trace.events[trace.events.len() - 1].source_cursor),
            Value::from(trace.source_cursor_end),
            None,
        )?;
    }
    if trace.terminal_event_indexes != observed_terminal_indexes {
        emit(
            &mut findings,
            "runtime.terminal_index_mismatch",
            serde_json::to_value(&observed_terminal_indexes).unwrap_or(Value::Null),
            serde_json::to_value(&trace.terminal_event_indexes).unwrap_or(Value::Null),
            None,
        )?;
    }

    for state in invocations {
        if state.seen && !state.terminal {
            emit(
                &mut findings,
                "runtime.invocation_terminal_missing",
                Value::String("one_terminal_per_invocation".to_owned()),
                Value::String("non_terminal_trace".to_owned()),
                None,
            )?;
        }
    }
    for key in cancelling_runs {
        if !cancelled_runs.contains(&key) {
            emit(
                &mut findings,
                "runtime.cancel_terminal_missing",
                Value::String("run.cancelled_or_result_unknown".to_owned()),
                Value::String("cancellation_pending".to_owned()),
                None,
            )?;
        }
    }

    Ok(findings)
}

#[derive(Clone, Debug, Default)]
struct InvocationState {
    seen: bool,
    terminal: bool,
    unknown: bool,
}

#[derive(Clone, Debug)]
struct TerminalObservation {
    kind: String,
    #[allow(dead_code)]
    cursor: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ApprovalState {
    Pending,
    Approved,
    Consumed,
    Denied,
    Expired,
    Cancelled,
    Unavailable,
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: Value,
    actual: Value,
    cursor: Option<u64>,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    let evidence_ref = cursor
        .map(|cursor| format!("event:{cursor}"))
        .unwrap_or_else(|| "trace:runtime".to_owned());
    findings.push(
        Finding::new(
            code,
            expected,
            actual,
            format!("runtime correctness finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}

fn identifier(
    value: &Map<String, Value>,
    data: Option<&Map<String, Value>>,
    field: &str,
) -> Option<String> {
    data.and_then(|fields| fields.get(field))
        .or_else(|| value.get(field))
        .and_then(safe_identifier)
}

fn safe_identifier(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    if text.is_empty()
        || text.len() > MAX_IDENTIFIER_BYTES
        || text
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        None
    } else {
        Some(text.to_owned())
    }
}

fn json_object_fields(value: &Map<String, Value>, fields: &[&str]) -> Value {
    fields
        .iter()
        .fold(Map::new(), |mut result, field| {
            result.insert(
                (*field).to_owned(),
                value.get(*field).cloned().unwrap_or(Value::Null),
            );
            result
        })
        .into()
}

fn stream_key(
    value: &Map<String, Value>,
    data: Option<&Map<String, Value>>,
    kind: &str,
) -> Option<String> {
    let aggregate_type = value.get("aggregate_type").and_then(safe_identifier);
    let aggregate_id = value.get("aggregate_id").and_then(safe_identifier);
    if let (Some(aggregate_type), Some(aggregate_id)) = (aggregate_type, aggregate_id) {
        return Some(format!("aggregate:{aggregate_type}:{aggregate_id}"));
    }
    let field = if kind.starts_with("approval.") || kind == "run.awaiting_approval" {
        "approval_id"
    } else if kind.starts_with("invocation.")
        || kind.starts_with("execution.")
        || kind.starts_with("capability.")
        || kind.starts_with("action.")
        || kind == "run.tool_call"
        || kind == "run.capability_requested"
        || kind == "run.tool_result"
    {
        "capability_request_id"
    } else if kind.starts_with("run.") {
        "run_id"
    } else {
        "request_id"
    };
    identifier(value, data, field)
        .or_else(|| identifier(value, data, "request_id"))
        .map(|key| format!("{kind}:{key}"))
}

fn lifecycle_key(
    value: &Map<String, Value>,
    data: Option<&Map<String, Value>>,
    kind: &str,
) -> Option<String> {
    stream_key(value, data, kind).or_else(|| invocation_aliases_for(value, data).into_iter().next())
}

fn run_key(value: &Map<String, Value>, data: Option<&Map<String, Value>>) -> Option<String> {
    identifier(value, data, "run_id").or_else(|| {
        (value.get("aggregate_type").and_then(Value::as_str) == Some("run"))
            .then(|| value.get("aggregate_id").and_then(safe_identifier))
            .flatten()
    })
}

fn invocation_aliases_for(
    value: &Map<String, Value>,
    data: Option<&Map<String, Value>>,
) -> Vec<String> {
    [
        "capability_request_id",
        "invocation_id",
        "execution_id",
        "call_id",
    ]
    .into_iter()
    .filter_map(|field| identifier(value, data, field).map(|id| format!("{field}:{id}")))
    .collect()
}

fn retry_key(value: &Map<String, Value>, data: Option<&Map<String, Value>>) -> Option<String> {
    [
        "model_call_id",
        "model_attempt_id",
        "attempt_id",
        "reservation_id",
        "run_id",
    ]
    .into_iter()
    .find_map(|field| identifier(value, data, field).map(|id| format!("{field}:{id}")))
}

fn is_invocation_related(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_call"
            | "run.capability_requested"
            | "run.tool_result"
            | "run.capability_blocked"
            | "capability.decision"
            | "capability.blocked"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.prepared"
            | "execution.result_committed"
            | "result.delivery_claimed"
    )
}

fn is_invocation_start(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_call"
            | "run.capability_requested"
            | "capability.decision"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.prepared"
    )
}

fn is_invocation_terminal(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_result"
            | "run.capability_blocked"
            | "capability.blocked"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "execution.result_committed"
    )
}

fn is_unknown_kind(kind: &str) -> bool {
    matches!(
        kind,
        "run.result_unknown"
            | "capability.result_unknown"
            | "execution.result_unknown"
            | "model.unknown"
            | "usage.unknown"
    ) || kind.ends_with(".result_unknown")
}

fn is_retry_after_unknown(kind: &str) -> bool {
    kind == "model.retry_scheduled"
        || kind == "model.attempt_started"
        || kind == "invocation.dispatching"
        || kind == "invocation.executing"
}

fn process_approval(
    approvals: &mut BTreeMap<String, ApprovalState>,
    findings: &mut Vec<Finding>,
    kind: &str,
    value: &Map<String, Value>,
    data: Option<&Map<String, Value>>,
    cursor: Option<u64>,
) -> Result<(), EvaluatorError> {
    let approval_event = kind == "run.awaiting_approval" || kind.starts_with("approval.");
    if !approval_event {
        return Ok(());
    }
    let Some(approval_id) = identifier(value, data, "approval_id") else {
        emit(
            findings,
            "runtime.approval_correlation_missing",
            Value::String("approval_id".to_owned()),
            Value::String(kind.to_owned()),
            cursor,
        )?;
        return Ok(());
    };
    let state = approvals.get(&approval_id).copied();
    let next = match kind {
        "run.awaiting_approval" | "approval.requested" | "approval.activated" => {
            if matches!(
                state,
                Some(
                    ApprovalState::Denied
                        | ApprovalState::Expired
                        | ApprovalState::Cancelled
                        | ApprovalState::Unavailable
                        | ApprovalState::Consumed
                )
            ) {
                emit(
                    findings,
                    "runtime.approval_after_terminal",
                    Value::String("new_approval_id_after_terminal".to_owned()),
                    Value::String(kind.to_owned()),
                    cursor,
                )?;
            }
            ApprovalState::Pending
        }
        "approval.approved" => {
            if !matches!(state, Some(ApprovalState::Pending)) {
                emit(
                    findings,
                    "runtime.approval_unmatched",
                    Value::String("approval.requested_or_activated".to_owned()),
                    Value::String(kind.to_owned()),
                    cursor,
                )?;
            }
            ApprovalState::Approved
        }
        "approval.consumed" => {
            if !matches!(state, Some(ApprovalState::Approved)) {
                emit(
                    findings,
                    "runtime.approval_consumption_invalid",
                    Value::String("approval.approved".to_owned()),
                    Value::String(kind.to_owned()),
                    cursor,
                )?;
            }
            ApprovalState::Consumed
        }
        "approval.denied"
        | "approval.expired"
        | "approval.cancelled"
        | "approval.continuation_unavailable" => {
            if !matches!(
                state,
                Some(ApprovalState::Pending | ApprovalState::Approved)
            ) {
                emit(
                    findings,
                    "runtime.approval_unmatched",
                    Value::String("approval.requested_or_activated".to_owned()),
                    Value::String(kind.to_owned()),
                    cursor,
                )?;
            }
            match kind {
                "approval.denied" => ApprovalState::Denied,
                "approval.expired" => ApprovalState::Expired,
                "approval.cancelled" => ApprovalState::Cancelled,
                _ => ApprovalState::Unavailable,
            }
        }
        _ => return Ok(()),
    };
    approvals.insert(approval_id, next);
    Ok(())
}

fn check_approval_binding(
    approvals: &BTreeMap<String, ApprovalState>,
    findings: &mut Vec<Finding>,
    kind: &str,
    value: &Map<String, Value>,
    data: Option<&Map<String, Value>>,
    cursor: Option<u64>,
) -> Result<(), EvaluatorError> {
    if !is_invocation_related(kind) {
        return Ok(());
    }
    let Some(approval_id) = identifier(value, data, "approval_id") else {
        return Ok(());
    };
    if !matches!(
        approvals.get(&approval_id),
        Some(ApprovalState::Approved | ApprovalState::Consumed)
    ) {
        emit(
            findings,
            "runtime.approval_not_granted",
            Value::String("approval.approved_or_consumed".to_owned()),
            Value::String(kind.to_owned()),
            cursor,
        )?;
    }
    Ok(())
}
