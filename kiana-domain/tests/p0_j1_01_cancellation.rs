use kiana_domain::{ExecutionStatus, RunCancellationState};

#[test]
fn cancel_transitions_are_total_and_irreversible() {
    assert!(RunCancellationState::Active
        .transition(RunCancellationState::Requested)
        .is_ok());
    assert!(RunCancellationState::Requested
        .transition(RunCancellationState::Stopping)
        .is_ok());
    assert!(RunCancellationState::Stopping
        .transition(RunCancellationState::Cancelled)
        .is_ok());
    assert!(RunCancellationState::Stopping
        .transition(RunCancellationState::ResultUnknown)
        .is_ok());
    for terminal in [
        RunCancellationState::Cancelled,
        RunCancellationState::ResultUnknown,
    ] {
        assert!(terminal.transition(RunCancellationState::Active).is_err());
        assert!(terminal
            .transition(RunCancellationState::Requested)
            .is_err());
        assert!(terminal.transition(RunCancellationState::Stopping).is_err());
    }

    assert!(ExecutionStatus::Accepted.can_transition_to(ExecutionStatus::Queued));
    assert!(ExecutionStatus::Queued.can_transition_to(ExecutionStatus::Cancelling));
    assert!(ExecutionStatus::Running.can_transition_to(ExecutionStatus::Cancelling));
    assert!(ExecutionStatus::Cancelling.can_transition_to(ExecutionStatus::Cancelled));
    assert!(ExecutionStatus::Cancelling.can_transition_to(ExecutionStatus::ResultUnknown));
    assert!(!ExecutionStatus::Cancelled.can_transition_to(ExecutionStatus::Running));
    assert!(!ExecutionStatus::ResultUnknown.can_transition_to(ExecutionStatus::Completed));
}

#[test]
fn cancellation_status_wire_names_are_stable() {
    assert_eq!(ExecutionStatus::Queued.as_str(), "queued");
    assert_eq!(ExecutionStatus::Cancelling.as_str(), "cancelling");
    assert_eq!(RunCancellationState::Requested.as_str(), "requested");
    assert_eq!(RunCancellationState::Stopping.as_str(), "stopping");
    assert_eq!(
        RunCancellationState::ResultUnknown.as_str(),
        "result_unknown"
    );
}
