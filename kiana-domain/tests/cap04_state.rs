use kiana_domain::{
    normalize_capability_result, CapabilityEffectState, CapabilityErrorCode,
    CapabilityExecutionState, CapabilityKind, CapabilityRequest, CapabilityResult,
    CapabilityStopState, RequestId,
};
use serde_json::json;

#[test]
fn capability_stage_cancellation_and_start_failure_are_explicit_transitions() {
    assert!(CapabilityExecutionState::Queued.can_transition_to(CapabilityExecutionState::Cancelled));
    assert!(
        CapabilityExecutionState::Authorized.can_transition_to(CapabilityExecutionState::Cancelled)
    );
    assert!(
        CapabilityExecutionState::Dispatching.can_transition_to(CapabilityExecutionState::Failed)
    );
    assert!(
        CapabilityExecutionState::Dispatching.can_transition_to(CapabilityExecutionState::Unknown)
    );
    assert!(
        !CapabilityExecutionState::Cancelled.can_transition_to(CapabilityExecutionState::Succeeded)
    );
}

#[test]
fn nonzero_shell_exit_remains_a_structured_tool_result() {
    let request_id = RequestId::new();
    let request = CapabilityRequest::new(
        request_id,
        CapabilityKind::Process,
        "shell.exec",
        json!({"exit_code": 17, "stdout": "diagnostic"}),
    );
    let normalized = normalize_capability_result(
        request_id,
        CapabilityResult::success(request.request_id, request.arguments.clone()),
    );
    assert!(!normalized.success);
    assert_eq!(
        normalized.failure_code(),
        Some(CapabilityErrorCode::ExecutionFailed)
    );
    let dimensions = normalized.dimensions();
    assert_eq!(
        dimensions.process,
        kiana_domain::CapabilityProcessState::Exited
    );
    assert_eq!(dimensions.effect, CapabilityEffectState::Failed);
    assert_eq!(dimensions.stop, CapabilityStopState::NotRequested);
    assert_eq!(dimensions.exit_code, Some(17));
    assert_eq!(
        dimensions.execution_state(),
        CapabilityExecutionState::Failed
    );
    assert_eq!(normalized.output["error_code"], "execution_failed");
    assert_eq!(normalized.output["outcome"]["execution_status"], "failed");
}
