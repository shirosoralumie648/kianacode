#[test]
fn sw04_child_grant_guard_keeps_all_authority_layers_before_cell_admission() {
    let capabilities = include_str!("../src/capabilities.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let registry = include_str!("../src/cell_registry.rs");
    let swarm = include_str!("../src/swarm.rs");
    for marker in [
        "derive_swarm_child_grant",
        "GrantScope::intersect_all",
        "template_scope",
        "department_scope",
        "project_scope",
        "packet_scope",
        "approval_scope",
        "swarm_packet_project_mismatch",
        "swarm_controller_delegation_invalid",
        "swarm_authority_epoch_missing",
        "parent.contains(&child)",
    ] {
        assert!(
            capabilities.contains(marker),
            "missing capability marker: {marker}"
        );
    }
    assert!(collaboration.contains("derive_swarm_child_grant"));
    assert!(collaboration.contains("parent.authority_epoch"));
    for marker in [
        "budget_is_subset",
        "spawn_budget_not_contained",
        "spawn_grant_not_contained",
        "parent.reservation.grant.delegation_allowed",
    ] {
        assert!(
            registry.contains(marker),
            "missing cell registry marker: {marker}"
        );
    }
    for marker in [
        "authority_epoch",
        "swarm_authority_epoch_stale",
        "swarm_controller_owner_mismatch",
        "ensure_swarm_controller",
    ] {
        assert!(
            swarm.contains(marker),
            "missing swarm fence marker: {marker}"
        );
    }
    assert!(!capabilities.contains("CapabilityBroker"));
    assert!(!capabilities.contains("DaemonHost"));
}
