#[test]
fn usage_vector_contract_keeps_unknown_zero_and_provenance_distinct() {
    let usage = include_str!("../../kiana-domain/src/billing_usage.rs");
    let baseline = include_str!("../../docs/roadmap/billing-contract-baseline.md");
    for marker in [
        "UsageVector",
        "NormalizedUsage",
        "UsagePresence",
        "ExplicitZero",
        "UsageConfidence",
        "UsageSource",
        "UsageObservation",
        "sequence",
        "unknown_reason",
        "raw_digest",
        "USAGE_VECTOR_SCHEMA",
        "NORMALIZED_USAGE_SCHEMA",
    ] {
        assert!(usage.contains(marker), "usage marker missing: {marker}");
    }
    assert!(baseline.contains("UsageVector"));
    assert!(baseline.contains("None` is unknown, not zero"));
    for forbidden in [
        "reqwest",
        "tokio",
        "std::fs",
        "CapabilityBroker",
        "RateCardStore",
    ] {
        assert!(
            !usage.contains(forbidden),
            "usage contract boundary widened: {forbidden}"
        );
    }
}
