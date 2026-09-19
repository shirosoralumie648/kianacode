//! CAP-29 source guard for the Streamable HTTP/SSE MCP product adapter.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-29 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn http_mcp_uses_the_same_discovery_catalog_and_unknown_boundary() {
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let http = include_str!("../../kiana-daemon/src/mcp_http.rs");
    let services = include_str!("../../kiana-services/src/mcp.rs");
    let network = include_str!("../../kiana-services/src/network_policy.rs");
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let p1 = include_str!("../../kiana-daemon/tests/p1_j4_01_mcp.rs");
    let cap22 = include_str!("cap22_mcp_drift_isolation_guard.rs");
    let baseline = include_str!("../../docs/roadmap/cap29-http-mcp-baseline.md");

    require(
        harness,
        &[
            "TransportType::Http",
            "TransportType::Sse",
            "McpInvocationClient",
            "mcp_http_endpoint_denied",
            "mcp_http_raw_credential_header_denied",
            "mcp.discovery_committed",
            "catalog_digest",
            "mcp_catalog_changed",
            "result_unknown:mcp_call_unconfirmed",
        ],
        "daemon HTTP route",
    );
    require(
        http,
        &[
            "HttpMcpClient",
            "streamable_http",
            "server-owned-not-authority",
            "call_started",
            "result_unknown:mcp_http_call_unconfirmed",
            "mcp_http_raw_credential_header_denied",
        ],
        "HTTP adapter",
    );
    require(
        services,
        &[
            "HttpMcpTransport",
            "SseMcpTransport",
            "text/event-stream",
            "mcp_redirect_policy",
            "sessionId",
            "json_rpc_result",
        ],
        "HTTP/SSE wire transport",
    );
    require(
        network,
        &[
            "HttpNetworkSurface::HttpMcp",
            "validate_http_redirect",
            "same_origin_redirects_only",
        ],
        "egress boundary",
    );
    require(
        stdio,
        &[
            "call_started",
            "mcp_response_id_mismatch",
            "ProcessSupervisor::stop",
        ],
        "shared result/stop semantics",
    );
    require(
        p1,
        &["TransportType::Http", "McpInvocationClient"],
        "P1-J4 regression",
    );
    require(
        cap22,
        &[
            "mcp_result_unknown",
            "mcp_output_schema_mismatch",
            "resource_links_fetched",
        ],
        "CAP-22 regression",
    );
    require(
        baseline,
        &[
            "http_mcp_token_audience_or_owner_mismatch_is_denied",
            "http_mcp_disconnect_after_send_is_result_unknown",
            "sse_reconnect_never_replays_tools_call",
        ],
        "CAP-29 acceptance card",
    );
    assert!(!harness.contains("CapabilityBroker::new"));
    assert!(!http.contains("ModelClient"));
}

#[test]
fn http_mcp_keeps_remote_effect_and_session_limits_explicit() {
    let http = include_str!("../../kiana-daemon/src/mcp_http.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let services = include_str!("../../kiana-services/src/mcp.rs");
    let baseline = include_str!("../../docs/roadmap/cap29-http-mcp-baseline.md");
    assert!(http.contains("session_identity"));
    assert!(http.contains("stop_confirmed"));
    assert!(harness.contains("\"transport\":\"stdio\""));
    assert!(services.contains("Duration::from_secs(10)"));
    require(
        baseline,
        &[
            "remote effect",
            "not bwrap",
            "live interoperability",
            "reconnection remains",
        ],
        "explicit limitations",
    );
}
