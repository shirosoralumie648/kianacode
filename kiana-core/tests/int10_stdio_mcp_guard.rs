#[test]
fn int10_handshake_stays_on_controlplane_and_never_trusts_server_scopes() {
    let core = include_str!("../src/connectors.rs");
    let policy = include_str!("../../kiana-policy/src/lib.rs");
    let commands = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let domain = include_str!("../../kiana-domain/src/mcp_connector.rs");
    let domain_connectors = include_str!("../../kiana-domain/src/connectors.rs");

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
                || domain.contains(marker)
                || domain_connectors.contains(marker),
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

/// The INT-10 acceptance fixtures must stay wired to the daemon adapter, the domain contract,
/// the manual lane and the baseline: a rename or a lane edit that silently stops running them
/// fails here instead of shrinking the evidence.
#[test]
fn int10_behavioural_fixtures_are_wired_and_deny_first() {
    let adapter = include_str!("../../kiana-daemon/src/mcp_connector.rs");
    let daemon_fixture = include_str!("../../kiana-daemon/tests/int10_stdio_mcp.rs");
    let domain_fixture = include_str!("../../kiana-domain/tests/int10_stdio_mcp.rs");
    let lane = include_str!("../../.github/workflows/int10-stdio-mcp.yml");
    let baseline = include_str!("../../docs/roadmap/int10-stdio-mcp-baseline.md");

    for name in [
        "mcp_remote_transport_and_foreign_handshakes_are_refused_before_a_child_starts",
        "mcp_scope_advertisement_is_refused_and_the_child_is_reclaimed",
        "mcp_handshake_requires_a_trusted_pinned_executable_and_scope",
        "mcp_startup_timeout_disconnect_and_success_are_bounded_reclaimed_and_audited",
    ] {
        assert!(adapter.contains(name), "daemon fixture missing: {name}");
        assert!(
            lane.contains(name),
            "the lane must run the daemon fixture by name: {name}"
        );
        assert!(
            baseline.contains(name),
            "the baseline must document: {name}"
        );
    }
    let integration = "handshake_is_deny_first_audited_and_reclaims_the_scope_free_session";
    assert!(
        daemon_fixture.contains(integration),
        "the integration fixture is missing"
    );
    assert!(
        lane.contains(integration),
        "the lane must run the daemon integration fixture by name"
    );
    assert!(
        baseline.contains(integration),
        "the baseline must document the integration fixture"
    );

    for name in [
        "handshake_bounds_protocol_session_catalog_and_unknown_fields",
        "declared_scope_intersection_decides_availability",
        "terminal_session_reclaims_conditional_tools_and_cannot_reopen",
        "handshake_evidence_binds_digests_without_session_or_scope_material",
    ] {
        assert!(
            domain_fixture.contains(name),
            "domain fixture missing: {name}"
        );
        assert!(
            baseline.contains(name),
            "the baseline must document: {name}"
        );
    }

    // The daemon fixtures need a real confined child, so the lane must install and verify the
    // runtime and run every named fixture exactly, without an ignore/fallback escape hatch.
    for required in [
        "bubblewrap",
        "bwrap --version",
        "python3 --version",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int10_stdio_mcp --locked",
        "cargo test -p kiana-daemon --test int10_stdio_mcp",
        "--exact",
        "--test-threads=1",
        "--no-run",
        "contents: read",
        "workflow_dispatch:",
    ] {
        assert!(lane.contains(required), "the lane must contain {required}");
    }
    for forbidden in [
        "#[ignore]",
        "--ignored",
        "continue-on-error",
        "execute_without_bwrap",
        "fallback_to_unsandboxed_host",
        "--share-net",
        "push:",
    ] {
        assert!(
            !lane.contains(forbidden),
            "the lane must not contain {forbidden}"
        );
    }
    assert!(
        daemon_fixture.contains("assert_no_survivor")
            && daemon_fixture.contains("connector.mcp_handshake")
            && adapter.contains("assert_no_survivor"),
        "the stdio fixtures must prove the child is reclaimed through host observation"
    );
    assert!(
        !daemon_fixture.contains("#[ignore]") && !adapter.contains("#[ignore]"),
        "INT-10 fixtures must not be ignored"
    );
}
