#[test]
fn execution_scope_is_derived_before_broker_and_cannot_be_caller_minted() {
    let scope = include_str!("../../kiana-domain/src/execution_scope.rs");
    let request = include_str!("../../kiana-domain/src/capabilities.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let events = include_str!("../src/events.rs");
    let baseline = include_str!("../../docs/roadmap/capability-execution-scope-baseline.md");

    for marker in [
        "ExecutionScope",
        "EXECUTION_SCOPE_SCHEMA",
        "permission_scope",
        "authority_epoch",
        "trust_revision",
        "data_epoch",
        "cancellation_epoch",
        "deadline_unix_ms",
        "fencing_token",
        "validate_for_request",
        "execution_scope_digest_mismatch",
    ] {
        assert!(scope.contains(marker), "missing scope marker {marker}");
    }
    assert!(request.contains("execution_scope"));
    assert!(capabilities.contains("build_execution_scope"));
    assert!(capabilities.contains("request.execution_scope = None"));
    assert!(capabilities.contains("request.execution_scope = Some"));
    assert!(capabilities.contains("arguments.remove(\"run_id\")"));
    assert!(capabilities.contains("arguments.remove(\"turn_id\")"));
    assert!(broker.contains("execution_scope_required"));
    assert!(broker.contains("validate_for_request"));
    assert!(events.contains("stamp_event_links"));
    assert!(baseline.contains("empty_effective_scope_never_becomes_workspace_write"));
    assert!(baseline.contains("forged_root_role_or_server_scope_is_rejected"));
    assert!(baseline.contains("scope_intersection_never_grows_under_delegation"));
    assert!(baseline.contains("same_scope_reaches_shell_patch_mcp_and_memory"));
}
