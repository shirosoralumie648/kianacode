#[test]
fn swarm_progress_is_a_bounded_observation_boundary() {
    let domain = include_str!("../../kiana-domain/src/swarm_progress.rs");
    let core = include_str!("../src/swarm_progress.rs");
    for marker in [
        "SwarmProgressBudget",
        "SwarmProgressObservation",
        "heartbeat_sequence",
        "checkpoint_sequence",
        "max_stalls",
        "swarm_progress_non_monotonic",
        "record_swarm_progress",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-09 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "dispatch_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-09 progress boundary must not execute effects: {forbidden}"
        );
    }
}
