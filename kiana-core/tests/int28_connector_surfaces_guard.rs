#[test]
fn connector_surface_facade_is_read_only_and_shared() {
    let domain = include_str!("../../kiana-domain/src/connector_surfaces.rs");
    let core = include_str!("../src/connector_surfaces.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "ConnectorSurface::Cli",
        "ConnectorSurface::Web",
        "ConnectorSurface::Workbench",
        "ConnectorSurface::Mcp",
        "ConnectorSurfaceQuery",
        "ConnectorSurfaceResponse",
        "read_only",
        "limitation",
        "validate_connector_query",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || protocol.contains(marker),
            "INT-28 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "consume_lease",
        "auto_approve",
        "actor_id",
        "approval_id",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-28 surface facade widened authority: {forbidden}"
        );
    }
}
