#[test]
fn automation_cancellation_is_a_fenced_observation_boundary() {
    let domain = include_str!("../../kiana-domain/src/automation_cancellation.rs");
    let core = include_str!("../src/automation_cancellation.rs");
    for marker in [
        "AutomationCancellationFact",
        "AutomationCancelState",
        "cancel_generation",
        "stop_confirmed",
        "ResultUnknown",
        "late_result",
        "validate_automation_cancel",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-17 marker missing: {marker}"
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
            "AUT-17 boundary executes effects: {forbidden}"
        );
    }
}
