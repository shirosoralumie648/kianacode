#[test]
fn company_plan_graph_is_versioned_and_deterministic() {
    let plan = include_str!("../../kiana-domain/src/plan.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "PLAN_PROPOSAL_SCHEMA",
        "PlanProposal",
        "PlanNode",
        "PlanEdge",
        "PlanEdgeKind::ParentChild",
        "PlanEdgeKind::RequiresArtifact",
        "PlanEdgeKind::RequiresRunSuccess",
        "PlanEdgeKind::RequiresAcceptance",
        "PlanEdgeKind::Related",
        "plan_edge_from_missing",
        "plan_edge_to_missing",
        "plan_dependency_cycle",
        "plan_required_node_isolated",
        "plan_parent_child_kind_invalid",
        "topological_order",
        "charter_baseline_version",
        "coverage_digest",
        "validate_dependency_dag",
    ] {
        assert!(
            plan.contains(marker) || domain.contains(marker) || core.contains(marker),
            "CO-12 marker missing: {marker}"
        );
    }
    assert!(plan.contains("self.digest != self.canonical_digest()"));
    assert!(plan.contains("self.nodes.is_empty()"));
    assert!(!plan.contains("CapabilityBroker"));
    assert!(!plan.contains("EventStorePort"));
    assert!(!core.contains("PlanProposal::topological_order"));
}
