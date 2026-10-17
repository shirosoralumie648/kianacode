#[test]
fn connector_surfaces_use_one_typed_command_route_and_never_supply_authority() {
    let protocol = include_str!("../../kiana-domain/src/connector_protocol.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let surface_encoder = include_str!("../src/connector_command.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    let cli = include_str!("../src/cli.rs");
    let product_command = include_str!("../src/product_command.rs");
    let mcp = include_str!("../src/mcp.rs");

    for marker in [
        "ConnectorCommandRequest",
        "RequestEnvelope::connector_command",
        "normalize_connector_intent",
        "connector_server_owned_override",
        "allow_external",
    ] {
        assert!(
            protocol.contains(marker) || client.contains(marker),
            "INT-14 shared normalization marker missing: {marker}"
        );
    }
    for marker in [
        "ConnectorSurfaceCommand",
        "CONNECTOR_SURFACE_COMMAND_SCHEMA",
        "normalize_surface_connector_arguments",
        "control_plane_route",
    ] {
        assert!(
            protocol.contains(marker),
            "INT-14 surface command marker missing: {marker}"
        );
    }
    for marker in [
        "SurfaceCommandRoute",
        "surface_command_route",
        "connector_health_route",
    ] {
        assert!(
            surface_encoder.contains(marker),
            "INT-14 entrypoint encoder marker missing: {marker}"
        );
    }
    for surface in [workbench, web, product_command] {
        assert!(
            surface.contains("connector_command::"),
            "surface must encode connector commands through the shared encoder"
        );
        assert!(
            !surface.contains("binding_snapshot")
                && !surface.contains("\"risk\"")
                && !surface.contains("\"endpoint\""),
            "entrypoint must not provide connector authority fields"
        );
    }
    assert!(
        !cli.contains("binding_snapshot")
            && !cli.contains("\"risk\"")
            && !cli.contains("\"endpoint\""),
        "entrypoint must not provide connector authority fields"
    );
    // The legacy MCP tool server executes tools outside ControlPlane (recorded security
    // finding); it must not mint or normalize connector commands locally either. MCP-originated
    // connector commands enter through the same DaemonHost path as the other surfaces.
    assert!(
        !mcp.contains("normalize_connector_intent")
            && !mcp.contains("ConnectorCommandRequest")
            && !mcp.contains("ConnectorSurfaceCommand"),
        "the legacy MCP tool server must not normalize connector commands locally"
    );
    for forbidden in [
        "CapabilityBroker",
        "authorize_and_execute",
        "dispatch_adapter",
        "EventStore",
        "consume_lease",
    ] {
        assert!(
            !client.contains(forbidden),
            "client crossed execution boundary: {forbidden}"
        );
        assert!(
            !surface_encoder.contains(forbidden),
            "surface encoder crossed execution boundary: {forbidden}"
        );
    }
}
