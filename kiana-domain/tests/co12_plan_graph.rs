use kiana_domain::*;

fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn node(id: &str, kind: PlanNodeKind, required: bool) -> PlanNode {
    PlanNode {
        node_id: id.to_owned(),
        project_id: "project-1".to_owned(),
        kind,
        version: 1,
        required,
        criterion_refs: Vec::new(),
    }
}

fn edge(from: &str, to: &str, kind: PlanEdgeKind, reference: Option<&str>) -> PlanEdge {
    PlanEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
        reference: reference.map(str::to_owned),
    }
}

#[test]
fn plan_rejects_missing_dependencies_cycles_and_cross_project_edges() {
    let missing = PlanProposal::new(
        "project-1",
        1,
        digest(),
        vec![node("m1", PlanNodeKind::Milestone, true)],
        vec![edge(
            "m1",
            "missing",
            PlanEdgeKind::RequiresRunSuccess,
            Some("run:1"),
        )],
    );
    assert_eq!(missing.unwrap_err(), "plan_edge_to_missing");

    let cycle = PlanProposal::new(
        "project-1",
        1,
        digest(),
        vec![
            node("m1", PlanNodeKind::Milestone, true),
            node("m2", PlanNodeKind::Milestone, true),
        ],
        vec![
            edge("m1", "m2", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
            edge("m2", "m1", PlanEdgeKind::RequiresRunSuccess, Some("run:2")),
        ],
    );
    assert_eq!(cycle.unwrap_err(), "plan_dependency_cycle");

    let mut foreign = node("m2", PlanNodeKind::Milestone, true);
    foreign.project_id = "other-project".to_owned();
    assert_eq!(
        PlanProposal::new(
            "project-1",
            1,
            digest(),
            vec![node("m1", PlanNodeKind::Milestone, true), foreign],
            vec![edge(
                "m1",
                "m2",
                PlanEdgeKind::RequiresRunSuccess,
                Some("run:1")
            )],
        )
        .unwrap_err(),
        "plan_node_project_or_version_invalid"
    );
}

#[test]
fn two_milestone_plan_preserves_dependency_and_acceptance_boundaries() {
    let plan = PlanProposal::new(
        "project-1",
        3,
        digest(),
        vec![
            node("m1", PlanNodeKind::Milestone, true),
            node("m2", PlanNodeKind::Milestone, true),
            node("p1", PlanNodeKind::Packet, true),
            node("p2", PlanNodeKind::Packet, true),
        ],
        vec![
            edge(
                "m1",
                "m2",
                PlanEdgeKind::RequiresAcceptance,
                Some("acceptance:m1"),
            ),
            edge("m1", "p1", PlanEdgeKind::ParentChild, None),
            edge("m2", "p2", PlanEdgeKind::ParentChild, None),
        ],
    )
    .expect("plan");
    let order = plan.topological_order().expect("topological order");
    assert!(order.iter().position(|id| id == "m1") < order.iter().position(|id| id == "m2"));
    assert!(order.iter().position(|id| id == "m1") < order.iter().position(|id| id == "p1"));
    assert!(order.iter().position(|id| id == "m2") < order.iter().position(|id| id == "p2"));
    assert_eq!(plan.digest, plan.canonical_digest());
}
