#[test]
fn scope_intersection_and_monotonic_decisions_are_core_boundaries() {
    let capabilities = include_str!("../src/capabilities.rs");
    let approvals = include_str!("../src/approvals.rs");
    let cell_registry = include_str!("../src/cell_registry.rs");
    let policy = include_str!("../../kiana-policy/src/lib.rs");
    let gates = include_str!("../../kiana-gates/src/lib.rs");
    let scope = include_str!("../../kiana-domain/src/scope.rs");
    let baseline = include_str!("../../docs/roadmap/control-plane-scope-baseline.md");

    assert!(scope.contains("ScopeDimension"));
    assert!(scope.contains("NotApplicable"));
    assert!(scope.contains("Restricted"));
    assert!(scope.contains("scope_intersection_empty"));
    assert!(scope.contains("is_subset_of"));
    assert!(capabilities.contains("effective_action_scope"));
    assert!(capabilities.contains("ScopeSet::new"));
    assert!(capabilities.contains("scope_intersection_invalid"));
    assert!(approvals.contains("if let PolicyDecision::Deny"));
    assert!(approvals.contains("GateDecision::AwaitingApproval"));
    assert!(approvals.contains("gate_authorization_changed"));
    assert!(cell_registry.contains("grant.contains"));
    assert!(cell_registry.contains("spawn_grant_not_contained"));
    assert!(policy.contains("hard_policy_denial"));
    assert!(policy.contains("role_path_denied"));
    assert!(gates.contains("Ask"));
    assert!(gates.contains("Deny"));
    assert!(baseline.contains("cp_allow_all_gate_cannot_override_hard_deny"));
    assert!(baseline.contains("cp_hook_allow_cannot_discharge_other_approval_requirement"));
    assert!(baseline.contains("cp_child_scope_is_subset_for_every_dimension"));
}
