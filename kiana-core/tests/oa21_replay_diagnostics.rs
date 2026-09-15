use kiana_core::{diagnose_replay, ReplayDiagnosticsError, ReplayExpectation};
use kiana_domain::{InvocationId, RequestId, RunId, RuntimeEvent, TraceStatus};
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

fn unknown_run() -> (RunId, RequestId, Vec<RuntimeEvent>) {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({"run_id":run_id,"session_id":"session-1","actor_id":"local-user","project_root":"/repo","authority_epoch":1,"data_epoch":1}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.capability_requested",
            json!({"run_id":run_id,"capability_request_id":request_id,"capability":"process","operation":"shell.exec","risk":"local_write","arguments":{"command":"raw-secret"},"action_digest":digest,"attempt":1}),
        ),
        event(
            run_id,
            request_id,
            3,
            "invocation.dispatching",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1}),
        ),
        event(
            run_id,
            request_id,
            4,
            "invocation.executing",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_started":true}),
        ),
        event(
            run_id,
            request_id,
            5,
            "execution.result_committed",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_known":false,"error":"result_unknown:provider_timeout","result":{"success":false,"output":{"raw":"raw-secret"}}}),
        ),
        event(
            run_id,
            request_id,
            6,
            "audit.forged",
            json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1,"actor_ref":"model"}),
        ),
    ];
    (run_id, request_id, events)
}

#[test]
fn replay_diagnostics_are_deterministic_safe_and_unknown_effect_is_locatable() {
    let (run_id, request_id, events) = unknown_run();
    let first = diagnose_replay(&events, Some(run_id), &[]).unwrap();
    let second = diagnose_replay(&events, Some(run_id), &[]).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.status, TraceStatus::Unknown);
    assert!(!first.diagnostics.is_empty());
    assert!(first.diagnostics.iter().any(|diagnostic| {
        diagnostic.divergence == kiana_domain::ReplayDivergenceKind::UnknownEffect
    }));
    assert!(first.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .error_code
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
    }));
    let encoded = serde_json::to_string(&first).unwrap();
    assert!(!encoded.contains("raw-secret"));
    first.validate().unwrap();

    let expectation = ReplayExpectation {
        invocation_id: Some(InvocationId::from_uuid(request_id.as_uuid())),
        attempt: Some(1),
        input_digest: Some(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
        ),
        expected_status: TraceStatus::Ok,
        expected_error_code: None,
    };
    let expected = diagnose_replay(&events[..5], Some(run_id), &[expectation]).unwrap();
    assert!(expected.diagnostics.iter().any(|diagnostic| {
        diagnostic.divergence == kiana_domain::ReplayDivergenceKind::StatusMismatch
    }));
}

#[test]
fn duplicate_source_is_rejected_before_any_projection_or_side_effect() {
    let (run_id, _request_id, mut events) = unknown_run();
    let duplicate = events[0].clone();
    events.push(duplicate);
    assert_eq!(
        diagnose_replay(&events, Some(run_id), &[]).unwrap_err(),
        ReplayDiagnosticsError::SourceDuplicate
    );
}

#[test]
fn replay_expectation_limit_is_bounded() {
    let run_id = RunId::new();
    let events = vec![event(
        run_id,
        RequestId::new(),
        1,
        "run.authorized",
        json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1}),
    )];
    let expectations = (0..257)
        .map(|_| ReplayExpectation {
            invocation_id: None,
            attempt: None,
            input_digest: None,
            expected_status: TraceStatus::Unknown,
            expected_error_code: None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        diagnose_replay(&events, None, &expectations).unwrap_err(),
        ReplayDiagnosticsError::ExpectationLimit
    );
}
