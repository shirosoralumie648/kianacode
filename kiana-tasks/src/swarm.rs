use crate::{
    commit_immutable_artifacts_with_unique_event, read_workflow_state, ProjectBoardProjection,
    ProjectBoardStatus, ProjectTaskCard, WorkPacket, WorkflowArtifactBatch, WorkflowArtifactInput,
    WorkflowError, WorkflowEventKind, WorkflowStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};
use thiserror::Error;

pub const SWARM_PLAN_SCHEMA: &str = "kiana.swarm-plan.v1";
pub const WORK_PACKET_PREVIEW_SCHEMA: &str = "kiana.swarm-workpacket-preview.v1";
pub const SWARM_WORK_PACKET_SCHEMA: &str = "kiana.swarm-workpacket.v1";
pub const SWARM_DISPATCH_MANIFEST_SCHEMA: &str = "kiana.swarm-dispatch-manifest.v1";
pub const SWARM_DISPATCH_RESULT_SCHEMA: &str = "kiana.swarm-dispatch-result.v1";
pub const SWARM_EXECUTION_MANIFEST_SCHEMA: &str = "kiana.swarm-execution-manifest.v1";
pub const SWARM_WORKER_LAUNCH_SCHEMA: &str = "kiana.swarm-worker-launch.v1";
pub const SWARM_EXECUTION_PREPARE_RESULT_SCHEMA: &str = "kiana.swarm-execution-prepare-result.v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SwarmPlanError {
    #[error("max_workers must be between 2 and 32")]
    InvalidMaxWorkers,
}

#[derive(Debug, Error)]
pub enum SwarmDispatchError {
    #[error("swarm plan is not ready for dispatch: {0}")]
    PlanNotReady(String),
    #[error("swarm worker budget is invalid: {0}")]
    InvalidBudget(String),
    #[error("swarm task id is unsafe for artifact persistence: {0}")]
    UnsafeTaskId(String),
    #[error("workflow status does not allow dispatch: {0}")]
    InvalidWorkflowStatus(String),
    #[error(transparent)]
    Workflow(#[from] WorkflowError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum SwarmExecutionError {
    #[error("swarm dispatch was not found: {0}")]
    DispatchNotFound(String),
    #[error("swarm dispatch manifest is invalid: {0}")]
    InvalidDispatchManifest(String),
    #[error("swarm execution launch tasks do not match dispatch tasks: expected {expected:?}, actual {actual:?}")]
    LaunchTaskMismatch {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    #[error("swarm execution contains duplicate launch task: {0}")]
    DuplicateLaunchTask(String),
    #[error("swarm isolation path is invalid: {0}")]
    InvalidIsolationPath(String),
    #[error("swarm runner metadata is invalid: {0}")]
    InvalidRunner(String),
    #[error("swarm worker launch is invalid: {0}")]
    InvalidLaunch(String),
    #[error(transparent)]
    Workflow(#[from] WorkflowError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPlan {
    pub schema: String,
    pub task_list_id: String,
    pub max_workers: usize,
    pub execution_mode: String,
    pub status: String,
    pub assignments: Vec<SwarmAssignment>,
    pub skipped: Vec<SwarmSkippedTask>,
    pub path_locks: Vec<SwarmPathLock>,
    pub summary: SwarmPlanSummary,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmAssignment {
    pub task_id: String,
    pub worker_type: String,
    pub workpacket: WorkPacket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSkippedTask {
    pub task_id: String,
    pub reason: String,
    pub conflicts_with: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwarmPathLock {
    pub task_id: String,
    pub path: String,
    pub resolved_path: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwarmWorkerBudget {
    pub max_attempts: u32,
    pub max_commands: u32,
    pub timeout_seconds: u64,
    pub max_output_bytes: u64,
}

impl Default for SwarmWorkerBudget {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            max_commands: 20,
            timeout_seconds: 30 * 60,
            max_output_bytes: 10 * 1024 * 1024,
        }
    }
}

impl SwarmWorkerBudget {
    pub fn validate(&self) -> Result<(), SwarmDispatchError> {
        if !(1..=3).contains(&self.max_attempts) {
            return Err(SwarmDispatchError::InvalidBudget(
                "max_attempts must be between 1 and 3".to_string(),
            ));
        }
        if !(1..=100).contains(&self.max_commands) {
            return Err(SwarmDispatchError::InvalidBudget(
                "max_commands must be between 1 and 100".to_string(),
            ));
        }
        if !(1..=86_400).contains(&self.timeout_seconds) {
            return Err(SwarmDispatchError::InvalidBudget(
                "timeout_seconds must be between 1 and 86400".to_string(),
            ));
        }
        if !(1_024..=100 * 1024 * 1024).contains(&self.max_output_bytes) {
            return Err(SwarmDispatchError::InvalidBudget(
                "max_output_bytes must be between 1024 and 104857600".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwarmTerminationPolicy {
    pub reasons: Vec<String>,
}

impl Default for SwarmTerminationPolicy {
    fn default() -> Self {
        Self {
            reasons: [
                "completed",
                "budget_exhausted",
                "timeout",
                "scope_violation",
                "policy_denied",
                "approval_required",
                "cancelled",
                "worker_failed",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmWorkPacket {
    pub schema: String,
    pub workpacket_id: String,
    pub dispatch_id: String,
    pub workflow_id: String,
    pub run_id: String,
    pub task_id: String,
    pub worker_type: String,
    pub goal: String,
    pub context_summary: Vec<String>,
    pub allowed_files: Vec<String>,
    pub forbidden_files: Vec<String>,
    pub path_locks: Vec<SwarmPathLock>,
    pub verification_commands: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub evidence_requirements: Vec<String>,
    pub rollback_plan: Vec<String>,
    pub review_focus: Vec<String>,
    pub depends_on: Vec<String>,
    pub budget: SwarmWorkerBudget,
    pub termination_policy: SwarmTerminationPolicy,
    pub retry_policy: String,
    pub approval_required: bool,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmDispatchManifest {
    pub schema: String,
    pub dispatch_id: String,
    pub workflow_id: String,
    pub run_id: String,
    pub task_list_id: String,
    pub plan_schema: String,
    pub plan_fingerprint: String,
    pub execution_mode: String,
    pub status: String,
    pub workpacket_paths: Vec<String>,
    pub task_ids: Vec<String>,
    pub path_locks: Vec<SwarmPathLock>,
    pub budget: SwarmWorkerBudget,
    pub created_at_ms: u64,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmDispatchResult {
    pub schema: String,
    pub dispatch_id: String,
    pub execution_mode: String,
    pub status: String,
    pub manifest_path: String,
    pub workpacket_paths: Vec<String>,
    pub event_seq: u64,
    pub reused_event: bool,
    pub next_action: String,
}

#[derive(Debug, Clone)]
pub struct SwarmWorkerLaunchInput {
    pub task_id: String,
    pub isolation_strategy: String,
    pub isolation_path: String,
    pub runner_script: String,
}

#[derive(Debug, Clone)]
pub struct SwarmExecutionRequest {
    pub dispatch_id: String,
    pub runner_mode: String,
    pub runner_fingerprint: String,
    pub launches: Vec<SwarmWorkerLaunchInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmExecutionManifest {
    pub schema: String,
    pub dispatch_id: String,
    pub workflow_id: String,
    pub run_id: String,
    pub runner_mode: String,
    pub runner_fingerprint: String,
    pub worker_ids: Vec<String>,
    pub launch_paths: Vec<String>,
    pub created_at_ms: u64,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmWorkerLaunch {
    pub schema: String,
    pub worker_id: String,
    pub dispatch_id: String,
    pub task_id: String,
    pub workpacket_path: String,
    pub prompt_path: String,
    pub runner_path: String,
    pub state_path: String,
    pub isolation_mode: String,
    pub isolation_strategy: String,
    pub isolation_path: String,
    pub budget: SwarmWorkerBudget,
    pub attempt: u32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmExecutionPrepareResult {
    pub schema: String,
    pub dispatch_id: String,
    pub status: String,
    pub manifest_path: String,
    pub launch_paths: Vec<String>,
    pub worker_ids: Vec<String>,
    pub event_seq: u64,
    pub reused_event: bool,
    pub next_action: String,
}

#[derive(Debug, Clone)]
struct NormalizedScope {
    logical_path: String,
    resolved_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPlanSummary {
    pub ready_tasks: usize,
    pub dispatched: usize,
    pub skipped: usize,
}

pub fn build_swarm_plan(
    board: &ProjectBoardProjection,
    max_workers: usize,
) -> Result<SwarmPlan, SwarmPlanError> {
    if !(2..=32).contains(&max_workers) {
        return Err(SwarmPlanError::InvalidMaxWorkers);
    }

    let mut ready_tasks = board
        .columns
        .iter()
        .filter(|column| column.status == ProjectBoardStatus::Ready)
        .flat_map(|column| column.tasks.iter().cloned())
        .collect::<Vec<_>>();
    ready_tasks.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });

    let ready_count = ready_tasks.len();
    let mut assignments = Vec::new();
    let mut skipped = Vec::new();
    let mut path_locks = Vec::new();

    for task in ready_tasks {
        if let Some(reason) = dispatch_policy_reason(&task) {
            skipped.push(skipped_task(&task, reason, Vec::new()));
            continue;
        }

        if task.allowed_paths.is_empty() {
            skipped.push(skipped_task(&task, "missing_allowed_paths", Vec::new()));
            continue;
        }

        let normalized_paths = match normalize_task_paths(&task, board.project_root.as_deref()) {
            Ok(paths) => paths,
            Err(()) => {
                skipped.push(skipped_task(&task, "invalid_allowed_path", Vec::new()));
                continue;
            }
        };

        let conflicts_with = conflicting_task_ids(&normalized_paths, &path_locks);
        if !conflicts_with.is_empty() {
            skipped.push(skipped_task(&task, "path_conflict", conflicts_with));
            continue;
        }

        if assignments.len() >= max_workers {
            skipped.push(skipped_task(&task, "worker_limit", Vec::new()));
            continue;
        }

        path_locks.extend(normalized_paths.iter().map(|path| SwarmPathLock {
            task_id: task.task_id.clone(),
            path: path.logical_path.clone(),
            resolved_path: path.resolved_path.clone(),
            mode: path_lock_mode(&path.logical_path).to_string(),
        }));
        assignments.push(SwarmAssignment {
            task_id: task.task_id.clone(),
            worker_type: "builder".to_string(),
            workpacket: workpacket_for_task(&task),
        });
    }

    let (status, next_action) = if assignments.len() >= 2 {
        ("ready", "run_swarm_dispatch")
    } else {
        for assignment in assignments.drain(..) {
            skipped.push(SwarmSkippedTask {
                task_id: assignment.task_id,
                reason: "insufficient_parallelism".to_string(),
                conflicts_with: Vec::new(),
            });
        }
        path_locks.clear();
        ("blocked", "prepare_at_least_two_ready_tasks")
    };

    Ok(SwarmPlan {
        schema: SWARM_PLAN_SCHEMA.to_string(),
        task_list_id: board.task_list_id.clone(),
        max_workers,
        execution_mode: "plan_only".to_string(),
        status: status.to_string(),
        summary: SwarmPlanSummary {
            ready_tasks: ready_count,
            dispatched: assignments.len(),
            skipped: skipped.len(),
        },
        assignments,
        skipped,
        path_locks,
        next_action: next_action.to_string(),
    })
}

pub fn persist_swarm_dispatch(
    artifact_dir: impl AsRef<Path>,
    plan: &SwarmPlan,
    budget: SwarmWorkerBudget,
) -> Result<SwarmDispatchResult, SwarmDispatchError> {
    if plan.status != "ready" || plan.assignments.len() < 2 {
        return Err(SwarmDispatchError::PlanNotReady(plan.status.clone()));
    }
    budget.validate()?;
    let artifact_dir = artifact_dir.as_ref();
    let state = read_workflow_state(artifact_dir)?;
    if !matches!(
        state.status,
        WorkflowStatus::Initialized | WorkflowStatus::Running
    ) {
        return Err(SwarmDispatchError::InvalidWorkflowStatus(format!(
            "{:?}",
            state.status
        )));
    }
    for assignment in &plan.assignments {
        validate_dispatch_task_id(&assignment.task_id)?;
    }

    let identity = json!({
        "workflow_id": state.workflow_id,
        "run_id": state.run_id,
        "task_list_id": plan.task_list_id,
        "assignments": plan.assignments.iter().map(|assignment| json!({
            "task_id": assignment.task_id,
            "allowed_files": assignment.workpacket.allowed_files,
            "verification_commands": assignment.workpacket.verification_commands,
            "path_locks": plan.path_locks.iter().filter(|lock| lock.task_id == assignment.task_id).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "budget": budget,
    });
    let plan_fingerprint = sha256_hex(&serde_json::to_vec(&identity)?);
    let dispatch_id = format!("dispatch-{}", &plan_fingerprint[..24]);
    let manifest_path = format!("workpackets/{dispatch_id}/manifest.json");
    let workpacket_paths = plan
        .assignments
        .iter()
        .map(|assignment| format!("workpackets/{dispatch_id}/{}.json", assignment.task_id))
        .collect::<Vec<_>>();

    let workflow_id = state.workflow_id.clone();
    let run_id = state.run_id.clone();
    let request = state.request.clone();
    let task_list_id = plan.task_list_id.clone();
    let plan_schema = plan.schema.clone();
    let assignments = plan.assignments.clone();
    let path_locks = plan.path_locks.clone();
    let event_dispatch_id = dispatch_id.clone();
    let event_manifest_path = manifest_path.clone();
    let event_workpacket_paths = workpacket_paths.clone();
    let event_plan_fingerprint = plan_fingerprint.clone();
    let commit = commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::WorkPacketCreated,
        "workpacket",
        "dispatch_id",
        dispatch_id.clone(),
        move |sequence, created_at_ms| {
            let mut artifacts = Vec::with_capacity(assignments.len() + 1);
            let task_ids = assignments
                .iter()
                .map(|assignment| assignment.task_id.clone())
                .collect::<Vec<_>>();
            for (assignment, relative_path) in assignments.iter().zip(&event_workpacket_paths) {
                let task_path_locks = path_locks
                    .iter()
                    .filter(|lock| lock.task_id == assignment.task_id)
                    .cloned()
                    .collect::<Vec<_>>();
                let packet = SwarmWorkPacket {
                    schema: SWARM_WORK_PACKET_SCHEMA.to_string(),
                    workpacket_id: format!("wp-{event_dispatch_id}-{}", assignment.task_id),
                    dispatch_id: event_dispatch_id.clone(),
                    workflow_id: workflow_id.clone(),
                    run_id: run_id.clone(),
                    task_id: assignment.task_id.clone(),
                    worker_type: assignment.worker_type.clone(),
                    goal: assignment.workpacket.goal.clone(),
                    context_summary: vec![
                        request.clone(),
                        format!(
                            "Task {} selected from task list {} for bounded parallel execution",
                            assignment.task_id, task_list_id
                        ),
                    ],
                    allowed_files: assignment.workpacket.allowed_files.clone(),
                    forbidden_files: assignment.workpacket.forbidden_files.clone(),
                    path_locks: task_path_locks,
                    verification_commands: assignment.workpacket.verification_commands.clone(),
                    acceptance_criteria: assignment.workpacket.acceptance_criteria.clone(),
                    evidence_requirements: assignment.workpacket.evidence_requirements.clone(),
                    rollback_plan: assignment.workpacket.rollback_plan.clone(),
                    review_focus: assignment.workpacket.review_focus.clone(),
                    depends_on: assignment.workpacket.depends_on.clone(),
                    budget: budget.clone(),
                    termination_policy: SwarmTerminationPolicy::default(),
                    retry_policy: format!("max_attempts={}", budget.max_attempts),
                    approval_required: assignment.workpacket.approval_required,
                    created_at_ms,
                };
                artifacts.push(WorkflowArtifactInput {
                    relative_path: relative_path.clone(),
                    contents: serde_json::to_vec_pretty(&packet)?,
                });
            }
            let manifest = SwarmDispatchManifest {
                schema: SWARM_DISPATCH_MANIFEST_SCHEMA.to_string(),
                dispatch_id: event_dispatch_id.clone(),
                workflow_id: workflow_id.clone(),
                run_id: run_id.clone(),
                task_list_id: task_list_id.clone(),
                plan_schema: plan_schema.clone(),
                plan_fingerprint: event_plan_fingerprint.clone(),
                execution_mode: "persist_only".to_string(),
                status: "persisted".to_string(),
                workpacket_paths: event_workpacket_paths.clone(),
                task_ids: task_ids.clone(),
                path_locks: path_locks.clone(),
                budget: budget.clone(),
                created_at_ms,
                next_action: "run_swarm_start".to_string(),
            };
            artifacts.push(WorkflowArtifactInput {
                relative_path: event_manifest_path.clone(),
                contents: serde_json::to_vec_pretty(&manifest)?,
            });
            Ok(WorkflowArtifactBatch {
                artifacts,
                event_data: json!({
                    "dispatch_id": event_dispatch_id,
                    "manifest_path": event_manifest_path,
                    "workpacket_paths": event_workpacket_paths,
                    "task_ids": task_ids,
                    "plan_fingerprint": event_plan_fingerprint,
                    "execution_mode": "persist_only",
                    "status": "persisted",
                    "sequence": sequence,
                    "created_at_ms": created_at_ms,
                    "next_action": "run_swarm_start"
                }),
            })
        },
    )?;

    Ok(SwarmDispatchResult {
        schema: SWARM_DISPATCH_RESULT_SCHEMA.to_string(),
        dispatch_id,
        execution_mode: "persist_only".to_string(),
        status: "persisted".to_string(),
        manifest_path,
        workpacket_paths,
        event_seq: commit.event.seq,
        reused_event: commit.reused_event,
        next_action: "run_swarm_start".to_string(),
    })
}

pub fn prepare_swarm_execution(
    artifact_dir: impl AsRef<Path>,
    request: SwarmExecutionRequest,
) -> Result<SwarmExecutionPrepareResult, SwarmExecutionError> {
    validate_safe_segment(&request.dispatch_id)
        .map_err(|_| SwarmExecutionError::InvalidDispatchManifest(request.dispatch_id.clone()))?;
    if request.runner_mode.trim().is_empty() || request.runner_fingerprint.trim().is_empty() {
        return Err(SwarmExecutionError::InvalidRunner(
            "runner_mode and runner_fingerprint are required".to_string(),
        ));
    }

    let artifact_dir = artifact_dir.as_ref();
    let state = read_workflow_state(artifact_dir)?;
    if !matches!(
        state.status,
        WorkflowStatus::Initialized | WorkflowStatus::Running
    ) {
        return Err(SwarmExecutionError::InvalidDispatchManifest(format!(
            "workflow status {:?} does not allow worker preparation",
            state.status
        )));
    }

    let dispatch_manifest_path = format!("workpackets/{}/manifest.json", request.dispatch_id);
    let dispatch_manifest_bytes = match fs::read(artifact_dir.join(&dispatch_manifest_path)) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SwarmExecutionError::DispatchNotFound(request.dispatch_id));
        }
        Err(error) => return Err(SwarmExecutionError::Io(error)),
    };
    let dispatch: SwarmDispatchManifest = serde_json::from_slice(&dispatch_manifest_bytes)?;
    if dispatch.schema != SWARM_DISPATCH_MANIFEST_SCHEMA
        || dispatch.dispatch_id != request.dispatch_id
        || dispatch.workflow_id != state.workflow_id
        || dispatch.run_id != state.run_id
        || dispatch.task_ids.len() < 2
    {
        return Err(SwarmExecutionError::InvalidDispatchManifest(
            request.dispatch_id,
        ));
    }

    let mut launches_by_task = BTreeMap::new();
    for mut launch in request.launches {
        validate_safe_segment(&launch.task_id)
            .map_err(|_| SwarmExecutionError::InvalidLaunch(launch.task_id.clone()))?;
        if launches_by_task.contains_key(&launch.task_id) {
            return Err(SwarmExecutionError::DuplicateLaunchTask(launch.task_id));
        }
        if !matches!(
            launch.isolation_strategy.as_str(),
            "git_worktree" | "snapshot_copy"
        ) {
            return Err(SwarmExecutionError::InvalidLaunch(format!(
                "unsupported isolation strategy {}",
                launch.isolation_strategy
            )));
        }
        launch.isolation_path = validate_isolation_path(
            &launch.isolation_path,
            &dispatch.dispatch_id,
            &launch.task_id,
        )?;
        if launch.runner_script.trim().is_empty() {
            return Err(SwarmExecutionError::InvalidLaunch(format!(
                "runner script is empty for {}",
                launch.task_id
            )));
        }
        launches_by_task.insert(launch.task_id.clone(), launch);
    }

    let mut expected = dispatch.task_ids.clone();
    expected.sort();
    let actual = launches_by_task.keys().cloned().collect::<Vec<_>>();
    if expected != actual {
        return Err(SwarmExecutionError::LaunchTaskMismatch { expected, actual });
    }

    let mut packet_paths_by_task = BTreeMap::new();
    let mut packets_by_task = BTreeMap::new();
    for relative_path in &dispatch.workpacket_paths {
        let packet_bytes = fs::read(artifact_dir.join(relative_path))?;
        let packet: SwarmWorkPacket = serde_json::from_slice(&packet_bytes)?;
        if packet.schema != SWARM_WORK_PACKET_SCHEMA
            || packet.dispatch_id != dispatch.dispatch_id
            || packet.workflow_id != state.workflow_id
            || packet.run_id != state.run_id
        {
            return Err(SwarmExecutionError::InvalidDispatchManifest(
                dispatch.dispatch_id,
            ));
        }
        packet_paths_by_task.insert(packet.task_id.clone(), relative_path.clone());
        packets_by_task.insert(packet.task_id.clone(), packet);
    }
    if packet_paths_by_task.keys().cloned().collect::<Vec<_>>() != actual {
        return Err(SwarmExecutionError::LaunchTaskMismatch {
            expected: actual,
            actual: packet_paths_by_task.keys().cloned().collect(),
        });
    }

    let manifest_path = format!("workers/{}/manifest.json", dispatch.dispatch_id);
    let worker_ids = dispatch
        .task_ids
        .iter()
        .map(|task_id| worker_id(&dispatch.dispatch_id, task_id))
        .collect::<Vec<_>>();
    let launch_paths = dispatch
        .task_ids
        .iter()
        .map(|task_id| format!("workers/{}/{task_id}/launch.json", dispatch.dispatch_id))
        .collect::<Vec<_>>();

    let event_dispatch_id = dispatch.dispatch_id.clone();
    let event_manifest_path = manifest_path.clone();
    let event_launch_paths = launch_paths.clone();
    let event_worker_ids = worker_ids.clone();
    let workflow_id = state.workflow_id.clone();
    let run_id = state.run_id.clone();
    let runner_mode = request.runner_mode.clone();
    let runner_fingerprint = request.runner_fingerprint.clone();
    let task_ids = dispatch.task_ids.clone();
    let budget = dispatch.budget.clone();
    let commit = commit_immutable_artifacts_with_unique_event(
        artifact_dir,
        WorkflowEventKind::SwarmWorkersPrepared,
        "swarm_execute",
        "swarm_execution_id",
        dispatch.dispatch_id.clone(),
        move |sequence, created_at_ms| {
            let mut artifacts = Vec::with_capacity(task_ids.len() * 3 + 1);
            for task_id in &task_ids {
                let launch_input = launches_by_task.get(task_id).ok_or_else(|| {
                    WorkflowError::InvalidEventLog(format!("missing launch input for {task_id}"))
                })?;
                let packet = packets_by_task.get(task_id).ok_or_else(|| {
                    WorkflowError::InvalidEventLog(format!("missing workpacket for {task_id}"))
                })?;
                let workpacket_path = packet_paths_by_task.get(task_id).ok_or_else(|| {
                    WorkflowError::InvalidEventLog(format!("missing workpacket path for {task_id}"))
                })?;
                let worker_id = worker_id(&event_dispatch_id, task_id);
                let base = format!("workers/{}/{task_id}", event_dispatch_id);
                let prompt_path = format!("{base}/prompt.md");
                let runner_path = format!("{base}/runner.sh");
                let state_path = format!("{base}/state.json");
                let launch = SwarmWorkerLaunch {
                    schema: SWARM_WORKER_LAUNCH_SCHEMA.to_string(),
                    worker_id,
                    dispatch_id: event_dispatch_id.clone(),
                    task_id: task_id.clone(),
                    workpacket_path: workpacket_path.clone(),
                    prompt_path: prompt_path.clone(),
                    runner_path: runner_path.clone(),
                    state_path,
                    isolation_mode: "worktree".to_string(),
                    isolation_strategy: launch_input.isolation_strategy.clone(),
                    isolation_path: launch_input.isolation_path.clone(),
                    budget: budget.clone(),
                    attempt: 1,
                    created_at_ms,
                };
                let prompt = swarm_worker_prompt(packet)?;
                artifacts.push(WorkflowArtifactInput {
                    relative_path: format!("{base}/launch.json"),
                    contents: serde_json::to_vec_pretty(&launch)?,
                });
                artifacts.push(WorkflowArtifactInput {
                    relative_path: prompt_path,
                    contents: prompt.into_bytes(),
                });
                artifacts.push(WorkflowArtifactInput {
                    relative_path: runner_path,
                    contents: launch_input.runner_script.as_bytes().to_vec(),
                });
            }
            let manifest = SwarmExecutionManifest {
                schema: SWARM_EXECUTION_MANIFEST_SCHEMA.to_string(),
                dispatch_id: event_dispatch_id.clone(),
                workflow_id: workflow_id.clone(),
                run_id: run_id.clone(),
                runner_mode: runner_mode.clone(),
                runner_fingerprint: runner_fingerprint.clone(),
                worker_ids: event_worker_ids.clone(),
                launch_paths: event_launch_paths.clone(),
                created_at_ms,
                next_action: "run_swarm_start".to_string(),
            };
            artifacts.push(WorkflowArtifactInput {
                relative_path: event_manifest_path.clone(),
                contents: serde_json::to_vec_pretty(&manifest)?,
            });
            Ok(WorkflowArtifactBatch {
                artifacts,
                event_data: json!({
                    "swarm_execution_id": event_dispatch_id,
                    "dispatch_id": event_dispatch_id,
                    "manifest_path": event_manifest_path,
                    "launch_paths": event_launch_paths,
                    "worker_ids": event_worker_ids,
                    "runner_mode": runner_mode,
                    "runner_fingerprint": runner_fingerprint,
                    "sequence": sequence,
                    "created_at_ms": created_at_ms,
                    "next_action": "run_swarm_start"
                }),
            })
        },
    )?;

    Ok(SwarmExecutionPrepareResult {
        schema: SWARM_EXECUTION_PREPARE_RESULT_SCHEMA.to_string(),
        dispatch_id: dispatch.dispatch_id,
        status: "prepared".to_string(),
        manifest_path,
        launch_paths,
        worker_ids,
        event_seq: commit.event.seq,
        reused_event: commit.reused_event,
        next_action: "run_swarm_start".to_string(),
    })
}

fn validate_safe_segment(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(());
    }
    Ok(())
}

fn validate_isolation_path(
    raw: &str,
    dispatch_id: &str,
    task_id: &str,
) -> Result<String, SwarmExecutionError> {
    let normalized = raw.trim().replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.is_empty()
        || path.is_absolute()
        || normalized.as_bytes().get(1) == Some(&b':')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(SwarmExecutionError::InvalidIsolationPath(normalized));
    }
    let normalized = path
        .components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    let expected = format!(".kiana/swarm-worktrees/{dispatch_id}/{task_id}");
    if normalized != expected {
        return Err(SwarmExecutionError::InvalidIsolationPath(normalized));
    }
    Ok(normalized)
}

fn worker_id(dispatch_id: &str, task_id: &str) -> String {
    format!(
        "worker-{}-{task_id}",
        dispatch_id.strip_prefix("dispatch-").unwrap_or(dispatch_id)
    )
}

fn swarm_worker_prompt(packet: &SwarmWorkPacket) -> Result<String, serde_json::Error> {
    Ok(format!(
        "# Kiana Bounded Swarm WorkPacket\n\n你只能执行下面的 WorkPacket。只修改 allowed files，遵守 forbidden files 与 path locks。运行可执行的验证命令并如实报告。不要 commit、push、merge 或 deploy。不要把退出码 0 当成验收完成。\n\n```json\n{}\n```\n",
        serde_json::to_string_pretty(packet)?
    ))
}

fn validate_dispatch_task_id(task_id: &str) -> Result<(), SwarmDispatchError> {
    if task_id.is_empty()
        || !task_id
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'))
    {
        return Err(SwarmDispatchError::UnsafeTaskId(task_id.to_string()));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn normalize_task_paths(
    task: &ProjectTaskCard,
    project_root: Option<&str>,
) -> Result<Vec<NormalizedScope>, ()> {
    let mut paths = BTreeSet::new();
    for path in &task.allowed_paths {
        let logical_path = normalize_scope_path(path)?;
        let resolved_path = match project_root {
            Some(root) => resolve_scope_path(Path::new(root), &logical_path)?,
            None => logical_path.clone(),
        };
        paths.insert((logical_path, resolved_path));
    }
    if paths.is_empty() {
        return Err(());
    }
    Ok(paths
        .into_iter()
        .map(|(logical_path, resolved_path)| NormalizedScope {
            logical_path,
            resolved_path,
        })
        .collect())
}

fn resolve_scope_path(project_root: &Path, logical_path: &str) -> Result<String, ()> {
    let project_root = std::fs::canonicalize(project_root).map_err(|_| ())?;
    if logical_path == "." {
        return Ok(".".to_string());
    }

    let target = project_root.join(logical_path);
    let mut existing = target.as_path();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or(())?.to_os_string());
                existing = existing.parent().ok_or(())?;
            }
            Err(_) => return Err(()),
        }
    }

    let mut resolved = std::fs::canonicalize(existing).map_err(|_| ())?;
    if !resolved.starts_with(&project_root) {
        return Err(());
    }
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    let relative = resolved.strip_prefix(&project_root).map_err(|_| ())?;
    normalize_resolved_relative(relative)
}

fn normalize_resolved_relative(path: &Path) -> Result<String, ()> {
    let components = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string).ok_or(()),
            _ => Err(()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(components.join("/"))
    }
}

fn normalize_scope_path(raw: &str) -> Result<String, ()> {
    let normalized = raw.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') || has_windows_drive_prefix(&normalized)
    {
        return Err(());
    }

    let mut components = Vec::new();
    let mut saw_glob = false;
    for component in Path::new(&normalized).components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(()),
            Component::Normal(value) => {
                let value = value.to_str().ok_or(())?;
                validate_glob_component(value)?;
                if contains_glob(value) {
                    saw_glob = true;
                    break;
                }
                components.push(value.to_string());
            }
        }
    }

    if components.is_empty() {
        return if saw_glob {
            Ok(".".to_string())
        } else {
            Err(())
        };
    }

    Ok(components.join("/"))
}

fn has_windows_drive_prefix(path: &str) -> bool {
    path.as_bytes().get(1) == Some(&b':')
}

fn contains_glob(component: &str) -> bool {
    component.contains(['*', '?', '[', ']'])
}

fn validate_glob_component(component: &str) -> Result<(), ()> {
    if component.contains(['{', '}']) {
        return Err(());
    }
    if component.contains('[') != component.contains(']') {
        return Err(());
    }
    Ok(())
}

fn conflicting_task_ids(paths: &[NormalizedScope], locks: &[SwarmPathLock]) -> Vec<String> {
    locks
        .iter()
        .filter(|lock| {
            paths
                .iter()
                .any(|path| paths_overlap(&path.resolved_path, &lock.resolved_path))
        })
        .map(|lock| lock.task_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == "."
        || right == "."
        || left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn path_lock_mode(path: &str) -> &'static str {
    match Path::new(path).file_name().and_then(|value| value.to_str()) {
        Some("Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "uv.lock") => {
            "exclusive"
        }
        _ => "write",
    }
}

fn skipped_task(
    task: &ProjectTaskCard,
    reason: &str,
    conflicts_with: Vec<String>,
) -> SwarmSkippedTask {
    SwarmSkippedTask {
        task_id: task.task_id.clone(),
        reason: reason.to_string(),
        conflicts_with,
    }
}

fn dispatch_policy_reason(task: &ProjectTaskCard) -> Option<&'static str> {
    if task.task_type.as_deref().is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "release" | "deploy" | "secrets" | "policy"
        )
    }) {
        return Some("restricted_task_type");
    }
    if task
        .risk_level
        .as_deref()
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "high" | "critical"))
    {
        return Some("high_risk_task");
    }
    if task.approval_required
        && !task
            .approval_status
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("approved"))
    {
        return Some("approval_required");
    }
    None
}

fn workpacket_for_task(task: &ProjectTaskCard) -> WorkPacket {
    WorkPacket {
        schema: WORK_PACKET_PREVIEW_SCHEMA.to_string(),
        id: format!("wp-swarm-{}", task.task_id),
        goal: task.title.clone(),
        in_scope: task.allowed_paths.clone(),
        not_building: vec!["files outside allowed_files".to_string()],
        allowed_files: task.allowed_paths.clone(),
        forbidden_files: vec![".git/**".to_string(), ".kiana/policy.json".to_string()],
        acceptance_criteria: task
            .verification_commands
            .iter()
            .map(|command| format!("command passes: {command}"))
            .collect(),
        verification_commands: task.verification_commands.clone(),
        evidence_requirements: task
            .verification_commands
            .iter()
            .map(|command| format!("command evidence: {command}"))
            .collect(),
        rollback_plan: vec!["discard worker diff or worktree before integration".to_string()],
        review_focus: vec![
            "scope compliance".to_string(),
            "path lock compliance".to_string(),
            "verification evidence".to_string(),
        ],
        depends_on: task.blocked_by.clone(),
        retry_policy: "max_attempts=1".to_string(),
        approval_required: task.approval_required,
    }
}
