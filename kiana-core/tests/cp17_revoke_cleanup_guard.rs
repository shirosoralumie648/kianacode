#[test]
fn cp17_revocation_cleanup_and_cell_retirement_are_fenced_and_idempotent() {
    let registry = include_str!("../src/cell_registry.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let governance = include_str!("../src/data_governance.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let projection = include_str!("../src/resource_projection.rs");
    let platform = include_str!("../src/platform.rs");
    let domain_platform = include_str!("../../kiana-domain/src/platform.rs");
    let cell_fixture = include_str!("p1_c02_cell_lifecycle.rs");
    let reliability = include_str!("p2_k6_01_reliability.rs");
    let data_fixture = include_str!("p2_k7_01_data_governance.rs");
    let swarm_fixture = include_str!("p4_j6_01_bounded_swarm.rs");

    for marker in [
        "commit_spawn",
        "transition_cell",
        "retire_cell",
        "release_resources",
        "resources_released",
        "active_capabilities",
        "cell_capability_in_flight",
        "path_locks.remove",
        "budget.release",
        "cell.retired",
        "spawn_reservation_already_released",
    ] {
        assert!(
            registry.contains(marker)
                || collaboration.contains(marker)
                || cell_fixture.contains(marker),
            "CP-17 cell cleanup marker missing: {marker}"
        );
    }
    for marker in [
        "cancel_swarm_children",
        "SwarmCommand::Cancel",
        "SwarmStatus::ResultUnknown",
        "retire_cell",
        "swarm_sibling_failed",
        "swarm_merged",
    ] {
        assert!(
            collaboration.contains(marker) || swarm_fixture.contains(marker),
            "CP-17 descendant marker missing: {marker}"
        );
    }
    for marker in [
        "run_data_revoked",
        "stop_project_runs",
        "data_revoked",
        "result_unknown:stop_unconfirmed",
        "settle_resumed_cell",
        "ExecutionStatus::ResultUnknown",
    ] {
        assert!(
            governance.contains(marker)
                || lifecycle.contains(marker)
                || data_fixture.contains(marker),
            "CP-17 revoke/stop marker missing: {marker}"
        );
    }
    for marker in [
        "resource.quarantined",
        "resource.released",
        "project_resources_quarantined",
        "release_requires_stop_evidence",
        "result_unknown",
    ] {
        assert!(
            dispatch.contains(marker)
                || projection.contains(marker)
                || reliability.contains(marker),
            "CP-17 quarantine marker missing: {marker}"
        );
    }
    for marker in [
        "FailureIncident",
        "HumanInboxKind::Reconciliation",
        "failure.reconciled",
        "automatic_retry_allowed:false",
        "resource_release_requires_reconciliation",
        "resource_release_stop_unconfirmed",
        "new_request_required",
    ] {
        assert!(
            platform.contains(marker)
                || domain_platform.contains(marker)
                || reliability.contains(marker),
            "CP-17 reconciliation marker missing: {marker}"
        );
    }
    for source in [
        registry,
        collaboration,
        lifecycle,
        governance,
        dispatch,
        projection,
    ] {
        for forbidden in [
            "auto_retry_unknown",
            "release_unknown_budget",
            "retire_without_stop",
        ] {
            assert!(
                !source.contains(forbidden),
                "CP-17 bypass marker: {forbidden}"
            );
        }
    }
}
