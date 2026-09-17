use kiana_core::{project_run_state, RunOutcome, RunPhase, RunProjectionError};
use kiana_domain::{RequestId, RunId, RuntimeEvent, TurnId};
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
fn run_projection_follows_approval_cancel_and_terminal_phases() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let turn_id = TurnId::new();
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({"run_id":run_id}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.queued",
            json!({"run_id":run_id}),
        ),
        event(
            run_id,
            request_id,
            3,
            "run.prompt",
            json!({"run_id":run_id,"turn_id":turn_id}),
        ),
        event(
            run_id,
            request_id,
            4,
            "approval.requested",
            json!({"run_id":run_id}),
        ),
    ];
    let awaiting = project_run_state(run_id, &events).unwrap();
    assert_eq!(awaiting.phase, RunPhase::AwaitingApproval);
    assert_eq!(awaiting.outcome, None);

    let mut cancelling = events;
    cancelling.push(event(
        run_id,
        request_id,
        5,
        "run.cancelling",
        json!({"run_id":run_id}),
    ));
    assert_eq!(
        project_run_state(run_id, &cancelling).unwrap().phase,
        RunPhase::Cancelling
    );

    cancelling.push(event(
        run_id,
        request_id,
        6,
        "run.cancelled",
        json!({"run_id":run_id,"error":"cancelled"}),
    ));
    let terminal = project_run_state(run_id, &cancelling).unwrap();
    assert_eq!(terminal.phase, RunPhase::Terminal);
    assert_eq!(terminal.outcome, Some(RunOutcome::Cancelled));
}

#[test]
fn terminal_projection_is_idempotent_and_rejects_conflicting_kinds() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let completed = event(
        run_id,
        request_id,
        1,
        "run.completed",
        json!({"run_id":run_id}),
    );
    let duplicate = event(
        run_id,
        request_id,
        2,
        "run.completed",
        json!({"run_id":run_id,"error":"late duplicate"}),
    );
    let state = project_run_state(run_id, &[completed, duplicate]).unwrap();
    assert_eq!(state.outcome, Some(RunOutcome::Completed));
    assert_eq!(state.error, None);

    let conflict = event(
        run_id,
        request_id,
        3,
        "run.failed",
        json!({"run_id":run_id,"error":"contradiction"}),
    );
    assert_eq!(
        project_run_state(
            run_id,
            &[
                conflict.clone(),
                event(
                    run_id,
                    request_id,
                    4,
                    "run.cancelled",
                    json!({"run_id":run_id})
                )
            ]
        )
        .unwrap_err(),
        RunProjectionError::TerminalConflict {
            kinds: vec!["run.cancelled".to_owned(), "run.failed".to_owned()],
        }
    );
}

#[test]
fn late_effect_is_ignored_until_a_new_prompt_turn() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let first_turn = TurnId::new();
    let second_turn = TurnId::new();
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.prompt",
            json!({"run_id":run_id,"turn_id":first_turn}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.completed",
            json!({"run_id":run_id}),
        ),
        event(
            run_id,
            request_id,
            3,
            "execution.result_committed",
            json!({"run_id":run_id,"effect_known":false}),
        ),
    ];
    let terminal = project_run_state(run_id, &events).unwrap();
    assert_eq!(terminal.outcome, Some(RunOutcome::Completed));

    let mut continued = events;
    continued.push(event(
        run_id,
        request_id,
        4,
        "run.prompt",
        json!({"run_id":run_id,"turn_id":second_turn}),
    ));
    let resumed = project_run_state(run_id, &continued).unwrap();
    assert_eq!(resumed.phase, RunPhase::Running);
    assert_eq!(resumed.outcome, None);
}
