#[test]
fn automation_dispatch_keeps_worker_and_observation_separate() {
    let domain = include_str!("../../kiana-domain/src/automation_dispatch.rs");
    let core = include_str!("../src/automation_dispatch.rs");
    for marker in [
        "AutomationDispatchIntent",
        "AutomationObservation",
        "AutomationObservationOutcome",
        "worker_id",
        "authority_epoch",
        "Unknown",
        "observe_automation_dispatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-15 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "dispatch_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-15 boundary executes effects: {forbidden}"
        );
    }
}
