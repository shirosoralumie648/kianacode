#[test]
fn allocation_projector_is_read_only_and_does_not_merge_dimensions() {
    let source = include_str!("../src/allocation_projector.rs");
    for marker in [
        "project_cost_allocations",
        "seen_usage_ids",
        "run_totals",
        "project_totals",
        "organization_totals",
        "workflow_totals",
        "cell_totals",
        "ALLOCATION_PROJECTOR_IS_READ_ONLY",
    ] {
        assert!(source.contains(marker), "BQ-19 marker missing: {marker}");
    }
    for forbidden in [
        "append_event",
        "EventStorePort",
        "CostAllocationAdmission",
        "fn authorize",
        "fn release",
        "fn reserve",
        "CapabilityBrokerPort",
        "tokio::spawn",
    ] {
        assert!(
            !source.contains(forbidden),
            "projection must not {forbidden}"
        );
    }
}
