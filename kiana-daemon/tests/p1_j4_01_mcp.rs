#[test]
fn mcp_tool_schema_and_health_are_traceable() {
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let harness = include_str!("../src/harness_mcp.rs");
    let stdio = include_str!("../src/mcp_stdio.rs");
    let baseline = include_str!("../../docs/roadmap/p1-j4-01-mcp-baseline.md");
    for marker in [
        "CapabilityKind::Network",
        "mcp.call",
        "mcp.discover",
        "minimum_risk",
        "resource_fields",
        "reconciliation",
        "idempotency",
        "register_static",
        "catalog_sealed",
        "mcp-snapshot.v1",
        "mcp-discovery.v1",
        "mcp.discovery_committed",
        "mcp_health_unavailable",
        "health_snapshot",
        "catalog_digest",
        "mcp_tool_schema_changed",
        "mcp_output_schema_mismatch",
        "mcp_result_limit",
        "mcp_config_drift_requires_discovery",
        "mcp_discovery_operator_required",
        "mcp_project_untrusted",
        "mcp_transport_unsupported",
        "mcp_response_frame_limit",
        "mcp_response_byte_limit",
        "process_group",
        "stop_confirmed",
        "result_unknown:mcp_stop_unconfirmed",
    ] {
        assert!(
            actions.contains(marker)
                || broker.contains(marker)
                || harness.contains(marker)
                || stdio.contains(marker)
                || baseline.contains(marker),
            "MCP lifecycle marker missing: {marker}"
        );
    }
    assert!(harness.contains("TransportType::Http"));
    assert!(harness.contains("McpInvocationClient"));
    assert!(!harness.contains("server.transport != TransportType::Stdio"));
    assert!(harness.contains("mcp.discovery_committed"));
    assert!(harness.contains("health_snapshot(&protocol, &tools)"));
    assert!(stdio.contains("2025-06-18"));
    assert!(stdio.contains("kill_on_drop(true)"));
    assert!(harness.contains("mcp_http_endpoint_denied"));
}
