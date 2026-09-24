#[test]
fn int10_handshake_stays_on_controlplane_and_never_trusts_server_scopes() {
    let core = include_str!("../src/connectors.rs");
    let policy = include_str!("../../kiana-policy/src/lib.rs");
    let commands = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let domain = include_str!("../../kiana-domain/src/mcp_connector.rs");

    for marker in [
        "CONNECTOR_MCP_HANDSHAKE_OPERATION",
        "connector.mcp_handshake",
        "binding_snapshot",
        "binding_authorized",
        "mcp_http_unsupported",
        "session_ref",
        "connector.mcp_handshake",
        "append_idempotent_expected",
        "McpCapabilityHandshakeRequest",
        "mcp_server_scope_not_authority",
    ] {
        assert!(
            core.contains(marker)
                || policy.contains(marker)
                || commands.contains(marker)
                || daemon.contains(marker)
                || domain.contains(marker),
            "INT-10 marker missing: {marker}"
        );
    }

    for forbidden in ["server_requested_scopes", "scope_grant", "authorization_id"] {
        assert!(
            !core.contains(forbidden),
            "ControlPlane normalization must not accept server scope material: {forbidden}"
        );
    }
    assert!(core.contains("RiskLevel::ReadOnly"));
    assert!(core.contains("intent.name == CONNECTOR_MCP_HANDSHAKE_OPERATION"));
}
