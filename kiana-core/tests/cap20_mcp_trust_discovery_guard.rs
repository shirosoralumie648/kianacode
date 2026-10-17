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
        ],
        "stdio lifecycle",
    );
    // Unsupported-transport dispatch belongs to the registry; the confined stdio client
    // receives a validated stdio config. Keep this refusal checked at its actual owner.
    assert!(
        harness.contains("mcp_transport_unsupported"),
        "CAP-20 unsupported transport refusal missing from MCP registry"
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

/// The card's deny-first and success fixtures must be real behaviour fixtures (they drive the
/// production prepare/handler path and spawn the planned stdio boundary), named in the module,
/// wired into the CAP-20 workflow and recorded in the baseline. Fixture bodies live in the
/// test-only module, so this guard reads the raw daemon source.
#[test]
fn cap20_behavioural_fixtures_are_wired_and_deny_first() {
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let workflow = include_str!("../../.github/workflows/cap20-mcp-trust-discovery.yml");
    let baseline = include_str!("../../docs/roadmap/cap20-mcp-trust-discovery-baseline.md");

    for fixture in [
        "untrusted_or_swapped_mcp_binary_cannot_start",
        "unknown_server_or_ambiguous_tool_is_rejected",
        "discovery_has_no_implicit_host_or_network_authority",
        "stdio_fixture_negotiates_lists_tools_and_has_no_network",
    ] {
        assert!(
            harness.contains(fixture),
            "CAP-20 behavioural fixture missing from the MCP boundary: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "CAP-20 behavioural fixture is not wired into the workflow: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "CAP-20 baseline does not record the fixture: {fixture}"
        );
    }

    require(
        harness,
        &[
            "parse_servers",
            "with_servers",
            "CAP20_MCP_SERVER",
            "config_source",
            "env_keys",
            "credential_refs",
            "server_id",
            "mcp_project_untrusted",
            "mcp_config_changed",
            "mcp_server_unknown",
            "mcp_transport_unsupported",
            "cancelled:mcp_not_started",
            "probe_network",
        ],
        "behavioural MCP trust/discovery boundary",
    );
    for bypass in [
        "execute_without_bwrap",
        "fallback_to_unsandboxed_host",
        "--share-net",
    ] {
        assert!(
            !harness.contains(bypass),
            "CAP-20 behavioural fixtures introduced a bypass marker: {bypass}"
        );
    }
    assert!(
        workflow.contains("bwrap --version") && workflow.contains("python3 --version"),
        "the CAP-20 workflow must install and verify the confined runtime and fixture toolchain"
    );
    assert!(
        workflow.contains("--exact"),
        "CAP-20 behavioural fixtures must run individually and exactly"
    );
}
