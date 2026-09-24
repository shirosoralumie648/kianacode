#[test]
fn int14_connector_protocol_is_typed_server_normalized_and_deny_first() {
    let protocol = include_str!("../../kiana-domain/src/connector_protocol.rs");
    let core = include_str!("../src/connectors.rs");
    let commands = include_str!("../src/commands.rs");
    let fixture = include_str!("../../kiana-domain/tests/int14_connector_protocol.rs");

    for marker in [
        "CONNECTOR_COMMAND_SCHEMA",
        "ConnectorCommandRequest",
        "ConnectorNormalizedIntent",
        "normalize_connector_intent",
        "ConnectorProtocolErrorCode",
        "ConnectorDataBoundary",
        "connector_server_owned_override",
        "broker_calls",
        "allow_external",
        "control_plane_route",
        "CONNECTOR_RECONCILE_OPERATION",
    ] {
        assert!(
            protocol.contains(marker) || core.contains(marker) || commands.contains(marker),
            "INT-14 marker missing: {marker}"
        );
    }
    for marker in [
        "ProjectUntrusted",
        "AuthenticationRequired",
        "AuthorityOverride",
        "assert_eq!(error.broker_calls, 0)",
    ] {
        assert!(
            fixture.contains(marker),
            "INT-14 deny fixture missing: {marker}"
        );
    }
    for forbidden in ["CapabilityBroker", "authorize_and_execute", "dispatch("] {
        assert!(
            !protocol.contains(forbidden),
            "wire normalizer must not execute or authorize through {forbidden}"
        );
    }
    assert!(core.contains("normalize_connector_intent"));
    assert!(
        core.contains("connector.reconcile") || commands.contains("CONNECTOR_RECONCILE_OPERATION")
    );
}
