#[test]
fn memory_acl_is_one_server_derived_record_filter() {
    let source = include_str!("../../kiana-domain/src/memory_acl.rs");
    for marker in [
        "MemoryAclRequest",
        "MemoryAclDecision",
        "MemoryAccessPath::Search",
        "MemoryAccessPath::AutoPrefetch",
        "MemoryAccessPath::ReviewList",
        "MemoryAccessPath::Citation",
        "MemoryAccessPath::ProposalSimilar",
        "MemoryAccessPath::Resume",
        "collection_denied",
        "purpose_denied",
        "sensitivity_denied",
        "validity_denied",
        "lifecycle_denied",
    ] {
        assert!(
            source.contains(marker),
            "CM-22 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
