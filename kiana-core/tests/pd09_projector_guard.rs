#[test]
fn projector_driver_is_replay_only_and_checkpoint_first() {
    let source = include_str!("../src/projection_checkpoint.rs");
    for marker in [
        "ProjectionDriver",
        "ProjectionDriverStatus",
        "projection_driver_paused",
        "projection_driver_retry_overflow",
        "ProjectionDriverStatus::RebuildRequired",
        "ReplayProjection::from_checkpoint",
        "ReplayProjection::from_zero",
    ] {
        assert!(
            source.contains(marker),
            "projector marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "reqwest",
    ] {
        assert!(
            !source.contains(forbidden),
            "projector authority widened: {forbidden}"
        );
    }
}
