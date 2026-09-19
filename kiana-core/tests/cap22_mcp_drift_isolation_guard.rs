//! CAP-22 source guard for MCP result/schema drift and per-invocation isolation.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-22 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn mcp_drift_invalidates_snapshot_and_keeps_effect_unknown() {
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
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
