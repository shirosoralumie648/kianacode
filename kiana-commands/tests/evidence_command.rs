use kiana_commands::evidence::EvidenceCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    append_evidence_event, append_workflow_event, build_verification_packet,
    initialize_local_hmac_key_at, initialize_workflow_run, read_evidence_events,
    write_verification_packet, EvidenceEvent, EvidenceEventDraft, EvidenceKind, EvidenceSource,
    EvidenceStatus, VerificationCheck, WorkflowEventKind, WorkflowInit, WorkflowInputKind,
    WorkflowProfile,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(label: &str) -> PathBuf {
    ensure_integrity_key();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-evidence-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn ensure_integrity_key() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let key_path = std::env::temp_dir()
            .join(format!(
                "kiana-evidence-integrity-key-{}",
                std::process::id()
            ))
            .join("workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", key_path);
    });
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]),
    }
}

fn workflow(root: &Path) -> kiana_tasks::WorkflowRun {
    initialize_workflow_run(
        root,
        WorkflowInit {
            request: "Inspect evidence".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap()
}

fn append_event(run: &kiana_tasks::WorkflowRun, task_id: Option<&str>) -> EvidenceEvent {
    append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: None,
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: task_id.map(str::to_string),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::TestResult,
            status: EvidenceStatus::Pass,
            summary: "targeted test passed".to_string(),
            source: EvidenceSource {
                source_type: "local_command".to_string(),
                name: "test fixture".to_string(),
                actor: Some("coordinator".to_string()),
            },
            payload: json!({"exit_code": 0, "command": "cargo test"}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: None,
            next_action: Some("review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap()
}

fn write_task(root: &Path, task_id: &str, evidence_reference: &str) {
    let dir = root.join(".kiana/tasks/default");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{task_id}.json")),
        serde_json::to_vec_pretty(&json!({
            "id": task_id,
            "title": task_id,
            "status": "completed",
            "blockedBy": [],
            "blocks": [],
            "metadata": {"evidence": [evidence_reference]}
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_packet(
    root: &Path,
    run: &kiana_tasks::WorkflowRun,
    event: &EvidenceEvent,
    task_id: &str,
) -> String {
    let events = read_evidence_events(&run.artifact_dir).unwrap();
    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        Some(task_id.to_string()),
        "project_p0",
        vec![VerificationCheck {
            check_id: "targeted-test".to_string(),
            description: "targeted test".to_string(),
            required: true,
            status: EvidenceStatus::Pass,
            evidence_id: Some(event.event_id.clone()),
            command: Some("cargo test".to_string()),
            reason: None,
        }],
        &events,
    )
    .unwrap();
    let path = write_verification_packet(&run.artifact_dir, &packet).unwrap();
    let relative = path.strip_prefix(root).unwrap().to_string_lossy();
    format!("verification:{relative}#{}", packet.verification_id)
}

#[tokio::test]
async fn evidence_requires_an_existing_workflow() {
    let root = temp_root("missing-workflow");
    std::fs::create_dir_all(&root).unwrap();

    let error = EvidenceCommand
        .execute(context("list --json", &root))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("workflow_not_found"),
        "unexpected error: {error}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_list_uses_latest_run_and_filters_by_task() {
    let root = temp_root("list");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let selected = append_event(&run, Some("task-1"));
    append_event(&run, Some("task-2"));

    let result = EvidenceCommand
        .execute(context("list --json --task task-1", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["schema"], "kiana.evidence-list.v1");
    assert_eq!(report["run_id"], run.run_id);
    assert_eq!(report["count"], 1);
    assert_eq!(report["events"][0]["event_id"], selected.event_id);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_show_returns_one_exact_event() {
    let root = temp_root("show");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));

    let result = EvidenceCommand
        .execute(context(
            format!("show --json --workflow {} {}", run.run_id, event.event_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["schema"], "kiana.evidence-show.v1");
    assert_eq!(report["event"]["event_id"], event.event_id);
    assert_eq!(report["event"]["task_id"], "task-1");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_checks_packet_evidence_and_task_references_without_rerun() {
    let root = temp_root("verify-pass");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));
    let reference = write_packet(&root, &run, &event, "task-1");
    write_task(&root, "task-1", &reference);

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["schema"], "kiana.evidence-integrity.v1");
    assert_eq!(report["status"], "pass");
    assert_eq!(report["event_count"], 1);
    assert_eq!(report["packet_count"], 1);
    assert_eq!(report["findings"].as_array().unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_repairs_lagging_state_before_integrity_checks() {
    let root = temp_root("verify-repair-lagging-state");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));
    let reference = write_packet(&root, &run, &event, "task-1");
    write_task(&root, "task-1", &reference);
    let state_path = run.artifact_dir.join("state.json");
    let stale_state = std::fs::read(&state_path).unwrap();
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::NodeEntered,
        "product_definition",
        json!({"reason": "simulate EventLog commit before state projection"}),
    )
    .unwrap();
    std::fs::write(&state_path, stale_state).unwrap();

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let repaired_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();

    assert_eq!(report["status"], "pass");
    assert_eq!(report["findings"].as_array().unwrap().len(), 0);
    assert_eq!(repaired_state["current_node"], "product_definition");
    assert!(repaired_state["last_event_seq"].as_u64().unwrap() >= 3);

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_rejects_forged_pass_packet_with_existing_references() {
    let root = temp_root("verify-forged-pass");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));
    let reference = write_packet(&root, &run, &event, "task-1");
    write_task(&root, "task-1", &reference);

    let packet_path = std::fs::read_dir(run.artifact_dir.join("verification"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut packet: Value =
        serde_json::from_str(&std::fs::read_to_string(&packet_path).unwrap()).unwrap();
    packet["pass_count"] = json!(99);
    std::fs::write(&packet_path, serde_json::to_vec_pretty(&packet).unwrap()).unwrap();

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "verification_packet_invalid"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_rejects_authenticated_packet_content_mutation() {
    let root = temp_root("verify-authenticated-mutation");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));
    let reference = write_packet(&root, &run, &event, "task-1");
    write_task(&root, "task-1", &reference);

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

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "workflow_integrity_invalid"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_blocks_missing_task_reference() {
    let root = temp_root("verify-missing-task");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("missing-task"));
    write_packet(&root, &run, &event, "missing-task");

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "missing_task_reference"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_detects_missing_check_evidence_reference() {
    let root = temp_root("verify-check-reference");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));
    let reference = write_packet(&root, &run, &event, "task-1");
    write_task(&root, "task-1", &reference);
    let packet_path = std::fs::read_dir(run.artifact_dir.join("verification"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut packet: Value =
        serde_json::from_str(&std::fs::read_to_string(&packet_path).unwrap()).unwrap();
    packet["checks"][0]["evidence_id"] = json!("missing-event");
    std::fs::write(&packet_path, serde_json::to_vec_pretty(&packet).unwrap()).unwrap();

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "missing_check_evidence_reference"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn evidence_verify_requires_exact_packet_path_and_id_in_task_link() {
    let root = temp_root("verify-task-link");
    std::fs::create_dir_all(&root).unwrap();
    let run = workflow(&root);
    let event = append_event(&run, Some("task-1"));
    let reference = write_packet(&root, &run, &event, "task-1");
    let verification_id = reference.rsplit_once('#').unwrap().1;
    write_task(
        &root,
        "task-1",
        &format!(
            "verification:.kiana/workflows/wrong/verification/{verification_id}.json#{verification_id}"
        ),
    );

    let result = EvidenceCommand
        .execute(context(
            format!("verify --json --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "missing_packet_task_link"));
    let _ = std::fs::remove_dir_all(root);
}
