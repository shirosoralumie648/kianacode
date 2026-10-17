#[test]
fn int10_stdio_adapter_is_confined_and_fail_closed() {
    let adapter = include_str!("../src/mcp_connector.rs");
    let stdio = include_str!("../src/mcp_stdio.rs");
    let harness = include_str!("../src/harness_mcp.rs");
    let domain = include_str!("../../kiana-domain/src/mcp_connector.rs");
    let ports = include_str!("../../kiana-ports/src/connector.rs");

    for marker in [
        "StdioMcpConnectorAdapter",
        "McpCapabilityHandshake",
        "capability_handshake_checked",
        "TransportType::Stdio",
        "mcp_http_unsupported",
        "mcp_startup_timeout",
        "mcp_disconnected",
        "result_unknown:mcp_stop_unconfirmed",
        "server_requested_scopes",
        "mcp_server_scope_not_authority",
        "conditional",
        "capability_handshake_checked",
        "mcp_server_binding_mismatch",
        "mcp_connector_transport_unsupported",
        "mcp_project_untrusted",
        "mcp_executable_changed",
        "mcp_snapshot_required",
        "McpSessionState::Closed",
        "retract_unavailable_tools",
        "mcp_session_not_terminal",
    ] {
        assert!(
            adapter.contains(marker)
                || stdio.contains(marker)
                || harness.contains(marker)
                || domain.contains(marker)
                || ports.contains(marker),
            "INT-10 marker missing: {marker}"
        );
    }

    for forbidden in [
        "reqwest::",
        "TcpStream",
        "connect_http",
        "connect_sse",
        "connect_ws",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "stdio connector adapter crossed a remote transport boundary: {forbidden}"
        );
    }
    assert!(adapter.contains("client.stop().await"));
    assert!(
        adapter.contains("CapabilityAdapterCapability") || adapter.contains("CapabilityHandshake")
    );
    // Exactly one spawn point: every handshake child comes from the confined client, and the
    // negotiation outcome is only reported after the confirmed stop next to it.
    assert_eq!(
        adapter.matches("ConfinedMcpClient::connect(").count(),
        1,
        "the adapter must have a single confined spawn point"
    );
    assert!(
        adapter.contains("client.stop().await") && adapter.contains("(Ok(handshake), Ok(_))"),
        "the handshake result must only be reported after a confirmed stop"
    );
    assert!(
        domain.contains("pub fn retract_unavailable_tools")
            && domain.contains("mcp_session_not_terminal")
            && domain.contains("pub retracted: bool"),
        "the terminal reclaim contract must stay domain-owned"
    );
}
