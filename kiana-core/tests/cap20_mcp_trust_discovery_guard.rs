//! CAP-20 source guard for trusted MCP configuration and discovery snapshots.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-20 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn mcp_registry_pins_config_binary_trust_and_discovery_before_call() {
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let trust = include_str!("../../kiana-daemon/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        harness,
        &[
            "McpRegistry",
            "McpServerConfig",
            "TransportType::Stdio",
            "mcp_server_unknown",
            "mcp_server_required",
            "mcp_project_untrusted",
            "config_pin",
            "config_hash",
            "mcp-snapshot.v1",
            "mcp-discovery.v1",
            "mcp.discovery_committed",
            "catalog_digest",
            "mcp_config_drift_requires_discovery",
            "mcp_discovery_operator_required",
            "register_static",
        ],
        "MCP registry",
    );
    require(
        stdio,
        &[
            "mcp_snapshot",
            "sealed_executable",
            "mcp_executable_changed",
            "mcp_executable_seal_unavailable",
            "ProcessSupervisor::prepare_command",
            "ProcessSupervisor::stop",
            "mcp_transport_unsupported",
        ],
        "stdio lifecycle",
    );
    require(
        trust,
        &[
            "project_trusted",
            "project_trust_snapshot",
            "StoredProjectTrustAuthority",
        ],
        "project trust",
    );
    require(
        baseline,
        &[
            "untrusted_or_swapped_mcp_binary_cannot_start",
            "unknown_server_or_ambiguous_tool_is_rejected",
            "discovery_has_no_implicit_host_or_network_authority",
        ],
        "CAP-20 card",
    );
    assert!(!harness.contains("connect_http"));
}
