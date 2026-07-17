use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::workflow::{
    append_workflow_event_with_data, append_workflow_event_with_unique_data_value,
    commit_immutable_artifacts_with_unique_event, read_workflow_events, ReviewPacket,
    ReviewSeverity, WorkflowArtifactBatch, WorkflowArtifactInput, WorkflowError, WorkflowEventKind,
};

pub const EVIDENCE_EVENT_SCHEMA: &str = "kiana.evidence-event.v1";
pub const VERIFICATION_PACKET_SCHEMA: &str = "kiana.verification-packet.v1";
pub const REVIEW_PACKET_SCHEMA: &str = "kiana.review-packet.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    CommandResult,
    DiffSummary,
    TestResult,
    BuildResult,
    LintResult,
    SecurityResult,
    ReviewFinding,
    ApprovalRecord,
    BlockerRecord,
    ArtifactCheck,
    EdaCheck,
    MemoryDecision,
}

impl EvidenceKind {
    fn is_command_like(self) -> bool {
        matches!(
            self,
            Self::CommandResult
                | Self::TestResult
                | Self::BuildResult
                | Self::LintResult
                | Self::SecurityResult
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    Blocked,
    Skipped,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceEventDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub workflow_id: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workpacket_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_at_ms: Option<u64>,
    pub kind: EvidenceKind,
    pub status: EvidenceStatus,
    pub summary: String,
    pub source: EvidenceSource,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceEvent {
    pub schema: String,
    pub sequence: u64,
    pub event_id: String,
    pub workflow_id: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workpacket_id: Option<String>,
    pub recorded_at_ms: u64,
    pub kind: EvidenceKind,
    pub status: EvidenceStatus,
    pub summary: String,
    pub source: EvidenceSource,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub check_id: String,
    pub description: String,
    pub required: bool,
    pub status: EvidenceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Pass,
    Fail,
    Blocked,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationPacket {
    pub schema: String,
    pub verification_id: String,
    pub workflow_id: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub profile: String,
    pub created_at_ms: u64,
    pub checks: Vec<VerificationCheck>,
    pub pass_count: usize,
    pub fail_count: usize,
    pub blocked_count: usize,
    pub skipped_count: usize,
    pub unknown_count: usize,
    pub final_status: VerificationStatus,
    pub evidence_ids: Vec<String>,
    pub next_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerSummary {
    pub event_count: usize,
    pub latest_sequence: u64,
    pub counts_by_status: BTreeMap<String, usize>,
    pub counts_by_kind: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedEvidenceText {
    pub text: String,
    pub original_bytes: usize,
    pub truncated: bool,
    pub redacted: bool,
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("invalid evidence field {field}: {reason}")]
    InvalidField { field: String, reason: String },
    #[error("corrupt evidence ledger at line {line}: {reason}")]
    CorruptLedger { line: usize, reason: String },
    #[error("duplicate evidence event_id: {0}")]
    DuplicateEventId(String),
    #[error("verification packet already exists: {0}")]
    DuplicateVerificationPacket(String),
    #[error("review packet already exists: {0}")]
    DuplicateReviewPacket(String),
    #[error("evidence sequence conflict: expected {expected}, found {actual}")]
    SequenceConflict { expected: u64, actual: u64 },
    #[error("incompatible evidence schema: {0}")]
    SchemaIncompatible(String),
    #[error("missing evidence reference: {0}")]
    MissingEvidenceReference(String),
    #[error("failed to serialize evidence: {0}")]
    Json(#[from] serde_json::Error),
    #[error("evidence I/O failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("evidence workflow failure: {0}")]
    Workflow(#[from] WorkflowError),
}

pub type EvidenceResult<T> = Result<T, EvidenceError>;

pub fn ensure_evidence_artifacts(artifact_dir: impl AsRef<Path>) -> EvidenceResult<()> {
    let artifact_dir = artifact_dir.as_ref();
    let evidence_dir = artifact_dir.join("evidence");
    fs::create_dir_all(&evidence_dir)?;
    fs::create_dir_all(artifact_dir.join("verification"))?;
    fs::create_dir_all(artifact_dir.join("review"))?;
    Ok(())
}

pub fn append_evidence_event(
    artifact_dir: impl AsRef<Path>,
    draft: EvidenceEventDraft,
) -> EvidenceResult<EvidenceEvent> {
    let artifact_dir = artifact_dir.as_ref();
    ensure_evidence_artifacts(artifact_dir)?;
    validate_draft(&draft)?;
    let explicit_event_id = draft.event_id.clone();
    let workflow_event = if let Some(event_id) = explicit_event_id.as_deref() {
        append_workflow_event_with_unique_data_value(
            artifact_dir,
            WorkflowEventKind::EvidenceRecorded,
            "verify",
            "event_id",
            event_id,
            move |sequence, workflow_at_ms| evidence_event_value(draft, sequence, workflow_at_ms),
        )
    } else {
        append_workflow_event_with_data(
            artifact_dir,
            WorkflowEventKind::EvidenceRecorded,
            "verify",
            move |sequence, workflow_at_ms| evidence_event_value(draft, sequence, workflow_at_ms),
        )
    }
    .map_err(|error| match error {
        WorkflowError::DuplicateEventData { field, value } if field == "event_id" => {
            EvidenceError::DuplicateEventId(value)
        }
        other => EvidenceError::Workflow(other),
    })?;
    let event: EvidenceEvent = serde_json::from_value(workflow_event.data)?;
    validate_event(&event)?;
    Ok(event)
}

fn evidence_event_value(draft: EvidenceEventDraft, sequence: u64, workflow_at_ms: u64) -> Value {
    let recorded_at_ms = draft.recorded_at_ms.unwrap_or(workflow_at_ms);
    let event_id = draft
        .event_id
        .unwrap_or_else(|| format!("evt_{recorded_at_ms}_{sequence:04}"));
    serde_json::to_value(EvidenceEvent {
        schema: EVIDENCE_EVENT_SCHEMA.to_string(),
        sequence,
        event_id,
        workflow_id: draft.workflow_id,
        run_id: draft.run_id,
        task_id: draft.task_id,
        workpacket_id: draft.workpacket_id,
        recorded_at_ms,
        kind: draft.kind,
        status: draft.status,
        summary: draft.summary,
        source: draft.source,
        payload: draft.payload,
        changed_files: draft.changed_files,
        confidence: draft.confidence,
        severity: draft.severity,
        next_action: draft.next_action,
        supersedes_event_id: draft.supersedes_event_id,
    })
    .expect("EvidenceEvent serialization cannot fail")
}

pub fn read_evidence_events(artifact_dir: impl AsRef<Path>) -> EvidenceResult<Vec<EvidenceEvent>> {
    let artifact_dir = artifact_dir.as_ref();
    ensure_evidence_artifacts(artifact_dir)?;
    let mut events = Vec::new();
    let mut ids = BTreeSet::new();
    for workflow_event in read_workflow_events(artifact_dir)?
        .into_iter()
        .filter(|event| event.kind == WorkflowEventKind::EvidenceRecorded)
    {
        let event: EvidenceEvent =
            serde_json::from_value(workflow_event.data).map_err(|error| {
                EvidenceError::CorruptLedger {
                    line: workflow_event.seq as usize,
                    reason: error.to_string(),
                }
            })?;
        if event.schema != EVIDENCE_EVENT_SCHEMA {
            return Err(EvidenceError::SchemaIncompatible(event.schema));
        }
        if event.sequence != workflow_event.seq {
            return Err(EvidenceError::SequenceConflict {
                expected: workflow_event.seq,
                actual: event.sequence,
            });
        }
        if !ids.insert(event.event_id.clone()) {
            return Err(EvidenceError::DuplicateEventId(event.event_id));
        }
        validate_event(&event).map_err(|error| EvidenceError::CorruptLedger {
            line: workflow_event.seq as usize,
            reason: error.to_string(),
        })?;
        events.push(event);
    }
    Ok(events)
}

pub fn build_verification_packet(
    workflow_id: impl Into<String>,
    run_id: impl Into<String>,
    task_id: Option<String>,
    profile: impl Into<String>,
    mut checks: Vec<VerificationCheck>,
    events: &[EvidenceEvent],
) -> EvidenceResult<VerificationPacket> {
    let workflow_id = workflow_id.into();
    let run_id = run_id.into();
    let profile = profile.into();
    require_non_empty("workflow_id", &workflow_id)?;
    require_non_empty("run_id", &run_id)?;
    require_non_empty("profile", &profile)?;

    let event_by_id = events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    let mut check_ids = BTreeSet::new();
    for check in &mut checks {
        require_non_empty("check_id", &check.check_id)?;
        require_non_empty("description", &check.description)?;
        if !check_ids.insert(check.check_id.clone()) {
            return Err(invalid("check_id", "duplicate verification check"));
        }
        if check.status == EvidenceStatus::Skipped
            && check
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .is_none()
        {
            return Err(invalid(
                "reason",
                "skipped verification check requires reason",
            ));
        }

        let Some(evidence_id) = check.evidence_id.as_deref() else {
            if matches!(
                check.status,
                EvidenceStatus::Skipped | EvidenceStatus::Unknown
            ) {
                continue;
            }
            check.status = EvidenceStatus::Blocked;
            check
                .reason
                .get_or_insert_with(|| "missing evidence reference".to_string());
            continue;
        };
        let Some(event) = event_by_id.get(evidence_id) else {
            check.status = EvidenceStatus::Blocked;
            check
                .reason
                .get_or_insert_with(|| format!("missing evidence event {evidence_id}"));
            continue;
        };
        if event.workflow_id != workflow_id || event.run_id != run_id {
            check.status = EvidenceStatus::Blocked;
            check
                .reason
                .get_or_insert_with(|| "evidence belongs to a different workflow run".to_string());
            continue;
        }
        if event.status != check.status {
            check.status = EvidenceStatus::Blocked;
            check.reason.get_or_insert_with(|| {
                format!(
                    "verification status does not match evidence status {:?}",
                    event.status
                )
            });
        }
    }

    let pass_count = count_status(&checks, EvidenceStatus::Pass);
    let fail_count = count_status(&checks, EvidenceStatus::Fail);
    let blocked_count = count_status(&checks, EvidenceStatus::Blocked);
    let skipped_count = count_status(&checks, EvidenceStatus::Skipped);
    let unknown_count = count_status(&checks, EvidenceStatus::Unknown);
    let required = checks
        .iter()
        .filter(|check| check.required)
        .collect::<Vec<_>>();
    let (final_status, next_action) = if required.is_empty() {
        (VerificationStatus::Blocked, "add_required_checks")
    } else if required
        .iter()
        .any(|check| check.status == EvidenceStatus::Fail)
    {
        (VerificationStatus::Fail, "fix_failed_checks")
    } else if required
        .iter()
        .any(|check| check.status == EvidenceStatus::Blocked)
    {
        (VerificationStatus::Blocked, "resolve_blockers")
    } else if required.iter().any(|check| {
        matches!(
            check.status,
            EvidenceStatus::Skipped | EvidenceStatus::Unknown
        )
    }) {
        (
            VerificationStatus::Inconclusive,
            "complete_or_explain_skipped_checks",
        )
    } else {
        (VerificationStatus::Pass, "review")
    };
    let evidence_ids = checks
        .iter()
        .filter_map(|check| check.evidence_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let created_at_ms = now_ms();
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    Ok(VerificationPacket {
        schema: VERIFICATION_PACKET_SCHEMA.to_string(),
        verification_id: format!("vp_{created_at_ms}_{}", suffix.to_ascii_lowercase()),
        workflow_id,
        run_id,
        task_id,
        profile,
        created_at_ms,
        checks,
        pass_count,
        fail_count,
        blocked_count,
        skipped_count,
        unknown_count,
        final_status,
        evidence_ids,
        next_action: next_action.to_string(),
    })
}

pub fn validate_verification_packet_integrity(
    packet: &VerificationPacket,
    events: &[EvidenceEvent],
) -> EvidenceResult<()> {
    if packet.schema != VERIFICATION_PACKET_SCHEMA {
        return Err(EvidenceError::SchemaIncompatible(packet.schema.clone()));
    }
    validate_artifact_id("verification_id", &packet.verification_id)?;
    require_non_empty("workflow_id", &packet.workflow_id)?;
    require_non_empty("run_id", &packet.run_id)?;
    require_non_empty("profile", &packet.profile)?;

    let event_by_id = events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    let mut check_ids = BTreeSet::new();
    let mut referenced_evidence_ids = BTreeSet::new();
    for check in &packet.checks {
        require_non_empty("check_id", &check.check_id)?;
        require_non_empty("description", &check.description)?;
        if !check_ids.insert(check.check_id.clone()) {
            return Err(invalid("check_id", "duplicate verification check"));
        }
        if check.status == EvidenceStatus::Skipped
            && check
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .is_none()
        {
            return Err(invalid(
                "reason",
                "skipped verification check requires reason",
            ));
        }
        let Some(evidence_id) = check.evidence_id.as_deref() else {
            if matches!(
                check.status,
                EvidenceStatus::Skipped | EvidenceStatus::Unknown
            ) {
                continue;
            }
            return Err(invalid(
                "evidence_id",
                &format!("check {} is missing evidence reference", check.check_id),
            ));
        };
        let event = event_by_id.get(evidence_id).ok_or_else(|| {
            invalid(
                "evidence_id",
                &format!(
                    "check {} references missing evidence {evidence_id}",
                    check.check_id
                ),
            )
        })?;
        if event.workflow_id != packet.workflow_id || event.run_id != packet.run_id {
            return Err(invalid(
                "evidence_id",
                &format!("evidence {evidence_id} belongs to a different workflow run"),
            ));
        }
        if let Some(task_id) = packet.task_id.as_deref() {
            if event.task_id.as_deref() != Some(task_id) {
                return Err(invalid(
                    "task_id",
                    &format!("evidence {evidence_id} belongs to a different task"),
                ));
            }
        }
        if event.status != check.status {
            return Err(invalid(
                "status",
                &format!(
                    "check {} status {:?} does not match evidence {:?}",
                    check.check_id, check.status, event.status
                ),
            ));
        }
        referenced_evidence_ids.insert(evidence_id.to_string());
    }

    let required = packet
        .checks
        .iter()
        .filter(|check| check.required)
        .collect::<Vec<_>>();
    if required.is_empty() {
        return Err(invalid(
            "checks",
            "verification packet requires at least one required check",
        ));
    }
    let expected_counts = [
        ("pass_count", packet.pass_count, EvidenceStatus::Pass),
        ("fail_count", packet.fail_count, EvidenceStatus::Fail),
        (
            "blocked_count",
            packet.blocked_count,
            EvidenceStatus::Blocked,
        ),
        (
            "skipped_count",
            packet.skipped_count,
            EvidenceStatus::Skipped,
        ),
        (
            "unknown_count",
            packet.unknown_count,
            EvidenceStatus::Unknown,
        ),
    ];
    for (field, actual, status) in expected_counts {
        let expected = count_status(&packet.checks, status);
        if actual != expected {
            return Err(invalid(
                field,
                &format!("expected {expected}, found {actual}"),
            ));
        }
    }

    let (expected_status, expected_next_action) = if required
        .iter()
        .any(|check| check.status == EvidenceStatus::Fail)
    {
        (VerificationStatus::Fail, "fix_failed_checks")
    } else if required
        .iter()
        .any(|check| check.status == EvidenceStatus::Blocked)
    {
        (VerificationStatus::Blocked, "resolve_blockers")
    } else if required.iter().any(|check| {
        matches!(
            check.status,
            EvidenceStatus::Skipped | EvidenceStatus::Unknown
        )
    }) {
        (
            VerificationStatus::Inconclusive,
            "complete_or_explain_skipped_checks",
        )
    } else {
        (VerificationStatus::Pass, "review")
    };
    if packet.final_status != expected_status {
        return Err(invalid(
            "final_status",
            &format!(
                "expected {:?}, found {:?}",
                expected_status, packet.final_status
            ),
        ));
    }
    if packet.next_action != expected_next_action {
        return Err(invalid(
            "next_action",
            &format!(
                "expected {expected_next_action}, found {}",
                packet.next_action
            ),
        ));
    }
    let packet_evidence_ids = packet.evidence_ids.iter().cloned().collect::<BTreeSet<_>>();
    if packet_evidence_ids != referenced_evidence_ids
        || packet.evidence_ids.len() != packet_evidence_ids.len()
    {
        return Err(invalid(
            "evidence_ids",
            "must exactly match unique check evidence references",
        ));
    }
    Ok(())
}

pub fn validate_verification_packet_completion(
    artifact_dir: impl AsRef<Path>,
    packet_path: impl AsRef<Path>,
    packet: &VerificationPacket,
) -> EvidenceResult<()> {
    let artifact_dir = artifact_dir.as_ref();
    let packet_path = fs::canonicalize(packet_path.as_ref())?;
    let expected_status = match packet.final_status {
        VerificationStatus::Pass => "pass",
        VerificationStatus::Fail => "fail",
        VerificationStatus::Blocked => "blocked",
        VerificationStatus::Inconclusive => "inconclusive",
    };
    let expected_evidence_ids = serde_json::to_value(&packet.evidence_ids)?;
    let events = read_workflow_events(artifact_dir)?;
    let completed = events.iter().any(|event| {
        if event.kind != WorkflowEventKind::VerificationCompleted {
            return false;
        }
        let Some(recorded_path) = event.data.get("packet_path").and_then(Value::as_str) else {
            return false;
        };
        let recorded_path = PathBuf::from(recorded_path);
        let recorded_path = fs::canonicalize(&recorded_path).unwrap_or(recorded_path);
        event.data.get("verification_id").and_then(Value::as_str)
            == Some(packet.verification_id.as_str())
            && event.data.get("workflow_id").and_then(Value::as_str)
                == Some(packet.workflow_id.as_str())
            && event.data.get("run_id").and_then(Value::as_str) == Some(packet.run_id.as_str())
            && event.data.get("task_id").and_then(Value::as_str) == packet.task_id.as_deref()
            && event.data.get("profile").and_then(Value::as_str) == Some(packet.profile.as_str())
            && event.data.get("final_status").and_then(Value::as_str) == Some(expected_status)
            && event.data.get("evidence_ids") == Some(&expected_evidence_ids)
            && recorded_path == packet_path
    });
    if !completed {
        return Err(invalid(
            "verification_completion",
            "packet has no matching VerificationCompleted workflow event",
        ));
    }
    Ok(())
}

pub fn write_verification_packet(
    artifact_dir: impl AsRef<Path>,
    packet: &VerificationPacket,
) -> EvidenceResult<PathBuf> {
    let artifact_dir = artifact_dir.as_ref();
    ensure_evidence_artifacts(artifact_dir)?;
    if packet.schema != VERIFICATION_PACKET_SCHEMA {
        return Err(EvidenceError::SchemaIncompatible(packet.schema.clone()));
    }
    validate_artifact_id("verification_id", &packet.verification_id)?;
    let verification_dir = artifact_dir.join("verification");
    let path = verification_dir.join(format!("{}.json", packet.verification_id));
    if path.exists() {
        return Err(EvidenceError::DuplicateVerificationPacket(
            packet.verification_id.clone(),
        ));
    }
    let relative_path = format!("verification/{}.json", packet.verification_id);
    let mut bytes = serde_json::to_vec_pretty(packet)?;
    bytes.push(b'\n');
    let commit = commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::VerificationCompleted,
        "verify",
        "verification_id",
        packet.verification_id.clone(),
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path,
                    contents: bytes,
                }],
                event_data: serde_json::json!({
                    "verification_id": packet.verification_id,
                    "workflow_id": packet.workflow_id,
                    "run_id": packet.run_id,
                    "task_id": packet.task_id,
                    "profile": packet.profile,
                    "final_status": packet.final_status,
                    "packet_path": path,
                    "evidence_ids": packet.evidence_ids,
                }),
            })
        },
    )?;
    if commit.reused_event {
        return Err(EvidenceError::DuplicateVerificationPacket(
            packet.verification_id.clone(),
        ));
    }
    Ok(path)
}

pub fn read_verification_packet(path: impl AsRef<Path>) -> EvidenceResult<VerificationPacket> {
    let contents = fs::read_to_string(path)?;
    let packet: VerificationPacket = serde_json::from_str(&contents)?;
    if packet.schema != VERIFICATION_PACKET_SCHEMA {
        return Err(EvidenceError::SchemaIncompatible(packet.schema));
    }
    Ok(packet)
}

pub fn list_verification_packets(
    artifact_dir: impl AsRef<Path>,
) -> EvidenceResult<Vec<(PathBuf, VerificationPacket)>> {
    let artifact_dir = artifact_dir.as_ref();
    ensure_evidence_artifacts(artifact_dir)?;
    let mut packets = Vec::new();
    for entry in fs::read_dir(artifact_dir.join("verification"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        packets.push((path.clone(), read_verification_packet(path)?));
    }
    packets.sort_by(|left, right| {
        left.1
            .created_at_ms
            .cmp(&right.1.created_at_ms)
            .then_with(|| left.1.verification_id.cmp(&right.1.verification_id))
    });
    Ok(packets)
}

pub fn write_review_packet(
    artifact_dir: impl AsRef<Path>,
    packet: &ReviewPacket,
) -> EvidenceResult<PathBuf> {
    let artifact_dir = artifact_dir.as_ref();
    ensure_evidence_artifacts(artifact_dir)?;
    validate_review_packet(packet)?;
    let events = read_evidence_events(artifact_dir)?;
    validate_review_packet_evidence_refs(packet, &events)?;
    let review_dir = artifact_dir.join("review");
    let path = review_dir.join(format!("{}.json", packet.id));
    let relative_path = format!("review/{}.json", packet.id);
    let conflict_path = relative_path.clone();
    let mut bytes = serde_json::to_vec_pretty(packet)?;
    bytes.push(b'\n');
    let commit = commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::ReviewCompleted,
        "review",
        "review_id",
        packet.id.clone(),
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path,
                    contents: bytes,
                }],
                event_data: serde_json::json!({
                    "review_id": packet.id,
                    "workflow_id": packet.workflow_id,
                    "run_id": packet.run_id,
                    "mode": packet.mode,
                    "decision": packet.decision,
                    "blocking_count": packet.blocking_count,
                    "packet_path": path,
                    "evidence_refs": packet.evidence_refs,
                }),
            })
        },
    )
    .map_err(|error| match error {
        WorkflowError::ArtifactConflict { path } if path == conflict_path => {
            EvidenceError::DuplicateReviewPacket(packet.id.clone())
        }
        other => EvidenceError::Workflow(other),
    })?;
    if commit.reused_event {
        return Err(EvidenceError::DuplicateReviewPacket(packet.id.clone()));
    }
    Ok(path)
}

pub fn read_review_packet(path: impl AsRef<Path>) -> EvidenceResult<ReviewPacket> {
    let contents = fs::read_to_string(path)?;
    let packet: ReviewPacket = serde_json::from_str(&contents)?;
    validate_review_packet(&packet)?;
    Ok(packet)
}

pub fn list_review_packets(
    artifact_dir: impl AsRef<Path>,
) -> EvidenceResult<Vec<(PathBuf, ReviewPacket)>> {
    let artifact_dir = artifact_dir.as_ref();
    ensure_evidence_artifacts(artifact_dir)?;
    let mut packets = Vec::new();
    for entry in fs::read_dir(artifact_dir.join("review"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        packets.push((path.clone(), read_review_packet(path)?));
    }
    packets.sort_by(|left, right| {
        left.1
            .created_at_ms
            .cmp(&right.1.created_at_ms)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    Ok(packets)
}

fn validate_review_packet(packet: &ReviewPacket) -> EvidenceResult<()> {
    if packet.schema != REVIEW_PACKET_SCHEMA {
        return Err(EvidenceError::SchemaIncompatible(packet.schema.clone()));
    }
    validate_artifact_id("review_id", &packet.id)?;
    require_non_empty("workflow_id", &packet.workflow_id)?;
    require_non_empty("run_id", &packet.run_id)?;
    require_non_empty("mode", &packet.mode)?;
    require_non_empty("reviewer_type", &packet.reviewer_type)?;
    require_non_empty("decision", &packet.decision)?;
    require_non_empty("next_action", &packet.next_action)?;
    if !(0.0..=100.0).contains(&packet.score) {
        return Err(invalid("score", "must be between 0 and 100"));
    }
    let expected_blocking = packet
        .findings
        .iter()
        .filter(|finding| finding.severity == ReviewSeverity::Block)
        .count();
    if packet.blocking_count != expected_blocking {
        return Err(invalid(
            "blocking_count",
            &format!(
                "expected {expected_blocking}, found {}",
                packet.blocking_count
            ),
        ));
    }
    if packet.evidence_refs.len() != packet.findings.len() {
        return Err(invalid(
            "evidence_refs",
            "must contain exactly one evidence event id per finding",
        ));
    }
    let unique_refs = packet
        .evidence_refs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if unique_refs.len() != packet.evidence_refs.len() {
        return Err(invalid("evidence_refs", "must not contain duplicates"));
    }
    for evidence_ref in &packet.evidence_refs {
        require_non_empty("evidence_ref", evidence_ref)?;
    }
    for finding in &packet.findings {
        if !(0.0..=1.0).contains(&finding.confidence) {
            return Err(invalid("finding.confidence", "must be between 0 and 1"));
        }
        require_non_empty("finding.title", &finding.title)?;
        require_non_empty("finding.recommendation", &finding.recommendation)?;
    }
    Ok(())
}

fn validate_review_packet_evidence_refs(
    packet: &ReviewPacket,
    events: &[EvidenceEvent],
) -> EvidenceResult<()> {
    let event_by_id = events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    for evidence_ref in &packet.evidence_refs {
        let event = event_by_id
            .get(evidence_ref.as_str())
            .ok_or_else(|| EvidenceError::MissingEvidenceReference(evidence_ref.clone()))?;
        if event.workflow_id != packet.workflow_id || event.run_id != packet.run_id {
            return Err(invalid(
                "evidence_refs",
                &format!("evidence {evidence_ref} belongs to a different workflow run"),
            ));
        }
    }
    Ok(())
}

pub fn summarize_evidence(events: &[EvidenceEvent]) -> LedgerSummary {
    let mut counts_by_status = BTreeMap::new();
    let mut counts_by_kind = BTreeMap::new();
    for event in events {
        *counts_by_status
            .entry(format!("{:?}", event.status).to_ascii_lowercase())
            .or_insert(0) += 1;
        *counts_by_kind
            .entry(format!("{:?}", event.kind).to_ascii_lowercase())
            .or_insert(0) += 1;
    }
    LedgerSummary {
        event_count: events.len(),
        latest_sequence: events.last().map(|event| event.sequence).unwrap_or(0),
        counts_by_status,
        counts_by_kind,
    }
}

pub fn unresolved_blocking_evidence(
    events: &[EvidenceEvent],
    workflow_id: &str,
    run_id: &str,
) -> Vec<EvidenceEvent> {
    let mut ordered = events
        .iter()
        .filter(|event| event.workflow_id == workflow_id && event.run_id == run_id)
        .collect::<Vec<_>>();
    ordered.sort_by_key(|event| event.sequence);

    let mut active = BTreeMap::<String, EvidenceEvent>::new();
    for event in ordered {
        if let Some(superseded) = event.supersedes_event_id.as_deref() {
            active.remove(superseded);
        }
        if is_blocking_evidence(event) {
            active.insert(event.event_id.clone(), event.clone());
        }
    }
    let mut unresolved = active.into_values().collect::<Vec<_>>();
    unresolved.sort_by_key(|event| event.sequence);
    unresolved
}

fn is_blocking_evidence(event: &EvidenceEvent) -> bool {
    match event.kind {
        EvidenceKind::BlockerRecord => {
            !matches!(event.status, EvidenceStatus::Pass | EvidenceStatus::Skipped)
        }
        EvidenceKind::ReviewFinding => {
            matches!(event.status, EvidenceStatus::Fail | EvidenceStatus::Blocked)
                || event
                    .severity
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_ascii_lowercase)
                    .is_some_and(|severity| {
                        matches!(
                            severity.as_str(),
                            "block" | "blocking" | "blocker" | "critical"
                        )
                    })
        }
        _ => false,
    }
}

pub fn redact_and_bound_evidence_text(input: &str, max_bytes: usize) -> BoundedEvidenceText {
    let original_bytes = input.len();
    let mut redacted = false;
    let mut in_private_key = false;
    let mut output = String::new();
    for line in input.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("-----begin ") && lower.contains("private key-----") {
            in_private_key = true;
            redacted = true;
            output.push_str("[REDACTED PRIVATE KEY]\n");
            continue;
        }
        if in_private_key {
            redacted = true;
            if lower.starts_with("-----end ") && lower.contains("private key-----") {
                in_private_key = false;
            }
            continue;
        }
        if let Some(index) = lower.find("authorization: bearer ") {
            redacted = true;
            output.push_str(&line[..index + "Authorization: Bearer ".len()]);
            output.push_str("[REDACTED]\n");
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            let key_lower = key.trim().to_ascii_lowercase();
            if ["token", "password", "secret", "api_key", "apikey"]
                .iter()
                .any(|needle| key_lower.contains(needle))
            {
                redacted = true;
                output.push_str(key);
                output.push_str("=[REDACTED]\n");
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }

    let truncated = output.len() > max_bytes;
    let bounded = if truncated {
        let mut start = output.len().saturating_sub(max_bytes);
        while start < output.len() && !output.is_char_boundary(start) {
            start += 1;
        }
        output[start..].to_string()
    } else {
        output
    };
    BoundedEvidenceText {
        text: bounded,
        original_bytes,
        truncated,
        redacted,
    }
}

fn validate_draft(draft: &EvidenceEventDraft) -> EvidenceResult<()> {
    require_non_empty("workflow_id", &draft.workflow_id)?;
    require_non_empty("run_id", &draft.run_id)?;
    require_non_empty("summary", &draft.summary)?;
    require_non_empty("source.type", &draft.source.source_type)?;
    require_non_empty("source.name", &draft.source.name)?;
    if let Some(confidence) = draft.confidence {
        if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Err(invalid("confidence", "must be finite and between 0 and 1"));
        }
    }
    validate_status_payload(draft.kind, draft.status, &draft.payload)
}

fn validate_event(event: &EvidenceEvent) -> EvidenceResult<()> {
    if event.schema != EVIDENCE_EVENT_SCHEMA {
        return Err(EvidenceError::SchemaIncompatible(event.schema.clone()));
    }
    let draft = EvidenceEventDraft {
        event_id: Some(event.event_id.clone()),
        workflow_id: event.workflow_id.clone(),
        run_id: event.run_id.clone(),
        task_id: event.task_id.clone(),
        workpacket_id: event.workpacket_id.clone(),
        recorded_at_ms: Some(event.recorded_at_ms),
        kind: event.kind,
        status: event.status,
        summary: event.summary.clone(),
        source: event.source.clone(),
        payload: event.payload.clone(),
        changed_files: event.changed_files.clone(),
        confidence: event.confidence,
        severity: event.severity.clone(),
        next_action: event.next_action.clone(),
        supersedes_event_id: event.supersedes_event_id.clone(),
    };
    validate_draft(&draft)
}

fn validate_status_payload(
    kind: EvidenceKind,
    status: EvidenceStatus,
    payload: &Value,
) -> EvidenceResult<()> {
    if status == EvidenceStatus::Skipped {
        let reason = payload
            .get("reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|reason| !reason.is_empty());
        if reason.is_none() {
            return Err(invalid(
                "payload.reason",
                "skipped evidence requires reason",
            ));
        }
    }
    if kind.is_command_like() {
        let exit_code = payload.get("exit_code").and_then(Value::as_i64);
        if status == EvidenceStatus::Pass && exit_code != Some(0) {
            return Err(invalid(
                "payload.exit_code",
                "passing command evidence requires exit_code 0",
            ));
        }
        if status == EvidenceStatus::Fail
            && exit_code == Some(0)
            && payload.get("semantic_failure").and_then(Value::as_bool) != Some(true)
        {
            return Err(invalid(
                "payload.exit_code",
                "failed command evidence with exit_code 0 requires semantic_failure",
            ));
        }
    }
    Ok(())
}

fn require_non_empty(field: &str, value: &str) -> EvidenceResult<()> {
    if value.trim().is_empty() {
        Err(invalid(field, "must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_artifact_id(field: &str, value: &str) -> EvidenceResult<()> {
    require_non_empty(field, value)?;
    if value.len() > 128 {
        return Err(invalid(field, "must be at most 128 ASCII characters"));
    }
    if !value
        .bytes()
        .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
    {
        return Err(invalid(
            field,
            "must contain only ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

fn invalid(field: &str, reason: &str) -> EvidenceError {
    EvidenceError::InvalidField {
        field: field.to_string(),
        reason: reason.to_string(),
    }
}

fn count_status(checks: &[VerificationCheck], status: EvidenceStatus) -> usize {
    checks.iter().filter(|check| check.status == status).count()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
