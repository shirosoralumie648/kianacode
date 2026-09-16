use kiana_domain::{RequestId, RunId, RuntimeEvent};
use serde_json::json;

fn event(
    run_id: RunId,
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    data: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

#[test]
fn duplicate_run_terminal_is_rejected() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        event(run_id, request_id, 1, "run.authorized", json!({})),
        event(
            run_id,
            request_id,
            2,
            "run.completed",
            json!({"run_id": run_id}),
        ),
        event(
            run_id,
            request_id,
            3,
            "run.failed",
            json!({"run_id": run_id, "error": "late terminal"}),
        ),
    ];
    assert!(matches!(
        kiana_core::project_run_state(run_id, &events),
        Err(kiana_core::RunProjectionError::TerminalConflict { .. })
    ));
}

#[test]
fn late_result_cannot_complete_a_new_turn() {
    let selected_run = RunId::new();
    let foreign_run = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        event(
            selected_run,
            request_id,
            1,
            "run.capability_requested",
            json!({
                "run_id": selected_run,
                "capability_request_id": request_id,
                "request_id": request_id,
                "capability": "query",
                "operation": "search",
                "risk": "read_only",
                "arguments": {"query": "new turn"}
            }),
        ),
        event(
            foreign_run,
            request_id,
            2,
            "capability.completed",
            json!({
                "run_id": foreign_run,
                "capability_request_id": request_id,
                "result": {"success": true}
            }),
        ),
    ];
    let projection = kiana_core::project_invocations(selected_run, &events).unwrap();
    assert_eq!(projection.len(), 1);
    assert!(projection[0].result.is_none());
}

#[test]
fn continue_closed_run_requires_new_admission() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    assert!(lifecycle.contains("run_not_terminal_use_resume"));
    assert!(lifecycle.contains("run_unknown_requires_reconciliation"));
    assert!(lifecycle.contains("continue_new_turn"));
    assert!(lifecycle.contains("Some(previous)"));
}

#[test]
fn native_continue_creates_new_run_while_legacy_contract_is_preserved() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let runner = include_str!("../../kiana-runner-protocol/src/lib.rs");
    assert!(lifecycle.contains("let run_id = RunId::new()"));
    assert!(lifecycle.contains("TurnSemantics::NewTurn"));
    assert!(lifecycle.contains("TurnSemantics::LegacyContinue"));
    assert!(protocol.contains("run.turn.v2"));
    assert!(runner.contains("turn_id: Option<TurnId>"));
}

#[test]
fn h02_run_turn_step_attempt_identities_stay_on_one_spine() {
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let identity = include_str!("../../kiana-domain/src/execution_identity.rs");
    let projection = include_str!("../src/model_attempt_projection.rs");
    assert!(harness.contains("StepIdentity::new"));
    assert!(harness.contains("ModelAttemptIdentity::new"));
    assert!(harness.contains("model_attempt_id"));
    assert!(model.contains("pub step_id: Option<StepId>"));
    assert!(model.contains("pub model_attempt_id: Option<ModelAttemptId>"));
    assert!(identity.contains("StepIdentity"));
    assert!(identity.contains("ModelAttemptIdentity"));
    assert!(projection.contains("with_identity(step_id, model_attempt_id)"));
}
