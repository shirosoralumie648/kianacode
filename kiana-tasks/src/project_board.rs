use crate::{
    inspect_workflow_integrity, read_evidence_events, read_verification_packet,
    validate_verification_packet_completion, validate_verification_packet_integrity,
    VerificationStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path};
use thiserror::Error;

const TASK_CARD_SCHEMA: &str = "kiana.project-task-card.v1";
const BOARD_SCHEMA: &str = "kiana.project-board.v1";
const NEXT_SCHEMA: &str = "kiana.project-next.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBoardStatus {
    Backlog,
    Spec,
    Ready,
    InProgress,
    Review,
    Blocked,
    Done,
    Archived,
}

impl ProjectBoardStatus {
    fn all() -> [Self; 8] {
        [
            Self::Backlog,
            Self::Spec,
            Self::Ready,
            Self::InProgress,
            Self::Review,
            Self::Blocked,
            Self::Done,
            Self::Archived,
        ]
    }

    fn key(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Spec => "spec",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Review => "review",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }

    pub fn all_for_display() -> [Self; 8] {
        Self::all()
    }

    pub fn display_key(self) -> &'static str {
        self.key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPolicyFinding {
    pub code: String,
    pub severity: String,
    pub task_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTaskCard {
    pub schema: String,
    pub task_id: String,
    pub title: String,
    pub source_status: String,
    pub board_status: ProjectBoardStatus,
    pub owner: Option<String>,
    pub workstream_id: Option<String>,
    pub milestone_id: Option<String>,
    pub priority: i64,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
    pub verification_commands: Vec<String>,
    pub evidence: Vec<String>,
    pub blocker_reason: Option<String>,
    pub unblock_condition: Option<String>,
    pub allowed_paths: Vec<String>,
    pub task_type: Option<String>,
    pub risk_level: Option<String>,
    pub approval_required: bool,
    pub approval_status: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub policy_findings: Vec<ProjectPolicyFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBoardColumn {
    pub status: ProjectBoardStatus,
    pub tasks: Vec<ProjectTaskCard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBoardProjection {
    pub schema: String,
    pub task_list_id: String,
    #[serde(skip)]
    pub project_root: Option<String>,
    pub columns: Vec<ProjectBoardColumn>,
    pub counts: BTreeMap<String, usize>,
    pub policy_findings: Vec<ProjectPolicyFinding>,
    pub ready_task_ids: Vec<String>,
    pub blocked_task_ids: Vec<String>,
    pub done_task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectNextAlternative {
    pub task_id: String,
    pub why_not: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBlockingReasonSummary {
    pub code: String,
    pub severity: String,
    pub count: usize,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectNextTaskReport {
    pub schema: String,
    pub task_list_id: String,
    pub mode: String,
    pub selected_task: Option<ProjectTaskCard>,
    pub why: Vec<String>,
    pub alternatives: Vec<ProjectNextAlternative>,
    pub blocked_summary: BTreeMap<String, usize>,
    pub primary_blockers: Vec<ProjectBlockingReasonSummary>,
}

#[derive(Debug, Error)]
pub enum ProjectBoardError {
    #[error("task at index {index} is missing required field {field}")]
    MissingField { index: usize, field: &'static str },
    #[error("task {task_id} has invalid field {field}: {reason}")]
    InvalidField {
        task_id: String,
        field: &'static str,
        reason: String,
    },
    #[error("duplicate task id {0}")]
    DuplicateTaskId(String),
}

pub type ProjectBoardResult<T> = Result<T, ProjectBoardError>;

#[derive(Debug, Clone)]
struct RawTask {
    task_id: String,
    title: String,
    source_status: String,
    owner: Option<String>,
    workstream_id: Option<String>,
    milestone_id: Option<String>,
    priority: i64,
    blocks: Vec<String>,
    blocked_by: Vec<String>,
    verification_commands: Vec<String>,
    evidence: Vec<String>,
    blocker_reason: Option<String>,
    unblock_condition: Option<String>,
    allowed_paths: Vec<String>,
    task_type: Option<String>,
    risk_level: Option<String>,
    approval_required: bool,
    approval_status: Option<String>,
    board_override: Option<ProjectBoardStatus>,
    created_at: i64,
    updated_at: i64,
}

pub fn build_project_board(
    task_list_id: impl Into<String>,
    tasks: &[Value],
) -> ProjectBoardResult<ProjectBoardProjection> {
    build_project_board_internal(None, task_list_id.into(), tasks)
}

pub fn build_project_board_at_root(
    project_root: impl AsRef<Path>,
    task_list_id: impl Into<String>,
    tasks: &[Value],
) -> ProjectBoardResult<ProjectBoardProjection> {
    build_project_board_internal(Some(project_root.as_ref()), task_list_id.into(), tasks)
}

fn build_project_board_internal(
    project_root: Option<&Path>,
    task_list_id: String,
    tasks: &[Value],
) -> ProjectBoardResult<ProjectBoardProjection> {
    let raw_tasks = parse_tasks(tasks)?;
    let task_by_id = raw_tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task))
        .collect::<HashMap<_, _>>();
    let completion_gates = raw_tasks
        .iter()
        .filter(|task| task.source_status == "completed")
        .map(|task| {
            (
                task.task_id.as_str(),
                evaluate_completion_gate(project_root, task),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut cards = Vec::with_capacity(raw_tasks.len());
    let mut board_findings = Vec::new();

    for raw in &raw_tasks {
        let completion_gate = completion_gates.get(raw.task_id.as_str());
        let mut findings = completion_gate
            .map(|gate| gate.findings.clone())
            .unwrap_or_default();
        let mut unresolved_dependencies = Vec::new();
        for dependency_id in &raw.blocked_by {
            match task_by_id.get(dependency_id.as_str()) {
                Some(dependency)
                    if dependency.source_status == "completed"
                        && completion_gates
                            .get(dependency.task_id.as_str())
                            .is_some_and(|gate| gate.passed) => {}
                Some(_) => unresolved_dependencies.push(dependency_id.clone()),
                None => {
                    unresolved_dependencies.push(dependency_id.clone());
                    findings.push(finding(
                        raw,
                        "missing_dependency_reference",
                        "blocker",
                        format!("dependency {dependency_id} does not exist"),
                    ));
                }
            }
        }

        let (board_status, blocker_reason, unblock_condition) = if let Some(gate) = completion_gate
        {
            if gate.passed {
                (ProjectBoardStatus::Done, None, None)
            } else {
                (
                    ProjectBoardStatus::Blocked,
                    Some(gate.blocker_reason.clone()),
                    Some(gate.unblock_condition.clone()),
                )
            }
        } else if !unresolved_dependencies.is_empty() {
            findings.push(finding(
                raw,
                "dependency_blocked",
                "blocker",
                format!(
                    "waiting for unresolved dependencies: {}",
                    unresolved_dependencies.join(", ")
                ),
            ));
            (
                ProjectBoardStatus::Blocked,
                raw.blocker_reason.clone().or_else(|| {
                    Some(format!(
                        "waiting for dependencies: {}",
                        unresolved_dependencies.join(", ")
                    ))
                }),
                raw.unblock_condition.clone().or_else(|| {
                    Some(format!(
                        "complete dependencies with evidence: {}",
                        unresolved_dependencies.join(", ")
                    ))
                }),
            )
        } else if raw.blocker_reason.is_some() {
            findings.push(finding(
                raw,
                "explicit_blocker",
                "blocker",
                raw.blocker_reason.clone().unwrap_or_default(),
            ));
            (
                ProjectBoardStatus::Blocked,
                raw.blocker_reason.clone(),
                raw.unblock_condition.clone().or_else(|| {
                    raw.blocker_reason
                        .as_ref()
                        .map(|reason| format!("resolve blocker: {reason}"))
                }),
            )
        } else if raw.source_status == "failed" {
            findings.push(finding(
                raw,
                "task_failed",
                "blocker",
                "task source status is failed".to_string(),
            ));
            (
                ProjectBoardStatus::Blocked,
                Some("task source status is failed".to_string()),
                raw.unblock_condition
                    .clone()
                    .or_else(|| Some("repair failure and rerun verification".to_string())),
            )
        } else if matches!(raw.source_status.as_str(), "cancelled" | "killed") {
            (ProjectBoardStatus::Archived, None, None)
        } else if raw.board_override == Some(ProjectBoardStatus::Review) {
            (ProjectBoardStatus::Review, None, None)
        } else if matches!(raw.source_status.as_str(), "in_progress" | "running") {
            (ProjectBoardStatus::InProgress, None, None)
        } else if raw.board_override == Some(ProjectBoardStatus::Spec) {
            (ProjectBoardStatus::Spec, None, None)
        } else if raw.board_override == Some(ProjectBoardStatus::Backlog) {
            (ProjectBoardStatus::Backlog, None, None)
        } else if !raw.verification_commands.is_empty() {
            (ProjectBoardStatus::Ready, None, None)
        } else {
            findings.push(finding(
                raw,
                "ready_missing_verification",
                "warning",
                "pending task cannot become ready without verification commands".to_string(),
            ));
            (ProjectBoardStatus::Backlog, None, None)
        };

        findings.sort_by(compare_findings);
        board_findings.extend(findings.clone());
        cards.push(ProjectTaskCard {
            schema: TASK_CARD_SCHEMA.to_string(),
            task_id: raw.task_id.clone(),
            title: raw.title.clone(),
            source_status: raw.source_status.clone(),
            board_status,
            owner: raw.owner.clone(),
            workstream_id: raw.workstream_id.clone(),
            milestone_id: raw.milestone_id.clone(),
            priority: raw.priority,
            blocks: raw.blocks.clone(),
            blocked_by: raw.blocked_by.clone(),
            verification_commands: raw.verification_commands.clone(),
            evidence: raw.evidence.clone(),
            blocker_reason,
            unblock_condition,
            allowed_paths: raw.allowed_paths.clone(),
            task_type: raw.task_type.clone(),
            risk_level: raw.risk_level.clone(),
            approval_required: raw.approval_required,
            approval_status: raw.approval_status.clone(),
            created_at: raw.created_at,
            updated_at: raw.updated_at,
            policy_findings: findings,
        });
    }

    board_findings.sort_by(compare_findings);

    let mut columns = Vec::new();
    let mut counts = BTreeMap::new();
    for status in ProjectBoardStatus::all() {
        let mut column_tasks = cards
            .iter()
            .filter(|card| card.board_status == status)
            .cloned()
            .collect::<Vec<_>>();
        column_tasks.sort_by(compare_cards);
        counts.insert(status.key().to_string(), column_tasks.len());
        columns.push(ProjectBoardColumn {
            status,
            tasks: column_tasks,
        });
    }

    Ok(ProjectBoardProjection {
        schema: BOARD_SCHEMA.to_string(),
        task_list_id,
        project_root: project_root.map(|root| {
            std::fs::canonicalize(root)
                .unwrap_or_else(|_| root.to_path_buf())
                .to_string_lossy()
                .to_string()
        }),
        ready_task_ids: task_ids_for_status(&columns, ProjectBoardStatus::Ready),
        blocked_task_ids: task_ids_for_status(&columns, ProjectBoardStatus::Blocked),
        done_task_ids: task_ids_for_status(&columns, ProjectBoardStatus::Done),
        columns,
        counts,
        policy_findings: board_findings,
    })
}

#[derive(Debug, Clone)]
struct CompletionGate {
    passed: bool,
    blocker_reason: String,
    unblock_condition: String,
    findings: Vec<ProjectPolicyFinding>,
}

fn evaluate_completion_gate(project_root: Option<&Path>, task: &RawTask) -> CompletionGate {
    if task.evidence.is_empty() {
        return failed_completion_gate(
            task,
            "completed_without_evidence",
            "completed task is missing evidence",
            "attach a passing VerificationPacket reference",
        );
    }

    let verification_references = task
        .evidence
        .iter()
        .filter(|reference| reference.starts_with("verification:"))
        .collect::<Vec<_>>();
    if verification_references.is_empty() {
        return failed_completion_gate(
            task,
            "legacy_evidence_requires_verification_packet",
            "completed task only has legacy evidence",
            "run kiana validate --task and attach its VerificationPacket reference",
        );
    }

    let Some(project_root) = project_root else {
        return failed_completion_gate(
            task,
            "verification_root_unavailable",
            "verification evidence cannot be resolved without a project root",
            "build the board with a project root and rerun verification",
        );
    };

    let mut last_failure = None;
    for reference in verification_references {
        match verify_reference(project_root, task, reference) {
            Ok(()) => {
                return CompletionGate {
                    passed: true,
                    blocker_reason: String::new(),
                    unblock_condition: String::new(),
                    findings: Vec::new(),
                };
            }
            Err((code, message, action)) => {
                last_failure = Some((code, message, action));
            }
        }
    }

    let (code, message, action) = last_failure.unwrap_or((
        "invalid_verification_reference",
        "completed task has no usable VerificationPacket reference".to_string(),
        "attach a valid verification:<path>#<id> reference".to_string(),
    ));
    failed_completion_gate(task, code, &message, &action)
}

fn verify_reference(
    project_root: &Path,
    task: &RawTask,
    reference: &str,
) -> Result<(), (&'static str, String, String)> {
    let target = reference
        .strip_prefix("verification:")
        .and_then(|value| value.rsplit_once('#'))
        .filter(|(path, verification_id)| !path.is_empty() && !verification_id.is_empty())
        .ok_or_else(|| {
            (
                "invalid_verification_reference",
                format!("invalid VerificationPacket reference: {reference}"),
                "use verification:<relative-packet-path>#<verification-id>".to_string(),
            )
        })?;
    let relative_path = Path::new(target.0);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err((
            "invalid_verification_reference",
            format!(
                "VerificationPacket path must stay inside the project: {}",
                target.0
            ),
            "attach a project-relative VerificationPacket path".to_string(),
        ));
    }

    let project_root = std::fs::canonicalize(project_root).map_err(|error| {
        (
            "verification_packet_unreadable",
            format!("cannot resolve project root: {error}"),
            "repair the project path and rerun verification".to_string(),
        )
    })?;
    let packet_path = project_root.join(relative_path);
    let canonical_packet_path = std::fs::canonicalize(&packet_path).map_err(|error| {
        (
            "verification_packet_unreadable",
            format!(
                "cannot read VerificationPacket {}: {error}",
                packet_path.display()
            ),
            "rerun kiana validate and attach the new packet reference".to_string(),
        )
    })?;
    if !canonical_packet_path.starts_with(&project_root) {
        return Err((
            "invalid_verification_reference",
            format!(
                "VerificationPacket path resolves outside the project: {}",
                packet_path.display()
            ),
            "attach a packet stored inside this project".to_string(),
        ));
    }
    let packet = read_verification_packet(&canonical_packet_path).map_err(|error| {
        (
            "verification_packet_unreadable",
            format!(
                "cannot read VerificationPacket {}: {error}",
                canonical_packet_path.display()
            ),
            "rerun kiana validate and attach the new packet reference".to_string(),
        )
    })?;
    if packet.verification_id != target.1 {
        return Err((
            "verification_id_mismatch",
            format!(
                "VerificationPacket id {} does not match reference {}",
                packet.verification_id, target.1
            ),
            "replace the evidence reference with the packet's exact id".to_string(),
        ));
    }
    if packet.task_id.as_deref() != Some(task.task_id.as_str()) {
        return Err((
            "verification_task_mismatch",
            format!(
                "VerificationPacket task {:?} does not match {}",
                packet.task_id, task.task_id
            ),
            "run kiana validate --task for this exact task".to_string(),
        ));
    }
    if packet.final_status != VerificationStatus::Pass {
        return Err((
            "verification_not_passing",
            format!(
                "VerificationPacket {} status is {:?}",
                packet.verification_id, packet.final_status
            ),
            "fix validation failures and produce a passing packet".to_string(),
        ));
    }
    let artifact_dir = canonical_packet_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            (
                "verification_packet_invalid",
                "VerificationPacket is not stored under a verification directory".to_string(),
                "rerun kiana validate to recreate the packet".to_string(),
            )
        })?;
    if artifact_dir.file_name().and_then(|value| value.to_str()) != Some(packet.run_id.as_str()) {
        return Err((
            "verification_packet_invalid",
            format!(
                "VerificationPacket run {} does not match artifact directory {}",
                packet.run_id,
                artifact_dir.display()
            ),
            "rerun kiana validate to recreate the packet".to_string(),
        ));
    }
    let events = read_evidence_events(artifact_dir).map_err(|error| {
        (
            "verification_packet_invalid",
            format!("cannot validate packet against EventLog: {error}"),
            "repair the WorkflowRun evidence ledger and rerun validation".to_string(),
        )
    })?;
    validate_verification_packet_integrity(&packet, &events).map_err(|error| {
        (
            "verification_packet_invalid",
            format!("VerificationPacket integrity failed: {error}"),
            "rerun kiana validate to produce a ledger-backed packet".to_string(),
        )
    })?;
    validate_verification_packet_completion(artifact_dir, &canonical_packet_path, &packet)
        .map_err(|error| {
            (
                "verification_completion_missing",
                format!("VerificationPacket completion event is missing: {error}"),
                "rerun kiana validate to publish a completed verification packet".to_string(),
            )
        })?;
    inspect_workflow_integrity(artifact_dir).map_err(|error| {
        (
            "verification_packet_integrity_invalid",
            format!("Workflow artifact integrity failed: {error}"),
            "restore or regenerate the authenticated VerificationPacket".to_string(),
        )
    })?;
    Ok(())
}

fn failed_completion_gate(
    task: &RawTask,
    code: &'static str,
    message: &str,
    action: &str,
) -> CompletionGate {
    CompletionGate {
        passed: false,
        blocker_reason: message.to_string(),
        unblock_condition: action.to_string(),
        findings: vec![finding(task, code, "blocker", message.to_string())],
    }
}

pub fn select_next_project_task(board: &ProjectBoardProjection) -> ProjectNextTaskReport {
    let mut candidates = board
        .columns
        .iter()
        .find(|column| column.status == ProjectBoardStatus::Ready)
        .map(|column| column.tasks.clone())
        .unwrap_or_default();
    candidates.sort_by(compare_cards);

    let selected_task = candidates.first().cloned();
    let why = selected_task
        .as_ref()
        .map(|task| {
            vec![
                "ready".to_string(),
                "verification commands available".to_string(),
                format!("priority={}", task.priority),
                format!("unblocks={}", task.blocks.len()),
                format!("updated_at={}", task.updated_at),
                format!("task_id={}", task.task_id),
            ]
        })
        .unwrap_or_else(|| vec!["no_ready_tasks".to_string()]);
    let alternatives = selected_task
        .as_ref()
        .map(|selected| {
            candidates
                .iter()
                .skip(1)
                .map(|task| ProjectNextAlternative {
                    task_id: task.task_id.clone(),
                    why_not: explain_lower_rank(selected, task),
                })
                .collect()
        })
        .unwrap_or_default();
    let blocked_summary = ["blocked", "backlog", "spec", "review"]
        .into_iter()
        .map(|key| (key.to_string(), board.counts.get(key).copied().unwrap_or(0)))
        .collect();
    let primary_blockers = summarize_blockers(&board.policy_findings);

    ProjectNextTaskReport {
        schema: NEXT_SCHEMA.to_string(),
        task_list_id: board.task_list_id.clone(),
        mode: "balanced".to_string(),
        selected_task,
        why,
        alternatives,
        blocked_summary,
        primary_blockers,
    }
}

fn parse_tasks(tasks: &[Value]) -> ProjectBoardResult<Vec<RawTask>> {
    let mut parsed = Vec::with_capacity(tasks.len());
    let mut task_ids = HashSet::new();
    for (index, task) in tasks.iter().enumerate() {
        let task_id = required_string(task, &["id", "task_id", "taskId"], index, "id")?;
        if !task_ids.insert(task_id.clone()) {
            return Err(ProjectBoardError::DuplicateTaskId(task_id));
        }
        let title = required_string(task, &["subject", "title", "description"], index, "title")?;
        let source_status = required_string(task, &["status"], index, "status")?;
        if !matches!(
            source_status.as_str(),
            "pending" | "in_progress" | "running" | "completed" | "failed" | "cancelled" | "killed"
        ) {
            return Err(ProjectBoardError::InvalidField {
                task_id,
                field: "status",
                reason: format!("unknown status {source_status}"),
            });
        }
        let board_override = optional_string_checked(
            task,
            &["board_status", "boardStatus"],
            &task_id,
            "board_status",
        )?
        .map(|value| parse_board_override(&task_id, &value))
        .transpose()?;

        parsed.push(RawTask {
            owner: optional_string_checked(task, &["owner"], &task_id, "owner")?,
            workstream_id: optional_string_checked(
                task,
                &["workstream_id", "workstreamId"],
                &task_id,
                "workstream_id",
            )?,
            milestone_id: optional_string_checked(
                task,
                &["milestone_id", "milestoneId"],
                &task_id,
                "milestone_id",
            )?,
            priority: optional_i64_checked(task, &["priority"], &task_id, "priority")?.unwrap_or(0),
            blocks: optional_string_array(task, &["blocks"], &task_id, "blocks")?,
            blocked_by: optional_string_array(
                task,
                &["blockedBy", "blocked_by"],
                &task_id,
                "blocked_by",
            )?,
            verification_commands: optional_string_array(
                task,
                &["verification_commands", "verificationCommands"],
                &task_id,
                "verification_commands",
            )?,
            evidence: optional_string_array(
                task,
                &["evidence", "evidence_refs", "evidenceRefs"],
                &task_id,
                "evidence",
            )?,
            blocker_reason: optional_string_checked(
                task,
                &["blocker_reason", "blockerReason"],
                &task_id,
                "blocker_reason",
            )?,
            unblock_condition: optional_string_checked(
                task,
                &["unblock_condition", "unblockCondition"],
                &task_id,
                "unblock_condition",
            )?,
            allowed_paths: optional_string_array(
                task,
                &["allowed_paths", "allowedPaths"],
                &task_id,
                "allowed_paths",
            )?,
            task_type: optional_string_checked(
                task,
                &["task_type", "taskType"],
                &task_id,
                "task_type",
            )?,
            risk_level: optional_string_checked(
                task,
                &["risk_level", "riskLevel"],
                &task_id,
                "risk_level",
            )?,
            approval_required: optional_bool_checked(
                task,
                &["approval_required", "approvalRequired"],
                &task_id,
                "approval_required",
            )?
            .unwrap_or(false),
            approval_status: optional_string_checked(
                task,
                &["approval_status", "approvalStatus"],
                &task_id,
                "approval_status",
            )?,
            created_at: optional_i64_root_checked(
                task,
                &["created_at", "createdAt"],
                &task_id,
                "created_at",
            )?
            .unwrap_or(0),
            updated_at: optional_i64_root_checked(
                task,
                &["updated_at", "updatedAt"],
                &task_id,
                "updated_at",
            )?
            .unwrap_or(0),
            task_id,
            title,
            source_status,
            board_override,
        });
    }
    Ok(parsed)
}

fn parse_board_override(task_id: &str, value: &str) -> ProjectBoardResult<ProjectBoardStatus> {
    match value {
        "backlog" => Ok(ProjectBoardStatus::Backlog),
        "spec" => Ok(ProjectBoardStatus::Spec),
        "review" => Ok(ProjectBoardStatus::Review),
        _ => Err(ProjectBoardError::InvalidField {
            task_id: task_id.to_string(),
            field: "board_status",
            reason: format!("unsupported override {value}"),
        }),
    }
}

fn required_string(
    task: &Value,
    aliases: &[&str],
    index: usize,
    field: &'static str,
) -> ProjectBoardResult<String> {
    aliases
        .iter()
        .filter_map(|alias| task.get(*alias).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(ProjectBoardError::MissingField { index, field })
}

fn optional_value<'a>(task: &'a Value, aliases: &[&str]) -> Option<&'a Value> {
    aliases
        .iter()
        .find_map(|alias| task.get(*alias))
        .or_else(|| {
            task.get("metadata")
                .and_then(Value::as_object)
                .and_then(|metadata| aliases.iter().find_map(|alias| metadata.get(*alias)))
        })
}

fn optional_string_checked(
    task: &Value,
    aliases: &[&str],
    task_id: &str,
    field: &'static str,
) -> ProjectBoardResult<Option<String>> {
    let Some(value) = optional_value(task, aliases) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| ProjectBoardError::InvalidField {
            task_id: task_id.to_string(),
            field,
            reason: "expected a string".to_string(),
        })?
        .trim();
    Ok((!value.is_empty()).then(|| value.to_string()))
}

fn optional_i64_checked(
    task: &Value,
    aliases: &[&str],
    task_id: &str,
    field: &'static str,
) -> ProjectBoardResult<Option<i64>> {
    let Some(value) = optional_value(task, aliases) else {
        return Ok(None);
    };
    value
        .as_i64()
        .map(Some)
        .ok_or_else(|| ProjectBoardError::InvalidField {
            task_id: task_id.to_string(),
            field,
            reason: "expected an integer".to_string(),
        })
}

fn optional_bool_checked(
    task: &Value,
    aliases: &[&str],
    task_id: &str,
    field: &'static str,
) -> ProjectBoardResult<Option<bool>> {
    let Some(value) = optional_value(task, aliases) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| ProjectBoardError::InvalidField {
            task_id: task_id.to_string(),
            field,
            reason: "expected a boolean".to_string(),
        })
}

fn optional_i64_root_checked(
    task: &Value,
    aliases: &[&str],
    task_id: &str,
    field: &'static str,
) -> ProjectBoardResult<Option<i64>> {
    let Some(value) = aliases.iter().find_map(|alias| task.get(*alias)) else {
        return Ok(None);
    };
    value
        .as_i64()
        .map(Some)
        .ok_or_else(|| ProjectBoardError::InvalidField {
            task_id: task_id.to_string(),
            field,
            reason: "expected an integer".to_string(),
        })
}

fn optional_string_array(
    task: &Value,
    aliases: &[&str],
    task_id: &str,
    field: &'static str,
) -> ProjectBoardResult<Vec<String>> {
    let Some(value) = optional_value(task, aliases) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| ProjectBoardError::InvalidField {
            task_id: task_id.to_string(),
            field,
            reason: "expected an array of strings".to_string(),
        })?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| ProjectBoardError::InvalidField {
                    task_id: task_id.to_string(),
                    field,
                    reason: "expected non-empty string entries".to_string(),
                })
        })
        .collect()
}

fn finding(task: &RawTask, code: &str, severity: &str, message: String) -> ProjectPolicyFinding {
    ProjectPolicyFinding {
        code: code.to_string(),
        severity: severity.to_string(),
        task_id: task.task_id.clone(),
        message,
    }
}

fn compare_cards(left: &ProjectTaskCard, right: &ProjectTaskCard) -> std::cmp::Ordering {
    right
        .priority
        .cmp(&left.priority)
        .then_with(|| right.blocks.len().cmp(&left.blocks.len()))
        .then_with(|| left.updated_at.cmp(&right.updated_at))
        .then_with(|| left.task_id.cmp(&right.task_id))
}

fn compare_findings(
    left: &ProjectPolicyFinding,
    right: &ProjectPolicyFinding,
) -> std::cmp::Ordering {
    left.task_id
        .cmp(&right.task_id)
        .then_with(|| left.code.cmp(&right.code))
        .then_with(|| left.severity.cmp(&right.severity))
        .then_with(|| left.message.cmp(&right.message))
}

fn explain_lower_rank(selected: &ProjectTaskCard, alternative: &ProjectTaskCard) -> String {
    if alternative.priority != selected.priority {
        format!(
            "lower_priority: {} < selected {}",
            alternative.priority, selected.priority
        )
    } else if alternative.blocks.len() != selected.blocks.len() {
        format!(
            "lower_unblocks: {} < selected {}",
            alternative.blocks.len(),
            selected.blocks.len()
        )
    } else if alternative.updated_at != selected.updated_at {
        format!(
            "newer_update: {} > selected {}",
            alternative.updated_at, selected.updated_at
        )
    } else {
        format!(
            "task_id_tiebreak: {} > selected {}",
            alternative.task_id, selected.task_id
        )
    }
}

fn summarize_blockers(findings: &[ProjectPolicyFinding]) -> Vec<ProjectBlockingReasonSummary> {
    let mut grouped: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for finding in findings {
        let task_ids = grouped
            .entry((finding.code.clone(), finding.severity.clone()))
            .or_default();
        if !task_ids.iter().any(|task_id| task_id == &finding.task_id) {
            task_ids.push(finding.task_id.clone());
        }
    }
    let mut summaries = grouped
        .into_iter()
        .map(|((code, severity), mut task_ids)| {
            task_ids.sort();
            ProjectBlockingReasonSummary {
                code,
                severity,
                count: task_ids.len(),
                task_ids,
            }
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        severity_rank(&right.severity)
            .cmp(&severity_rank(&left.severity))
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left.code.cmp(&right.code))
    });
    summaries
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "blocker" => 2,
        "warning" => 1,
        _ => 0,
    }
}

fn task_ids_for_status(columns: &[ProjectBoardColumn], status: ProjectBoardStatus) -> Vec<String> {
    columns
        .iter()
        .find(|column| column.status == status)
        .map(|column| {
            column
                .tasks
                .iter()
                .map(|task| task.task_id.clone())
                .collect()
        })
        .unwrap_or_default()
}
