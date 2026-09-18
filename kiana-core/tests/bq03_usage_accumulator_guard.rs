#[test]
fn usage_accumulator_guards_sequence_dedup_containment_and_overflow() {
    let accumulator = include_str!("../../kiana-domain/src/billing_usage_accumulator.rs");
    for marker in [
        "UsageAccumulator",
        "UsageApplyOutcome",
        "usage_sequence_conflict",
        "usage_sequence_regression",
        "usage_snapshot_not_containing",
        "usage_delta_without_snapshot",
        "usage_accumulator_overflow",
        "UsageObservation::Snapshot",
        "UsageObservation::Delta",
        "UsageObservation::Final",
    ] {
        assert!(
            accumulator.contains(marker),
            "accumulator marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest",
        "tokio",
        "std::fs",
        "CapabilityBroker",
        "SystemTime",
    ] {
        assert!(
            !accumulator.contains(forbidden),
            "accumulator boundary widened: {forbidden}"
        );
    }
}
