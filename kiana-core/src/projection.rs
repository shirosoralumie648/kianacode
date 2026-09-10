use super::*;
use kiana_domain::RunId;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunPhase {
    Authorized,
    Running,
    AwaitingApproval,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Completed,
    Failed,
    Cancelled,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RunProjectionError {
    #[error("run terminal conflict: {}", kinds.join(", "))]
    TerminalConflict { kinds: Vec<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunState {
    pub run_id: RunId,
    pub phase: RunPhase,
    pub outcome: Option<RunOutcome>,
    pub error: Option<String>,
}

fn outcome_for_kind(kind: &str) -> Option<RunOutcome> {
    match kind {
        "run.completed" => Some(RunOutcome::Completed),
        "run.failed" => Some(RunOutcome::Failed),
        "run.cancelled" => Some(RunOutcome::Cancelled),
        "run.result_unknown" => Some(RunOutcome::ResultUnknown),
        _ => None,
    }
}

fn event_run_id(event: &RuntimeEvent) -> Option<RunId> {
    let payload_run_id = event
        .data
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str);
    if payload_run_id.is_some() {
        return payload_run_id;
    }
    match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(aggregate_id)) => RunId::parse_str(aggregate_id),
        _ => None,
    }
}

fn event_error(event: &RuntimeEvent) -> Option<String> {
    match event.data.get("error") {
        Some(Value::String(error)) if !error.is_empty() => Some(error.to_owned()),
        _ => None,
    }
}

pub fn project_run_state(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<RunState, RunProjectionError> {
    let mut phase = RunPhase::Authorized;
    let mut outcome = None;
    let mut error = None;
    let mut terminal_kinds = BTreeSet::new();
    let mut seen_keys = HashSet::new();

    let mut ordered_events = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event_run_id(event).is_some_and(|event_run_id| event_run_id == run_id))
        .collect::<Vec<_>>();
    ordered_events
        .sort_by_key(|(index, event)| (event.stream_version.unwrap_or(event.sequence), *index));

    for (_, event) in ordered_events {
        if event_run_id(event).is_none_or(|event_run_id| event_run_id != run_id) {
            continue;
        }
        let order = event.stream_version.unwrap_or(event.sequence);
        let sequence_scope = event.stream_version.map(|_| event.request_id);
        if !seen_keys.insert((sequence_scope, order)) {
            continue;
        }

        if let Some(event_outcome) = outcome_for_kind(&event.kind) {
            if !terminal_kinds.insert(event.kind.clone()) {
                continue;
            }
            if terminal_kinds.len() > 1 {
                return Err(RunProjectionError::TerminalConflict {
                    kinds: terminal_kinds.into_iter().collect(),
                });
            }
            phase = RunPhase::Terminal;
            outcome = Some(event_outcome);
            if event.kind != "run.completed" {
                error = event_error(event);
            }
            continue;
        }

        if outcome.is_some() {
            continue;
        }

        phase = match event.kind.as_str() {
            "run.authorized" | "run.started" => RunPhase::Running,
            "approval.requested" => RunPhase::AwaitingApproval,
            "approval.approved" | "approval.denied" => RunPhase::Running,
            _ => phase,
        };
    }

    Ok(RunState {
        run_id,
        phase,
        outcome,
        error,
    })
}

impl ControlPlane {
    /// 只读地从事件账本重建 run 状态；不读取或改变 ControlPlane 的内存缓存。
    pub async fn run_state(&self, run_id: RunId) -> Result<RunState, CoreError> {
        let unsupported =
            || CoreError::Port(PortError::Failed("run_projection_unsupported".to_owned()));
        let events = self.read_all_events().await?.ok_or_else(unsupported)?;
        let run_events = crate::receipts::filter_run_events(&events, run_id);
        if run_events.is_empty() {
            return Err(CoreError::Port(PortError::Failed(
                "run_not_found".to_owned(),
            )));
        }
        project_run_state(run_id, &run_events).map_err(|error| match error {
            RunProjectionError::TerminalConflict { kinds } => CoreError::Port(PortError::Failed(
                format!("run_terminal_conflict:{}", kinds.join(",")),
            )),
        })
    }
}
