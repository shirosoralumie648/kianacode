#[test]
fn pd11_authority_projection_is_fact_only_and_does_not_grant_or_dispatch() {
    let source = include_str!("../src/authority_read_model.rs");
    assert!(source.contains("AuthorityReadModel"));
    assert!(source.contains("authority_epoch"));
    assert!(source.contains("ChildParentMissing"));
    assert!(source.contains("SettlementWithoutReservation"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("authorize_and_execute"));
    assert!(!source.contains("EventStorePort::append"));
    assert!(!source.contains("RunnerPort::send"));
}
