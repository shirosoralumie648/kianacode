use kiana_domain::{RequestId, RunCancellationFact, RunCancellationState, RunId};

fn stopping_fact() -> RunCancellationFact {
    RunCancellationFact::new(
        RunId::new(),
        RequestId::new(),
        RunCancellationState::Stopping,
        "user",
        Some("actor".to_owned()),
        vec![RequestId::new()],
        4,
        true,
        None,
        1,
    )
    .unwrap()
}

#[test]
fn cancellation_fact_requires_confirmed_stop_for_cancelled() {
    let fact = stopping_fact();
    let cancelled = RunCancellationFact::new(
        fact.run_id,
        fact.command_id,
        RunCancellationState::Cancelled,
        "user",
        fact.actor_id.clone(),
        fact.target_invocation_ids.clone(),
        fact.expected_version,
        true,
        Some(true),
        2,
    )
    .unwrap();
    assert_eq!(cancelled.state, RunCancellationState::Cancelled);
    assert_eq!(cancelled.stop_confirmed, Some(true));
    assert!(RunCancellationFact::new(
        fact.run_id,
        fact.command_id,
        RunCancellationState::Cancelled,
        "user",
        fact.actor_id,
        fact.target_invocation_ids,
        fact.expected_version,
        true,
        None,
        2,
    )
    .is_err());
}

#[test]
fn unknown_cancellation_keeps_stop_unconfirmed_and_terminal_transition_is_one_way() {
    let fact = stopping_fact();
    let unknown = RunCancellationFact::new(
        fact.run_id,
        fact.command_id,
        RunCancellationState::ResultUnknown,
        "timeout",
        fact.actor_id,
        fact.target_invocation_ids,
        fact.expected_version,
        true,
        Some(false),
        2,
    )
    .unwrap();
    assert_eq!(unknown.state, RunCancellationState::ResultUnknown);
    assert_eq!(unknown.stop_confirmed, Some(false));
    assert!(RunCancellationState::Stopping
        .transition(RunCancellationState::Cancelled)
        .is_ok());
    assert!(RunCancellationState::Stopping
        .transition(RunCancellationState::ResultUnknown)
        .is_ok());
    assert!(RunCancellationState::Cancelled
        .transition(RunCancellationState::ResultUnknown)
        .is_err());
}

#[test]
fn non_terminal_cancellation_cannot_claim_a_confirmed_stop() {
    let fact = stopping_fact();
    for state in [
        RunCancellationState::Requested,
        RunCancellationState::Stopping,
    ] {
        assert_eq!(
            RunCancellationFact::new(
                fact.run_id,
                fact.command_id,
                state,
                "user",
                fact.actor_id.clone(),
                fact.target_invocation_ids.clone(),
                fact.expected_version,
                true,
                Some(true),
                3,
            )
            .unwrap_err(),
            "run_cancellation_fact_stop_conflict"
        );
    }
}

#[test]
fn unknown_cancellation_cannot_claim_a_confirmed_stop_or_a_success_state() {
    let fact = stopping_fact();
    assert_eq!(
        RunCancellationFact::new(
            fact.run_id,
            fact.command_id,
            RunCancellationState::ResultUnknown,
            "timeout",
            fact.actor_id.clone(),
            fact.target_invocation_ids.clone(),
            fact.expected_version,
            true,
            Some(true),
            3,
        )
        .unwrap_err(),
        "run_cancellation_fact_unknown_stop_conflict"
    );
    assert_eq!(
        RunCancellationFact::new(
            fact.run_id,
            fact.command_id,
            RunCancellationState::Active,
            "user",
            fact.actor_id,
            fact.target_invocation_ids,
            fact.expected_version,
            true,
            None,
            3,
        )
        .unwrap_err(),
        "run_cancellation_fact_state_invalid"
    );
}

#[test]
fn terminal_cancellation_states_are_one_way() {
    use RunCancellationState::{Active, Cancelled, Requested, ResultUnknown, Stopping};
    assert!(Active.transition(Requested).is_ok());
    assert!(Requested.transition(Stopping).is_ok());
    for (from, to) in [
        (Active, Stopping),
        (Requested, Cancelled),
        (Cancelled, Stopping),
        (Cancelled, Cancelled),
        (ResultUnknown, Cancelled),
        (ResultUnknown, Stopping),
        (ResultUnknown, ResultUnknown),
    ] {
        assert!(
            from.transition(to).is_err(),
            "{from:?}->{to:?} must be denied"
        );
    }
}

#[test]
fn unknown_fact_survives_round_trip_and_rejects_tampering() {
    let fact = stopping_fact();
    let unknown = RunCancellationFact::new(
        fact.run_id,
        fact.command_id,
        RunCancellationState::ResultUnknown,
        "timeout",
        fact.actor_id,
        fact.target_invocation_ids,
        fact.expected_version,
        true,
        Some(false),
        5,
    )
    .unwrap();
    let json = unknown.to_json().unwrap();
    let decoded = RunCancellationFact::from_json(&json).unwrap();
    assert_eq!(decoded, unknown);
    assert_eq!(decoded.state, RunCancellationState::ResultUnknown);
    assert_eq!(decoded.stop_confirmed, Some(false));

    // Relabelling an unknown effect as a confirmed success is rejected by the content digest.
    let mut relabelled = json.clone();
    relabelled["state"] = serde_json::json!("cancelled");
    relabelled["stop_confirmed"] = serde_json::json!(true);
    assert_eq!(
        RunCancellationFact::from_json(&relabelled).unwrap_err(),
        "run_cancellation_fact_digest_mismatch"
    );

    // Dropping the stop evidence is also rejected: unknown stops stay unconfirmed.
    let mut without_stop = json.clone();
    without_stop["stop_confirmed"] = serde_json::Value::Null;
    assert_eq!(
        RunCancellationFact::from_json(&without_stop).unwrap_err(),
        "run_cancellation_fact_digest_mismatch"
    );

    // Unknown fields are never silently accepted.
    let mut extra = json;
    extra["silently_accepted"] = serde_json::json!(true);
    assert_eq!(
        RunCancellationFact::from_json(&extra).unwrap_err(),
        "run_cancellation_fact_decode_failed"
    );
}
