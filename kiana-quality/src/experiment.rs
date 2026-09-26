//! EQ-38 replayable EvalExperiment admission/terminal and case-result index contract.
//!
//! This is a pure reducer over caller-supplied experiment events. It does not persist results,
//! run a case, invoke a judge, or submit a quality-gate decision.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const EXPERIMENT_REPLAY_INPUT_SCHEMA: &str = "kiana.quality-experiment-replay-input.v1";
pub const EXPERIMENT_REPLAY_EVALUATOR_ID: &str = "experiment-replay";
const EVENT_SCHEMA: &str = "kiana.quality-experiment-event.v1";
const EXPERIMENT_SCHEMA: &str = "kiana.quality-experiment.v1";
const MAX_EVENTS: usize = 4_096;
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentEventKind {
    Admitted,
    Started,
    CaseCompleted,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    Admitted,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ExperimentStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentEvent {
    pub schema: String,
    pub sequence: u64,
    pub kind: ExperimentEventKind,
    pub case_id: Option<String>,
    pub result_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalResultIndex {
    pub case_id: String,
    pub result_digest: String,
    pub source_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalExperiment {
    pub schema: String,
    pub experiment_id: String,
    pub suite_ref: String,
    pub status: ExperimentStatus,
    pub case_results: BTreeMap<String, EvalResultIndex>,
    pub terminal_sequence: Option<u64>,
    pub replay_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentReplayInput {
    pub schema: String,
    pub experiment_id: String,
    pub suite_ref: String,
    pub events: Vec<ExperimentEvent>,
}

impl ExperimentReplayInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != EXPERIMENT_REPLAY_INPUT_SCHEMA
            || !valid_text(&self.experiment_id)
            || !valid_text(&self.suite_ref)
            || self.events.len() > MAX_EVENTS
        {
            return Err("experiment_replay_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExperimentReplayError {
    #[error("experiment_event_schema_invalid")]
    EventSchemaInvalid,
    #[error("experiment_event_sequence_invalid")]
    EventSequenceInvalid,
    #[error("experiment_admission_missing")]
    AdmissionMissing,
    #[error("experiment_transition_invalid")]
    TransitionInvalid,
    #[error("experiment_case_result_invalid")]
    CaseResultInvalid,
    #[error("experiment_case_result_conflict")]
    CaseResultConflict,
    #[error("experiment_event_after_terminal")]
    EventAfterTerminal,
    #[error("experiment_terminal_missing")]
    TerminalMissing,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExperimentReplayEvaluator;

impl DeterministicEvaluator for ExperimentReplayEvaluator {
    fn evaluator_id(&self) -> &str {
        EXPERIMENT_REPLAY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: ExperimentReplayInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("experiment_replay"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
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

pub fn replay_experiment(
    input: &ExperimentReplayInput,
) -> Result<EvalExperiment, ExperimentReplayError> {
    if input.events.is_empty() {
        return Err(ExperimentReplayError::AdmissionMissing);
    }
    let mut status = None;
    let mut case_results = BTreeMap::new();
    let mut terminal_sequence = None;
    for (index, event) in input.events.iter().enumerate() {
        if event.schema != EVENT_SCHEMA {
            return Err(ExperimentReplayError::EventSchemaInvalid);
        }
        if event.sequence != (index + 1) as u64 {
            return Err(ExperimentReplayError::EventSequenceInvalid);
        }
        if status.is_some_and(ExperimentStatus::is_terminal) {
            return Err(ExperimentReplayError::EventAfterTerminal);
        }
        match event.kind {
            ExperimentEventKind::Admitted => {
                if index != 0 || status.is_some() {
                    return Err(ExperimentReplayError::AdmissionMissing);
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
                if !matches!(status, Some(ExperimentStatus::Running)) {
                    return Err(ExperimentReplayError::TransitionInvalid);
                }
                let (Some(case_id), Some(result_digest)) = (&event.case_id, &event.result_digest)
                else {
                    return Err(ExperimentReplayError::CaseResultInvalid);
                };
                if !valid_text(case_id) || !valid_digest(result_digest) {
                    return Err(ExperimentReplayError::CaseResultInvalid);
                }
                if let Some(previous) = case_results.get(case_id) {
                    if previous.result_digest != *result_digest {
                        return Err(ExperimentReplayError::CaseResultConflict);
                    }
                } else {
                    case_results.insert(
                        case_id.clone(),
                        EvalResultIndex {
                            case_id: case_id.clone(),
                            result_digest: result_digest.clone(),
                            source_sequence: event.sequence,
                        },
                    );
                }
            }
            ExperimentEventKind::Completed
            | ExperimentEventKind::Failed
            | ExperimentEventKind::Cancelled => {
                if !matches!(
                    status,
                    Some(ExperimentStatus::Running | ExperimentStatus::Admitted)
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
    if !status.is_terminal() {
        return Err(ExperimentReplayError::TerminalMissing);
    }
    let replay_digest = json_digest(&serde_json::json!({
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

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
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
    let actual = if actual.len() <= MAX_TEXT_BYTES && !actual.contains(['\0', '\n', '\r']) {
        actual.to_owned()
    } else {
        "invalid".to_owned()
    };
    findings.push(
        Finding::new(
            code,
            serde_json::Value::String(expected.to_owned()),
            serde_json::Value::String(actual),
            format!("experiment replay finding: {code}"),
            "experiment:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
