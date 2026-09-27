#[test]
fn swarm_handoff_is_directed_structured_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/swarm_handoff.rs");
    let core = include_str!("../src/swarm_handoff.rs");
    for marker in [
        "recipient_id",
        "HandoffAck",
        "StatusReport",
        "Evidence",
        "Incident",
        "authority_granted",
        "symposium_max_rounds",
        "validate_swarm_handoff_record",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-16 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "SendMessage",
        "free_message",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-16 handoff widened boundary: {forbidden}"
        );
    }
}
