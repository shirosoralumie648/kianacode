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
        "connector.mcp_handshake",
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
}
