#[test]
fn cleanup_contract_and_existing_supervisors_keep_unknown_isolated() {
    let domain = include_str!("../../kiana-domain/src/resource_cleanup.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let core = include_str!("../src/lib.rs");
    for marker in [
        "CleanupCause",
        "CleanupResourceKind",
        "IsolatedUnknown",
        "stop_confirmed",
        "effect_known",
        "safe_to_release_all",
        "ResourceRetention",
    ] {
        assert!(
            domain.contains(marker),
            "missing H33 cleanup marker: {marker}"
        );
    }
    for marker in ["RunCancellation", "unregister_in_flight", "checkpoint"] {
        assert!(
            runner.contains(marker),
            "missing runner cleanup marker: {marker}"
        );
    }
    for marker in ["kill_on_drop", "stop_confirmed", "ProcessGuard", "Unknown"] {
        assert!(
            supervisor.contains(marker),
            "missing process cleanup marker: {marker}"
        );
    }
    for marker in ["pub async fn shutdown", "flush", "shutdown_observability"] {
        assert!(
            daemon.contains(marker),
            "missing daemon shutdown marker: {marker}"
        );
    }
    for marker in ["release_builder_path_locks", "TerminalScopeGuard"] {
        assert!(
            core.contains(marker),
            "missing Core release marker: {marker}"
        );
    }
}

#[test]
fn cleanup_does_not_equate_host_drop_with_confirmed_stop() {
    let domain = include_str!("../../kiana-domain/src/resource_cleanup.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    assert!(domain.contains("result_unknown"));
    assert!(domain.contains("stop_confirmed"));
    assert!(supervisor.contains("ProcessGroupState::Unknown"));
    assert!(supervisor.contains("confirmed"));
}
