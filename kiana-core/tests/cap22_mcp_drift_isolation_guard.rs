//! CAP-22 source guard for MCP result/schema drift and per-invocation isolation.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        let present = match *marker {
            "mcp_result_unknown" => {
                let run = source_guard::item_source(source, "async fn run(");
                let code = source_guard::compact_code(&run);
                code.contains("letneeds_reconciliation=!stop_confirmed||(started&&performed.is_err());")
                    && code.contains("ifcancelled||needs_reconciliation{")
                    && code.contains("CapabilityErrorCode::ResultUnknown")
                    && code.contains("mcp_call_unconfirmed")
                    && code.contains("if!started&&stop_confirmed{AdapterCommitState::NotStarted}else{AdapterCommitState::Unknown}")
                    && code.contains("attach_adapter_result(result,AdapterResultKind::Mcp,commit)")
            }
            _ => source.contains(marker),
        };
        assert!(present, "CAP-22 {label} marker missing: {marker}");
    }
}

#[test]
fn mcp_drift_invalidates_snapshot_and_keeps_effect_unknown() {
    let harness =
        &source_guard::production_source(include_str!("../../kiana-daemon/src/harness_mcp.rs"));
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let approvals = include_str!("../src/approvals.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        harness,
        &[
            "mcp_config_drift_requires_discovery",
            "mcp_catalog_changed",
            "mcp_tool_schema_changed",
            "mcp_arguments_schema_mismatch",
            "mcp_output_schema_mismatch",
            "mcp_result_unknown",
            "mcp_call_unconfirmed",
            "mcp_workspace_retain_unconfirmed",
            "server_config_hash",
            "catalog_digest",
            "input_schema_hash",
            "output_schema_hash",
            "process_scope",
            "resource_links_fetched",
        ],
        "MCP catalog/result boundary",
    );
    require(
        stdio,
        &[
            "catalog_changed",
            "mcp_response_id_mismatch",
            "mcp_response_envelope_invalid",
            "mcp_provider_error",
            "ProcessSupervisor::stop",
        ],
        "stdio response boundary",
    );
    require(
        approvals,
        &[
            "approval_action_changed",
            "approval_requirements_changed",
            "request_hash",
        ],
        "approval invalidation",
    );
    require(
        baseline,
        &[
            "mcp_output_schema_failure_after_call_is_not_safe_retry",
            "mcp_schema_drift_invalidates_pending_approval",
            "mcp_pool_never_crosses_owner_or_scope",
        ],
        "CAP-22 card",
    );
    assert!(harness.contains("per_invocation"));
    assert!(!harness.contains("shared_mcp_pool"));
}

/// The card's deny-first and bounded-result fixtures must be real behaviour fixtures: they drive
/// the production `McpRegistry::prepare`/`McpHandler::run` path (`ConfinedMcpClient` under the
/// bwrap boundary), are named in the daemon module, wired into the CAP-22 workflow and recorded
/// in the baseline. Fixture bodies live in the test-only module, so this guard reads the raw
/// daemon source. The result/schema checks must also stay ahead of the per-invocation connect.
#[test]
fn cap22_behavioural_fixtures_are_wired_and_deny_first() {
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let workflow = include_str!("../../.github/workflows/cap22-mcp-drift-isolation.yml");
    let baseline = include_str!("../../docs/roadmap/cap22-mcp-drift-isolation-baseline.md");

    for fixture in [
        "mcp_output_schema_failure_after_call_is_not_safe_retry",
        "mcp_schema_drift_invalidates_pending_approval",
        "mcp_pool_never_crosses_owner_or_scope",
        "mcp_legal_results_errors_and_resource_links_stay_bounded",
        "mcp_explicit_revoke_and_config_change_fence_prepared_authorization",
        "mcp_result_validator_bounds_structured_content_and_content_types",
    ] {
        assert!(
            harness.contains(fixture),
            "CAP-22 behavioural fixture missing from kiana-daemon/src/harness_mcp.rs: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "CAP-22 behavioural fixture not wired into the focused lane: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "CAP-22 behavioural fixture not recorded in the baseline: {fixture}"
        );
    }
    for marker in [
        "const CAP22_MCP_SERVER",
        "cap22_catalog",
        "cap22_context",
        "cap22_discover",
        "cap22_call_request",
        "mcp_structured_content_required",
        "mcp_output_schema_mismatch",
        "mcp_content_invalid",
        "mcp_content_limit",
        "mcp_catalog_changed",
        "mcp_snapshot_scope_changed",
        "mcp_config_changed",
        "mcp_config_drift_requires_discovery",
        "mcp_discovery_required",
        "mcp_project_untrusted",
        "mcp_call_unconfirmed",
        "capability_action_digest",
        "per_invocation",
        "resource_links_fetched",
    ] {
        assert!(
            harness.contains(marker),
            "CAP-22 fixture coverage marker missing: {marker}"
        );
    }
    assert!(
        !harness.contains("#[ignore]"),
        "CAP-22 fixtures must run, not be ignored"
    );

    // A prepared call may only reach the per-invocation connect after its scope identity and the
    // committed catalog have both been compared against the snapshot.
    let run = source_guard::item_source(harness, "async fn run(");
    let code = source_guard::compact_code(&run);
    let scope = code
        .find("mcp_snapshot_scope_changed")
        .expect("the run loop must compare the prepared scope identity");
    let catalog = code
        .find("mcp_catalog_changed")
        .expect("the run loop must compare the committed catalog");
    let connect = code
        .find("McpInvocationClient::connect(")
        .expect("the run loop must connect the per-invocation client");
    assert!(
        scope < connect,
        "owner/scope drift must be refused before any process starts"
    );
    assert!(
        catalog < connect,
        "catalog drift must be refused before any process starts"
    );

    for wiring in [
        "bubblewrap",
        "bwrap --version",
        "python3 --version",
        "harness_mcp::tests::",
        "--exact",
        "--test-threads=1",
        "cap22_mcp_drift_isolation_guard",
        "p1_j4_01_mcp",
        "--no-run",
    ] {
        assert!(
            workflow.contains(wiring),
            "CAP-22 focused lane is missing required wiring: {wiring}"
        );
    }
    for bypass in ["--share-net", "privileged: true"] {
        assert!(
            !workflow.contains(bypass),
            "CAP-22 focused lane must not grant sandbox bypass: {bypass}"
        );
    }
    assert!(
        baseline.contains("No local runtime tests"),
        "CAP-22 baseline must state that the authoring session ran no local runtime tests"
    );
}
