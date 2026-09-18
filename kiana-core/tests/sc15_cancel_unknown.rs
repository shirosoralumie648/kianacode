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
