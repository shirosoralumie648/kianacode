#[test]
fn effect_usage_projection_is_read_only_and_terminal_fact_bound() {
    let source = include_str!("../src/effect_usage_projection.rs");
    assert!(source.contains("EffectUsageObservation::from_json"));
    assert!(source.contains("source_cursor"));
    assert!(source.contains("effect_started_false_but_usage_started"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("ModelOutput"));
    assert!(!source.contains("reqwest"));
    assert!(!source.contains("EventStorePort"));
}
