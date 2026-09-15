//! Provider-independent evaluation runner over committed normalized facts.
//!
//! Evaluation is a read-only quality gate. It never calls a model/provider/Broker and cannot
//! promote a capability or rewrite a receipt; it only reports whether an explicit evidence spec
//! was satisfied.

use kiana_domain::{
    json_digest, EvalCaseResult, EvalCaseSpec, EvalCostKind, EvalSuiteReport, EvalVerdict, EventId,
    RunId, RuntimeEvent, TraceStatus,
};
use serde_json::json;
use std::collections::{BTreeSet, HashSet};

use super::{
    diagnose_replay, project_operational_metrics, project_run_state, project_span_lifecycle,
    rebuild_audit_projection, ReplayExpectation,
};
use super::{ControlPlane, CoreError};

const MAX_EVAL_EVENTS: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EvalError {
    #[error("eval_source_empty")]
    SourceEmpty,
    #[error("eval_source_event_limit")]
    SourceLimit,
    #[error("eval_source_event_duplicate")]
    SourceDuplicate,
    #[error("eval_case_invalid:{0}")]
    CaseInvalid(String),
    #[error("eval_suite_invalid:{0}")]
    SuiteInvalid(String),
}

fn source_ids(events: &[RuntimeEvent]) -> Result<Vec<EventId>, EvalError> {
    if events.is_empty() {
        return Err(EvalError::SourceEmpty);
    }
    if events.len() > MAX_EVAL_EVENTS {
        return Err(EvalError::SourceLimit);
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(events.len().min(256));
    for event in events {
        if !seen.insert(event.event_id.to_string()) {
            return Err(EvalError::SourceDuplicate);
        }
        if ids.len() < kiana_domain::MAX_SOURCE_EVENT_IDS {
            ids.push(event.event_id);
        }
    }
    Ok(ids)
}

fn source_cursor(events: &[RuntimeEvent]) -> u64 {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            event
                .data
                .get("source_cursor")
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    u64::try_from(index)
                        .ok()
                        .and_then(|value| value.checked_add(1))
                })
        })
        .max()
        .unwrap_or_default()
}

fn normalized_event_digest(events: &[RuntimeEvent]) -> String {
    let kinds = events
        .iter()
        .map(|event| {
            json!({
                "kind": event.kind,
                "sequence": event.sequence,
                "stream_version": event.stream_version,
            })
        })
        .collect::<Vec<_>>();
    json_digest(&json!(kinds))
}

fn contains_secret(events: &[RuntimeEvent]) -> bool {
    let text = serde_json::to_string(events)
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "bearer ",
        "sk-",
        "password",
        "raw-secret",
        "authorization:",
        "api_key",
        "token=",
    ]
    .iter()
    .any(|sentinel| text.contains(sentinel))
}

fn observed_status(run_id: Option<RunId>, events: &[RuntimeEvent]) -> TraceStatus {
    let Some(run_id) = run_id else {
        return TraceStatus::Degraded;
    };
    match project_run_state(run_id, events)
        .ok()
        .and_then(|state| state.outcome)
    {
        Some(super::RunOutcome::Completed) => TraceStatus::Ok,
        Some(super::RunOutcome::Failed) => TraceStatus::Error,
        Some(super::RunOutcome::Cancelled | super::RunOutcome::ResultUnknown) => {
            TraceStatus::Unknown
        }
        None => TraceStatus::Degraded,
    }
}

fn observed_effect(events: &[RuntimeEvent]) -> bool {
    events.iter().any(|event| {
        event
            .data
            .get("effect_started")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
            || (event.kind == "execution.result_committed"
                && event
                    .data
                    .get("effect_known")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
    })
}

fn latency_bucket(events: &[RuntimeEvent]) -> Option<String> {
    let elapsed = events
        .iter()
        .filter_map(|event| {
            event
                .data
                .get("elapsed_ms")
                .and_then(serde_json::Value::as_u64)
        })
        .max()?;
    Some(
        if elapsed <= 100 {
            "fast"
        } else if elapsed <= 1_000 {
            "moderate"
        } else {
            "slow"
        }
        .to_owned(),
    )
}

fn evaluate_case_inner(
    events: &[RuntimeEvent],
    spec: &EvalCaseSpec,
    run_id: Option<RunId>,
) -> Result<EvalCaseResult, EvalError> {
    spec.validate().map_err(EvalError::CaseInvalid)?;
    let source_event_ids = source_ids(events)?;
    let source_cursor = source_cursor(events);
    if source_cursor == 0 {
        return Err(EvalError::SourceEmpty);
    }
    let normalized = events
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<BTreeSet<_>>();
    let mut failures = Vec::new();
    for expected in &spec.expected_event_kinds {
        if !normalized.contains(expected.as_str()) {
            failures.push(format!("missing_event_kind:{expected}"));
        }
    }
    let secret_detected = contains_secret(events);
    if spec.forbidden_secrets && secret_detected {
        failures.push("secret_sentinel_detected".to_owned());
    }
    let effect_observed = observed_effect(events);
    if spec.forbidden_effect && effect_observed {
        failures.push("forbidden_effect_observed".to_owned());
    }

    let observed_status = observed_status(run_id, events);
    if observed_status != spec.expected_status {
        failures.push(format!(
            "status_mismatch:expected={:?}:observed={:?}",
            spec.expected_status, observed_status
        ));
    }

    let audit_digest = if spec.require_audit {
        match rebuild_audit_projection(events, 1) {
            Ok(snapshot) => Some(snapshot.digest()),
            Err(error) => {
                failures.push(format!("audit_projection:{error}"));
                None
            }
        }
    } else {
        None
    };
    let metric_digest = if spec.require_metrics {
        match project_operational_metrics(events, None) {
            Ok(snapshot) => Some(snapshot.digest()),
            Err(error) => {
                failures.push(format!("metric_projection:{error}"));
                None
            }
        }
    } else {
        None
    };
    let span_digest = if spec.require_spans {
        match run_id {
            Some(run_id) => match project_span_lifecycle(run_id, events) {
                Ok(spans) => Some(json_digest(&json!(spans))),
                Err(error) => {
                    failures.push(format!("span_projection:{error}"));
                    None
                }
            },
            None => {
                failures.push("span_run_required".to_owned());
                None
            }
        }
    } else {
        None
    };
    let receipt_digest = if spec.require_receipt {
        match run_id.and_then(|run_id| project_run_state(run_id, events).ok()) {
            Some(state) => Some(json_digest(&json!({
                "run_id": state.run_id,
                "phase": format!("{:?}", state.phase),
                "outcome": state.outcome.map(|outcome| format!("{:?}", outcome)),
                "error": state.error,
            }))),
            None => {
                failures.push("receipt_projection_missing".to_owned());
                None
            }
        }
    } else {
        None
    };
    let replay_diverged = match diagnose_replay(events, run_id, &[] as &[ReplayExpectation]) {
        Ok(snapshot) => {
            if !snapshot.diagnostics.is_empty() {
                failures.push("replay_divergence".to_owned());
                true
            } else {
                false
            }
        }
        Err(error) => {
            failures.push(format!("replay_diagnostics:{error}"));
            true
        }
    };
    if let Some(expected_bucket) = &spec.latency_bucket {
        if latency_bucket(events).as_deref() != Some(expected_bucket.as_str()) {
            failures.push("latency_bucket_mismatch".to_owned());
        }
    }
    if spec.cost_kind == EvalCostKind::Measured
        && !events.iter().any(|event| {
            event
                .data
                .get("usage_complete")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
    {
        failures.push("measured_cost_evidence_missing".to_owned());
    }
    let verdict = if secret_detected || (spec.forbidden_effect && effect_observed) {
        EvalVerdict::Blocked
    } else if failures.is_empty() {
        EvalVerdict::Pass
    } else {
        EvalVerdict::Fail
    };
    EvalCaseResult::new(
        spec.case_id.clone(),
        verdict,
        observed_status,
        normalized_event_digest(events),
        audit_digest,
        metric_digest,
        span_digest,
        receipt_digest,
        effect_observed,
        secret_detected,
        replay_diverged,
        source_cursor,
        source_event_ids,
        failures,
        spec.cost_kind,
        latency_bucket(events),
    )
    .map_err(EvalError::CaseInvalid)
}

/// Evaluate all cases against one normalized committed fact set.
pub fn evaluate_provider_independent(
    events: &[RuntimeEvent],
    specs: &[EvalCaseSpec],
    run_id: Option<RunId>,
) -> Result<EvalSuiteReport, EvalError> {
    if events.is_empty() {
        return Err(EvalError::SourceEmpty);
    }
    let source_event_ids = source_ids(events)?;
    let cases = specs
        .iter()
        .map(|spec| evaluate_case_inner(events, spec, run_id))
        .collect::<Result<Vec<_>, _>>()?;
    EvalSuiteReport::new(
        "provider-independent",
        source_cursor(events),
        source_event_ids,
        cases,
        Vec::new(),
    )
    .map_err(EvalError::SuiteInvalid)
}

pub fn evaluate_suite(
    events: &[RuntimeEvent],
    specs: &[EvalCaseSpec],
    run_id: Option<RunId>,
) -> Result<EvalSuiteReport, EvalError> {
    evaluate_provider_independent(events, specs, run_id)
}

impl ControlPlane {
    pub async fn evaluate_provider_independent(
        &self,
        specs: &[EvalCaseSpec],
        run_id: Option<RunId>,
    ) -> Result<EvalSuiteReport, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("eval_read_all_unsupported".to_owned())
        })?;
        evaluate_provider_independent(&events, specs, run_id)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
