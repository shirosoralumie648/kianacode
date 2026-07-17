use kiana_tasks::{
    build_swarm_plan, ProjectBoardColumn, ProjectBoardProjection, ProjectBoardStatus,
    ProjectTaskCard,
};
use std::collections::BTreeMap;

fn task(id: &str, priority: i64, created_at: i64, allowed_paths: &[&str]) -> ProjectTaskCard {
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
        allowed_paths: allowed_paths
            .iter()
            .map(|path| (*path).to_string())
            .collect(),
        task_type: None,
        risk_level: None,
        approval_required: false,
        approval_status: None,
        created_at,
        updated_at: created_at,
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

#[test]
fn planner_dispatches_independent_ready_tasks_with_workpackets() {
    let plan = build_swarm_plan(
        &board(vec![
            task("api", 20, 1, &["src/api/**"]),
            task("docs", 10, 2, &["docs/guide.md"]),
        ]),
        2,
    )
    .unwrap();

    assert_eq!(plan.schema, "kiana.swarm-plan.v1");
    assert_eq!(plan.execution_mode, "plan_only");
    assert_eq!(plan.status, "ready");
    assert_eq!(plan.next_action, "run_swarm_dispatch");
    assert_eq!(plan.assignments.len(), 2);
    assert_eq!(plan.path_locks.len(), 2);
    assert_eq!(plan.summary.ready_tasks, 2);
    assert_eq!(plan.summary.dispatched, 2);
    assert!(plan.assignments.iter().all(|assignment| {
        assignment.worker_type == "builder"
            && assignment.workpacket.schema == "kiana.swarm-workpacket-preview.v1"
            && !assignment.workpacket.allowed_files.is_empty()
            && !assignment.workpacket.verification_commands.is_empty()
    }));
}

#[test]
fn planner_skips_conflicts_missing_scopes_and_invalid_paths() {
    let plan = build_swarm_plan(
        &board(vec![
            task("api", 30, 1, &["src/api/**"]),
            task("docs", 20, 2, &["docs/**"]),
            task("api-client", 10, 3, &["src/api/client.rs"]),
            task("missing", 9, 4, &[]),
            task("escape", 8, 5, &["../outside.rs"]),
        ]),
        5,
    )
    .unwrap();

    assert_eq!(plan.status, "ready");
    assert_eq!(plan.assignments.len(), 2);
    assert_eq!(plan.summary.skipped, 3);
    let conflict = plan
        .skipped
        .iter()
        .find(|item| item.task_id == "api-client")
        .unwrap();
    assert_eq!(conflict.reason, "path_conflict");
    assert_eq!(conflict.conflicts_with, vec!["api"]);
    assert_eq!(
        plan.skipped
            .iter()
            .find(|item| item.task_id == "missing")
            .unwrap()
            .reason,
        "missing_allowed_paths"
    );
    assert_eq!(
        plan.skipped
            .iter()
            .find(|item| item.task_id == "escape")
            .unwrap()
            .reason,
        "invalid_allowed_path"
    );
}

#[test]
fn planner_sorts_by_priority_and_applies_worker_limit() {
    let plan = build_swarm_plan(
        &board(vec![
            task("low", 1, 1, &["low/**"]),
            task("newer-high", 10, 3, &["newer/**"]),
            task("older-high", 10, 2, &["older/**"]),
        ]),
        2,
    )
    .unwrap();

    assert_eq!(
        plan.assignments
            .iter()
            .map(|item| item.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["older-high", "newer-high"]
    );
    let limited = plan
        .skipped
        .iter()
        .find(|item| item.task_id == "low")
        .unwrap();
    assert_eq!(limited.reason, "worker_limit");
}

#[test]
fn planner_blocks_when_fewer_than_two_tasks_are_dispatchable() {
    let plan = build_swarm_plan(&board(vec![task("only", 1, 1, &["src/only.rs"])]), 4).unwrap();

    assert_eq!(plan.status, "blocked");
    assert!(plan.assignments.is_empty());
    assert!(plan.path_locks.is_empty());
    assert_eq!(plan.next_action, "prepare_at_least_two_ready_tasks");
    assert_eq!(plan.skipped[0].reason, "insufficient_parallelism");
}

#[test]
fn planner_rejects_absolute_paths_and_marks_lockfiles_exclusive() {
    let plan = build_swarm_plan(
        &board(vec![
            task("lockfile", 30, 1, &["Cargo.lock"]),
            task("docs", 20, 2, &["docs/**"]),
            task("absolute", 10, 3, &["/tmp/outside.rs"]),
        ]),
        3,
    )
    .unwrap();

    assert_eq!(plan.status, "ready");
    assert_eq!(plan.assignments.len(), 2);
    assert_eq!(
        plan.path_locks
            .iter()
            .find(|lock| lock.path == "Cargo.lock")
            .unwrap()
            .mode,
        "exclusive"
    );
    assert_eq!(
        plan.skipped
            .iter()
            .find(|item| item.task_id == "absolute")
            .unwrap()
            .reason,
        "invalid_allowed_path"
    );
}

#[test]
fn planner_locks_globs_at_the_last_complete_directory() {
    let plan = build_swarm_plan(
        &board(vec![
            task("glob", 30, 1, &["src/foo*.rs"]),
            task("docs", 20, 2, &["docs/**"]),
            task("concrete", 10, 3, &["src/foobar.rs"]),
        ]),
        3,
    )
    .unwrap();

    assert_eq!(plan.status, "ready");
    assert_eq!(plan.assignments.len(), 2);
    assert!(plan
        .path_locks
        .iter()
        .any(|lock| lock.task_id == "glob" && lock.path == "src"));
    let skipped = plan
        .skipped
        .iter()
        .find(|item| item.task_id == "concrete")
        .unwrap();
    assert_eq!(skipped.reason, "path_conflict");
    assert_eq!(skipped.conflicts_with, vec!["glob"]);
}

#[test]
fn planner_normalizes_internal_current_directory_components() {
    let plan = build_swarm_plan(
        &board(vec![
            task("alias", 30, 1, &["src/./api/**"]),
            task("docs", 20, 2, &["docs/**"]),
            task("concrete", 10, 3, &["src/api/client.rs"]),
        ]),
        3,
    )
    .unwrap();

    assert_eq!(plan.assignments.len(), 2);
    assert!(plan
        .path_locks
        .iter()
        .any(|lock| lock.task_id == "alias" && lock.path == "src/api"));
    let skipped = plan
        .skipped
        .iter()
        .find(|item| item.task_id == "concrete")
        .unwrap();
    assert_eq!(skipped.reason, "path_conflict");
    assert_eq!(skipped.conflicts_with, vec!["alias"]);
}

#[test]
fn planner_rejects_worker_limits_below_parallelism_minimum() {
    let error = build_swarm_plan(
        &board(vec![
            task("api", 20, 1, &["src/api/**"]),
            task("docs", 10, 2, &["docs/**"]),
        ]),
        1,
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "max_workers must be between 2 and 32");
}
