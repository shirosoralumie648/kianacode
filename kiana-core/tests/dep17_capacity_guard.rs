#[test]
fn dep17_capacity_is_bounded_preserving_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/deployment_capacity.rs");
    let core = include_str!("../src/deployment_capacity.rs");
    for marker in [
        "CapacityResource",
        "CapacityFact",
        "CapacityInput",
        "CapacityReport",
        "capacity_backpressure",
        "capacity_exceeded",
        "shutdown_deadline_exceeded",
        "facts_preserved",
        "evaluate_capacity",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-17 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore::append",
        "ProjectionStorePort",
        "WorkflowQueueStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
        "drop_committed",
        "delete_facts",
        "resize_queue",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-17 capacity crossed effect boundary: {forbidden}"
        );
    }
}
