use kiana_commands::tasks::TasksCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    append_evidence_event, append_workflow_event, build_verification_packet, read_evidence_events,
    write_verification_packet, EvidenceEventDraft, EvidenceKind, EvidenceSource, EvidenceStatus,
    VerificationCheck, WorkflowEventKind,
};
use serde_json::{json, Value};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-workflow-transition-{label}-{}-{}",
        std::process::id(),
        ROOT_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy()))]),
    }
}

async fn run(args: impl Into<String>, root: &Path) -> anyhow::Result<String> {
    Ok(TasksCommand.execute(context(args, root)).await?.value)
}

async fn create_run(root: &Path) -> Value {
    std::fs::create_dir_all(root).unwrap();
    let output = run(
        "workflow init --json --type ship Transition command test",
        root,
    )
    .await
    .unwrap();
    serde_json::from_str(&output).unwrap()
}

fn artifact_dir(run: &Value) -> PathBuf {
    PathBuf::from(run["artifact_dir"].as_str().unwrap())
}

fn write_real_evidence(run: &Value, relative: &str) {
    let path = artifact_dir(run).join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(
        path,
        "# Verified evidence\n\nDecision and acceptance evidence captured from the real run.\n",
    )
    .unwrap();
}

fn passing_packet(run: &Value) -> String {
    packet_with_status(run, "evt_workflow_completion_pass", EvidenceStatus::Pass)
}

fn packet_with_status(run: &Value, event_id: &str, status: EvidenceStatus) -> String {
    let artifact_dir = artifact_dir(run);
    let workflow_id = run["workflow_id"].as_str().unwrap();
    let run_id = run["run_id"].as_str().unwrap();
    let evidence = append_evidence_event(
        &artifact_dir,
        EvidenceEventDraft {
            event_id: Some(event_id.to_string()),
            workflow_id: workflow_id.to_string(),
            run_id: run_id.to_string(),
            task_id: None,
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::TestResult,
            status,
            summary: format!("workflow completion check is {status:?}"),
            source: EvidenceSource {
                source_type: "local_command".to_string(),
                name: "cargo test --workspace".to_string(),
                actor: Some("coordinator".to_string()),
            },
            payload: json!({
                "check_id": "workspace_tests",
                "command": "cargo test --workspace",
                "exit_code": if status == EvidenceStatus::Pass { 0 } else { 1 },
                "stdout": format!("{status:?}"),
                "stderr": ""
            }),
            changed_files: Vec::new(),
            confidence: Some(1.0),
            severity: None,
            next_action: Some("review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    let ledger = read_evidence_events(&artifact_dir).unwrap();
    let packet = build_verification_packet(
        workflow_id,
        run_id,
        None,
        "project_p0",
        vec![VerificationCheck {
            check_id: "workspace_tests".to_string(),
            description: "workspace tests pass".to_string(),
            required: true,
            status,
            evidence_id: Some(evidence.event_id),
            command: Some("cargo test --workspace".to_string()),
            reason: None,
        }],
        &ledger,
    )
    .unwrap();
    let verification_id = packet.verification_id.clone();
    write_verification_packet(&artifact_dir, &packet).unwrap();
    verification_id
}

#[tokio::test]
async fn workflow_advance_records_evidence_bound_dag_transition() {
    let root = root("advance");
    let created = create_run(&root).await;
    write_real_evidence(&created, "problem-definition.md");
    let run_id = created["run_id"].as_str().unwrap();

    let output = run(
        format!(
            "workflow advance --json --decision clear --evidence problem-definition.md {run_id}"
        ),
        &root,
    )
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();

    assert_eq!(report["schema"], "kiana.workflow-transition.v1");
    assert_eq!(report["from"], "capture");
    assert_eq!(report["to"], "product_definition");
    assert_eq!(report["decision"], "clear");
    assert_eq!(report["current_node"], "product_definition");
    assert_eq!(report["status"], "running");
    assert_eq!(report["evidence"][0]["path"], "problem-definition.md");
    assert_eq!(report["evidence"][0]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(report["last_event_seq"].as_u64().unwrap(), 5);

    let events = kiana_tasks::read_workflow_events(artifact_dir(&created)).unwrap();
    assert_eq!(events[2].kind, WorkflowEventKind::GateEvaluated);
    assert_eq!(events[3].kind, WorkflowEventKind::NodeExited);
    assert_eq!(events[4].kind, WorkflowEventKind::NodeEntered);
    assert_eq!(
        events[2].data["transition_id"],
        events[4].data["transition_id"]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn workflow_advance_rejects_invalid_decisions_and_evidence() {
    let root = root("invalid");
    let created = create_run(&root).await;
    let run_id = created["run_id"].as_str().unwrap();
    std::fs::create_dir_all(artifact_dir(&created).join("evidence-dir")).unwrap();

    for (args, expected) in [
        (
            format!(
                "workflow advance --decision missing --evidence problem-definition.md {run_id}"
            ),
            "allowed decisions",
        ),
        (
            format!("workflow advance --decision clear {run_id}"),
            "requires at least one evidence",
        ),
        (
            format!("workflow advance --decision clear --evidence ../outside {run_id}"),
            "outside workflow artifact directory",
        ),
        (
            format!("workflow advance --decision clear --evidence evidence-dir {run_id}"),
            "regular file",
        ),
        (
            format!("workflow advance --decision clear --evidence problem-definition.md {run_id}"),
            "placeholder",
        ),
    ] {
        let error = run(args, &root).await.unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn workflow_advance_rejects_absolute_symlink_and_duplicate_scalar_options() {
    let root = root("path-boundaries");
    let created = create_run(&root).await;
    let run_id = created["run_id"].as_str().unwrap();
    let outside = root.join("outside.md");
    std::fs::write(&outside, "verified outside content").unwrap();

    let error = run(
        format!(
            "workflow advance --decision clear --evidence {} {run_id}",
            outside.display()
        ),
        &root,
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("outside workflow artifact directory"),
        "{error:#}"
    );

    #[cfg(unix)]
    {
        symlink(&outside, artifact_dir(&created).join("escape.md")).unwrap();
        let error = run(
            format!("workflow advance --decision clear --evidence escape.md {run_id}"),
            &root,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("regular file"), "{error:#}");
    }

    write_real_evidence(&created, "problem-definition.md");
    for args in [
        format!(
            "workflow advance --decision clear --decision unclear --evidence problem-definition.md {run_id}"
        ),
        format!(
            "workflow advance --decision clear --evidence problem-definition.md --note first --note second {run_id}"
        ),
    ] {
        let error = run(args, &root).await.unwrap_err();
        assert!(error.to_string().contains("duplicate option"), "{error:#}");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn workflow_complete_requires_valid_same_run_pass_packet_and_is_idempotent() {
    let root = root("complete");
    let created = create_run(&root).await;
    let run_id = created["run_id"].as_str().unwrap();
    append_workflow_event(
        artifact_dir(&created),
        WorkflowEventKind::NodeEntered,
        "completed",
        json!({"reason": "test terminal setup"}),
    )
    .unwrap();
    let verification_id = passing_packet(&created);

    let first = run(
        format!("workflow complete --json --verification {verification_id} {run_id}"),
        &root,
    )
    .await
    .unwrap();
    let first: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["schema"], "kiana.workflow-completion.v1");
    assert_eq!(first["status"], "completed");
    assert_eq!(first["current_node"], "completed");
    assert_eq!(first["idempotent"], false);

    let second = run(
        format!("workflow complete --json --verification {verification_id} {run_id}"),
        &root,
    )
    .await
    .unwrap();
    let second: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second["idempotent"], true);
    assert_eq!(second["event_seq"], first["event_seq"]);

    let error = run(
        format!("workflow complete --verification missing_packet {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("already completed"));

    let events = kiana_tasks::read_workflow_events(artifact_dir(&created)).unwrap();
    assert_eq!(
        events.last().unwrap().kind,
        WorkflowEventKind::WorkflowCompleted
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn workflow_complete_rejects_missing_and_non_terminal_runs() {
    let root = root("complete-invalid");
    let created = create_run(&root).await;
    let run_id = created["run_id"].as_str().unwrap();

    let error = run(
        format!("workflow complete --verification missing_packet {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("terminal node completed"),
        "{error:#}"
    );

    append_workflow_event(
        artifact_dir(&created),
        WorkflowEventKind::NodeEntered,
        "completed",
        json!({"reason": "test terminal setup"}),
    )
    .unwrap();
    let error = run(
        format!("workflow complete --verification missing_packet {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("verification packet"),
        "{error:#}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn workflow_complete_rejects_foreign_tampered_and_non_pass_packets() {
    let root = root("complete-packet-validation");
    let created = create_run(&root).await;
    let run_id = created["run_id"].as_str().unwrap();
    append_workflow_event(
        artifact_dir(&created),
        WorkflowEventKind::NodeEntered,
        "completed",
        json!({"reason": "test terminal setup"}),
    )
    .unwrap();

    let foreign = create_run(&root).await;
    let foreign_id = passing_packet(&foreign);
    std::fs::copy(
        artifact_dir(&foreign)
            .join("verification")
            .join(format!("{foreign_id}.json")),
        artifact_dir(&created)
            .join("verification")
            .join(format!("{foreign_id}.json")),
    )
    .unwrap();
    let error = run(
        format!("workflow complete --verification {foreign_id} {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("different workflow run"),
        "{error:#}"
    );

    let tampered_id = packet_with_status(
        &created,
        "evt_workflow_completion_tampered",
        EvidenceStatus::Pass,
    );
    let tampered_path = artifact_dir(&created)
        .join("verification")
        .join(format!("{tampered_id}.json"));
    let mut tampered: Value =
        serde_json::from_slice(&std::fs::read(&tampered_path).unwrap()).unwrap();
    tampered["pass_count"] = json!(99);
    std::fs::write(
        &tampered_path,
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    let error = run(
        format!("workflow complete --verification {tampered_id} {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("pass_count"), "{error:#}");

    let failed_id = packet_with_status(
        &created,
        "evt_workflow_completion_failed",
        EvidenceStatus::Fail,
    );
    let error = run(
        format!("workflow complete --verification {failed_id} {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("final status must be pass"),
        "{error:#}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn workflow_complete_rejects_symlinked_verification_packet() {
    let root = root("complete-symlink-packet");
    let created = create_run(&root).await;
    let run_id = created["run_id"].as_str().unwrap();
    append_workflow_event(
        artifact_dir(&created),
        WorkflowEventKind::NodeEntered,
        "completed",
        json!({"reason": "test terminal setup"}),
    )
    .unwrap();

    let verification_id = passing_packet(&created);
    let packet_path = artifact_dir(&created)
        .join("verification")
        .join(format!("{verification_id}.json"));
    let moved_packet = root.join("moved-verification-packet.json");
    std::fs::rename(&packet_path, &moved_packet).unwrap();
    symlink(&moved_packet, &packet_path).unwrap();

    let error = run(
        format!("workflow complete --verification {verification_id} {run_id}"),
        &root,
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("regular file") || error.to_string().contains("symbolic link"),
        "{error:#}"
    );

    let _ = std::fs::remove_dir_all(root);
}
