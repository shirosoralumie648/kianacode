#[test]
fn connector_propagation_core_facade_keeps_revocation_and_views_explicit() {
    let domain = include_str!("../../kiana-domain/src/connector_propagation.rs");
    let core = include_str!("../src/connector_propagation.rs");
    for marker in [
        "ConnectorPropagationFact",
        "data_class",
        "purpose",
        "sharing_grant_digest",
        "retention_policy_digest",
        "data_epoch",
        "tombstone_epoch",
        "memory_invalidated",
        "index_invalidated",
        "cache_invalidated",
        "validate_connector_propagation_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-26 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "cache_write",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-26 propagation widened effect boundary: {forbidden}"
        );
    }
}
