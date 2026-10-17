//! PD-10 behavioural fixtures: the persistence read model is rebuilt from committed facts only.
//!
//! Rejection first: a foreign run, a missing terminal, an old data epoch, a conflicting terminal,
//! a duplicated source event and a terminal result without its attempt facts must all fail closed
//! (or stay unresolved) and may never produce a `completed` receipt. Only then do the success
//! fixtures rebuild Run state, Invocation projections, capability attempts and the receipt
//! evidence — including from a durable journal reopened by a fresh process.

use kiana_core::{project_persistence_read_model, PERSISTENCE_READ_MODEL_SCHEMA};
use kiana_domain::{
    ApprovalId, CapabilityAdmissionState, CapabilityAttemptRecord, CapabilityEffectState,
    CapabilityExecutionState, CapabilityKind, CapabilityRequest, CapabilityStopState, ExecutionId,
    ExecutionReceipt, InvocationId, RequestContext, RequestId, RunId, RuntimeEvent, TraceStatus,
    TurnId,
};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn event(run_id: RunId, sequence: u64, kind: &str) -> RuntimeEvent {
    RuntimeEvent::new(
        kiana_domain::RequestId::new(),
        sequence,
        kind,
        json!({"run_id": run_id, "actor_id": "local-user", "project_root": "/project"}),
    )
    .unwrap()
    .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn context() -> RequestContext {
    let mut context = RequestContext::local("pd10", "/project");
    context.project_trusted = true;
    context
}

/// A committed capability lifecycle: run.started → requested → allowed → prepared → executing →
/// result_committed(success) → run.completed.
///
/// The facts carry no stream metadata, exactly like the durable fixture below: they project
/// directly and can be appended to `JsonlEventLog` unchanged, so the in-process and fresh-process
/// fixtures share one fact shape.
fn capability_lifecycle(
    run_id: RunId,
) -> (RequestId, ExecutionId, InvocationId, Vec<RuntimeEvent>) {
    let request_id = RequestId::new();
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::new();
    let turn_id = TurnId::new();
    let request = CapabilityRequest::new(
        request_id,
        CapabilityKind::Query,
        "search",
        json!({"query": "read model"}),
    );
    let action_digest = format!("sha256:{}", "a".repeat(64));
    let fact = |sequence: u64, kind: &str, data: Value| {
        RuntimeEvent::new(request_id, sequence, kind, data).expect("capability fact")
    };
    let events = vec![
        RuntimeEvent::new(
            RequestId::new(),
            1,
            "run.started",
            json!({"run_id": run_id, "actor_id": "local-user", "project_root": "/project"}),
        )
        .expect("run.started fact"),
        fact(
            2,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "capability": request.capability,
                "operation": request.operation,
                "arguments": request.arguments,
                "risk": request.risk,
                "action_digest": action_digest,
                "attempt": 1,
            }),
        ),
        fact(
            3,
            "capability.decision",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "action_digest": action_digest,
                "gate": {"decision": "allowed"},
            }),
        ),
        fact(
            4,
            "execution.prepared",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "turn_id": turn_id,
                "attempt": 1,
                "permit": {
                    "request_id": request_id,
                    "execution_id": execution_id,
                    "invocation_id": invocation_id,
                    "turn_id": turn_id,
                    "action_digest": action_digest,
                },
            }),
        ),
        fact(
            5,
            "invocation.executing",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "turn_id": turn_id,
                "attempt": 1,
                "action_digest": action_digest,
            }),
        ),
        fact(
            6,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "attempt": 1,
                "effect_known": true,
                "result": {
                    "request_id": request_id,
                    "success": true,
                    "output": {"answer": "one"},
                },
            }),
        ),
        RuntimeEvent::new(
            RequestId::new(),
            7,
            "run.completed",
            json!({"run_id": run_id, "actor_id": "local-user", "project_root": "/project"}),
        )
        .expect("run.completed fact"),
    ];
    (request_id, execution_id, invocation_id, events)
}

fn pd10_temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd10-{label}-{stamp}.jsonl"))
}

#[test]
fn read_model_rebuilds_run_invocations_and_receipt_from_facts() {
    let run_id = RunId::new();
    let model = project_persistence_read_model(
        &context(),
        run_id,
        &[
            event(run_id, 1, "run.started"),
            event(run_id, 2, "run.completed"),
        ],
        42,
        None,
    )
    .expect("read model");
    assert_eq!(model.schema, PERSISTENCE_READ_MODEL_SCHEMA);
    assert_eq!(model.source_cursor, 42);
    assert_eq!(model.source_event_ids.len(), 2);
    assert_eq!(model.run.outcome.as_deref(), Some("completed"));
    assert!(model.invocations.is_empty());
    assert!(model.attempts.is_empty());
    assert_eq!(model.receipt["source_cursor"], 42);
    assert_eq!(model.receipt["run_receipt"]["status"], "completed");
    assert_eq!(model.receipt["run_receipt"]["source_cursor"], 2);
    model.validate().unwrap();
}

#[test]
fn missing_terminal_foreign_run_old_epoch_and_conflict_fail_closed() {
    let run_id = RunId::new();
    let missing = project_persistence_read_model(
        &context(),
        run_id,
        &[event(run_id, 1, "run.started")],
        1,
        None,
    )
    .unwrap();
    assert_eq!(missing.run.outcome, None);
    assert_eq!(missing.receipt["run_receipt"]["status"], "running");

    let foreign = project_persistence_read_model(
        &context(),
        run_id,
        &[event(RunId::new(), 1, "run.completed")],
        1,
        None,
    );
    assert!(foreign.is_err());

    let mut old_epoch = event(run_id, 1, "run.started");
    old_epoch.data_epoch = Some(1);
    assert_eq!(
        project_persistence_read_model(&context(), run_id, &[old_epoch], 1, Some(2)).unwrap_err(),
        "persistence_read_model_data_epoch_mismatch"
    );

    assert!(project_persistence_read_model(
        &context(),
        run_id,
        &[
            event(run_id, 1, "run.completed"),
            event(run_id, 2, "run.failed"),
        ],
        2,
        None,
    )
    .unwrap_err()
    .contains("run_terminal_conflict"));
}

#[test]
fn duplicate_source_event_is_not_projected_twice() {
    let run_id = RunId::new();
    let first = event(run_id, 1, "run.started");
    let mut duplicate = first.clone();
    duplicate.sequence = 2;
    assert_eq!(
        project_persistence_read_model(&context(), run_id, &[first, duplicate], 2, None)
            .unwrap_err(),
        "persistence_read_model_event_duplicate"
    );
}

#[test]
fn terminal_conflict_preserves_stable_error_code_with_both_kinds() {
    let terminal_kinds = [
        "run.completed",
        "run.failed",
        "run.cancelled",
        "run.result_unknown",
    ];
    for first in terminal_kinds {
        for second in terminal_kinds {
            if first == second {
                continue;
            }
            let run_id = RunId::new();
            let error = project_persistence_read_model(
                &context(),
                run_id,
                &[event(run_id, 1, first), event(run_id, 2, second)],
                2,
                None,
            )
            .unwrap_err();
            assert!(
                error.starts_with("persistence_read_model_run:run_terminal_conflict:"),
                "stable code missing for {first}/{second}: {error}"
            );
            assert!(
                error.contains(first) && error.contains(second),
                "conflicting facts missing for {first}/{second}: {error}"
            );
        }
    }
}

#[test]
fn missing_terminal_cannot_become_completed_even_with_other_events() {
    for (kinds, phase) in [
        (vec!["run.started"], "running"),
        (vec!["run.queued", "run.started"], "running"),
        (
            vec![
                "run.started",
                "run.capability_requested",
                "capability.decision",
                "approval.requested",
            ],
            "awaiting_approval",
        ),
    ] {
        let run_id = RunId::new();
        let request_id = RequestId::new();
        let request = CapabilityRequest::new(
            request_id,
            CapabilityKind::Query,
            "search",
            json!({"query": "read model"}),
        );
        let events = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let mut fact = event(run_id, index as u64 + 1, kind);
                match *kind {
                    "run.capability_requested" => {
                        fact.data = json!({
                            "run_id": run_id,
                            "capability_request_id": request_id,
                            "capability": request.capability,
                            "operation": request.operation,
                            "arguments": request.arguments,
                            "risk": request.risk,
                        });
                    }
                    "capability.decision" => {
                        fact.data = json!({
                            "run_id": run_id,
                            "capability_request_id": request_id,
                            "gate": {"decision": "awaiting_approval"},
                        });
                    }
                    "approval.requested" => {
                        fact.data = json!({
                            "run_id": run_id,
                            "capability_request_id": request_id,
                            "approval_id": ApprovalId::new(),
                        });
                    }
                    _ => {}
                }
                fact
            })
            .collect::<Vec<_>>();
        let model =
            project_persistence_read_model(&context(), run_id, &events, events.len() as u64, None)
                .unwrap();
        assert_eq!(model.run.phase, phase);
        assert_eq!(model.run.outcome, None);
        // The typed run receipt carries the unresolved status; there is no top-level `status`.
        assert_eq!(model.receipt["run_receipt"]["status"], phase);
        assert_ne!(model.receipt["run_receipt"]["status"], "completed");
        if phase == "awaiting_approval" {
            assert_eq!(model.invocations.len(), 1);
            assert_eq!(
                model.invocations[0].state,
                CapabilityExecutionState::AwaitingApproval
            );
        }
    }

    // An approval without its capability request identity is still malformed.
    let run_id = RunId::new();
    assert_eq!(
        project_persistence_read_model(
            &context(),
            run_id,
            &[
                event(run_id, 1, "run.started"),
                event(run_id, 2, "approval.requested"),
            ],
            2,
            None,
        )
        .unwrap_err(),
        "persistence_read_model_invocation:invocation_request_id_missing"
    );
}

/// 【成功/证据】Attempt 生命周期与 receipt 证据必须和 Run/Invocation 来自同一批事实。
#[test]
fn read_model_projects_capability_attempts_and_receipt_evidence() {
    let run_id = RunId::new();
    let (request_id, execution_id, invocation_id, events) = capability_lifecycle(run_id);
    let model =
        project_persistence_read_model(&context(), run_id, &events, events.len() as u64, None)
            .expect("read model");
    assert_eq!(model.run.phase, "terminal");
    assert_eq!(model.run.outcome.as_deref(), Some("completed"));
    assert_eq!(model.invocations.len(), 1);
    assert_eq!(
        model.invocations[0].state,
        CapabilityExecutionState::Succeeded
    );

    assert_eq!(model.attempts.len(), 1);
    let attempt = &model.attempts[0];
    assert_eq!(attempt.run_id, Some(run_id));
    assert_eq!(attempt.request_id, request_id);
    assert_eq!(attempt.attempt, 1);
    assert_eq!(attempt.execution_id, Some(execution_id));
    assert_eq!(attempt.invocation_id, Some(invocation_id));
    assert_eq!(attempt.capability_id, "query");
    assert_eq!(attempt.operation, "search");
    assert_eq!(attempt.admission, CapabilityAdmissionState::Allowed);
    assert_eq!(attempt.effect, CapabilityEffectState::Succeeded);
    assert!(attempt.effect_known);
    assert_eq!(attempt.stop, CapabilityStopState::NotRequested);
    assert_eq!(attempt.stop_confirmed, None);
    assert!(attempt.fenced);
    assert!(!attempt.zero_effect);
    assert_eq!(attempt.status, TraceStatus::Ok);
    assert_eq!(attempt.error_code, None);
    assert_eq!(attempt.source_cursor, 6);
    assert_eq!(attempt.source_event_ids.len(), 5);
    assert!(attempt.action_digest.starts_with("sha256:"));

    // Receipt evidence: the run receipt status/cursor, the execution receipt and the observability
    // attempt list must all be derived from the same facts as the read model itself.
    let receipt = &model.receipt;
    assert_eq!(receipt["run_receipt"]["status"], "completed");
    assert_eq!(receipt["run_receipt"]["source_cursor"], 7);
    assert_eq!(
        receipt["run_receipt"]["source_event_ids"]
            .as_array()
            .expect("run receipt source event ids")
            .len(),
        7
    );
    assert!(receipt["run_receipt"]["receipt_digest"]
        .as_str()
        .expect("run receipt digest")
        .starts_with("sha256:"));
    let execution_receipts = receipt["execution_receipts"]
        .as_array()
        .expect("execution receipts");
    assert_eq!(execution_receipts.len(), 1);
    let execution_receipt: ExecutionReceipt =
        serde_json::from_value(execution_receipts[0].clone()).expect("execution receipt");
    assert_eq!(execution_receipt.request_id, request_id);
    assert_eq!(
        execution_receipt.status,
        CapabilityExecutionState::Succeeded
    );
    assert_eq!(execution_receipt.source_cursor, 6);
    let observability_attempt: CapabilityAttemptRecord =
        serde_json::from_value(receipt["observability"]["capability_attempts"][0].clone())
            .expect("observability attempt");
    assert_eq!(observability_attempt, *attempt);
    assert_eq!(
        receipt["observability"]["operator_evidence"]["effect_success_claim"],
        false
    );
    assert_eq!(model.source_event_ids.len(), 7);
    model.validate().unwrap();
}

/// 【拒绝】没有 attempt 事实的终态结果、同 attempt 的冲突终态，以及越界 attempt 一律失败关闭。
#[test]
fn foreign_or_conflicting_attempt_facts_fail_closed() {
    let run_id = RunId::new();
    let (request_id, execution_id, invocation_id, events) = capability_lifecycle(run_id);

    // A terminal result for an attempt that never had request/dispatch facts must not invent one.
    // The request-level fold tolerates the replayed shape (same result output), so this rejects on
    // the attempt layer instead of silently projecting a phantom attempt 2.
    let mut foreign_attempt = events.clone();
    foreign_attempt.push(
        RuntimeEvent::new(
            request_id,
            8,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "attempt": 2,
                "effect_known": true,
                "result": {
                    "request_id": request_id,
                    "success": true,
                    "output": {"answer": "one"},
                },
            }),
        )
        .expect("foreign attempt fact"),
    );
    let error = project_persistence_read_model(
        &context(),
        run_id,
        &foreign_attempt,
        foreign_attempt.len() as u64,
        None,
    )
    .unwrap_err();
    assert!(
        error.starts_with("persistence_read_model_attempt:capability_attempt_foreign_result:")
            && error.ends_with(":2"),
        "unexpected foreign attempt error: {error}"
    );

    // A second terminal fact for the same attempt with different evidence (here an unconfirmed
    // stop) must fail closed at the attempt layer even when the request-level fold sees a replay.
    let mut conflicting_attempt = events.clone();
    conflicting_attempt.push(
        RuntimeEvent::new(
            request_id,
            8,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "attempt": 1,
                "effect_known": true,
                "stop_confirmed": false,
                "result": {
                    "request_id": request_id,
                    "success": true,
                    "output": {"answer": "one"},
                },
            }),
        )
        .expect("conflicting attempt fact"),
    );
    assert_eq!(
        project_persistence_read_model(
            &context(),
            run_id,
            &conflicting_attempt,
            conflicting_attempt.len() as u64,
            None,
        )
        .unwrap_err(),
        format!("persistence_read_model_attempt:capability_attempt_terminal_conflict:{request_id}")
    );

    // Even a caller that rebinds an attempt record to another run must fail model validation
    // instead of exposing a cross-run attempt list.
    let mut model =
        project_persistence_read_model(&context(), run_id, &events, events.len() as u64, None)
            .unwrap();
    model.attempts[0].run_id = Some(RunId::new());
    assert_eq!(
        model.validate().unwrap_err(),
        "persistence_read_model_attempt_scope_invalid"
    );
}

const PD10_REBUILD_CHILD_ENV: &str = "KIANA_PD10_REBUILD_CHILD";
const PD10_REBUILD_EVENTS_ENV: &str = "KIANA_PD10_REBUILD_EVENTS";
const PD10_REBUILD_RUN_ENV: &str = "KIANA_PD10_REBUILD_RUN";
const PD10_REBUILD_OUT_ENV: &str = "KIANA_PD10_REBUILD_OUT";

async fn append_durable_facts(path: &Path, events: &[RuntimeEvent]) {
    let journal = JsonlEventLog::open(path).expect("open durable journal");
    for fact in events {
        journal
            .append(fact.clone())
            .await
            .expect("append durable fact");
    }
}

/// 【成功/新鲜进程】新进程只读同一批持久事实，就必须重建出与在线投影相同的读模型
/// （Run/Invocation/Attempt/Receipt），并且不得追加、改写或截断任何事实。
#[tokio::test]
async fn read_model_rebuilds_the_same_state_in_a_fresh_process() {
    if let Some(events_path) = std::env::var_os(PD10_REBUILD_EVENTS_ENV) {
        // Fresh-process side: read facts from the durable journal and write the rebuilt model.
        let events_path = PathBuf::from(events_path);
        let run_id = RunId::parse_str(&std::env::var(PD10_REBUILD_RUN_ENV).expect("child run id"))
            .expect("child run id");
        let output_path =
            PathBuf::from(std::env::var_os(PD10_REBUILD_OUT_ENV).expect("child output path"));
        let journal = JsonlEventLog::open(&events_path).expect("child open durable journal");
        let facts = journal.read_all().await.expect("child read durable facts");
        let model =
            project_persistence_read_model(&context(), run_id, &facts, facts.len() as u64, None)
                .expect("child rebuild");
        std::fs::write(
            &output_path,
            serde_json::to_vec(&serde_json::to_value(&model).expect("child encode model"))
                .expect("child encode model"),
        )
        .expect("child write model");
        return;
    }

    let run_id = RunId::new();
    let (_, _, _, events) = capability_lifecycle(run_id);
    let events_path = pd10_temp_path("rebuild");
    let output_path = pd10_temp_path("rebuild-out");
    append_durable_facts(&events_path, &events).await;
    let before = std::fs::read(&events_path).expect("read journal bytes");

    let status = std::process::Command::new(std::env::current_exe().expect("current exe"))
        .arg("--exact")
        .arg("read_model_rebuilds_the_same_state_in_a_fresh_process")
        .arg("--nocapture")
        .env(PD10_REBUILD_CHILD_ENV, "1")
        .env(PD10_REBUILD_EVENTS_ENV, &events_path)
        .env(PD10_REBUILD_RUN_ENV, run_id.to_string())
        .env(PD10_REBUILD_OUT_ENV, &output_path)
        .status()
        .expect("spawn rebuild child");
    assert!(status.success(), "fresh-process rebuild failed: {status}");

    // The rebuild is read-only: the child must not append, rewrite or truncate committed facts.
    assert_eq!(
        std::fs::read(&events_path).expect("read journal bytes"),
        before
    );

    let child_model: Value =
        serde_json::from_slice(&std::fs::read(&output_path).expect("child model"))
            .expect("decode child model");
    let journal = JsonlEventLog::open(&events_path).expect("reopen durable journal");
    let facts = journal.read_all().await.expect("read durable facts");
    assert_eq!(facts.len(), events.len());
    let online =
        project_persistence_read_model(&context(), run_id, &facts, facts.len() as u64, None)
            .expect("online projection");
    let online_model = serde_json::to_value(&online).expect("encode online model");

    // Fresh process, same facts, identical read model.
    assert_eq!(child_model, online_model);
    assert_eq!(child_model["run"]["phase"], "terminal");
    assert_eq!(child_model["run"]["outcome"], "completed");
    assert_eq!(
        child_model["attempts"].as_array().expect("attempts").len(),
        1
    );
    assert_eq!(child_model["attempts"][0]["status"], "ok");
    assert_eq!(child_model["attempts"][0]["effect"], "succeeded");
    assert_eq!(child_model["receipt"]["run_receipt"]["status"], "completed");
    assert_eq!(child_model["source_cursor"], facts.len() as u64);

    let _ = std::fs::remove_file(&events_path);
    let _ = std::fs::remove_file(&output_path);
    let _ = std::fs::remove_file(events_path.with_extension("jsonl.lock"));
}
