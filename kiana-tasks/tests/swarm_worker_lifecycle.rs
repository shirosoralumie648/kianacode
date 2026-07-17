use kiana_tasks::{
    build_swarm_plan, initialize_workflow_run, persist_swarm_dispatch, prepare_swarm_execution,
    read_workflow_events, ProjectBoardColumn, ProjectBoardProjection, ProjectBoardStatus,
    ProjectTaskCard, SwarmExecutionError, SwarmExecutionRequest, SwarmWorkerBudget,
    SwarmWorkerLaunchInput, WorkflowEventKind, WorkflowInit, WorkflowInputKind, WorkflowProfile,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-swarm-worker-{name}-{}-{unique}",
        std::process::id()
    ))
}

fn task(id: &str, priority: i64, path: &str) -> ProjectTaskCard {
    ProjectTaskCard {
        schema: "kiana.project-task-card.v1".to_string(),
        task_id: id.to_string(),
        title: format!("Task {id}"),
        source_status: "pending".to_string(),
        board_status: ProjectBoardStatus::Ready,
        owner: None,
        workstream_id: None,
        milestone_id: None,
        priority,
        blocks: Vec::new(),
        blocked_by: Vec::new(),
        verification_commands: vec![format!("verify-{id}")],
        evidence: Vec::new(),
        blocker_reason: None,
        unblock_condition: None,
        allowed_paths: vec![path.to_string()],
        task_type: Some("implementation".to_string()),
        risk_level: Some("normal".to_string()),
        approval_required: false,
        approval_status: None,
        created_at: priority,
        updated_at: priority,
        policy_findings: Vec::new(),
    }
}

fn board(tasks: Vec<ProjectTaskCard>) -> ProjectBoardProjection {
    let count = tasks.len();
    ProjectBoardProjection {
        schema: "kiana.project-board.v1".to_string(),
        task_list_id: "default".to_string(),
        project_root: None,
        columns: vec![ProjectBoardColumn {
            status: ProjectBoardStatus::Ready,
            tasks,
        }],
        counts: BTreeMap::from([("ready".to_string(), count)]),
        policy_findings: Vec::new(),
        ready_task_ids: Vec::new(),
        blocked_task_ids: Vec::new(),
        done_task_ids: Vec::new(),
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn fixture() -> (PathBuf, kiana_tasks::WorkflowRun, String) {
    let root = temp_root("fixture");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Run API and docs workers".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let plan = build_swarm_plan(
        &board(vec![
            task("api", 20, "src/api/**"),
            task("docs", 10, "docs/**"),
        ]),
        2,
    )
    .unwrap();
    let dispatch =
        persist_swarm_dispatch(&run.artifact_dir, &plan, SwarmWorkerBudget::default()).unwrap();
    (root, run, dispatch.dispatch_id)
}

fn request(dispatch_id: &str) -> SwarmExecutionRequest {
    SwarmExecutionRequest {
        dispatch_id: dispatch_id.to_string(),
        runner_mode: "fixture_executable".to_string(),
        runner_fingerprint: "sha256:fixture".to_string(),
        launches: vec![
            SwarmWorkerLaunchInput {
                task_id: "api".to_string(),
                isolation_strategy: "snapshot_copy".to_string(),
                isolation_path: format!(".kiana/swarm-worktrees/{dispatch_id}/api"),
                runner_script: "#!/usr/bin/env bash\nexit 0\n".to_string(),
            },
            SwarmWorkerLaunchInput {
                task_id: "docs".to_string(),
                isolation_strategy: "snapshot_copy".to_string(),
                isolation_path: format!(".kiana/swarm-worktrees/{dispatch_id}/docs"),
                runner_script: "#!/usr/bin/env bash\nexit 0\n".to_string(),
            },
        ],
    }
}

#[test]
fn prepare_execution_persists_launch_artifacts_and_one_idempotent_event() {
    let (root, run, dispatch_id) = fixture();
    let initial_events = read_workflow_events(&run.artifact_dir).unwrap().len();

    let first = prepare_swarm_execution(&run.artifact_dir, request(&dispatch_id)).unwrap();
    let second = prepare_swarm_execution(&run.artifact_dir, request(&dispatch_id)).unwrap();

    assert_eq!(first.schema, "kiana.swarm-execution-prepare-result.v1");
    assert_eq!(first.status, "prepared");
    assert_eq!(first.dispatch_id, dispatch_id);
    assert_eq!(
        first.manifest_path,
        format!("workers/{dispatch_id}/manifest.json")
    );
    assert_eq!(first.launch_paths.len(), 2);
    assert_eq!(first.worker_ids.len(), 2);
    assert!(!first.reused_event);
    assert!(second.reused_event);
    assert_eq!(first.event_seq, second.event_seq);
    assert_eq!(first.next_action, "run_swarm_start");

    let manifest = read_json(&run.artifact_dir.join(&first.manifest_path));
    assert_eq!(manifest["schema"], "kiana.swarm-execution-manifest.v1");
    assert_eq!(manifest["dispatch_id"], dispatch_id);
    assert_eq!(manifest["workflow_id"], run.workflow_id);
    assert_eq!(manifest["run_id"], run.run_id);
    assert_eq!(manifest["runner_mode"], "fixture_executable");
    assert_eq!(manifest["runner_fingerprint"], "sha256:fixture");
    assert_eq!(manifest["next_action"], "run_swarm_start");
    assert!(!manifest
        .to_string()
        .contains(root.to_string_lossy().as_ref()));

    for launch_path in &first.launch_paths {
        assert!(!launch_path.starts_with('/'));
        let launch = read_json(&run.artifact_dir.join(launch_path));
        assert_eq!(launch["schema"], "kiana.swarm-worker-launch.v1");
        assert_eq!(launch["dispatch_id"], dispatch_id);
        assert_eq!(launch["attempt"], 1);
        assert_eq!(launch["isolation_strategy"], "snapshot_copy");
        assert!(launch["isolation_path"]
            .as_str()
            .unwrap()
            .starts_with(".kiana/swarm-worktrees/"));

        let worker_dir = run
            .artifact_dir
            .join(launch_path)
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(worker_dir.join("prompt.md").is_file());
        assert!(worker_dir.join("runner.sh").is_file());
        let prompt = fs::read_to_string(worker_dir.join("prompt.md")).unwrap();
        assert!(prompt.contains("kiana.swarm-workpacket.v1"));
        assert!(prompt.contains("不要 commit、push、merge 或 deploy"));
    }

    let events = read_workflow_events(&run.artifact_dir).unwrap();
    assert_eq!(events.len(), initial_events + 1);
    assert_eq!(
        events.last().unwrap().kind,
        WorkflowEventKind::SwarmWorkersPrepared
    );
    assert_eq!(events.last().unwrap().data["dispatch_id"], dispatch_id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_execution_rejects_launch_task_mismatch_without_new_event() {
    let (root, run, dispatch_id) = fixture();
    let initial_events = read_workflow_events(&run.artifact_dir).unwrap().len();
    let mut invalid = request(&dispatch_id);
    invalid.launches.pop();

    let error = prepare_swarm_execution(&run.artifact_dir, invalid).unwrap_err();

    assert!(matches!(
        error,
        SwarmExecutionError::LaunchTaskMismatch { .. }
    ));
    assert_eq!(
        read_workflow_events(&run.artifact_dir).unwrap().len(),
        initial_events
    );
    assert!(!run.artifact_dir.join("workers").join(&dispatch_id).exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_execution_rejects_absolute_isolation_path() {
    let (root, run, dispatch_id) = fixture();
    let initial_events = read_workflow_events(&run.artifact_dir).unwrap().len();
    let mut invalid = request(&dispatch_id);
    invalid.launches[0].isolation_path = "/tmp/escape".to_string();

    let error = prepare_swarm_execution(&run.artifact_dir, invalid).unwrap_err();

    assert!(matches!(
        error,
        SwarmExecutionError::InvalidIsolationPath(_)
    ));
    assert_eq!(
        read_workflow_events(&run.artifact_dir).unwrap().len(),
        initial_events
    );

    let _ = fs::remove_dir_all(root);
}
