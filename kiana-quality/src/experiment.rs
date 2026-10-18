//! EQ-38 replayable `EvalExperiment` admission/terminal states and case-result index contract.
//!
//! An experiment is a value reduced from caller-supplied events only: exactly one admission,
//! an optional start, case-completed results and exactly one terminal event reduce
//! deterministically into an experiment status, a bounded case-result index and a replay digest.
//! Sequence gaps, a missing admission, a missing terminal, illegal transitions, a repeated or
//! drifted case result and any event after a terminal fail closed, so a caller can never
//! manufacture a completed experiment by reordering, repeating, trimming or extending the log.
//!
//! This is a pure value/reducer contract.  It does not persist results, run a case, invoke a
//! provider or judge, allocate budget or submit a quality-gate decision.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Versioned envelope for one experiment event replay.
pub const EXPERIMENT_REPLAY_INPUT_SCHEMA: &str = "kiana.quality-experiment-replay-input.v1";
/// Stable registry identifier for the experiment replay evaluator.
pub const EXPERIMENT_REPLAY_EVALUATOR_ID: &str = "experiment-replay";
/// Versioned schema for one caller-supplied experiment event.
pub const EXPERIMENT_EVENT_SCHEMA: &str = "kiana.quality-experiment-event.v1";
/// Versioned schema for the reduced experiment value.
pub const EXPERIMENT_SCHEMA: &str = "kiana.quality-experiment.v1";
/// Maximum events accepted for one experiment replay.
pub const MAX_EXPERIMENT_EVENTS: usize = 4_096;
/// Maximum bytes accepted in one bounded experiment/case identity token.
pub const MAX_EXPERIMENT_TEXT_BYTES: usize = 512;

const FIRST_SEQUENCE: u64 = 1;
const INPUT_EVIDENCE: &str = "experiment:input";

/// Kind of one caller-supplied experiment event.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentEventKind {
    /// The experiment was admitted; this must be the first event and appear exactly once.
    Admitted,
    /// Execution started after admission; exactly one start is accepted.
    Started,
    /// One case produced a final result digest while the experiment is running.
    CaseCompleted,
    /// The experiment reached the completed terminal state.
    Completed,
    /// The experiment reached the failed terminal state.
    Failed,
    /// The experiment reached the cancelled terminal state.
    Cancelled,
}

/// Reduction state of one experiment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    /// Admitted but not started.
    Admitted,
    /// Started and not terminal yet.
    Running,
    /// Terminal: the experiment finished and reports the case results it collected.
    Completed,
    /// Terminal: the experiment failed.
    Failed,
    /// Terminal: the experiment was cancelled.
    Cancelled,
}

impl ExperimentStatus {
    /// Terminal statuses can never receive another event.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Stable wire and digest spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One caller-supplied experiment event.  `sequence` must be contiguous from 1.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentEvent {
    /// Event schema; a drift is refused before the event is reduced.
    pub schema: String,
    /// Contiguous index of this event inside the experiment log, starting at 1.
    pub sequence: u64,
    /// Event kind selecting the transition.
    pub kind: ExperimentEventKind,
    /// Case result identity; only meaningful for `case_completed`.
    pub case_id: Option<String>,
    /// Strict `sha256:` result digest; only meaningful for `case_completed`.
    pub result_digest: Option<String>,
}

impl ExperimentEvent {
    /// Build one event without a case result (`admitted`/`started`/terminal kinds).
    pub fn new(sequence: u64, kind: ExperimentEventKind) -> Self {
        Self {
            schema: EXPERIMENT_EVENT_SCHEMA.to_owned(),
            sequence,
            kind,
            case_id: None,
            result_digest: None,
        }
    }

    /// Attach the case result carried by a `case_completed` event.
    pub fn with_case_result(
        mut self,
        case_id: impl Into<String>,
        result_digest: impl Into<String>,
    ) -> Self {
        self.case_id = Some(case_id.into());
        self.result_digest = Some(result_digest.into());
        self
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("experiment event is serializable")
    }
}

/// Indexed case result: which case finished, its result digest and the exact source event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalResultIndex {
    /// Case identity this result belongs to.
    pub case_id: String,
    /// Strict `sha256:` digest of the case result.
    pub result_digest: String,
    /// Sequence of the single `case_completed` event that produced this entry.
    pub source_sequence: u64,
}

/// Reduced, replayable experiment value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalExperiment {
    /// Schema of the reduced value.
    pub schema: String,
    /// Experiment identity supplied by the caller.
    pub experiment_id: String,
    /// Suite reference supplied by the caller.
    pub suite_ref: String,
    /// Reduction status derived from the events alone.
    pub status: ExperimentStatus,
    /// Case-result index, ordered by stable case identity.
    pub case_results: BTreeMap<String, EvalResultIndex>,
    /// Sequence of the single terminal event.
    pub terminal_sequence: Option<u64>,
    /// Canonical digest binding identity, status, index and the complete event log.
    pub replay_digest: String,
}

impl EvalExperiment {
    /// Terminal experiments cannot accept further events.
    pub const fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    /// Indexed result for one case, when the log reported one.
    pub fn case_result(&self, case_id: &str) -> Option<&EvalResultIndex> {
        self.case_results.get(case_id)
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("experiment is serializable")
    }
}

/// Versioned input envelope for [`ExperimentReplayEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentReplayInput {
    /// Envelope schema; a drift is refused.
    pub schema: String,
    /// Bounded, single-line experiment identity.
    pub experiment_id: String,
    /// Bounded, single-line suite reference.
    pub suite_ref: String,
    /// Complete event log in contiguous sequence order.
    pub events: Vec<ExperimentEvent>,
}

impl ExperimentReplayInput {
    /// Build one replay input from an ordered event log.
    pub fn new(
        experiment_id: impl Into<String>,
        suite_ref: impl Into<String>,
        events: Vec<ExperimentEvent>,
    ) -> Self {
        Self {
            schema: EXPERIMENT_REPLAY_INPUT_SCHEMA.to_owned(),
            experiment_id: experiment_id.into(),
            suite_ref: suite_ref.into(),
            events,
        }
    }

    /// Structural bounds fail closed before any event is reduced: a drifted envelope schema, an
    /// unbounded/empty/whitespace identity or more than [`MAX_EXPERIMENT_EVENTS`] events are
    /// refusals, not findings.
    pub fn validate(&self) -> Result<(), ExperimentReplayError> {
        if self.schema != EXPERIMENT_REPLAY_INPUT_SCHEMA {
            return Err(ExperimentReplayError::InputSchemaInvalid);
        }
        if !valid_identity(&self.experiment_id) || !valid_identity(&self.suite_ref) {
            return Err(ExperimentReplayError::IdentityInvalid);
        }
        if self.events.len() > MAX_EXPERIMENT_EVENTS {
            return Err(ExperimentReplayError::EventLimitExceeded);
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("experiment replay input is serializable")
    }
}

/// Refusal of a non-replayable experiment event log.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExperimentReplayError {
    /// The envelope schema is not the version supported by this contract.
    #[error("experiment_replay_schema_invalid")]
    InputSchemaInvalid,
    /// The experiment or suite identity is empty, whitespace or out of bounds.
    #[error("experiment_replay_identity_invalid")]
    IdentityInvalid,
    /// The event log exceeds [`MAX_EXPERIMENT_EVENTS`].
    #[error("experiment_event_limit_exceeded")]
    EventLimitExceeded,
    /// One event carries a schema this contract does not support.
    #[error("experiment_event_schema_invalid")]
    EventSchemaInvalid,
    /// Sequences are not contiguous from 1, so the log has a gap, a repeat or a reorder.
    #[error("experiment_event_sequence_invalid")]
    EventSequenceInvalid,
    /// The log does not start with exactly one admission event.
    #[error("experiment_admission_missing")]
    AdmissionMissing,
    /// An event cannot follow the state reached by the previous events.
    #[error("experiment_transition_invalid")]
    TransitionInvalid,
    /// A case result is missing its bounded identity or its strict `sha256:` digest.
    #[error("experiment_case_result_invalid")]
    CaseResultInvalid,
    /// Two `case_completed` events disagree about one case result.
    #[error("experiment_case_result_conflict")]
    CaseResultConflict,
    /// One case produced more than one result event, even with an identical digest.
    #[error("experiment_case_result_repeated")]
    CaseResultRepeated,
    /// An event was appended after the experiment reached a terminal state.
    #[error("experiment_event_after_terminal")]
    EventAfterTerminal,
    /// The log never reached a terminal state.
    #[error("experiment_terminal_missing")]
    TerminalMissing,
}

/// Deterministic evaluator that refuses a non-replayable experiment event log.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExperimentReplayEvaluator;

impl DeterministicEvaluator for ExperimentReplayEvaluator {
    fn evaluator_id(&self) -> &str {
        EXPERIMENT_REPLAY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: ExperimentReplayInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("experiment_replay"))?;
        decoded
            .validate()
            .map_err(|_| EvaluatorError::InputInvalid("experiment_replay"))?;
        match replay_experiment(&decoded) {
            Ok(_) => Ok(Vec::new()),
            Err(error) => {
                let mut findings = Vec::new();
                emit(
                    &mut findings,
                    "experiment.replay_invalid",
                    "replayable_event_log",
                    error.to_string().as_str(),
                )?;
                Ok(findings)
            }
        }
    }
}

/// Reduce one experiment event log into a replayable experiment value.
///
/// The log must start with exactly one `admitted` event, carry contiguous sequences from 1,
/// report at most one result per case, reach exactly one terminal state and stop there.  Any
/// other shape is a refusal, so a caller cannot trim a gap, reorder events, repeat a case result
/// or append after a terminal and still obtain a completed experiment.
pub fn replay_experiment(
    input: &ExperimentReplayInput,
) -> Result<EvalExperiment, ExperimentReplayError> {
    input.validate()?;
    if input.events.is_empty() {
        return Err(ExperimentReplayError::AdmissionMissing);
    }
    let mut status: Option<ExperimentStatus> = None;
    let mut case_results: BTreeMap<String, EvalResultIndex> = BTreeMap::new();
    let mut terminal_sequence: Option<u64> = None;
    for (index, event) in input.events.iter().enumerate() {
        if event.schema != EXPERIMENT_EVENT_SCHEMA {
            return Err(ExperimentReplayError::EventSchemaInvalid);
        }
        if event.sequence != index as u64 + FIRST_SEQUENCE {
            return Err(ExperimentReplayError::EventSequenceInvalid);
        }
        if index == 0 && event.kind != ExperimentEventKind::Admitted {
            return Err(ExperimentReplayError::AdmissionMissing);
        }
        if status.is_some_and(ExperimentStatus::is_terminal) {
            return Err(ExperimentReplayError::EventAfterTerminal);
        }
        match event.kind {
            ExperimentEventKind::Admitted => {
                if index != 0 || status.is_some() {
                    return Err(ExperimentReplayError::TransitionInvalid);
                }
                status = Some(ExperimentStatus::Admitted);
            }
            ExperimentEventKind::Started => {
                if status != Some(ExperimentStatus::Admitted) {
                    return Err(ExperimentReplayError::TransitionInvalid);
                }
                status = Some(ExperimentStatus::Running);
            }
            ExperimentEventKind::CaseCompleted => {
                if status != Some(ExperimentStatus::Running) {
                    return Err(ExperimentReplayError::TransitionInvalid);
                }
                let (Some(case_id), Some(result_digest)) =
                    (event.case_id.as_deref(), event.result_digest.as_deref())
                else {
                    return Err(ExperimentReplayError::CaseResultInvalid);
                };
                if !valid_identity(case_id) || !valid_digest(result_digest) {
                    return Err(ExperimentReplayError::CaseResultInvalid);
                }
                if let Some(previous) = case_results.get(case_id) {
                    return Err(if previous.result_digest == result_digest {
                        ExperimentReplayError::CaseResultRepeated
                    } else {
                        ExperimentReplayError::CaseResultConflict
                    });
                }
                case_results.insert(
                    case_id.to_owned(),
                    EvalResultIndex {
                        case_id: case_id.to_owned(),
                        result_digest: result_digest.to_owned(),
                        source_sequence: event.sequence,
                    },
                );
            }
            ExperimentEventKind::Completed
            | ExperimentEventKind::Failed
            | ExperimentEventKind::Cancelled => {
                if !matches!(
                    status,
                    Some(ExperimentStatus::Admitted | ExperimentStatus::Running)
                ) {
                    return Err(ExperimentReplayError::TransitionInvalid);
                }
                status = Some(match event.kind {
                    ExperimentEventKind::Completed => ExperimentStatus::Completed,
                    ExperimentEventKind::Failed => ExperimentStatus::Failed,
                    ExperimentEventKind::Cancelled => ExperimentStatus::Cancelled,
                    _ => unreachable!(),
                });
                terminal_sequence = Some(event.sequence);
            }
        }
    }
    let status = status.ok_or(ExperimentReplayError::AdmissionMissing)?;
    if !status.is_terminal() || terminal_sequence.is_none() {
        return Err(ExperimentReplayError::TerminalMissing);
    }
    let replay_digest = json_digest(&json!({
        "schema": EXPERIMENT_SCHEMA,
        "experiment_id": input.experiment_id,
        "suite_ref": input.suite_ref,
        "status": status,
        "case_results": case_results,
        "terminal_sequence": terminal_sequence,
        "events": input.events,
    }));
    Ok(EvalExperiment {
        schema: EXPERIMENT_SCHEMA.to_owned(),
        experiment_id: input.experiment_id.clone(),
        suite_ref: input.suite_ref.clone(),
        status,
        case_results,
        terminal_sequence,
        replay_digest,
    })
}

fn valid_identity(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_EXPERIMENT_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    let actual =
        if actual.len() <= MAX_EXPERIMENT_TEXT_BYTES && !actual.contains(['\0', '\n', '\r']) {
            actual.to_owned()
        } else {
            "invalid".to_owned()
        };
    findings.push(
        Finding::redacted(
            code,
            Value::String(expected.to_owned()),
            Value::String(actual),
            format!("experiment replay finding: {code}"),
            INPUT_EVIDENCE,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
