#[test]
fn query_projector_is_read_only_and_digest_bound() {
    let source = include_str!("../src/cost_ledger_projector.rs");
    assert!(source.contains("project_cost_ledger"));
    assert!(source.contains("append_correction"));
    assert!(source.contains("source_cursor"));
    assert!(!source.contains("EventStorePort"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("ModelOutput"));
}
