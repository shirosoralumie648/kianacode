#[test]
fn connector_registry_guard_requires_immutable_digest_and_cas() {
    let domain = include_str!("../../kiana-domain/src/connector_registry.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    for marker in [
        "ConnectorRegistrySnapshot",
        "connector_registry_cas_conflict",
        "connector_registry_version_not_monotonic",
        "content_digest",
        "signature_digest",
        "connector.invoked",
    ] {
        assert!(
            domain.contains(marker) || daemon.contains(marker),
            "missing marker: {marker}"
        );
    }
}
