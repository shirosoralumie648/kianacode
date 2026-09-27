#[test]
fn automation_retry_is_pure_and_unknown_first() {
    let domain = include_str!("../../kiana-domain/src/automation_retry.rs");
    let core = include_str!("../src/automation_retry.rs");
    for marker in [
        "AutomationRetryInput",
        "RetryNewAttempt",
        "ReconcileUnknown",
        "capability_idempotent",
        "approval_valid",
        "deadline_remaining_ms",
        "classify_retry",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-16 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-16 retry boundary executes effects: {forbidden}"
        );
    }
}
