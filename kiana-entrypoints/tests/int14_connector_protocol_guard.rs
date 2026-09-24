#[test]
fn connector_surfaces_use_one_typed_command_route_and_never_supply_authority() {
    let protocol = include_str!("../../kiana-domain/src/connector_protocol.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    let cli = include_str!("../src/cli.rs");

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
    for surface in [workbench, web, cli] {
        assert!(
            !surface.contains("binding_snapshot")
                && !surface.contains("\"risk\"")
                && !surface.contains("\"endpoint\""),
            "entrypoint must not provide connector authority fields"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "authorize_and_execute",
        "dispatch_adapter",
    ] {
        assert!(
            !client.contains(forbidden),
            "client crossed execution boundary: {forbidden}"
        );
    }
}
