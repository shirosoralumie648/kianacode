use super::*;
use kiana_domain::RunId;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunPhase {
    Authorized,
    Queued,
    Cancelling,
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
    #[error("run_event_run_id_conflict")]
    EventRunIdConflict,
    #[error("run_event_run_id_invalid")]
    EventRunIdInvalid,
    #[error("run_event_identity_conflict:{event_id}")]
    EventIdentityConflict { event_id: String },
    #[error("run_identity_links_invalid:{reason}")]
    IdentityLinksInvalid { reason: String },
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

fn event_run_id(event: &RuntimeEvent) -> Result<Option<RunId>, RunProjectionError> {
    let aggregate_run_id = match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(aggregate_id)) => {
            Some(RunId::parse_str(aggregate_id).ok_or(RunProjectionError::EventRunIdInvalid)?)
        }
        (Some("run"), None) => return Err(RunProjectionError::EventRunIdInvalid),
        _ => None,
    };
    let payload_run_id = match event.data.get("run_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            Some(RunId::parse_str(value).ok_or(RunProjectionError::EventRunIdInvalid)?)
        }
        Some(_) => return Err(RunProjectionError::EventRunIdInvalid),
    };
    if let (Some(aggregate_run_id), Some(payload_run_id)) = (aggregate_run_id, payload_run_id) {
        if aggregate_run_id != payload_run_id {
            return Err(RunProjectionError::EventRunIdConflict);
        }
    }
    Ok(payload_run_id.or(aggregate_run_id))
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
    let mut seen_events = HashMap::new();

    let mut ordered_events = Vec::new();
    for (index, event) in events.iter().enumerate() {
        event
            .validate_identity_links()
            .map_err(|reason| RunProjectionError::IdentityLinksInvalid { reason })?;
        if event_run_id(event)?.is_some_and(|event_run_id| event_run_id == run_id) {
            ordered_events.push((index, event));
        }
    }
    // Stream versions are comparable only within one aggregate. read_all supplies
    // durable append order for mixed run, packet, invocation and approval facts.
    let same_stream = ordered_events.first().is_some_and(|(_, first)| {
        first.aggregate_type.is_some()
            && first.aggregate_id.is_some()
            && first.stream_version.is_some()
            && ordered_events.iter().all(|(_, event)| {
                event.aggregate_type == first.aggregate_type
                    && event.aggregate_id == first.aggregate_id
                    && event.stream_version.is_some()
            })
    });
    if same_stream {
        // Request-local sequence is not an aggregate version; legacy/mixed inputs preserve append order.
        ordered_events
            .sort_by_key(|(index, event)| (event.stream_version.unwrap_or(event.sequence), *index));
    }
    for (_, event) in ordered_events {
        if let Some(previous) = seen_events.insert(event.event_id, event) {
            if previous != event {
                return Err(RunProjectionError::EventIdentityConflict {
                    event_id: event.event_id.to_string(),
                });
            }
            continue;
        }
        // Continue opens a new turn on the same run; its outcome supersedes the previous turn.
        if event.kind == "run.prompt" {
            phase = RunPhase::Running;
            outcome = None;
            error = None;
            terminal_kinds.clear();
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
            "run.queued" => RunPhase::Queued,
            "run.cancelling" => RunPhase::Cancelling,
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
    fn event_belongs_to_run(event: &RuntimeEvent, run_id: RunId) -> bool {
        let run_id = run_id.to_string();
        let exact_run_stream = event.aggregate_type.as_deref() == Some("run")
            && event.aggregate_id.as_deref() == Some(run_id.as_str());
        let conflicting_run_stream = event.aggregate_type.as_deref() == Some("run")
            && event
                .aggregate_id
                .as_deref()
                .is_some_and(|value| value != run_id);
        match event.data.get("run_id") {
            Some(Value::String(value)) => value == &run_id && !conflicting_run_stream,
            Some(_) => false,
            None => exact_run_stream,
        }
    }

    pub(crate) fn cache_invocation_projection(
        &self,
        run_id: RunId,
        events: &[RuntimeEvent],
    ) -> Result<Vec<InvocationProjection>, CoreError> {
        let projections = crate::project_invocations(run_id, events)
            .map_err(|reason| CoreError::Port(PortError::Failed(reason)))?;
        let event_ids = events
            .iter()
            .filter(|event| Self::event_belongs_to_run(event, run_id))
            .map(|event| event.event_id.to_string())
            .collect::<HashSet<_>>();
        self.invocation_projections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run_id, projections.clone());
        self.invocation_projection_event_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run_id, event_ids);
        Ok(projections)
    }

    pub(crate) fn invalidate_invocation_projection(&self, run_id: RunId) {
        self.invocation_projections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
        self.invocation_projection_event_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
    }

    /// Rebuild an invocation projection on the first access of a run. The event journal is the
    /// authority; the in-process map only stores the exact folded result for subsequent reads.
    pub async fn invocation_state(
        &self,
        run_id: RunId,
    ) -> Result<Vec<InvocationProjection>, CoreError> {
        let cached = self
            .invocation_projections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run_id)
            .cloned();
        if let Some(cached) = cached {
            // The broker's JournalPermitVerifier appends invocation.dispatching directly to the
            // EventStore. Compare run-scoped event IDs before trusting a cached fold so a process
            // restart or a broker crash cannot expose the pre-dispatch state indefinitely.
            if let Some(all) = self.read_all_events().await? {
                let known = self
                    .invocation_projection_event_ids
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&run_id)
                    .cloned()
                    .unwrap_or_default();
                let has_new_event = all
                    .iter()
                    .filter(|event| Self::event_belongs_to_run(event, run_id))
                    .any(|event| !known.contains(&event.event_id.to_string()));
                if !has_new_event {
                    return Ok(cached);
                }
                self.invalidate_invocation_projection(run_id);
            } else {
                return Ok(cached);
            }
        }
        let run_stream = self.events.read_stream("run", &run_id.to_string()).await?;
        let mut events = match self.read_all_events().await? {
            Some(all) => all,
            None => run_stream.clone(),
        };
        let mut seen = events
            .iter()
            .map(|event| event.event_id.to_string())
            .collect::<HashSet<_>>();
        for event in run_stream {
            if seen.insert(event.event_id.to_string()) {
                events.push(event);
            }
        }
        if events.is_empty() {
            return Err(CoreError::Port(PortError::Failed(
                "run_not_found".to_owned(),
            )));
        }
        self.cache_invocation_projection(run_id, &events)
    }

    /// 从事件账本重建 run 状态，并让 invocation 投影走同一套惰性缓存入口。
    pub async fn run_state(&self, run_id: RunId) -> Result<RunState, CoreError> {
        let unsupported =
            || CoreError::Port(PortError::Failed("run_projection_unsupported".to_owned()));
        let events = self.read_all_events().await?.ok_or_else(unsupported)?;
        let run_events = crate::receipts::try_filter_run_events(&events, run_id)
            .map_err(|reason| CoreError::Port(PortError::Failed(reason)))?;
        if run_events.is_empty() {
            return Err(CoreError::Port(PortError::Failed(
                "run_not_found".to_owned(),
            )));
        }
        project_run_state(run_id, &run_events).map_err(|error| match error {
            RunProjectionError::TerminalConflict { kinds } => CoreError::Port(PortError::Failed(
                format!("run_terminal_conflict:{}", kinds.join(",")),
            )),
            RunProjectionError::EventRunIdConflict => {
                CoreError::Port(PortError::Failed("run_event_run_id_conflict".to_owned()))
            }
            RunProjectionError::EventRunIdInvalid => {
                CoreError::Port(PortError::Failed("run_event_run_id_invalid".to_owned()))
            }
            RunProjectionError::EventIdentityConflict { event_id } => CoreError::Port(
                PortError::Failed(format!("run_event_identity_conflict:{event_id}")),
            ),
            RunProjectionError::IdentityLinksInvalid { reason } => CoreError::Port(
                PortError::Failed(format!("run_identity_links_invalid:{reason}")),
            ),
        })
    }
}
