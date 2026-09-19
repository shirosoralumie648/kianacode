use kiana_core::{project_persistence_read_model, PERSISTENCE_READ_MODEL_SCHEMA};
use kiana_domain::{RequestContext, RunId, RuntimeEvent};
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
