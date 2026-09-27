#[test]
fn connector_mapping_core_facade_keeps_mapping_data_only() {
    let domain = include_str!("../../kiana-domain/src/connector_mapping.rs");
    let core = include_str!("../src/connector_mapping.rs");
    for marker in [
        "ConnectorObjectMapping",
        "ConnectorInputArtifact",
        "ConnectorPageCursor",
        "ConnectorMappedPage",
        "mapping_digest",
        "provenance_digest",
        "forbidden_target",
        "validate_connector_object_mapping",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-25 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "ConnectorAdapter::invoke",
        "consume_approval",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-25 mapping widened authority: {forbidden}"
        );
    }
}
