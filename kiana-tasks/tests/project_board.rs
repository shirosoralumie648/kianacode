use kiana_tasks::{
    append_evidence_event, build_project_board, build_project_board_at_root,
    build_verification_packet, initialize_local_hmac_key_at, initialize_workflow_run,
    read_evidence_events, select_next_project_task,
    write_verification_packet as persist_verification_packet, EvidenceEventDraft, EvidenceKind,
    EvidenceSource, EvidenceStatus, ProjectBoardProjection, ProjectBoardStatus, ProjectTaskCard,
    VerificationCheck, WorkflowInit, WorkflowInputKind, WorkflowProfile,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> PathBuf {
    ensure_integrity_key();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-project-board-{}-{unique}",
        std::process::id()
    ))
}

fn ensure_integrity_key() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let key_path = std::env::temp_dir()
            .join(format!(
                "kiana-project-board-integrity-key-{}",
                std::process::id()
            ))
            .join("workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", key_path);
    });
}

fn write_verification_packet(
    root: &Path,
    run_id: &str,
    file_id: &str,
    packet_id: &str,
    task_id: Option<&str>,
    final_status: &str,
) -> String {
    std::fs::create_dir_all(root).unwrap();
    let run = initialize_workflow_run(
        root,
        WorkflowInit {
            request: format!("board fixture {run_id}"),
            input_kind: WorkflowInputKind::Qa,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let status = if final_status == "pass" {
        EvidenceStatus::Pass
    } else {
        EvidenceStatus::Fail
    };
    let event = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: None,
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: task_id.map(str::to_string),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::TestResult,
            status,
            summary: "board fixture".to_string(),
            source: EvidenceSource {
                source_type: "local_command".to_string(),
                name: "fixture".to_string(),
                actor: Some("coordinator".to_string()),
            },
            payload: json!({"exit_code": if status == EvidenceStatus::Pass { 0 } else { 1 }}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: None,
            next_action: Some("review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    let events = read_evidence_events(&run.artifact_dir).unwrap();
    let mut packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        task_id.map(str::to_string),
        "project_p0",
        vec![VerificationCheck {
            check_id: "fixture".to_string(),
            description: "fixture".to_string(),
            required: true,
            status,
            evidence_id: Some(event.event_id),
            command: Some("cargo test".to_string()),
            reason: None,
        }],
        &events,
    )
    .unwrap();
    packet.verification_id = packet_id.to_string();
    let written = persist_verification_packet(&run.artifact_dir, &packet).unwrap();
    let relative = PathBuf::from(".kiana")
        .join("workflows")
        .join(&run.run_id)
        .join("verification")
        .join(format!("{file_id}.json"));
    let path = root.join(&relative);
    if written != path {
        std::fs::rename(written, &path).unwrap();
    }
    format!("verification:{}#{file_id}", relative.display())
}

fn task(
    id: &str,
    status: &str,
    blocked_by: &[&str],
    blocks: &[&str],
    metadata: Value,
    updated_at: i64,
) -> Value {
    json!({
        "id": id,
        "title": format!("Task {id}"),
        "status": status,
        "blockedBy": blocked_by,
        "blocks": blocks,
        "created_at": 1,
        "updated_at": updated_at,
        "metadata": metadata,
    })
}

fn card<'a>(board: &'a ProjectBoardProjection, task_id: &str) -> &'a ProjectTaskCard {
    board
        .columns
        .iter()
        .flat_map(|column| column.tasks.iter())
        .find(|card| card.task_id == task_id)
        .unwrap_or_else(|| panic!("missing task card {task_id}"))
}

#[test]
fn board_projects_ready_blocked_and_evidence_guarded_done_states() {
    let tasks = vec![
        task(
            "dependency",
            "pending",
            &[],
            &["dependent"],
            json!({"verification_commands": ["cargo test -p dependency"]}),
            10,
        ),
        task(
            "dependent",
            "pending",
            &["dependency"],
            &[],
            json!({"verification_commands": ["cargo test -p dependent"]}),
            20,
        ),
        task("missing-evidence", "completed", &[], &[], json!({}), 30),
        task(
            "verified-done",
            "completed",
            &[],
            &[],
            json!({"evidence": ["cargo test --workspace:pass"]}),
            40,
        ),
        task(
            "ready-task",
            "pending",
            &[],
            &[],
            json!({"verification_commands": ["cargo test -p ready"]}),
            50,
        ),
    ];

    let board = build_project_board("default", &tasks).unwrap();

    assert_eq!(
        card(&board, "dependency").board_status,
        ProjectBoardStatus::Ready
    );
    assert_eq!(
        card(&board, "dependent").board_status,
        ProjectBoardStatus::Blocked
    );
    assert_eq!(
        card(&board, "missing-evidence").board_status,
        ProjectBoardStatus::Blocked
    );
    assert!(card(&board, "missing-evidence")
        .policy_findings
        .iter()
        .any(|finding| finding.code == "completed_without_evidence"));
    assert_eq!(
        card(&board, "missing-evidence")
            .unblock_condition
            .as_deref(),
        Some("attach a passing VerificationPacket reference")
    );
    assert_eq!(
        card(&board, "verified-done").board_status,
        ProjectBoardStatus::Blocked
    );
    assert!(card(&board, "verified-done")
        .policy_findings
        .iter()
        .any(|finding| finding.code == "legacy_evidence_requires_verification_packet"));
    assert_eq!(
        card(&board, "ready-task").board_status,
        ProjectBoardStatus::Ready
    );
    assert_eq!(board.columns.len(), 8);
}

#[test]
fn next_task_selection_is_deterministic_and_explainable() {
    let tasks = vec![
        task(
            "older-low-fanout",
            "pending",
            &[],
            &["one"],
            json!({
                "priority": 10,
                "verification_commands": ["cargo test -p older"]
            }),
            10,
        ),
        task(
            "high-fanout",
            "pending",
            &[],
            &["one", "two"],
            json!({
                "priority": 10,
                "verification_commands": ["cargo test -p fanout"]
            }),
            20,
        ),
        task(
            "lower-priority",
            "pending",
            &[],
            &["one", "two", "three"],
            json!({
                "priority": 9,
                "verification_commands": ["cargo test -p lower"]
            }),
            1,
        ),
    ];

    let board = build_project_board("default", &tasks).unwrap();
    let report = select_next_project_task(&board);

    assert_eq!(report.schema, "kiana.project-next.v1");
    assert_eq!(
        report.selected_task.as_ref().unwrap().task_id,
        "high-fanout"
    );
    assert!(report.why.iter().any(|reason| reason == "priority=10"));
    assert!(report.why.iter().any(|reason| reason == "unblocks=2"));
    assert!(report.why.iter().any(|reason| reason == "updated_at=20"));
    assert!(report
        .why
        .iter()
        .any(|reason| reason == "task_id=high-fanout"));
    assert_eq!(report.alternatives.len(), 2);
}

#[test]
fn next_task_reports_blocked_and_backlog_when_nothing_is_ready() {
    let tasks = vec![
        task("backlog", "pending", &[], &[], json!({}), 10),
        task(
            "failed",
            "failed",
            &[],
            &[],
            json!({"blocker_reason": "targeted test failed"}),
            20,
        ),
    ];

    let board = build_project_board("default", &tasks).unwrap();
    let report = select_next_project_task(&board);

    assert!(report.selected_task.is_none());
    assert_eq!(report.blocked_summary["blocked"], 1);
    assert_eq!(report.blocked_summary["backlog"], 1);
    assert_eq!(report.primary_blockers[0].code, "explicit_blocker");
    assert_eq!(report.primary_blockers[0].count, 1);
    assert_eq!(report.primary_blockers[0].task_ids, vec!["failed"]);
}

#[test]
fn completed_dependency_with_legacy_evidence_does_not_unblock_ready_task() {
    let tasks = vec![
        task(
            "dependency",
            "completed",
            &[],
            &["dependent"],
            json!({"evidence": ["verification:pass"]}),
            10,
        ),
        task(
            "dependent",
            "pending",
            &["dependency"],
            &[],
            json!({"verification_commands": ["cargo test -p dependent"]}),
            20,
        ),
    ];

    let board = build_project_board("default", &tasks).unwrap();

    assert_eq!(
        card(&board, "dependency").board_status,
        ProjectBoardStatus::Blocked
    );
    assert_eq!(
        card(&board, "dependent").board_status,
        ProjectBoardStatus::Blocked
    );
}

#[test]
fn completed_task_requires_matching_passing_verification_packet() {
    let root = temp_root();
    let evidence = write_verification_packet(
        &root,
        "run-pass",
        "verify-pass",
        "verify-pass",
        Some("verified-done"),
        "pass",
    );
    let tasks = vec![task(
        "verified-done",
        "completed",
        &[],
        &[],
        json!({"evidence": [evidence]}),
        10,
    )];

    let board = build_project_board_at_root(&root, "default", &tasks).unwrap();

    assert_eq!(
        card(&board, "verified-done").board_status,
        ProjectBoardStatus::Done
    );
    assert!(card(&board, "verified-done").policy_findings.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn completed_task_rejects_packet_modified_after_completion_event() {
    let root = temp_root();
    let evidence = write_verification_packet(
        &root,
        "run-mutated",
        "verify-mutated",
        "verify-mutated",
        Some("mutated-done"),
        "pass",
    );
    let packet_path = root.join(
        evidence
            .strip_prefix("verification:")
            .unwrap()
            .rsplit_once('#')
            .unwrap()
            .0,
    );
    let mut packet: Value =
        serde_json::from_str(&std::fs::read_to_string(&packet_path).unwrap()).unwrap();
    let created_at_ms = packet["created_at_ms"].as_u64().unwrap();
    packet["created_at_ms"] = json!(created_at_ms + 1);
    std::fs::write(&packet_path, serde_json::to_vec_pretty(&packet).unwrap()).unwrap();
    let tasks = vec![task(
        "mutated-done",
        "completed",
        &[],
        &[],
        json!({"evidence": [evidence]}),
        10,
    )];

    let board = build_project_board_at_root(&root, "default", &tasks).unwrap();
    let task = card(&board, "mutated-done");

    assert_eq!(task.board_status, ProjectBoardStatus::Blocked);
    assert!(task
        .policy_findings
        .iter()
        .any(|finding| finding.code == "verification_packet_integrity_invalid"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_verification_packets_block_completion_with_actionable_findings() {
    let root = temp_root();
    let id_mismatch = write_verification_packet(
        &root,
        "run-id",
        "expected-id",
        "actual-id",
        Some("id-mismatch"),
        "pass",
    );
    let task_mismatch = write_verification_packet(
        &root,
        "run-task",
        "task-mismatch",
        "task-mismatch",
        Some("another-task"),
        "pass",
    );
    let failed = write_verification_packet(
        &root,
        "run-fail",
        "failed-packet",
        "failed-packet",
        Some("failed-verification"),
        "fail",
    );
    let tasks = vec![
        task(
            "id-mismatch",
            "completed",
            &[],
            &[],
            json!({"evidence": [id_mismatch]}),
            10,
        ),
        task(
            "task-mismatch",
            "completed",
            &[],
            &[],
            json!({"evidence": [task_mismatch]}),
            20,
        ),
        task(
            "failed-verification",
            "completed",
            &[],
            &[],
            json!({"evidence": [failed]}),
            30,
        ),
        task(
            "missing-packet",
            "completed",
            &[],
            &[],
            json!({
                "evidence": ["verification:.kiana/workflows/missing/verification/nope.json#nope"]
            }),
            40,
        ),
        task(
            "path-traversal",
            "completed",
            &[],
            &[],
            json!({"evidence": ["verification:../outside.json#outside"]}),
            50,
        ),
    ];

    let board = build_project_board_at_root(&root, "default", &tasks).unwrap();

    for (task_id, finding_code) in [
        ("id-mismatch", "verification_id_mismatch"),
        ("task-mismatch", "verification_task_mismatch"),
        ("failed-verification", "verification_not_passing"),
        ("missing-packet", "verification_packet_unreadable"),
        ("path-traversal", "invalid_verification_reference"),
    ] {
        let task = card(&board, task_id);
        assert_eq!(task.board_status, ProjectBoardStatus::Blocked);
        assert!(task
            .policy_findings
            .iter()
            .any(|finding| finding.code == finding_code));
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn completed_dependency_only_unblocks_after_strict_verification() {
    let root = temp_root();
    let evidence = write_verification_packet(
        &root,
        "run-dependency",
        "dependency-pass",
        "dependency-pass",
        Some("dependency"),
        "pass",
    );
    let tasks = vec![
        task(
            "dependency",
            "completed",
            &[],
            &["dependent"],
            json!({"evidence": [evidence]}),
            10,
        ),
        task(
            "dependent",
            "pending",
            &["dependency"],
            &[],
            json!({"verification_commands": ["cargo test -p dependent"]}),
            20,
        ),
    ];

    let board = build_project_board_at_root(&root, "default", &tasks).unwrap();

    assert_eq!(
        card(&board, "dependency").board_status,
        ProjectBoardStatus::Done
    );
    assert_eq!(
        card(&board, "dependent").board_status,
        ProjectBoardStatus::Ready
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn orphan_verification_packet_cannot_mark_completed_task_done() {
    let root = temp_root();
    std::fs::create_dir_all(&root).unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "orphan packet fixture".to_string(),
            input_kind: WorkflowInputKind::Qa,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let event = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_orphan_packet".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: Some("orphan-task".to_string()),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::TestResult,
            status: EvidenceStatus::Pass,
            summary: "orphan fixture passed".to_string(),
            source: EvidenceSource {
                source_type: "local_command".to_string(),
                name: "fixture".to_string(),
                actor: Some("coordinator".to_string()),
            },
            payload: json!({"exit_code": 0}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: None,
            next_action: Some("review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    let events = read_evidence_events(&run.artifact_dir).unwrap();
    let mut packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        Some("orphan-task".to_string()),
        "project_p0",
        vec![VerificationCheck {
            check_id: "fixture".to_string(),
            description: "fixture".to_string(),
            required: true,
            status: EvidenceStatus::Pass,
            evidence_id: Some(event.event_id),
            command: Some("cargo test".to_string()),
            reason: None,
        }],
        &events,
    )
    .unwrap();
    packet.verification_id = "vp_orphan".to_string();
    let relative = PathBuf::from(".kiana")
        .join("workflows")
        .join(&run.run_id)
        .join("verification")
        .join("vp_orphan.json");
    std::fs::write(
        root.join(&relative),
        serde_json::to_vec_pretty(&packet).unwrap(),
    )
    .unwrap();
    let tasks = vec![task(
        "orphan-task",
        "completed",
        &[],
        &[],
        json!({
            "evidence": [format!("verification:{}#vp_orphan", relative.display())]
        }),
        10,
    )];

    let board = build_project_board_at_root(&root, "default", &tasks).unwrap();
    let task = card(&board, "orphan-task");
    assert_eq!(task.board_status, ProjectBoardStatus::Blocked);
    assert!(task
        .policy_findings
        .iter()
        .any(|finding| finding.code == "verification_completion_missing"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_task_explains_updated_at_and_task_id_tie_breaks() {
    let tasks = vec![
        task(
            "older",
            "pending",
            &[],
            &["one"],
            json!({
                "priority": 10,
                "verification_commands": ["cargo test -p older"]
            }),
            10,
        ),
        task(
            "newer",
            "pending",
            &[],
            &["one"],
            json!({
                "priority": 10,
                "verification_commands": ["cargo test -p newer"]
            }),
            20,
        ),
        task(
            "zeta",
            "pending",
            &[],
            &["one"],
            json!({
                "priority": 10,
                "verification_commands": ["cargo test -p zeta"]
            }),
            10,
        ),
    ];

    let board = build_project_board("default", &tasks).unwrap();
    let report = select_next_project_task(&board);

    assert_eq!(report.selected_task.as_ref().unwrap().task_id, "older");
    assert!(report
        .alternatives
        .iter()
        .any(|alternative| alternative.task_id == "newer"
            && alternative.why_not.contains("newer_update")));
    assert!(report
        .alternatives
        .iter()
        .any(|alternative| alternative.task_id == "zeta"
            && alternative.why_not.contains("task_id_tiebreak")));
}

#[test]
fn malformed_optional_fields_are_rejected_instead_of_silently_defaulted() {
    let invalid_board_status = vec![task(
        "invalid-board-status",
        "pending",
        &[],
        &[],
        json!({
            "board_status": 7,
            "verification_commands": ["cargo test"]
        }),
        1,
    )];
    let error = build_project_board("default", &invalid_board_status)
        .unwrap_err()
        .to_string();
    assert!(error.contains("board_status"));

    let invalid_priority = vec![task(
        "invalid-priority",
        "pending",
        &[],
        &[],
        json!({
            "priority": "high",
            "verification_commands": ["cargo test"]
        }),
        1,
    )];
    let error = build_project_board("default", &invalid_priority)
        .unwrap_err()
        .to_string();
    assert!(error.contains("priority"));
}

#[test]
fn policy_findings_are_stable_across_input_order() {
    let missing_evidence = task("b", "completed", &[], &[], json!({}), 2);
    let backlog = task("a", "pending", &[], &[], json!({}), 1);

    let first =
        build_project_board("default", &[missing_evidence.clone(), backlog.clone()]).unwrap();
    let second = build_project_board("default", &[backlog, missing_evidence]).unwrap();

    assert_eq!(first.policy_findings, second.policy_findings);
}

#[test]
fn empty_subject_falls_back_to_non_empty_title_alias() {
    let tasks = vec![json!({
        "id": "alias",
        "subject": "",
        "title": "Valid title",
        "status": "pending",
        "blocks": [],
        "blockedBy": [],
        "created_at": 1,
        "updated_at": 1,
        "metadata": {"verification_commands": ["cargo test"]}
    })];

    let board = build_project_board("default", &tasks).unwrap();

    assert_eq!(card(&board, "alias").title, "Valid title");
}
