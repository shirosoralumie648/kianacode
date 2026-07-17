use kiana_tasks::{
    append_workflow_event, append_workflow_event_with_unique_data_value,
    append_workflow_transition, commit_immutable_artifacts_with_unique_event,
    default_workflow_template, initialize_workflow_run, list_workflow_runs, read_workflow_events,
    resume_workflow_run, WorkflowArtifactBatch, WorkflowArtifactInput, WorkflowError,
    WorkflowEventKind, WorkflowInit, WorkflowInputKind, WorkflowProfile, WorkflowResumeStatus,
    WorkflowStatus, WorkflowTransitionEvidence, WorkflowTransitionInput,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-workflow-{name}-{}-{unique}",
        std::process::id()
    ))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn default_template_covers_the_full_workflow_path_and_branches() {
    let template = default_workflow_template();

    assert_eq!(template.schema, "kiana.workflow-dag.v1");
    assert_eq!(template.id, "kiana-full-workflow");
    assert!(template.node("capture").is_some());
    assert!(template.node("context_intake").is_some());
    assert!(template.node("research").is_some());
    assert!(template.node("design").is_some());
    assert!(template.node("plan").is_some());
    assert!(template.node("execute").is_some());
    assert!(template.node("quality_gate").is_some());
    assert!(template.node("behavior_verify").is_some());
    assert!(template.node("multi_review").is_some());
    assert!(template.node("ship").is_some());
    assert!(template.node("learn").is_some());
    assert!(template.node("block_handoff").is_some());
    assert!(template.has_edge("capture", "product_definition", "clear"));
    assert!(template.has_edge("capture", "clarify_question", "vague"));
    assert!(template.has_edge("execute", "fix_loop", "failed"));
    assert!(template.has_edge("security_fix", "block_handoff", "critical"));
}

#[test]
fn initialize_workflow_run_creates_artifacts_state_and_eventlog() {
    let root = temp_root("init");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Build the complete workflow runtime".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: true,
        },
    )
    .unwrap();

    assert_eq!(run.schema, "kiana.workflow-run.v1");
    assert_eq!(run.status, WorkflowStatus::Running);
    assert_eq!(run.current_node, "capture");
    assert!(run.artifact_dir.ends_with(&run.run_id));
    assert!(run.artifact_dir.is_dir());

    let dag_path = run.artifact_dir.join("workflow_dag.json");
    let state_path = run.artifact_dir.join("state.json");
    let eventlog_path = run.artifact_dir.join("eventlog.jsonl");

    assert_eq!(read_json(&dag_path)["id"], "kiana-full-workflow");
    assert_eq!(read_json(&state_path)["current_node"], "capture");

    let eventlog = fs::read_to_string(&eventlog_path).unwrap();
    let events = eventlog.lines().collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert!(events[0].contains("\"workflow_created\""));
    assert!(events[1].contains("\"node_entered\""));

    for relative in [
        "context_pack.md",
        "problem-definition.md",
        "findings.md",
        "task_plan.md",
        "plan-confirmation.md",
        "autoplan-review.md",
        "decision-briefs.md",
        "next-agent-handoff.md",
        "learnings.md",
        "review/scope.md",
    ] {
        assert!(run.artifact_dir.join(relative).is_file(), "{relative}");
    }

    for relative in ["workpackets", "resultpackets", "evidence", "reviews"] {
        assert!(run.artifact_dir.join(relative).is_dir(), "{relative}");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn append_workflow_event_is_append_only_and_updates_state_timestamp() {
    let root = temp_root("append");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Review a risky change".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    let state_before = read_json(&run.artifact_dir.join("state.json"));

    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::GateEvaluated,
        "capture",
        serde_json::json!({
            "decision": "continue",
            "reason": "goal is clear"
        }),
    )
    .unwrap();

    let eventlog = fs::read_to_string(run.artifact_dir.join("eventlog.jsonl")).unwrap();
    let events = eventlog.lines().collect::<Vec<_>>();
    assert_eq!(events.len(), 3);
    assert!(events[2].contains("\"gate_evaluated\""));

    let state_after = read_json(&run.artifact_dir.join("state.json"));
    assert!(
        state_after["updated_at_ms"].as_u64().unwrap()
            >= state_before["updated_at_ms"].as_u64().unwrap()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_workflow_runs_orders_latest_run_first() {
    let root = temp_root("list");
    let first = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "First workflow".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Second workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: true,
        },
    )
    .unwrap();

    let runs = list_workflow_runs(&root).unwrap();

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].run_id, second.run_id);
    assert_eq!(runs[1].run_id, first.run_id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_workflow_runs_rejects_state_projection_tampering() {
    let root = temp_root("list-projection-tamper");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject forged workflow list state".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let mut state = read_json(&state_path);
    state["current_node"] = serde_json::json!("ship");
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let error = list_workflow_runs(&root).unwrap_err();

    assert!(error.to_string().contains("state projection mismatch"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_reports_ready_when_state_and_eventlog_match() {
    let root = temp_root("resume-ready");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Resume a valid workflow".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(report.schema, "kiana.workflow-resume.v1");
    assert_eq!(report.resume_status, WorkflowResumeStatus::Ready);
    assert!(report.eventlog_consistent);
    assert_eq!(report.event_count, 2);
    assert_eq!(report.state_last_event_seq, report.eventlog_last_seq);
    assert_eq!(report.run.run_id, run.run_id);
    assert_eq!(report.recommended_action, "continue:capture");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_blocks_state_eventlog_sequence_conflict() {
    let root = temp_root("resume-blocked");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Detect inconsistent workflow state".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: true,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let mut state = read_json(&state_path);
    state["last_event_seq"] = serde_json::json!(99);
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(report.resume_status, WorkflowResumeStatus::Blocked);
    assert!(!report.eventlog_consistent);
    assert_eq!(report.eventlog_last_seq, 2);
    assert_eq!(report.state_last_event_seq, 99);
    assert!(report
        .blocker
        .as_deref()
        .unwrap()
        .contains("state last_event_seq 99 does not match eventlog 2"));
    assert_eq!(report.recommended_action, "repair:reconcile-eventlog");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_blocks_state_projection_tampering() {
    let root = temp_root("resume-projection-tamper");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Detect tampered workflow projection".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let mut state = read_json(&state_path);
    state["current_node"] = serde_json::json!("ship");
    state["status"] = serde_json::json!("completed");
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(report.resume_status, WorkflowResumeStatus::Blocked);
    assert!(!report.eventlog_consistent);
    assert!(report
        .blocker
        .as_deref()
        .is_some_and(|value| value.contains("current_node") || value.contains("status")));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_repairs_state_lagging_valid_eventlog() {
    let root = temp_root("resume-repair-lag");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Repair a lagging workflow state".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let stale_state = fs::read(&state_path).unwrap();
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::NodeEntered,
        "product_definition",
        serde_json::json!({"reason": "simulate eventlog committed before state projection"}),
    )
    .unwrap();
    fs::write(&state_path, stale_state).unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(
        serde_json::to_value(report.resume_status).unwrap(),
        serde_json::json!("repaired")
    );
    assert!(report.eventlog_consistent);
    assert_eq!(report.eventlog_last_seq, 3);
    assert_eq!(report.state_last_event_seq, 3);
    assert_eq!(report.run.current_node, "product_definition");
    assert_eq!(report.recommended_action, "continue:product_definition");
    let repaired = read_json(&state_path);
    assert_eq!(repaired["last_event_seq"], 3);
    assert_eq!(repaired["current_node"], "product_definition");
    assert!(!fs::read_dir(&run.artifact_dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".state.json.tmp-")
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_latest_workflow_run_repairs_state_lagging_valid_eventlog() {
    let root = temp_root("resume-latest-repair-lag");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Repair latest lagging workflow state".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let stale_state = fs::read(&state_path).unwrap();
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::NodeEntered,
        "product_definition",
        serde_json::json!({"reason": "simulate latest run state lag"}),
    )
    .unwrap();
    fs::write(&state_path, stale_state).unwrap();

    let report = resume_workflow_run(&root, None).unwrap();

    assert_eq!(report.run.run_id, run.run_id);
    assert_eq!(
        serde_json::to_value(report.resume_status).unwrap(),
        serde_json::json!("repaired")
    );
    assert_eq!(report.run.current_node, "product_definition");
    assert_eq!(report.state_last_event_seq, 3);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_blocks_tampered_lagging_state() {
    let root = temp_root("resume-tampered-lag");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject a tampered lagging workflow state".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let mut stale_state = read_json(&state_path);
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::NodeEntered,
        "product_definition",
        serde_json::json!({"reason": "create a valid eventlog tail"}),
    )
    .unwrap();
    stale_state["request"] = serde_json::json!("tampered request");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&stale_state).unwrap(),
    )
    .unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(report.resume_status, WorkflowResumeStatus::Blocked);
    assert!(!report.eventlog_consistent);
    assert_eq!(report.state_last_event_seq, 2);
    assert!(report
        .blocker
        .as_deref()
        .is_some_and(|value| value.contains("request does not match EventLog")));
    assert_eq!(read_json(&state_path)["request"], "tampered request");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_blocks_tampered_lagging_checkpoint() {
    let root = temp_root("resume-tampered-lag-checkpoint");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject a tampered lagging checkpoint".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let mut stale_state = read_json(&state_path);
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::NodeEntered,
        "product_definition",
        serde_json::json!({"reason": "create a valid eventlog tail"}),
    )
    .unwrap();
    stale_state["checkpoint"] = serde_json::json!("forged-checkpoint");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&stale_state).unwrap(),
    )
    .unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(report.resume_status, WorkflowResumeStatus::Blocked);
    assert!(report
        .blocker
        .as_deref()
        .is_some_and(|value| value.contains("checkpoint")));
    assert_eq!(read_json(&state_path)["checkpoint"], "forged-checkpoint");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_blocks_tampered_lagging_pending_approvals() {
    let root = temp_root("resume-tampered-lag-approvals");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject tampered lagging approvals".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: true,
        },
    )
    .unwrap();
    let state_path = run.artifact_dir.join("state.json");
    let mut stale_state = read_json(&state_path);
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::NodeEntered,
        "product_definition",
        serde_json::json!({"reason": "create a valid eventlog tail"}),
    )
    .unwrap();
    stale_state["pending_approvals"] = serde_json::json!(["ship:forged-approval"]);
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&stale_state).unwrap(),
    )
    .unwrap();

    let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

    assert_eq!(report.resume_status, WorkflowResumeStatus::Blocked);
    assert!(report
        .blocker
        .as_deref()
        .is_some_and(|value| value.contains("pending_approvals")));
    assert_eq!(
        read_json(&state_path)["pending_approvals"],
        serde_json::json!(["ship:forged-approval"])
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resume_workflow_run_blocks_tampered_lagging_timestamps() {
    for field in ["created_at_ms", "updated_at_ms"] {
        let root = temp_root(&format!("resume-tampered-lag-{field}"));
        let run = initialize_workflow_run(
            &root,
            WorkflowInit {
                request: format!("Reject a tampered lagging {field}"),
                input_kind: WorkflowInputKind::Review,
                profile: WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let state_path = run.artifact_dir.join("state.json");
        let mut stale_state = read_json(&state_path);
        append_workflow_event(
            &run.artifact_dir,
            WorkflowEventKind::NodeEntered,
            "product_definition",
            serde_json::json!({"reason": "create a valid eventlog tail"}),
        )
        .unwrap();
        stale_state[field] = serde_json::json!(1);
        fs::write(
            &state_path,
            serde_json::to_vec_pretty(&stale_state).unwrap(),
        )
        .unwrap();

        let report = resume_workflow_run(&root, Some(&run.run_id)).unwrap();

        assert_eq!(
            report.resume_status,
            WorkflowResumeStatus::Blocked,
            "tampered field {field} must block repair"
        );
        assert!(report
            .blocker
            .as_deref()
            .is_some_and(|value| value.contains(field)));
        assert_eq!(read_json(&state_path)[field], 1);

        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn append_workflow_transition_rejects_edge_missing_from_persisted_dag() {
    let root = temp_root("runtime-transition-edge");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject a forged runtime transition".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();

    let error = append_workflow_transition(
        &run.artifact_dir,
        WorkflowTransitionInput {
            from: "capture".to_string(),
            to: "ship".to_string(),
            decision: "forged".to_string(),
            evidence: vec![WorkflowTransitionEvidence {
                path: "problem-definition.md".to_string(),
                bytes: 1,
                sha256: "a".repeat(64),
            }],
            note: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("DAG edge"), "{error}");
    assert_eq!(read_workflow_events(&run.artifact_dir).unwrap().len(), 2);
    let state = read_json(&run.artifact_dir.join("state.json"));
    assert_eq!(state["current_node"], "capture");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn append_workflow_transition_rejects_tampered_dag_snapshot() {
    let root = temp_root("runtime-transition-dag-tamper");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject a tampered workflow DAG".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let dag_path = run.artifact_dir.join("workflow_dag.json");
    let mut dag = read_json(&dag_path);
    dag["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "from": "capture",
            "to": "ship",
            "decision": "forged"
        }));
    fs::write(&dag_path, serde_json::to_vec_pretty(&dag).unwrap()).unwrap();

    let error = append_workflow_transition(
        &run.artifact_dir,
        WorkflowTransitionInput {
            from: "capture".to_string(),
            to: "ship".to_string(),
            decision: "forged".to_string(),
            evidence: vec![WorkflowTransitionEvidence {
                path: "problem-definition.md".to_string(),
                bytes: 1,
                sha256: "a".repeat(64),
            }],
            note: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("DAG integrity"), "{error}");
    assert_eq!(read_workflow_events(&run.artifact_dir).unwrap().len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn workflow_writer_recovers_stale_lease_but_preserves_active_lease() {
    let root = temp_root("writer-lease-recovery");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Recover stale writer lease".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let lock_path = run.artifact_dir.join(".eventlog.lock");
    fs::write(&lock_path, "pid=999999999 acquired_at_ms=0\n").unwrap();

    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::GateEvaluated,
        "capture",
        serde_json::json!({"decision": "continue"}),
    )
    .unwrap();
    assert!(lock_path.is_file());

    let active_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    active_lock.try_lock().unwrap();
    let error = append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::GateEvaluated,
        "capture",
        serde_json::json!({"decision": "continue"}),
    )
    .unwrap_err();
    assert!(matches!(error, kiana_tasks::WorkflowError::WriterBusy(_)));
    drop(active_lock);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workflow_writer_enforces_unique_event_data_inside_writer_lease() {
    let root = temp_root("unique-event-data");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Guard evidence event identity".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();

    append_workflow_event_with_unique_data_value(
        &run.artifact_dir,
        WorkflowEventKind::EvidenceRecorded,
        "verify",
        "event_id",
        "evt_atomic",
        |sequence, _| serde_json::json!({"event_id": "evt_atomic", "sequence": sequence}),
    )
    .unwrap();
    let error = append_workflow_event_with_unique_data_value(
        &run.artifact_dir,
        WorkflowEventKind::EvidenceRecorded,
        "verify",
        "event_id",
        "evt_atomic",
        |sequence, _| serde_json::json!({"event_id": "evt_atomic", "sequence": sequence}),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        kiana_tasks::WorkflowError::DuplicateEventData { .. }
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workflow_artifact_commit_is_idempotent_and_append_only() {
    let root = temp_root("artifact-commit-idempotent");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Persist swarm dispatch".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let initial_events = read_workflow_events(&run.artifact_dir).unwrap().len();

    let commit_once = || {
        commit_immutable_artifacts_with_unique_event(
            &run.artifact_dir,
            WorkflowEventKind::WorkPacketCreated,
            "workpacket",
            "dispatch_id",
            "dispatch-test",
            |sequence, at_ms| {
                Ok(WorkflowArtifactBatch {
                    artifacts: vec![
                        WorkflowArtifactInput {
                            relative_path: "workpackets/dispatch-test/task-a.json".to_string(),
                            contents: serde_json::to_vec_pretty(&serde_json::json!({
                                "schema": "kiana.swarm-workpacket.v1",
                                "task_id": "task-a"
                            }))?,
                        },
                        WorkflowArtifactInput {
                            relative_path: "workpackets/dispatch-test/manifest.json".to_string(),
                            contents: serde_json::to_vec_pretty(&serde_json::json!({
                                "schema": "kiana.swarm-dispatch-manifest.v1",
                                "dispatch_id": "dispatch-test"
                            }))?,
                        },
                    ],
                    event_data: serde_json::json!({
                        "dispatch_id": "dispatch-test",
                        "sequence": sequence,
                        "created_at_ms": at_ms
                    }),
                })
            },
        )
    };

    let first = commit_once().unwrap();
    let second = commit_once().unwrap();

    assert!(!first.reused_event);
    assert!(second.reused_event);
    assert_eq!(first.event.seq, second.event.seq);
    assert_eq!(
        read_workflow_events(&run.artifact_dir).unwrap().len(),
        initial_events + 1
    );
    assert!(run
        .artifact_dir
        .join("workpackets/dispatch-test/task-a.json")
        .is_file());
    assert!(run
        .artifact_dir
        .join("workpackets/dispatch-test/manifest.json")
        .is_file());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workflow_artifact_commit_rejects_existing_content_conflict() {
    let root = temp_root("artifact-commit-conflict");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject conflicting dispatch artifact".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let path = run.artifact_dir.join("workpackets/shared.json");
    fs::write(&path, b"original").unwrap();

    let error = commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::WorkPacketCreated,
        "workpacket",
        "dispatch_id",
        "dispatch-conflict",
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: "workpackets/shared.json".to_string(),
                    contents: b"replacement".to_vec(),
                }],
                event_data: serde_json::json!({"dispatch_id": "dispatch-conflict"}),
            })
        },
    )
    .unwrap_err();

    assert!(matches!(error, WorkflowError::ArtifactConflict { .. }));
    assert_eq!(fs::read(&path).unwrap(), b"original");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workflow_artifact_commit_rejects_path_traversal() {
    let root = temp_root("artifact-commit-traversal");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject artifact path traversal".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();

    let error = commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::WorkPacketCreated,
        "workpacket",
        "dispatch_id",
        "dispatch-traversal",
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: "../outside.json".to_string(),
                    contents: b"outside".to_vec(),
                }],
                event_data: serde_json::json!({"dispatch_id": "dispatch-traversal"}),
            })
        },
    )
    .unwrap_err();

    assert!(matches!(error, WorkflowError::InvalidArtifactPath(_)));
    assert!(!root.join("outside.json").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workflow_artifact_commit_recovers_matching_orphan_artifact() {
    let root = temp_root("artifact-commit-orphan");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Recover orphan dispatch artifact".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let orphan_path = run.artifact_dir.join("workpackets/orphan.json");
    fs::write(&orphan_path, b"stable").unwrap();
    let initial_events = read_workflow_events(&run.artifact_dir).unwrap().len();

    let commit = commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::WorkPacketCreated,
        "workpacket",
        "dispatch_id",
        "dispatch-orphan",
        |sequence, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: "workpackets/orphan.json".to_string(),
                    contents: b"stable".to_vec(),
                }],
                event_data: serde_json::json!({
                    "dispatch_id": "dispatch-orphan",
                    "sequence": sequence
                }),
            })
        },
    )
    .unwrap();

    assert!(!commit.reused_event);
    assert_eq!(
        read_workflow_events(&run.artifact_dir).unwrap().len(),
        initial_events + 1
    );
    assert_eq!(fs::read(orphan_path).unwrap(), b"stable");

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn workflow_artifact_commit_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = temp_root("artifact-commit-symlink");
    let outside = root.with_extension("outside");
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Reject workflow artifact symlink escape".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, run.artifact_dir.join("workpackets/escape")).unwrap();

    let error = commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::WorkPacketCreated,
        "workpacket",
        "dispatch_id",
        "dispatch-symlink",
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: "workpackets/escape/packet.json".to_string(),
                    contents: b"escape".to_vec(),
                }],
                event_data: serde_json::json!({"dispatch_id": "dispatch-symlink"}),
            })
        },
    )
    .unwrap_err();

    assert!(matches!(error, WorkflowError::InvalidArtifactPath(_)));
    assert!(!outside.join("packet.json").exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}
