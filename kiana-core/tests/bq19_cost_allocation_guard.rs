#[test]
fn core_allocation_admission_stays_on_control_plane_boundary() {
    let source = include_str!("../src/billing_allocation.rs");
    for marker in [
        "CostAllocationAdmission",
        "validate_server_binding",
        "authorize_sharing_grant",
        "COST_ALLOCATION_EVENT",
        "with_stream_metadata",
    ] {
        assert!(source.contains(marker), "BQ-19 marker missing: {marker}");
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "EventStorePort",
        "reqwest",
        "tokio::",
        "loop {",
        "FinancialBudget",
    ] {
        assert!(
            !source.contains(forbidden),
            "allocation admission must not {forbidden}"
        );
    }
}
