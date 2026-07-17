use kiana_commands::report::ReportCommand;
use kiana_commands::validate::ValidateCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    append_evidence_event, build_verification_packet, initialize_local_hmac_key_at,
    initialize_workflow_run, EvidenceEventDraft, EvidenceKind, EvidenceSource, EvidenceStatus,
    VerificationCheck, WorkflowInit, WorkflowInputKind, WorkflowProfile,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]),
    }
}

fn root() -> PathBuf {
    ensure_integrity_key();
    std::env::temp_dir().join(format!(
        "kiana-report-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn ensure_integrity_key() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let key_path = std::env::temp_dir()
            .join(format!("kiana-report-integrity-key-{}", std::process::id()))
            .join("workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", key_path);
    });
}

#[tokio::test]
async fn report_progress_joins_workflow_board_evidence_and_latest_packet() {
    let root = root();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("scripts/release-smoke.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::create_dir_all(root.join(".kiana/tasks/default")).unwrap();
    std::fs::write(
        root.join(".kiana/tasks/default/task-1.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "task-1", "title": "Task one", "status": "completed",
            "blockedBy": [], "blocks": [], "created_at": 1, "updated_at": 2,
            "metadata": {"verification_commands": ["bash scripts/release-smoke.sh"]}
        }))
        .unwrap(),
    )
    .unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Report progress".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    ValidateCommand
        .execute(context(
            format!("--json --workflow {} --task task-1", run.run_id),
            &root,
        ))
        .await
        .unwrap();

    let result = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["schema"], "kiana.progress-report.v1");
    assert_eq!(report["workflow"]["run_id"], run.run_id);
    assert_eq!(report["board"]["counts"]["done"], 1);
    assert_eq!(report["evidence"]["event_count"], 1);
    assert_eq!(report["latest_verification"]["final_status"], "pass");
    assert_eq!(report["blockers"].as_array().unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_rejects_authenticated_packet_content_mutation() {
    let root = root();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("scripts/release-smoke.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::create_dir_all(root.join(".kiana/tasks/default")).unwrap();
    std::fs::write(
        root.join(".kiana/tasks/default/task-1.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "task-1", "title": "Task one", "status": "completed",
            "blockedBy": [], "blocks": [], "created_at": 1, "updated_at": 2,
            "metadata": {"verification_commands": ["bash scripts/release-smoke.sh"]}
        }))
        .unwrap(),
    )
    .unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Report packet mutation".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    ValidateCommand
        .execute(context(
            format!("--json --workflow {} --task task-1", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let packet_path = std::fs::read_dir(run.artifact_dir.join("verification"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut packet: Value =
        serde_json::from_str(&std::fs::read_to_string(&packet_path).unwrap()).unwrap();
    let created_at_ms = packet["created_at_ms"].as_u64().unwrap();
    packet["created_at_ms"] = json!(created_at_ms + 1);
    std::fs::write(&packet_path, serde_json::to_vec_pretty(&packet).unwrap()).unwrap();

    let result = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert!(report["latest_verification"].is_null());
    assert!(report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker["code"] == "workflow_integrity_invalid"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_rejects_orphan_verification_packet() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "orphan report packet".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let mut packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![VerificationCheck {
            check_id: "external".to_string(),
            description: "external".to_string(),
            required: true,
            status: EvidenceStatus::Skipped,
            evidence_id: None,
            command: Some("external-check".to_string()),
            reason: Some("credential unavailable".to_string()),
        }],
        &[],
    )
    .unwrap();
    packet.verification_id = "vp_orphan_report".to_string();
    std::fs::write(
        run.artifact_dir
            .join("verification")
            .join("vp_orphan_report.json"),
        serde_json::to_vec_pretty(&packet).unwrap(),
    )
    .unwrap();

    let result = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert!(report["latest_verification"].is_null());
    assert!(report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker["code"] == "verification_packet_orphaned"));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_projects_only_unresolved_blocking_evidence() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "blocking evidence report".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let blocker = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_report_blocker".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: Some("task-report".to_string()),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::BlockerRecord,
            status: EvidenceStatus::Blocked,
            summary: "release credential missing".to_string(),
            source: EvidenceSource {
                source_type: "manual".to_string(),
                name: "fixture".to_string(),
                actor: Some("reviewer".to_string()),
            },
            payload: json!({"reason": "credential missing"}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: Some("block".to_string()),
            next_action: Some("provide_credential".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();

    let result = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert!(report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["code"] == "evidence_blocker_unresolved"));

    append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_report_resolved".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: Some("task-report".to_string()),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::BlockerRecord,
            status: EvidenceStatus::Pass,
            summary: "release credential supplied".to_string(),
            source: EvidenceSource {
                source_type: "manual".to_string(),
                name: "fixture".to_string(),
                actor: Some("reviewer".to_string()),
            },
            payload: json!({}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: None,
            next_action: Some("continue".to_string()),
            supersedes_event_id: Some(blocker.event_id),
        },
    )
    .unwrap();
    let result = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert!(!report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["code"] == "evidence_blocker_unresolved"));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_requires_workflow() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let error = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("workflow_not_found"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_exposes_missing_verification_as_blocker() {
    let root = root();
    std::fs::create_dir_all(root.join(".kiana/tasks/default")).unwrap();
    std::fs::write(
        root.join(".kiana/tasks/default/task-1.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "task-1", "title": "Unverified completion", "status": "completed",
            "blockedBy": [], "blocks": [], "created_at": 1, "updated_at": 2,
            "metadata": {"verification_commands": ["cargo test"]}
        }))
        .unwrap(),
    )
    .unwrap();
    initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Report blockers".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();

    let result = ReportCommand
        .execute(context("progress --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["board"]["counts"]["blocked"], 1);
    assert!(!report["blockers"].as_array().unwrap().is_empty());
    assert_eq!(report["blockers"][0]["task_id"], "task-1");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_text_is_concise_chinese() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Chinese progress report".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();

    let result = ReportCommand
        .execute(context("progress", &root))
        .await
        .unwrap();
    assert!(result.value.starts_with("项目进度报告\n"));
    assert!(result.value.contains("下一步："));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_surfaces_corrupt_eventlog_as_blocker() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Corrupt evidence report".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    use std::io::Write;
    let mut eventlog = std::fs::OpenOptions::new()
        .append(true)
        .open(run.artifact_dir.join("eventlog.jsonl"))
        .unwrap();
    writeln!(eventlog, "{{not-json}}").unwrap();

    let result = ReportCommand
        .execute(context(
            format!("progress --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["workflow"]["run_id"], run.run_id);
    assert_eq!(report["evidence"]["status"], "blocked");
    assert!(report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "evidence_ledger_unreadable"));
    assert_eq!(report["next_action"], "repair:reconcile-eventlog");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn report_progress_surfaces_recovery_integrity_as_blocker() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Recovery integrity report".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let recovery_dir = run
        .artifact_dir
        .join("integrations/integration-test/recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    std::fs::write(recovery_dir.join("journal.json"), br#"{"schema":"legacy"}"#).unwrap();

    let result = ReportCommand
        .execute(context(
            format!("progress --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert!(report["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker["code"] == "workflow_recovery_integrity"));

    let _ = std::fs::remove_dir_all(root);
}
