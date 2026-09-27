#[test]
fn swarm_release_gate_keeps_deny_unknown_replay_and_spine_visible() {
    let domain = include_str!("../../kiana-domain/src/swarm_release_gate.rs");
    let core = include_str!("../src/swarm_release_gate.rs");
    for marker in [
        "SwarmReleaseScenario",
        "Unknown",
        "Replay",
        "Race",
        "effect_count",
        "same_spine",
        "secret_free",
        "validate_swarm_release_evidence",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-18 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "publish_live",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-18 release gate effect marker present: {forbidden}"
        );
    }
}
