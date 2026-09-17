use kiana_domain::{
    json_digest, RequestId, RunCancellationFact, RunCancellationState, RunId,
    RUN_CANCELLATION_SCHEMA, RUN_CANCELLATION_VERSION,
};
use serde_json::json;

fn ids() -> (RunId, RequestId, RequestId, RequestId) {
    (
        RunId::new(),
        RequestId::new(),
        RequestId::new(),
        RequestId::new(),
    )
}

#[test]
fn cancellation_fact_requires_durable_stop_evidence_for_cancelled() {
    let (run_id, command_id, first, second) = ids();
    let stopping = RunCancellationFact::new(
        run_id,
        command_id,
        RunCancellationState::Stopping,
        "user requested stop",
        Some("actor-1".to_owned()),
        vec![second, first],
        7,
        true,
        None,
        1_700_000_000_000,
    )
    .unwrap();
    assert_eq!(stopping.schema, RUN_CANCELLATION_SCHEMA);
    assert_eq!(stopping.version, RUN_CANCELLATION_VERSION);
    assert_eq!(stopping.target_invocation_ids, vec![first, second]);
    assert!(stopping.validate().is_ok());
    assert_eq!(
        RunCancellationFact::from_json(&stopping.to_json().unwrap()).unwrap(),
        stopping
    );

    let mut cancelled = stopping.clone();
    cancelled.state = RunCancellationState::Cancelled;
    cancelled.stop_confirmed = Some(true);
    cancelled.fact_digest = cancelled.digest();
    assert!(cancelled.validate().is_ok());

    let mut unknown = stopping;
    unknown.state = RunCancellationState::ResultUnknown;
    unknown.stop_confirmed = Some(false);
    unknown.fact_digest = unknown.digest();
    assert!(unknown.validate().is_ok());

    let mut premature = cancelled.clone();
    premature.stop_confirmed = None;
    premature.fact_digest = premature.digest();
    assert_eq!(
        premature.validate().unwrap_err(),
        "run_cancellation_fact_stop_confirmation_required"
    );
}

#[test]
fn cancellation_fact_is_strict_and_digest_bound() {
    let (run_id, command_id, _, _) = ids();
    let fact = RunCancellationFact::new(
        run_id,
        command_id,
        RunCancellationState::Stopping,
        "safe reason",
        None,
        Vec::new(),
        0,
        true,
        Some(false),
        1,
    )
    .unwrap();
    assert_eq!(
        fact.reason_digest,
        json_digest(&json!({"reason":"safe reason"}))
    );

    let mut unknown = fact.to_json().unwrap();
    unknown["raw_reason"] = json!("must-not-cross-the-boundary");
    assert_eq!(
        RunCancellationFact::from_json(&unknown).unwrap_err(),
        "run_cancellation_fact_decode_failed"
    );

    let mut tampered = fact.clone();
    tampered.stop_requested = false;
    tampered.fact_digest = tampered.digest();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "run_cancellation_fact_header_invalid"
    );
}
