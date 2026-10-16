use kiana_core::{project_persistence_read_model, PERSISTENCE_READ_MODEL_SCHEMA};
use kiana_domain::{
    ApprovalId, CapabilityExecutionState, CapabilityKind, CapabilityRequest, RequestContext,
    RequestId, RunId, RuntimeEvent,
};
use serde_json::json;

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
    assert_eq!(model.receipt["source_cursor"], 42);
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
    assert_ne!(missing.receipt["status"], "completed");

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
        assert_ne!(model.receipt["status"], "completed");
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
