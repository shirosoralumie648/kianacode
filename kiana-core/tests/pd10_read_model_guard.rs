#[test]
fn pd10_read_model_is_fact_only_and_has_no_execution_authority() {
    let source = include_str!("../src/persistence_read_model.rs");
    assert!(source.contains("project_run_state"));
    assert!(source.contains("project_invocations"));
    assert!(source.contains("receipt_from_events"));
    assert!(source.contains("source_cursor"));
    assert!(source.contains("data_epoch"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("authorize_and_execute"));
    assert!(!source.contains("EventStorePort::append"));
    assert!(!source.contains("RunnerPort::send"));
}
