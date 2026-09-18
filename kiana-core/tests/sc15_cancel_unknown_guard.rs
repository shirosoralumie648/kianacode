#[test]
fn sc15_cancel_fence_and_unknown_reconciliation_boundary_is_present() {
    let cancellation = include_str!("../../kiana-domain/src/cancellation.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let driver = include_str!("../../kiana-runner/src/state_driver.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let h08 = include_str!("../../kiana-runner/tests/h08_cancellation_guard.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let sessions = include_str!("../src/sessions.rs");
    let platform = include_str!("../src/platform.rs");
    let recovery = include_str!("../src/recovery.rs");

    for marker in [
        "RunCancellationFact",
        "stop_confirmed",
        "run_cancellation_fact_stop_confirmation_required",
        "run_cancellation_fact_unknown_stop_conflict",
        "fact_digest",
        "RunCancellationState::ResultUnknown",
    ] {
        assert!(
            cancellation.contains(marker) || states.contains(marker),
            "SC-15 cancellation fact marker missing: {marker}"
        );
    }
    for marker in [
        "RequestCancel",
        "StopConfirmed",
        "RecoverUnknown",
        "HarnessPhase::RecoveryRequired",
        "DriverTerminal::Unknown",
        "CancelPendingTools",
    ] {
        assert!(
            driver.contains(marker),
            "SC-15 runner state marker missing: {marker}"
        );
    }
    for marker in [
        "cancellation.request",
        "ToolCancelled",
        "cancelled:{reason}",
        "run_not_found",
        "cancel_during_retry_wait_prevents_next_attempt",
    ] {
        assert!(
            harness.contains(marker) || h08.contains(marker),
            "SC-15 harness marker missing: {marker}"
        );
    }
    for marker in [
        "record_cancel_requested",
        "run.cancelling",
        "RunnerCommand::Cancel",
        "await_capability_stop",
        "run.cancelled",
        "run.result_unknown",
        "cancel_response_run_id_mismatch",
        "cancel_confirmation_inconsistent",
        "cancel_confirmation_missing",
    ] {
        assert!(
            lifecycle.contains(marker),
            "SC-15 lifecycle marker missing: {marker}"
        );
    }
    for marker in [
        "stop_requested",
        "stop_confirmed",
        "effect_known",
        "fenced",
        "CapabilityErrorCode::ResultUnknown",
        "result_unknown:capability_result_mismatch",
    ] {
        assert!(
            dispatch.contains(marker),
            "SC-15 dispatch marker missing: {marker}"
        );
    }
    for marker in ["signal_cancel", "pre-signaled", "await_capability_stop"] {
        assert!(
            sessions.contains(marker),
            "SC-15 session marker missing: {marker}"
        );
    }
    for marker in [
        "FailureIncident",
        "HumanInboxItem",
        "reconciliation",
        "automatic_retry_allowed:false",
        "failure_reconciliation_evidence_required",
        "resource_release_stop_unconfirmed",
    ] {
        assert!(
            platform.contains(marker),
            "SC-15 reconciliation marker missing: {marker}"
        );
    }
    for marker in [
        "ExecutionStatus::ResultUnknown",
        "CellLifecycle::Quarantined",
        "requires_reconciliation",
        "Unknown effects retain their resource reservation",
    ] {
        assert!(
            recovery.contains(marker),
            "SC-15 recovery marker missing: {marker}"
        );
    }
    for source in [driver, harness, lifecycle, dispatch, platform, recovery] {
        for forbidden in [
            "cancelled_without_stop_confirmation_is_success",
            "retry_after_cancel",
            "unknown_effect_is_success",
        ] {
            assert!(
                !source.contains(forbidden),
                "SC-15 cancellation bypass marker: {forbidden}"
            );
        }
    }
}
