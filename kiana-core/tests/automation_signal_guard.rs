#[test]
fn automation_signal_is_owner_and_checkpoint_bound() {
    let domain = include_str!("../../kiana-domain/src/automation_signal.rs");
    let core = include_str!("../src/automation_signal.rs");
    for marker in [
        "AutomationSignalFact",
        "Paused",
        "Resumed",
        "Consumed",
        "owner_id",
        "checkpoint_sequence",
        "validate_automation_signal_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-18 marker missing: {marker}"
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
            "AUT-18 boundary executes effects: {forbidden}"
        );
    }
}
