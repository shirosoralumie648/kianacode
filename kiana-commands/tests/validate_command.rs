use kiana_commands::validate::ValidateCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    append_workflow_event, initialize_workflow_run, WorkflowEventKind, WorkflowInit,
    WorkflowInputKind, WorkflowProfile,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-validate-{label}-{}-{unique}",
        std::process::id()
    ))
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
            request: "Validate temporary project".to_string(),
            input_kind: WorkflowInputKind::Qa,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap()
}

fn write_release_smoke(root: &Path, exit_code: i32) {
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(
        scripts.join("release-smoke.sh"),
        format!("#!/usr/bin/env bash\necho validation-smoke\nexit {exit_code}\n"),
    )
    .unwrap();
}

fn write_task(root: &Path, task_id: &str, metadata: Value) -> PathBuf {
    let dir = root.join(".kiana").join("tasks").join("default");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{task_id}.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "id": task_id,
            "title": format!("Task {task_id}"),
            "status": "completed",
            "blockedBy": [],
            "blocks": [],
            "metadata": metadata
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

fn assert_no_validation_artifacts(run: &kiana_tasks::WorkflowRun) {
    assert_eq!(
        std::fs::read_dir(run.artifact_dir.join("verification"))
            .unwrap()
            .count(),
        0
    );
    assert!(
        !std::fs::read_to_string(run.artifact_dir.join("eventlog.jsonl"))
            .unwrap()
            .contains("evidence_recorded")
    );
}

#[tokio::test]
async fn validate_requires_an_existing_workflow() {
    let root = temp_root("missing-workflow");
    std::fs::create_dir_all(&root).unwrap();

    let error = ValidateCommand
        .execute(context("--json --profile project_p0", &root))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("workflow_not_found"),
        "unexpected error: {error}"
    );
    assert!(error.contains("workflow init"), "unexpected error: {error}");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_records_real_pass_checks_as_evidence_and_verification_packet() {
    let root = temp_root("pass");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 0);
    let run = workflow(&root);

    let result = ValidateCommand
        .execute(context(
            format!("--json --profile project_p0 --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["schema"], "kiana.validation-run.v1");
    assert_eq!(report["workflow_id"], run.workflow_id);
    assert_eq!(report["run_id"], run.run_id);
    assert_eq!(report["profile"], "project_p0");
    assert_eq!(report["final_status"], "pass");
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(report["summary"]["failed"], 0);
    assert_eq!(report["evidence_ids"].as_array().unwrap().len(), 1);

    let evidence = std::fs::read_to_string(run.artifact_dir.join("eventlog.jsonl")).unwrap();
    assert!(evidence.contains("validation-smoke"));
    assert!(evidence.contains("release_smoke"));
    let packet_path = PathBuf::from(report["packet_path"].as_str().unwrap());
    assert!(packet_path.is_file());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_records_failed_check_without_claiming_pass() {
    let root = temp_root("fail");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 7);
    let run = workflow(&root);

    let result = ValidateCommand
        .execute(context(
            format!("--json --profile project_p0 --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["final_status"], "fail");
    assert_eq!(report["summary"]["failed"], 1);
    let packet: Value = serde_json::from_str(
        &std::fs::read_to_string(report["packet_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(packet["final_status"], "fail");
    assert_eq!(packet["checks"][0]["status"], "fail");

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_appends_verification_reference_without_overwriting_task_metadata() {
    let root = temp_root("task-link");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 0);
    let task_path = write_task(
        &root,
        "task-1",
        json!({
            "owner": "engineer",
            "evidence": ["legacy:keep"]
        }),
    );
    let run = workflow(&root);

    let result = ValidateCommand
        .execute(context(
            format!(
                "--json --profile project_p0 --workflow {} --task task-1",
                run.run_id
            ),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let task: Value = serde_json::from_str(&std::fs::read_to_string(&task_path).unwrap()).unwrap();
    let evidence = task["metadata"]["evidence"].as_array().unwrap();

    assert_eq!(task["metadata"]["owner"], "engineer");
    assert_eq!(evidence[0], "legacy:keep");
    assert_eq!(evidence.len(), 2);
    assert!(evidence[1]
        .as_str()
        .unwrap()
        .starts_with("verification:.kiana/workflows/"));
    assert!(evidence[1]
        .as_str()
        .unwrap()
        .contains(report["verification_id"].as_str().unwrap()));

    let packet: Value = serde_json::from_str(
        &std::fs::read_to_string(report["packet_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(packet["task_id"], "task-1");

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_rejects_missing_task_before_running_checks_or_writing_packet() {
    let root = temp_root("missing-task");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 0);
    let run = workflow(&root);

    let error = ValidateCommand
        .execute(context(
            format!(
                "--json --profile project_p0 --workflow {} --task missing",
                run.run_id
            ),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("task_not_found"),
        "unexpected error: {error}"
    );
    assert_no_validation_artifacts(&run);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_rejects_duplicate_task_identity_before_running_checks() {
    let root = temp_root("duplicate-task");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 0);
    let original = write_task(&root, "duplicate", json!({}));
    std::fs::copy(
        &original,
        original.parent().unwrap().join("duplicate-copy.json"),
    )
    .unwrap();
    let run = workflow(&root);

    let error = ValidateCommand
        .execute(context(
            format!(
                "--json --profile project_p0 --workflow {} --task duplicate",
                run.run_id
            ),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("task_identity_conflict"),
        "unexpected error: {error}"
    );
    assert_no_validation_artifacts(&run);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_rejects_invalid_task_metadata_before_running_checks() {
    for (label, metadata, expected) in [
        (
            "metadata-type",
            json!("invalid"),
            "metadata must be a JSON object",
        ),
        (
            "evidence-type",
            json!({"evidence": "invalid"}),
            "metadata.evidence must be an array",
        ),
        (
            "evidence-item-type",
            json!({"evidence": [7]}),
            "metadata.evidence must contain only strings",
        ),
    ] {
        let root = temp_root(label);
        std::fs::create_dir_all(&root).unwrap();
        write_release_smoke(&root, 0);
        write_task(&root, "invalid-task", metadata);
        let run = workflow(&root);

        let error = ValidateCommand
            .execute(context(
                format!(
                    "--json --profile project_p0 --workflow {} --task invalid-task",
                    run.run_id
                ),
                &root,
            ))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains(expected), "unexpected error: {error}");
        assert_no_validation_artifacts(&run);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[tokio::test]
async fn validate_blocks_before_running_checks_when_workflow_state_is_inconsistent() {
    let root = temp_root("inconsistent");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 0);
    let run = workflow(&root);
    let state_path = run.artifact_dir.join("state.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    state["last_event_seq"] = json!(99);
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let error = ValidateCommand
        .execute(context(
            format!("--json --profile project_p0 --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("workflow_inconsistent"),
        "unexpected error: {error}"
    );
    assert_no_validation_artifacts(&run);

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn validate_repairs_lagging_state_before_running_checks() {
    let root = temp_root("repair-lagging-state");
    std::fs::create_dir_all(&root).unwrap();
    write_release_smoke(&root, 0);
    let run = workflow(&root);
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

    let result = ValidateCommand
        .execute(context(
            format!("--json --profile project_p0 --workflow {}", run.run_id),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let repaired_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();

    assert_eq!(report["schema"], "kiana.validation-run.v1");
    assert_eq!(report["final_status"], "pass");
    assert_eq!(repaired_state["current_node"], "product_definition");
    assert!(repaired_state["last_event_seq"].as_u64().unwrap() >= 3);

    let _ = std::fs::remove_dir_all(root);
}
