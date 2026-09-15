//! Read-only replay and reconciliation diagnostics.
//!
//! This module compares deterministic projections of committed facts. It never invokes a model,
//! Provider, Broker or recovery action, and it records divergence rather than rewriting the
//! source or guessing an unknown schema.

use kiana_domain::{
    json_digest, CapabilityExecutionState, EventCursor, EventId, EventStoreCapabilities,
    HealthProbeKind, InvocationId, ReplayDiagnostic, ReplayDiagnosticSnapshot,
    ReplayDivergenceKind, RequestId, RunId, RuntimeEvent, TraceStatus,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

use super::{
    project_health_snapshot, project_invocations, project_operational_metrics, project_run_state,
    project_span_lifecycle, rebuild_audit_projection,
};
use super::{ControlPlane, CoreError};

const MAX_EXPECTATIONS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayExpectation {
    pub invocation_id: Option<InvocationId>,
    pub attempt: Option<u32>,
    pub input_digest: Option<String>,
    pub expected_status: TraceStatus,
    pub expected_error_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ReplayDiagnosticsError {
    #[error("replay_source_empty")]
    SourceEmpty,
    #[error("replay_source_cursor_invalid")]
    SourceCursorInvalid,
    #[error("replay_source_cursor_gap")]
    SourceCursorGap,
    #[error("replay_source_duplicate")]
    SourceDuplicate,
    #[error("replay_expectation_limit")]
    ExpectationLimit,
    #[error("replay_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

fn source_cursor(
    event: &RuntimeEvent,
    index: usize,
) -> Result<EventCursor, ReplayDiagnosticsError> {
    let fallback = u64::try_from(index)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(ReplayDiagnosticsError::SourceCursorInvalid)?;
    match event.data.get("source_cursor") {
        None => Ok(fallback),
        Some(value) => value
            .as_u64()
            .filter(|cursor| *cursor > 0)
            .ok_or(ReplayDiagnosticsError::SourceCursorInvalid),
    }
}

fn safe_error_code(error: &str) -> String {
    let code = error
        .trim()
        .split(':')
        .next()
        .unwrap_or("replay_projection_error");
    if code.is_empty()
        || code.len() > 128
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || "._-".contains(byte as char))
    {
        "replay_projection_error".to_owned()
    } else {
        code.to_owned()
    }
}

fn event_input_digest(event: &RuntimeEvent) -> Option<String> {
    for name in ["input_digest", "action_digest", "args_fingerprint"] {
        let Some(value) = event.data.get(name).and_then(Value::as_str) else {
            continue;
        };
        if value.starts_with("sha256:") && value.len() == 71 {
            return Some(value.to_owned());
        }
    }
    Some(json_digest(&json!({
        "kind": event.kind,
        "request_id": event.request_id,
    })))
}

fn invocation_id(event: &RuntimeEvent) -> Option<InvocationId> {
    event
        .data
        .get("invocation_id")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .or_else(|| {
            event
                .data
                .get("capability_request_id")
                .and_then(|value| serde_json::from_value::<RequestId>(value.clone()).ok())
                .map(|id| InvocationId::from_uuid(id.as_uuid()))
        })
}

fn source_ids(events: &[RuntimeEvent]) -> Vec<EventId> {
    let mut seen = HashSet::new();
    events
        .iter()
        .filter(|event| seen.insert(event.event_id.to_string()))
        .map(|event| event.event_id)
        .take(kiana_domain::MAX_SOURCE_EVENT_IDS)
        .collect()
}

fn diagnostic(
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
    divergence: ReplayDivergenceKind,
    error_code: impl Into<String>,
    event: Option<&RuntimeEvent>,
    expected_status: Option<TraceStatus>,
    observed_status: Option<TraceStatus>,
) -> Result<ReplayDiagnostic, ReplayDiagnosticsError> {
    let (invocation_id, input_digest) = event
        .map(|event| (invocation_id(event), event_input_digest(event)))
        .unwrap_or((None, None));
    ReplayDiagnostic::new(
        invocation_id,
        event
            .and_then(|event| event.data.get("attempt"))
            .and_then(Value::as_u64)
            .and_then(|attempt| u32::try_from(attempt).ok()),
        input_digest,
        expected_status,
        observed_status,
        divergence,
        safe_error_code(&error_code.into()),
        source_cursor,
        source_ids(events),
    )
    .map_err(ReplayDiagnosticsError::SnapshotInvalid)
}

fn status_for_invocation(state: CapabilityExecutionState) -> TraceStatus {
    match state {
        CapabilityExecutionState::Succeeded => TraceStatus::Ok,
        CapabilityExecutionState::Failed
        | CapabilityExecutionState::Cancelled
        | CapabilityExecutionState::Denied => TraceStatus::Error,
        CapabilityExecutionState::Unknown => TraceStatus::Unknown,
        _ => TraceStatus::Degraded,
    }
}

/// Compare deterministic projections and return the first safe divergence per failing boundary.
pub fn diagnose_replay(
    events: &[RuntimeEvent],
    run_id: Option<RunId>,
    expectations: &[ReplayExpectation],
) -> Result<ReplayDiagnosticSnapshot, ReplayDiagnosticsError> {
    if events.is_empty() {
        return Err(ReplayDiagnosticsError::SourceEmpty);
    }
    if expectations.len() > MAX_EXPECTATIONS {
        return Err(ReplayDiagnosticsError::ExpectationLimit);
    }
    let mut seen = HashSet::new();
    let mut cursors = Vec::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        if !seen.insert(event.event_id.to_string()) {
            return Err(ReplayDiagnosticsError::SourceDuplicate);
        }
        cursors.push(source_cursor(event, index)?);
    }
    let source_cursor = *cursors
        .iter()
        .max()
        .ok_or(ReplayDiagnosticsError::SourceEmpty)?;
    let mut diagnostics = Vec::new();
    let mut projection_digests = BTreeMap::new();

    match project_operational_metrics(events, None) {
        Ok(metrics) => {
            projection_digests.insert("metrics".to_owned(), metrics.digest());
            if metrics
                .limitations
                .iter()
                .any(|limitation| limitation == "eventlog_cursor_gap")
            {
                diagnostics.push(diagnostic(
                    events,
                    source_cursor,
                    ReplayDivergenceKind::SourceGap,
                    "eventlog_cursor_gap",
                    None,
                    None,
                    Some(TraceStatus::Unknown),
                )?);
            }
        }
        Err(error) => diagnostics.push(diagnostic(
            events,
            source_cursor,
            ReplayDivergenceKind::ProjectionError,
            error.to_string(),
            events.first(),
            None,
            Some(TraceStatus::Unknown),
        )?),
    }

    match rebuild_audit_projection(events, cursors[0]) {
        Ok(audit) => {
            projection_digests.insert("audit".to_owned(), audit.digest());
        }
        Err(error) => diagnostics.push(diagnostic(
            events,
            source_cursor,
            if error.to_string().contains("cursor") {
                ReplayDivergenceKind::SourceGap
            } else if error.to_string().contains("conflict") {
                ReplayDivergenceKind::TerminalConflict
            } else {
                ReplayDivergenceKind::ProjectionError
            },
            error.to_string(),
            events.first(),
            None,
            Some(TraceStatus::Unknown),
        )?),
    }

    if let Some(run_id) = run_id {
        match project_invocations(run_id, events) {
            Ok(invocations) => {
                projection_digests
                    .insert("invocations".to_owned(), json_digest(&json!(invocations)));
                for invocation in invocations {
                    let invocation_id = InvocationId::from_uuid(invocation.request_id.as_uuid());
                    let observed = status_for_invocation(invocation.state);
                    if invocation.state == CapabilityExecutionState::Unknown {
                        diagnostics.push(diagnostic(
                            events,
                            source_cursor,
                            ReplayDivergenceKind::UnknownEffect,
                            "result_unknown",
                            None,
                            None,
                            Some(observed),
                        )?);
                    }
                    for expectation in expectations.iter().filter(|expectation| {
                        expectation
                            .invocation_id
                            .is_none_or(|id| id == invocation_id)
                    }) {
                        let status_mismatch = expectation.expected_status != observed;
                        let input_mismatch =
                            expectation.input_digest.as_ref().is_some_and(|expected| {
                                invocation.args_fingerprint.as_ref() != Some(expected)
                            });
                        if status_mismatch || input_mismatch {
                            diagnostics.push(
                                ReplayDiagnostic::new(
                                    Some(invocation_id),
                                    expectation.attempt,
                                    expectation.input_digest.clone(),
                                    Some(expectation.expected_status),
                                    Some(observed),
                                    ReplayDivergenceKind::StatusMismatch,
                                    if input_mismatch {
                                        "replay_input_digest_mismatch"
                                    } else {
                                        "replay_status_mismatch"
                                    },
                                    source_cursor,
                                    source_ids(events),
                                )
                                .map_err(ReplayDiagnosticsError::SnapshotInvalid)?,
                            );
                        }
                    }
                }
            }
            Err(error) => diagnostics.push(diagnostic(
                events,
                source_cursor,
                ReplayDivergenceKind::ProjectionError,
                error,
                events.first(),
                None,
                Some(TraceStatus::Unknown),
            )?),
        }
        match project_run_state(run_id, events) {
            Ok(state) => {
                projection_digests.insert(
                    "run".to_owned(),
                    json_digest(&json!({
                        "run_id": state.run_id,
                        "phase": format!("{:?}", state.phase),
                        "outcome": state.outcome.map(|outcome| format!("{:?}", outcome)),
                        "error": state.error,
                    })),
                );
            }
            Err(error) => diagnostics.push(diagnostic(
                events,
                source_cursor,
                ReplayDivergenceKind::TerminalConflict,
                error.to_string(),
                events.first(),
                None,
                Some(TraceStatus::Unknown),
            )?),
        }
    }

    match project_health_snapshot(
        events,
        &EventStoreCapabilities::default(),
        HealthProbeKind::Liveness,
        1,
    ) {
        Ok(health) => {
            projection_digests.insert("health".to_owned(), health.digest());
        }
        Err(error) => diagnostics.push(diagnostic(
            events,
            source_cursor,
            ReplayDivergenceKind::ProjectionError,
            error.to_string(),
            events.first(),
            None,
            Some(TraceStatus::Unknown),
        )?),
    }
    if let Some(run_id) = run_id {
        match project_span_lifecycle(run_id, events) {
            Ok(spans) => {
                projection_digests.insert("spans".to_owned(), json_digest(&json!(spans)));
            }
            Err(error) => diagnostics.push(diagnostic(
                events,
                source_cursor,
                ReplayDivergenceKind::ProjectionError,
                error.to_string(),
                events.first(),
                None,
                Some(TraceStatus::Unknown),
            )?),
        }
    }

    let status = if diagnostics.is_empty() {
        TraceStatus::Ok
    } else {
        TraceStatus::Unknown
    };
    ReplayDiagnosticSnapshot::new(
        source_cursor,
        source_ids(events),
        status,
        projection_digests,
        diagnostics,
        Vec::new(),
    )
    .map_err(ReplayDiagnosticsError::SnapshotInvalid)
}

impl ControlPlane {
    pub async fn replay_diagnostics(
        &self,
        run_id: Option<RunId>,
        expectations: &[ReplayExpectation],
    ) -> Result<ReplayDiagnosticSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("replay_read_all_unsupported".to_owned())
        })?;
        diagnose_replay(&events, run_id, expectations)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
