#[test]
fn swarm_projection_is_read_only_and_terminal_replay_fenced() {
    let domain = include_str!("../../kiana-domain/src/swarm_projection.rs");
    let core = include_str!("../src/swarm_projection.rs");
    for marker in [
        "SwarmProjectionEvent",
        "source_cursor",
        "sequence_gap",
        "epoch_mismatch",
        "terminal_resurrection",
        "validate_swarm_projection_event_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-17 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "dispatch_child",
        "replay_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-17 projection widened effect boundary: {forbidden}"
        );
    }
}
