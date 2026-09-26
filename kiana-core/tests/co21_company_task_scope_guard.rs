#[test]
fn company_and_standalone_runs_share_one_fresh_scope_boundary() {
    let scope = include_str!("../../kiana-domain/src/company_task_scope.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let company = include_str!("../src/company.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");

    for marker in [
        "COMPANY_TASK_SCOPE_SCHEMA",
        "FRESH_TASK_RUN_SCHEMA",
        "TaskExecutionMode::Company",
        "TaskExecutionMode::Standalone",
        "company_task_private_history_forbidden",
        "company_task_path_scope_mismatch",
        "standalone_company_binding_forbidden",
        "company_task_session_must_be_fresh",
        "authorized_retrieval_refs",
        "FreshTaskRun",
        "spawn_from_packet",
        "CompanyTaskScope::company_builder",
        "CompanyTaskScope::standalone",
        "work_packet_id = Some(packet_id.clone())",
        "bind_company_run_scope",
        "spawn_session_not_fresh",
        "RequestContext",
    ] {
        assert!(
            scope.contains(marker)
                || collaboration.contains(marker)
                || company.contains(marker)
                || lifecycle.contains(marker)
                || daemon.contains(marker),
            "CO-21 marker missing: {marker}"
        );
    }
    assert!(scope.contains("private_history_forbidden"));
    assert!(!scope.contains("CapabilityBroker"));
    assert!(!scope.contains("EventStorePort"));
}
