#[test]
fn catalog_has_visible_skip_and_stable_dedupe_contract_without_io() {
    let source = include_str!("../src/catalog.rs");
    assert!(source.contains("deep_opt_in_required"));
    assert!(source.contains("no_golden_trace"));
    assert!(source.contains("duplicate_count"));
    assert!(source.contains("stable_order"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
