#[test]
fn effect_usage_projection_is_read_only_and_terminal_fact_bound() {
    let source = include_str!("../src/effect_usage_projection.rs");
    assert!(source.contains("EffectUsageObservation::from_json"));
    assert!(source.contains("source_cursor"));
    assert!(source.contains("effect_started_false_but_usage_started"));
    // BQ-15 envelope fences: an unmetered started fact and a terminal kind that disagrees with
    // the usage state are refused before any Receipt projection.
    assert!(source.contains("effect_usage_envelope_missing"));
    assert!(source.contains("completed_usage_not_success"));
    assert!(source.contains("failed_usage_success"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("ModelOutput"));
    assert!(!source.contains("reqwest"));
    assert!(!source.contains("EventStorePort"));
}

#[test]
fn effect_usage_projection_never_reserves_or_settles_quota() {
    let source = include_str!("../src/effect_usage_projection.rs");
    for forbidden in [
        "QuotaReservation",
        "RuntimeBudget",
        "settle",
        "reserve(",
        "CapabilityBroker",
        "tokio::",
    ] {
        assert!(
            !source.contains(forbidden),
            "projection must stay read-only: {forbidden}"
        );
    }
}
