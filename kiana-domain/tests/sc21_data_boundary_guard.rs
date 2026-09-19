#[test]
fn data_class_purpose_boundary_and_retention_are_strict_domain_contracts() {
    let governance = include_str!("../src/governance.rs");
    let authority = include_str!("../src/authority.rs");
    let contracts = include_str!("../src/contracts.rs");
    for marker in [
        "pub enum DataClass",
        "pub struct Purpose",
        "pub struct Retention",
        "pub struct ProcessingGrant",
        "pub struct DataPolicy",
        "pub struct DataBoundary",
        "DATA_BOUNDARY_SCHEMA",
        "data_epoch",
    ] {
        assert!(
            governance.contains(marker) || authority.contains(marker) || contracts.contains(marker),
            "SC-21 marker missing: {marker}"
        );
    }
    assert!(!governance.contains("CapabilityBrokerPort"));
    assert!(!governance.contains("std::fs"));
    assert!(!authority.contains("CapabilityBrokerPort"));
}
