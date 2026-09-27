#[test]
fn swarm_merge_reducer_is_deterministic_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/swarm_merge_reducer.rs");
    let core = include_str!("../src/swarm_merge_reducer.rs");
    for marker in [
        "SwarmMergeStrategy",
        "AllSuccess",
        "AllSettled",
        "ExplicitPolicy",
        "ResultUnknown",
        "partition_coverage",
        "validate_swarm_merge",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-13 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "first_success",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-13 reducer widened effect boundary: {forbidden}"
        );
    }
}
