use kiana_domain::{RequestId, RunId, RuntimeEvent};
use serde_json::json;

fn event(run_id: RunId, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), 1, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), 1)
}

#[test]
fn duplicate_terminal_kind_is_idempotent_but_conflict_is_unknown() {
    let run_id = RunId::new();
    let duplicate = vec![
        event(run_id, "run.authorized", json!({"run_id":run_id})),
        event(
            run_id,
            "run.completed",
            json!({"run_id":run_id,"text":"done"}),
        ),
        event(
            run_id,
            "run.completed",
            json!({"run_id":run_id,"text":"done-again"}),
        ),
    ];
    let state = kiana_core::project_run_state(run_id, &duplicate).unwrap();
    assert_eq!(state.outcome, Some(kiana_core::RunOutcome::Completed));
    assert_eq!(state.error, None);

    let conflict = vec![
        event(run_id, "run.authorized", json!({"run_id":run_id})),
        event(run_id, "run.completed", json!({"run_id":run_id})),
        event(
            run_id,
            "run.result_unknown",
            json!({"run_id":run_id,"error":"result_unknown:late"}),
        ),
    ];
    assert!(matches!(
        kiana_core::project_run_state(run_id, &conflict),
        Err(kiana_core::RunProjectionError::TerminalConflict { .. })
    ));
}

#[test]
fn late_events_cannot_resurrect_terminal_run() {
    let run_id = RunId::new();
    let events = vec![
        event(run_id, "run.authorized", json!({"run_id":run_id})),
        event(
            run_id,
            "run.failed",
            json!({"run_id":run_id,"error":"known"}),
        ),
        event(
            run_id,
            "invocation.executing",
            json!({"run_id":run_id,"effect_started":true}),
        ),
    ];
    let state = kiana_core::project_run_state(run_id, &events).unwrap();
    assert_eq!(state.phase, kiana_core::RunPhase::Terminal);
    assert_eq!(state.outcome, Some(kiana_core::RunOutcome::Failed));
    assert_eq!(state.error.as_deref(), Some("known"));
}

#[test]
fn terminal_writer_and_shutdown_are_cas_and_flush_ordered() {
    let events = include_str!("../src/events.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "terminal_kind_invalid",
        "run_terminal_conflict",
        "run:{run_id}:turn:{turn}:terminal",
        "commit_confirmed",
        "resource.quarantined",
        "result_unknown:terminal_append_unconfirmed",
    ] {
        assert!(events.contains(marker), "terminal marker missing: {marker}");
    }
    let flush = daemon
        .find("self.flush_event_store().await?")
        .expect("shutdown must flush EventStore first");
    let drain = daemon
        .find("self.flush_observability().await")
        .expect("shutdown must drain observability after facts");
    let close = daemon
        .find("self.close_event_store().await")
        .expect("shutdown must close EventStore after flush");
    assert!(flush < drain && drain < close);
}
