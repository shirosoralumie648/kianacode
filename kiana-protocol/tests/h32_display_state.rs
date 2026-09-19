use kiana_protocol::{
    DisplayApply, ExecutionStatus, ResponseEnvelope, RunDisplayState, RunId, RunStreamEnvelope,
    RunStreamEvent, UiCursor, UiSnapshot, PROTOCOL_SCHEMA,
};
use serde_json::json;

fn envelope(run_id: RunId, epoch: &str, sequence: u64, event: RunStreamEvent) -> RunStreamEnvelope {
    RunStreamEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        epoch: epoch.to_owned(),
        sequence,
        ui_cursor: sequence,
        event,
    }
}

#[test]
fn duplicate_and_gap_rules_preserve_terminal_state() {
    let run_id = RunId::new();
    let mut state = RunDisplayState::new(run_id);
    assert_eq!(
        state
            .apply(&envelope(
                run_id,
                "epoch-a",
                1,
                RunStreamEvent::Delta {
                    run_id,
                    text: "hello".to_owned(),
                },
            ))
            .unwrap(),
        DisplayApply::Applied
    );
    assert_eq!(
        state
            .apply(&envelope(
                run_id,
                "epoch-a",
                1,
                RunStreamEvent::Delta {
                    run_id,
                    text: "duplicate".to_owned(),
                },
            ))
            .unwrap(),
        DisplayApply::IgnoredDuplicate
    );
    assert_eq!(
        state
            .apply(&envelope(
                run_id,
                "epoch-a",
                3,
                RunStreamEvent::Delta {
                    run_id,
                    text: "gap".to_owned(),
                },
            ))
            .unwrap(),
        DisplayApply::GapRequiresSnapshot
    );
    let terminal = ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: kiana_protocol::RequestId::new(),
        status: ExecutionStatus::Completed,
        output: json!({"text":"done"}),
        error: None,
    };
    assert_eq!(
        state
            .apply(&envelope(
                run_id,
                "epoch-a",
                4,
                RunStreamEvent::Terminal {
                    run_id,
                    response: terminal,
                },
            ))
            .unwrap(),
        DisplayApply::Applied
    );
    assert!(state.terminal);
    assert_eq!(state.status, "completed");
    assert_eq!(state.text, "hello");
}

#[test]
fn old_epoch_cannot_mutate_snapshot_and_hydration_resets_display_gap() {
    let run_id = RunId::new();
    let mut state = RunDisplayState::new(run_id);
    state
        .apply(&envelope(
            run_id,
            "epoch-a",
            1,
            RunStreamEvent::Delta {
                run_id,
                text: "old".to_owned(),
            },
        ))
        .unwrap();
    let before = state.clone();
    assert_eq!(
        state
            .apply(&envelope(
                run_id,
                "epoch-b",
                1,
                RunStreamEvent::Delta {
                    run_id,
                    text: "new".to_owned(),
                },
            ))
            .unwrap_err(),
        "display_epoch_changed"
    );
    assert_eq!(state, before);
    let snapshot = UiSnapshot {
        schema: "kiana.protocol.v1".to_owned(),
        cursor: UiCursor {
            epoch: "epoch-b".to_owned(),
            sequence: 2,
        },
        session_id: "session".to_owned(),
        run_id: Some(run_id),
        stream_cursor: None,
        status: Some(ExecutionStatus::Running),
        pending_actions: Vec::new(),
    };
    state.hydrate(&snapshot).unwrap();
    assert_eq!(state.cursor.epoch, "epoch-b");
    assert_eq!(state.status, "running");
    assert!(!state.gap_detected);
}
