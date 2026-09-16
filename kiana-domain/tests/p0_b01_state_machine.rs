use kiana_domain::{
    ApprovalState, CapabilityExecutionState, ExecutionStatus, ProjectStatus, RunCancellationState,
    WorkPacketStatus,
};

#[test]
fn illegal_state_transitions_are_rejected_and_terminals_do_not_reopen() {
    assert!(ApprovalState::Consumed
        .transition(ApprovalState::Active)
        .is_err());
    assert!(CapabilityExecutionState::Unknown
        .transition(CapabilityExecutionState::Succeeded)
        .is_err());
    assert!(RunCancellationState::ResultUnknown
        .transition(RunCancellationState::Cancelled)
        .is_err());
    assert!(WorkPacketStatus::Closed
        .transition(WorkPacketStatus::Running)
        .is_err());
    assert!(ExecutionStatus::ResultUnknown
        .transition(ExecutionStatus::Completed)
        .is_err());
    assert!(ProjectStatus::Closed
        .transition(ProjectStatus::Active)
        .is_err());
}

#[test]
fn result_unknown_is_terminal_and_only_explicit_paths_can_leave_intermediate_states() {
    assert!(ExecutionStatus::ResultUnknown.is_terminal());
    assert!(CapabilityExecutionState::Unknown.is_terminal());
    assert!(RunCancellationState::ResultUnknown.as_str() == "result_unknown");
    assert!(ExecutionStatus::Running.can_transition_to(ExecutionStatus::ResultUnknown));
    assert!(
        CapabilityExecutionState::Executing.can_transition_to(CapabilityExecutionState::Unknown)
    );
    assert!(WorkPacketStatus::Blocked.can_transition_to(WorkPacketStatus::Running));
}
