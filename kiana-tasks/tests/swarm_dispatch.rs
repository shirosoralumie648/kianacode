use kiana_tasks::{
    build_swarm_plan, initialize_workflow_run, persist_swarm_dispatch, read_workflow_events,
    ProjectBoardColumn, ProjectBoardProjection, ProjectBoardStatus, ProjectTaskCard,
    SwarmDispatchError, SwarmWorkerBudget, WorkflowEventKind, WorkflowInit, WorkflowInputKind,
    WorkflowProfile,
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
        "kiana-swarm-dispatch-{name}-{}-{unique}",
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

#[test]
fn swarm_dispatch_persists_formal_packets_manifest_and_one_idempotent_event() {
    let root = temp_root("persist");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Implement API and documentation in parallel".to_string(),
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
    let initial_event_count = read_workflow_events(&run.artifact_dir).unwrap().len();

    let first =
        persist_swarm_dispatch(&run.artifact_dir, &plan, SwarmWorkerBudget::default()).unwrap();
    let second =
        persist_swarm_dispatch(&run.artifact_dir, &plan, SwarmWorkerBudget::default()).unwrap();

    assert_eq!(first.schema, "kiana.swarm-dispatch-result.v1");
    assert_eq!(first.execution_mode, "persist_only");
    assert_eq!(first.status, "persisted");
    assert_eq!(first.next_action, "run_swarm_start");
    assert_eq!(first.dispatch_id, second.dispatch_id);
    assert!(!first.reused_event);
    assert!(second.reused_event);
    assert_eq!(first.event_seq, second.event_seq);
    assert_eq!(first.workpacket_paths.len(), 2);
    assert!(first.manifest_path.starts_with("workpackets/dispatch-"));
    assert!(!first.manifest_path.starts_with('/'));

    let manifest = read_json(&run.artifact_dir.join(&first.manifest_path));
    assert_eq!(manifest["schema"], "kiana.swarm-dispatch-manifest.v1");
    assert_eq!(manifest["dispatch_id"], first.dispatch_id);
    assert_eq!(manifest["workflow_id"], run.workflow_id);
    assert_eq!(manifest["run_id"], run.run_id);
    assert_eq!(manifest["execution_mode"], "persist_only");
    assert_eq!(manifest["next_action"], "run_swarm_start");
    assert_eq!(manifest["task_ids"], serde_json::json!(["api", "docs"]));

    for path in &first.workpacket_paths {
        let packet = read_json(&run.artifact_dir.join(path));
        assert_eq!(packet["schema"], "kiana.swarm-workpacket.v1");
        assert_eq!(packet["dispatch_id"], first.dispatch_id);
        assert_eq!(packet["workflow_id"], run.workflow_id);
        assert_eq!(packet["run_id"], run.run_id);
        assert_eq!(packet["budget"]["max_attempts"], 1);
        assert_eq!(packet["budget"]["max_commands"], 20);
        assert_eq!(packet["budget"]["timeout_seconds"], 1800);
        assert_eq!(packet["budget"]["max_output_bytes"], 10_485_760);
        assert!(packet["termination_policy"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "scope_violation"));
        assert!(!packet
            .to_string()
            .contains("kiana.swarm-workpacket-preview.v1"));
    }

    let events = read_workflow_events(&run.artifact_dir).unwrap();
    assert_eq!(events.len(), initial_event_count + 1);
    let event = events.last().unwrap();
    assert_eq!(event.kind, WorkflowEventKind::WorkPacketCreated);
    assert_eq!(event.data["dispatch_id"], first.dispatch_id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn swarm_dispatch_rejects_blocked_plan_without_artifacts_or_events() {
    let root = temp_root("blocked");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Do not persist a single-task swarm".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let plan = build_swarm_plan(&board(vec![task("only", 10, "src/only.rs")]), 2).unwrap();
    let initial_event_count = read_workflow_events(&run.artifact_dir).unwrap().len();

    let error =
        persist_swarm_dispatch(&run.artifact_dir, &plan, SwarmWorkerBudget::default()).unwrap_err();

    assert!(matches!(error, SwarmDispatchError::PlanNotReady(_)));
    assert_eq!(
        read_workflow_events(&run.artifact_dir).unwrap().len(),
        initial_event_count
    );
    assert_eq!(
        fs::read_dir(run.artifact_dir.join("workpackets"))
            .unwrap()
            .count(),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn swarm_dispatch_rejects_invalid_worker_budget() {
    let root = temp_root("budget");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Validate worker budget".to_string(),
            input_kind: WorkflowInputKind::Task,
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
    let budget = SwarmWorkerBudget {
        max_commands: 0,
        ..SwarmWorkerBudget::default()
    };

    let error = persist_swarm_dispatch(&run.artifact_dir, &plan, budget).unwrap_err();

    assert!(matches!(error, SwarmDispatchError::InvalidBudget(_)));
    assert_eq!(
        fs::read_dir(run.artifact_dir.join("workpackets"))
            .unwrap()
            .count(),
        0
    );

    let _ = fs::remove_dir_all(root);
}
