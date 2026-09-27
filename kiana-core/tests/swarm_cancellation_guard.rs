#[test]
fn swarm_cancellation_is_a_fenced_observation_boundary() {
    let domain = include_str!("../../kiana-domain/src/swarm_cancellation.rs");
    let core = include_str!("../src/swarm_cancellation.rs");
    for marker in [
        "SwarmCancellationFact",
        "SwarmCancelState",
        "cancel_generation",
        "stop_confirmed",
        "ResultUnknown",
        "late_result",
        "validate_child_cancellation",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-10 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "retry_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-10 cancellation boundary must not execute effects: {forbidden}"
        );
    }
}
