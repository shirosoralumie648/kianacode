//! Read-only replay and reconciliation diagnostics.
//!
//! This module compares deterministic projections of committed facts. It never invokes a model,
//! Provider, Broker or recovery action, and it records divergence rather than rewriting the
//! source or guessing an unknown schema.
//!
//! Two comparison boundaries exist:
//!
//! 1. the committed source itself (duplicate ids, cursor gaps, unknown audit schema, terminal and
//!    projection conflicts, unknown effects, invocation expectations), and
//! 2. an optional [`ReplayConsistencyBaseline`], which pins the canonical digests of the audit,
//!    receipt, health and other deterministic projectors.
//!
//! A baseline mismatch adds a `projection_mismatch` divergence; it can never turn a mismatch into
//! a pass, and a divergence is never swallowed into a limitation.

use kiana_domain::{
    json_digest, CapabilityExecutionState, EventCursor, EventId, EventStoreCapabilities,
    HealthProbeKind, InvocationId, ReplayConsistencyBaseline, ReplayDiagnostic,
    ReplayDiagnosticSnapshot, ReplayDivergenceKind, RequestId, RunId, RuntimeEvent, TraceStatus,
    MAX_SOURCE_EVENT_IDS,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

use super::{
    aggregate_receipt_facts, project_health_snapshot, project_invocations,
    project_operational_metrics, project_run_state, project_span_lifecycle,
    rebuild_audit_projection, InvocationProjection,
};
use super::{ControlPlane, CoreError};

const MAX_EXPECTATIONS: usize = 256;

/// Always present: the snapshot is a read-only projection, not a provider-side reconciliation.
const LIMITATION_PROJECTION_ONLY: &str = "replay_projection_only_read_only";
/// Present when the caller did not pin the deterministic projectors to a baseline.
const LIMITATION_BASELINE_NOT_PROVIDED: &str = "replay_baseline_not_provided";
/// Present when more committed events exist than the snapshot can bind by id.
const LIMITATION_SOURCE_IDS_TRUNCATED: &str = "replay_source_event_ids_truncated";
/// Present when a run was replayed but no receipt facts could be folded.
const LIMITATION_RECEIPT_UNAVAILABLE: &str = "replay_receipt_projection_unavailable";

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
    /// Expectations cannot be evaluated without a run whose invocations can be projected;
    /// silently ignoring them would swallow a divergence the caller asked to be visible.
    #[error("replay_expectation_run_required")]
    ExpectationRunRequired,
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
        .take(MAX_SOURCE_EVENT_IDS)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn diagnostic(
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
    kind: ReplayDivergenceKind,
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
        kind,
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

/// Observed error code of one invocation, derived from the committed terminal result. The result
/// payload is never echoed; only its bounded leading error token is compared.
fn observed_error_code(invocation: &InvocationProjection) -> Option<String> {
    // `execution.result_committed` may carry the stable error code next to (not inside) the
    // nested result object; the invocation projection only keeps `result`, so an unknown
    // effect without an inner code still names its code from the terminal fact.
    if invocation.state == CapabilityExecutionState::Unknown {
        if let Some(code) = invocation
            .result
            .as_ref()
            .and_then(|result| result.get("error_code").or_else(|| result.get("error")))
            .and_then(Value::as_str)
        {
            return Some(safe_error_code(code));
        }
        return Some("result_unknown".to_owned());
    }
    invocation
        .result
        .as_ref()
        .and_then(|result| result.get("error"))
        .and_then(Value::as_str)
        .map(safe_error_code)
}

/// Classify an audit rebuild failure without guessing: schema/unknown errors are schema
/// conflicts, cursor/duplicate errors are source gaps, contradictory decisions are terminal
/// conflicts and everything else stays a projection error.
fn audit_error_kind(reason: &str) -> ReplayDivergenceKind {
    if reason.contains("duplicate") {
        ReplayDivergenceKind::SourceDuplicate
    } else if reason.contains("cursor") {
        ReplayDivergenceKind::SourceGap
    } else if reason.contains("untrusted")
        || reason.contains("unknown")
        || reason.contains("schema")
        || reason.contains("record_limit")
    {
        ReplayDivergenceKind::SchemaConflict
    } else if reason.contains("conflict") || reason.contains("contradict") {
        ReplayDivergenceKind::TerminalConflict
    } else {
        ReplayDivergenceKind::ProjectionError
    }
}

/// Compare the replayed projection digests against a caller-supplied consistency baseline.
///
/// Every mismatch and every baseline entry that replay could not produce becomes a
/// `projection_mismatch` divergence; none of them are collapsed into a limitation.
fn compare_consistency_baseline(
    baseline: &ReplayConsistencyBaseline,
    replay_source_ids: &[EventId],
    projection_digests: &BTreeMap<String, String>,
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
    diagnostics: &mut Vec<ReplayDiagnostic>,
) -> Result<(), ReplayDiagnosticsError> {
    if !baseline.source_event_ids.is_empty() && baseline.source_event_ids != replay_source_ids {
        diagnostics.push(diagnostic(
            events,
            source_cursor,
            ReplayDivergenceKind::ProjectionMismatch,
            "replay_baseline_source_mismatch",
            None,
            None,
            Some(TraceStatus::Unknown),
        )?);
    }
    for (projection, expected) in &baseline.projection_digests {
        let error_code = match projection_digests.get(projection) {
            Some(observed) if observed == expected => continue,
            Some(_) => format!("replay_{projection}_digest_mismatch"),
            None => format!("replay_{projection}_digest_unavailable"),
        };
        diagnostics.push(diagnostic(
            events,
            source_cursor,
            ReplayDivergenceKind::ProjectionMismatch,
            error_code,
            None,
            None,
            Some(TraceStatus::Unknown),
        )?);
    }
    Ok(())
}

/// Compare deterministic projections and return the first safe divergence per failing boundary.
pub fn diagnose_replay(
    events: &[RuntimeEvent],
    run_id: Option<RunId>,
    expectations: &[ReplayExpectation],
) -> Result<ReplayDiagnosticSnapshot, ReplayDiagnosticsError> {
    diagnose_replay_with_baseline(events, run_id, expectations, None)
}

/// Read-only replay diagnostics that also compare the deterministic projectors against an explicit
/// consistency baseline. The baseline cannot add authority: it can only surface a divergence.
pub fn diagnose_replay_with_baseline(
    events: &[RuntimeEvent],
    run_id: Option<RunId>,
    expectations: &[ReplayExpectation],
    baseline: Option<&ReplayConsistencyBaseline>,
) -> Result<ReplayDiagnosticSnapshot, ReplayDiagnosticsError> {
    if events.is_empty() {
        return Err(ReplayDiagnosticsError::SourceEmpty);
    }
    if expectations.len() > MAX_EXPECTATIONS {
        return Err(ReplayDiagnosticsError::ExpectationLimit);
    }
    if run_id.is_none() && !expectations.is_empty() {
        return Err(ReplayDiagnosticsError::ExpectationRunRequired);
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
    let replay_source_ids = source_ids(events);
    let mut diagnostics = Vec::new();
    let mut projection_digests = BTreeMap::new();
    let mut limitations = vec![LIMITATION_PROJECTION_ONLY.to_owned()];
    if baseline.is_none() {
        limitations.push(LIMITATION_BASELINE_NOT_PROVIDED.to_owned());
    }
    if events.len() > replay_source_ids.len() {
        limitations.push(LIMITATION_SOURCE_IDS_TRUNCATED.to_owned());
    }

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
        Err(error) => {
            let reason = error.to_string();
            diagnostics.push(diagnostic(
                events,
                source_cursor,
                audit_error_kind(&reason),
                reason,
                events.first(),
                None,
                Some(TraceStatus::Unknown),
            )?)
        }
    }

    if let Some(run_id) = run_id {
        match aggregate_receipt_facts(run_id, events) {
            Ok(aggregation) => {
                projection_digests.insert("receipt".to_owned(), aggregation.digest());
            }
            Err(_) => limitations.push(LIMITATION_RECEIPT_UNAVAILABLE.to_owned()),
        }
        let mut invocations_projected = false;
        match project_invocations(run_id, events) {
            Ok(invocations) => {
                invocations_projected = true;
                projection_digests
                    .insert("invocations".to_owned(), json_digest(&json!(invocations)));
                let mut matched = vec![false; expectations.len()];
                for invocation in &invocations {
                    let invocation_id = InvocationId::from_uuid(invocation.request_id.as_uuid());
                    let observed = status_for_invocation(invocation.state);
                    let observed_error = observed_error_code(invocation);
                    if invocation.state == CapabilityExecutionState::Unknown {
                        // The unknown effect stays locatable by its own invocation binding; it is
                        // never collapsed into a projection-level note.
                        diagnostics.push(
                            ReplayDiagnostic::with_error_codes(
                                Some(invocation_id),
                                None,
                                invocation.args_fingerprint.clone(),
                                None,
                                Some(observed),
                                None,
                                observed_error.clone(),
                                ReplayDivergenceKind::UnknownEffect,
                                "result_unknown",
                                source_cursor,
                                replay_source_ids.clone(),
                            )
                            .map_err(ReplayDiagnosticsError::SnapshotInvalid)?,
                        );
                    }
                    for (index, expectation) in expectations.iter().enumerate() {
                        if expectation
                            .invocation_id
                            .is_some_and(|id| id != invocation_id)
                        {
                            continue;
                        }
                        matched[index] = true;
                        let status_mismatch = expectation.expected_status != observed;
                        let input_mismatch =
                            expectation.input_digest.as_ref().is_some_and(|expected| {
                                invocation.args_fingerprint.as_ref() != Some(expected)
                            });
                        let error_mismatch =
                            expectation
                                .expected_error_code
                                .as_ref()
                                .is_some_and(|expected| {
                                    observed_error.as_deref() != Some(expected.as_str())
                                });
                        if !(status_mismatch || input_mismatch || error_mismatch) {
                            continue;
                        }
                        let error_code = if input_mismatch {
                            "replay_input_digest_mismatch"
                        } else if error_mismatch {
                            "replay_error_code_mismatch"
                        } else {
                            "replay_status_mismatch"
                        };
                        diagnostics.push(
                            ReplayDiagnostic::with_error_codes(
                                Some(invocation_id),
                                expectation.attempt,
                                expectation.input_digest.clone(),
                                Some(expectation.expected_status),
                                Some(observed),
                                expectation.expected_error_code.clone(),
                                observed_error.clone(),
                                ReplayDivergenceKind::StatusMismatch,
                                error_code,
                                source_cursor,
                                replay_source_ids.clone(),
                            )
                            .map_err(ReplayDiagnosticsError::SnapshotInvalid)?,
                        );
                    }
                }
                // An expectation that matched no invocation is still a divergence: a missing
                // attempt must not disappear from the snapshot.
                for (index, expectation) in expectations.iter().enumerate() {
                    if matched[index] {
                        continue;
                    }
                    diagnostics.push(
                        ReplayDiagnostic::with_error_codes(
                            expectation.invocation_id,
                            expectation.attempt,
                            expectation.input_digest.clone(),
                            Some(expectation.expected_status),
                            None,
                            expectation.expected_error_code.clone(),
                            None,
                            ReplayDivergenceKind::StatusMismatch,
                            "replay_expectation_invocation_missing",
                            source_cursor,
                            replay_source_ids.clone(),
                        )
                        .map_err(ReplayDiagnosticsError::SnapshotInvalid)?,
                    );
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
        if !invocations_projected {
            limitations.push("replay_invocation_projection_unavailable".to_owned());
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

    if let Some(baseline) = baseline {
        compare_consistency_baseline(
            baseline,
            &replay_source_ids,
            &projection_digests,
            events,
            source_cursor,
            &mut diagnostics,
        )?;
    }

    // Canonical locator order: the snapshot is deterministic regardless of projector iteration
    // order, and `first_divergence()` always identifies the same
    // `(invocation_id, attempt, input_digest, status, error_code)` tuple.
    diagnostics.sort_by_key(ReplayDiagnostic::locator_key);
    let status = if diagnostics.is_empty() {
        TraceStatus::Ok
    } else {
        TraceStatus::Unknown
    };
    ReplayDiagnosticSnapshot::new(
        source_cursor,
        replay_source_ids,
        status,
        projection_digests,
        diagnostics,
        limitations,
        baseline.map(|baseline| baseline.baseline_digest.clone()),
    )
    .map_err(ReplayDiagnosticsError::SnapshotInvalid)
}

impl ControlPlane {
    pub async fn replay_diagnostics(
        &self,
        run_id: Option<RunId>,
        expectations: &[ReplayExpectation],
    ) -> Result<ReplayDiagnosticSnapshot, CoreError> {
        self.replay_diagnostics_against_baseline(run_id, expectations, None)
            .await
    }

    /// Read-only replay comparison that additionally pins the deterministic projectors to a
    /// caller-supplied consistency baseline. Reading the EventLog is the only authority this
    /// route uses; no recovery action, retry or compensation is triggered.
    pub async fn replay_diagnostics_against_baseline(
        &self,
        run_id: Option<RunId>,
        expectations: &[ReplayExpectation],
        baseline: Option<&ReplayConsistencyBaseline>,
    ) -> Result<ReplayDiagnosticSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("replay_read_all_unsupported".to_owned())
        })?;
        diagnose_replay_with_baseline(&events, run_id, expectations, baseline)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
