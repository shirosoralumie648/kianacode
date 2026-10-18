use kiana_domain::*;
use std::collections::BTreeSet;
use uuid::Uuid;

fn digest_a() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn digest_b() -> String {
    format!("sha256:{}", "b".repeat(64))
}

fn scope_digest() -> String {
    json_digest(&serde_json::json!({"project": "project-1"}))
}

fn cid(value: u128) -> CriterionId {
    CriterionId::from_uuid(Uuid::from_u128(value))
}

fn node(id: &str, kind: PlanNodeKind, required: bool) -> PlanNode {
    PlanNode {
        node_id: id.to_owned(),
        project_id: "project-1".to_owned(),
        kind,
        version: 1,
        required,
        criterion_refs: Vec::new(),
        input_refs: Vec::new(),
    }
}

fn node_with(
    id: &str,
    kind: PlanNodeKind,
    required: bool,
    criterion_refs: Vec<CriterionId>,
    input_refs: Vec<String>,
) -> PlanNode {
    PlanNode {
        criterion_refs,
        input_refs,
        ..node(id, kind, required)
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

/// Craft a proposal value without going through the freeze contract, exactly like a wire payload
/// that claims to be a plan. The digest is recomputed so every refusal below is about the graph
/// contract itself and never about a stale self-digest.
fn raw_plan(
    nodes: Vec<PlanNode>,
    edges: Vec<PlanEdge>,
    required_criteria: Vec<CriterionId>,
    read_scope: Vec<String>,
    plan_version: u64,
    charter_baseline_version: u64,
    coverage_digest: String,
) -> PlanProposal {
    let mut plan = PlanProposal {
        schema: PLAN_PROPOSAL_SCHEMA.to_owned(),
        project_id: "project-1".to_owned(),
        charter_baseline_version,
        plan_version,
        nodes: nodes
            .into_iter()
            .map(|node| (node.node_id.clone(), node))
            .collect(),
        edges,
        required_criteria,
        read_scope,
        coverage_digest,
        digest: String::new(),
    };
    plan.digest = plan.canonical_digest();
    plan
}

/// Two-milestone candidate built as a batch, in a deliberately non-topological order: blocking
/// edges and children are pushed before the nodes they point at.
fn two_milestone_draft() -> PlanProposalDraft {
    let mut draft = PlanProposalDraft::new();
    draft
        .push_edge(edge("m1", "p1", PlanEdgeKind::ParentChild, None))
        .expect("edge");
    draft
        .push_edge(edge(
            "m1",
            "m2",
            PlanEdgeKind::RequiresAcceptance,
            Some("acceptance:m1"),
        ))
        .expect("edge");
    draft
        .push_edge(edge("m2", "p2", PlanEdgeKind::ParentChild, None))
        .expect("edge");
    draft
        .push_edge(edge("m1", "p2", PlanEdgeKind::Related, None))
        .expect("edge");
    draft
        .push_node(node("p1", PlanNodeKind::Packet, true))
        .expect("node");
    draft
        .push_node(node("m2", PlanNodeKind::Milestone, true))
        .expect("node");
    draft
        .push_node(node("p2", PlanNodeKind::Packet, true))
        .expect("node");
    draft
        .push_node(node("m1", PlanNodeKind::Milestone, true))
        .expect("node");
    draft
}

fn two_milestone_plan() -> PlanProposal {
    two_milestone_draft()
        .freeze_versioned("project-1", 1, 2, digest_a())
        .expect("plan")
}

fn revision(plan_version: u64) -> PlanProposal {
    two_milestone_draft()
        .freeze_versioned("project-1", 1, plan_version, digest_a())
        .expect("plan revision")
}

/// Crafted revision with another baseline/coverage identity for the drift matrix.
fn crafted_revision(
    plan_version: u64,
    baseline_version: u64,
    coverage_digest: String,
) -> PlanProposal {
    raw_plan(
        vec![
            node("m1", PlanNodeKind::Milestone, true),
            node("p1", PlanNodeKind::Packet, true),
        ],
        vec![edge("m1", "p1", PlanEdgeKind::ParentChild, None)],
        Vec::new(),
        Vec::new(),
        plan_version,
        baseline_version,
        coverage_digest,
    )
}

fn authority(actor: &str, role: &str) -> CompanyAuthority {
    CompanyAuthority {
        actor_id: actor.to_owned(),
        role_id: role.to_owned(),
        session_id: SessionId::new("co12-session"),
        now_ms: 42,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

fn approved_project() -> Project {
    Project {
        project_id: "project-1".to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope".to_owned(),
        success_criteria: vec!["用户登录可靠".to_owned()],
        non_goals: vec!["no scope creep".to_owned()],
        project_budget_ref: "budget:1".to_owned(),
        risk_summary: "low".to_owned(),
        decision_ref: Some("artifact:charter-decision".to_owned()),
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Approved,
        version: 2,
    }
}

fn charter_baseline() -> ProjectCharterBaseline {
    ProjectCharterBaseline {
        project_id: "project-1".to_owned(),
        version: 1,
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope".to_owned(),
        success_criteria: vec!["用户登录可靠".to_owned()],
        non_goals: vec!["no scope creep".to_owned()],
        risk_summary: "low".to_owned(),
        project_budget_ref: "budget:1".to_owned(),
        budget_digest: digest_a(),
        charter_draft_digest: None,
        charter_draft_version: None,
        stakeholder_principals: Vec::new(),
    }
}

fn approved_state() -> CompanyState {
    let mut state = CompanyState::default();
    state
        .projects
        .insert("project-1".to_owned(), approved_project());
    state
        .charter_baselines
        .insert("project-1".to_owned(), charter_baseline());
    state
}

fn plan_for_coverage(coverage_digest: &str) -> PlanProposal {
    let mut draft = PlanProposalDraft::new();
    draft
        .push_node(node("m1", PlanNodeKind::Milestone, true))
        .expect("node");
    draft
        .push_node(node("p1", PlanNodeKind::Packet, true))
        .expect("node");
    draft
        .push_edge(edge("m1", "p1", PlanEdgeKind::ParentChild, None))
        .expect("edge");
    draft.freeze("project-1", 1, coverage_digest).expect("plan")
}

/// One frozen CO-11 coverage whose approval signs the exact coverage-bound plan digest.
fn frozen_coverage() -> FrozenCriterionCoverage {
    let criterion = Criterion::new(
        cid(90),
        "charter:v1",
        1,
        "plan covers the acceptance criterion",
        "ci_fixture",
        vec!["verification_log".to_owned()],
        false,
        scope_digest(),
    )
    .expect("criterion");
    let graph = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope_digest(),
        vec![CriterionCoverageEntry {
            criterion,
            target: AcceptanceTarget::Project("project-1".to_owned()),
        }],
        Vec::new(),
        BTreeSet::new(),
    )
    .expect("coverage graph");
    let plan = plan_for_coverage(&graph.digest);
    let approval = CriterionCoverageApproval::new(
        "coverage-approval-co12",
        "project-1",
        graph.baseline_version,
        graph.digest.clone(),
        plan.digest.clone(),
        "artifact:coverage-decision",
        "assignment-sponsor-1",
        "sponsor",
        42,
    )
    .expect("coverage approval");
    FrozenCriterionCoverage::freeze(graph, approval).expect("frozen coverage")
}

#[test]
fn plan_rejects_missing_dependencies_cycles_and_cross_project_edges() {
    let missing = PlanProposal::new(
        "project-1",
        1,
        digest_a(),
        vec![node("m1", PlanNodeKind::Milestone, true)],
        vec![edge(
            "m1",
            "missing",
            PlanEdgeKind::RequiresRunSuccess,
            Some("run:1"),
        )],
    );
    assert_eq!(missing.unwrap_err(), "plan_edge_to_missing");

    // The same candidate graph reports the unresolved endpoint by edge index instead of hiding it
    // behind the first error, and the report is deterministic.
    let candidate = raw_plan(
        vec![node("m1", PlanNodeKind::Milestone, true)],
        vec![edge(
            "m1",
            "missing",
            PlanEdgeKind::RequiresRunSuccess,
            Some("run:1"),
        )],
        Vec::new(),
        Vec::new(),
        1,
        1,
        digest_a(),
    );
    let reported = candidate.graph_report();
    assert_eq!(reported.rejection.as_deref(), Some("plan_edge_to_missing"));
    assert_eq!(reported.node_count, 1);
    assert_eq!(reported.edge_count, 1);
    assert_eq!(reported.topological_order, None);
    assert_eq!(reported.cycle_path, None);
    assert_eq!(reported.missing_references.len(), 1);
    assert_eq!(
        reported.missing_references[0].kind,
        PlanReferenceKind::EdgeTo
    );
    assert_eq!(reported.missing_references[0].edge_index, 0);
    assert_eq!(reported.missing_references[0].reference, "missing");
    assert_eq!(reported.missing_references[0].peer, "m1");
    assert_eq!(reported, candidate.graph_report());

    let cycle = PlanProposal::new(
        "project-1",
        1,
        digest_a(),
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

    // The cycle path is stable across edge permutations and names the blocking loop only.
    let cycle_edges = vec![
        edge("m1", "m2", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
        edge("m2", "m1", PlanEdgeKind::RequiresRunSuccess, Some("run:2")),
        edge("m1", "m3", PlanEdgeKind::Related, None),
    ];
    let mut permuted = cycle_edges.clone();
    permuted.reverse();
    for edges in [cycle_edges, permuted] {
        let candidate = raw_plan(
            vec![
                node("m1", PlanNodeKind::Milestone, true),
                node("m2", PlanNodeKind::Milestone, true),
                node("m3", PlanNodeKind::Packet, true),
            ],
            edges,
            Vec::new(),
            Vec::new(),
            1,
            1,
            digest_a(),
        );
        assert_eq!(
            candidate.cycle_path(),
            Some(vec!["m1".to_owned(), "m2".to_owned(), "m1".to_owned()])
        );
        assert_eq!(
            candidate.graph_report().rejection.as_deref(),
            Some("plan_dependency_cycle")
        );
        assert_eq!(candidate.graph_report().topological_order, None);
    }

    let mut foreign = node("m2", PlanNodeKind::Milestone, true);
    foreign.project_id = "other-project".to_owned();
    assert_eq!(
        PlanProposal::new(
            "project-1",
            1,
            digest_a(),
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
fn plan_rejects_empty_required_milestones_isolated_criteria_and_narrow_read_scope() {
    // A required Milestone whose only child is an optional Packet is a planning hole.
    assert_eq!(
        PlanProposal::new(
            "project-1",
            1,
            digest_a(),
            vec![
                node("m1", PlanNodeKind::Milestone, true),
                node("p1", PlanNodeKind::Packet, true),
                node("p2", PlanNodeKind::Packet, false),
            ],
            vec![
                edge("m1", "p2", PlanEdgeKind::ParentChild, None),
                edge("m1", "p1", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
            ],
        )
        .unwrap_err(),
        "plan_required_milestone_empty"
    );
    // Making that output required is the only difference between refused and admitted.
    assert!(PlanProposal::new(
        "project-1",
        1,
        digest_a(),
        vec![
            node("m1", PlanNodeKind::Milestone, true),
            node("p1", PlanNodeKind::Packet, true),
            node("p2", PlanNodeKind::Packet, true),
        ],
        vec![
            edge("m1", "p2", PlanEdgeKind::ParentChild, None),
            edge("m1", "p1", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
        ],
    )
    .is_ok());

    // A declared required criterion has to be owned by a required node; text on an optional
    // node (or no node at all) is an isolated criterion.
    let mut draft = two_milestone_draft();
    draft.require_criterion(cid(5)).expect("criterion");
    assert_eq!(
        draft.freeze("project-1", 1, digest_a()).unwrap_err(),
        "plan_required_criterion_isolated"
    );
    let mut owned = two_milestone_draft();
    owned.nodes.insert(
        "p1".to_owned(),
        node_with("p1", PlanNodeKind::Packet, true, vec![cid(5)], Vec::new()),
    );
    owned.require_criterion(cid(5)).expect("criterion");
    owned.require_criterion(cid(6)).expect("second criterion");
    assert_eq!(
        owned.freeze("project-1", 1, digest_a()).unwrap_err(),
        "plan_required_criterion_isolated"
    );

    // The read scope is closed: a node input outside it is insufficient, entries are typed,
    // bounded and unique.
    let base_nodes = |inputs: Vec<String>| {
        vec![
            node("m1", PlanNodeKind::Milestone, true),
            node_with("p1", PlanNodeKind::Packet, true, Vec::new(), inputs),
        ]
    };
    let base_edge = || vec![edge("m1", "p1", PlanEdgeKind::ParentChild, None)];
    assert_eq!(
        PlanProposal::new(
            "project-1",
            1,
            digest_a(),
            base_nodes(vec!["artifact:charter".to_owned()]),
            base_edge(),
        )
        .unwrap_err(),
        "plan_read_scope_insufficient"
    );
    let mut narrow = two_milestone_draft();
    narrow.permit_read("data/loose/path").expect("scope entry");
    assert_eq!(
        narrow.freeze("project-1", 1, digest_a()).unwrap_err(),
        "plan_read_scope_reference_invalid"
    );
    let mut duplicated = two_milestone_draft();
    duplicated
        .permit_read("artifact:charter")
        .expect("scope entry");
    duplicated
        .permit_read("artifact:charter")
        .expect("scope entry");
    assert_eq!(
        duplicated.freeze("project-1", 1, digest_a()).unwrap_err(),
        "plan_read_scope_reference_duplicate"
    );
    let mut duplicated_input = two_milestone_draft();
    duplicated_input.nodes.insert(
        "p1".to_owned(),
        node_with(
            "p1",
            PlanNodeKind::Packet,
            true,
            Vec::new(),
            vec!["artifact:charter".to_owned(), "artifact:charter".to_owned()],
        ),
    );
    duplicated_input
        .permit_read("artifact:charter")
        .expect("scope entry");
    assert_eq!(
        duplicated_input
            .freeze("project-1", 1, digest_a())
            .unwrap_err(),
        "plan_node_input_reference_invalid"
    );
    let mut sufficient = two_milestone_draft();
    sufficient.nodes.insert(
        "p1".to_owned(),
        node_with(
            "p1",
            PlanNodeKind::Packet,
            true,
            Vec::new(),
            vec!["artifact:charter".to_owned()],
        ),
    );
    sufficient
        .permit_read("artifact:charter")
        .expect("scope entry");
    assert!(sufficient.freeze("project-1", 1, digest_a()).is_ok());
}

#[test]
fn plan_draft_accepts_any_insert_order_and_validates_once_at_freeze() {
    let mut draft = PlanProposalDraft::new();
    assert_eq!(draft.schema, PLAN_DRAFT_SCHEMA);
    assert_eq!(PlanProposalDraft::default(), draft);

    // Edges first, then children, then parents: batch creation never enforces insertion order.
    draft
        .push_edge(edge("m1", "p1", PlanEdgeKind::ParentChild, None))
        .expect("edge before nodes");
    draft
        .push_node(node("p1", PlanNodeKind::Packet, true))
        .expect("child before parent");
    draft
        .push_node(node("m1", PlanNodeKind::Milestone, true))
        .expect("parent");
    // Duplicates and malformed additions fail at push time with their own reasons.
    assert_eq!(
        draft.push_node(node("p1", PlanNodeKind::Packet, true)),
        Err("plan_node_duplicate")
    );
    assert_eq!(
        draft.push_edge(edge("m1", "p1", PlanEdgeKind::ParentChild, None)),
        Err("plan_edge_duplicate")
    );
    assert_eq!(
        draft.push_edge(edge("m1", "p1", PlanEdgeKind::Related, None)),
        Ok(())
    );
    assert_eq!(draft.require_criterion(cid(4)), Ok(()));
    assert_eq!(
        draft.require_criterion(cid(4)),
        Err("plan_required_criterion_duplicate")
    );
    draft.nodes.insert(
        "p1".to_owned(),
        node_with("p1", PlanNodeKind::Packet, true, vec![cid(4)], Vec::new()),
    );
    assert_eq!(draft.permit_read("artifact:charter"), Ok(()));
    assert_eq!(
        draft.permit_read("artifact:charter"),
        Err("plan_read_scope_reference_duplicate")
    );
    assert_eq!(
        draft.permit_read("no-scheme"),
        Err("plan_read_scope_reference_invalid")
    );
    let plan = draft.freeze("project-1", 1, digest_a()).expect("freeze");
    assert_eq!(plan.schema, PLAN_PROPOSAL_SCHEMA);
    assert_eq!(plan.plan_version, 1);
    assert_eq!(plan.nodes.len(), 2);
    assert_eq!(plan.edges.len(), 2);
    assert_eq!(plan.required_criteria, vec![cid(4)]);
    assert_eq!(plan.read_scope, vec!["artifact:charter".to_owned()]);
    assert_eq!(plan.digest, plan.canonical_digest());

    // The complete graph is validated exactly here: a draft that closes a cycle only fails at
    // freeze and yields no partially admitted plan.
    let mut broken = two_milestone_draft();
    broken
        .push_edge(edge(
            "m2",
            "m1",
            PlanEdgeKind::RequiresRunSuccess,
            Some("run:3"),
        ))
        .expect("late edge");
    assert_eq!(
        broken.freeze("project-1", 1, digest_a()).unwrap_err(),
        "plan_dependency_cycle"
    );

    // Revision freezing records the exact version/baseline pair in the digest.
    let mut revised = two_milestone_draft();
    let plan = revised
        .freeze_versioned("project-1", 2, 3, digest_a())
        .expect("revision");
    assert_eq!(plan.plan_version, 3);
    assert_eq!(plan.charter_baseline_version, 2);
    assert_eq!(plan.digest, plan.canonical_digest());
    assert_ne!(plan.digest, two_milestone_plan().digest);

    assert_eq!(
        PlanProposalDraft::new()
            .freeze("project-1", 1, digest_a())
            .unwrap_err(),
        "plan_nodes_required_or_too_large"
    );
}

#[test]
fn frozen_plan_graph_pins_dependency_versions_and_rejects_every_drift() {
    let plan = two_milestone_plan();
    let frozen = FrozenPlanGraph::from_plan(&plan).expect("frozen plan graph");
    assert_eq!(frozen.schema, FROZEN_PLAN_GRAPH_SCHEMA);
    assert_eq!(frozen.plan_digest, plan.digest);
    assert_eq!(frozen.plan_version, 2);
    assert_eq!(frozen.charter_baseline_version, 1);
    assert_eq!(frozen.coverage_digest, digest_a());
    assert_eq!(frozen.node_versions.get("m1"), Some(&1));
    assert_eq!(frozen.node_versions.get("m2"), Some(&1));
    assert_eq!(frozen.node_versions.get("p1"), Some(&1));
    assert_eq!(frozen.node_versions.get("p2"), Some(&1));
    assert_eq!(frozen.node_versions.len(), 4);
    assert_eq!(frozen.digest, frozen.canonical_digest());
    assert_eq!(frozen.verify(&plan), Ok(()));

    // Project drift is refused before any version comparison.
    let mut foreign = plan.clone();
    foreign.project_id = "project-2".to_owned();
    for node in foreign.nodes.values_mut() {
        node.project_id = "project-2".to_owned();
    }
    foreign.digest = foreign.canonical_digest();
    assert_eq!(
        frozen.verify(&foreign),
        Err("frozen_plan_graph_project_mismatch")
    );

    // A bumped dependency version, a removed node and an extra node each get their own reason
    // even though the candidate digest is recomputed.
    let mut bumped = plan.clone();
    bumped.nodes.get_mut("m2").expect("m2").version = 2;
    bumped.digest = bumped.canonical_digest();
    assert_eq!(
        frozen.verify(&bumped),
        Err("frozen_plan_graph_node_version_mismatch")
    );
    let mut shrunk = plan.clone();
    shrunk.nodes.remove("p2");
    shrunk.digest = shrunk.canonical_digest();
    assert_eq!(
        frozen.verify(&shrunk),
        Err("frozen_plan_graph_node_missing")
    );
    let mut grown = plan.clone();
    grown
        .nodes
        .insert("p3".to_owned(), node("p3", PlanNodeKind::Packet, false));
    grown.digest = grown.canonical_digest();
    assert_eq!(frozen.verify(&grown), Err("frozen_plan_graph_node_unknown"));

    // Version, baseline and coverage identities are checked before the whole-plan digest.
    let mut plan_version_drift = plan.clone();
    plan_version_drift.plan_version = 3;
    plan_version_drift.digest = plan_version_drift.canonical_digest();
    assert_eq!(
        frozen.verify(&plan_version_drift),
        Err("frozen_plan_graph_plan_version_mismatch")
    );
    let mut baseline_drift = plan.clone();
    baseline_drift.charter_baseline_version = 2;
    baseline_drift.digest = baseline_drift.canonical_digest();
    assert_eq!(
        frozen.verify(&baseline_drift),
        Err("frozen_plan_graph_baseline_mismatch")
    );
    let mut coverage_drift = plan.clone();
    coverage_drift.coverage_digest = digest_b();
    coverage_drift.digest = coverage_drift.canonical_digest();
    assert_eq!(
        frozen.verify(&coverage_drift),
        Err("frozen_plan_graph_coverage_mismatch")
    );
    // Only the edge reference changed; the frozen dependency versions still match, so the whole
    // plan digest is the refusal that catches it.
    let mut reference_drift = plan.clone();
    for edge in &mut reference_drift.edges {
        if edge.kind == PlanEdgeKind::RequiresAcceptance {
            edge.reference = Some("acceptance:m1-revised".to_owned());
        }
    }
    reference_drift.digest = reference_drift.canonical_digest();
    assert_eq!(
        frozen.verify(&reference_drift),
        Err("frozen_plan_graph_plan_digest_mismatch")
    );

    // The frozen record itself is self-checking and fail-closed.
    let mut digest_drift = frozen.clone();
    digest_drift.digest = digest_b();
    assert_eq!(
        digest_drift.validate(),
        Err("frozen_plan_graph_freeze_digest_mismatch")
    );
    let mut schema_drift = frozen.clone();
    schema_drift.schema = "kiana.frozen-plan-graph.v9".to_owned();
    assert_eq!(
        schema_drift.validate(),
        Err("frozen_plan_graph_schema_invalid")
    );
    let mut version_drift = frozen.clone();
    version_drift.plan_version = 0;
    assert_eq!(
        version_drift.validate(),
        Err("frozen_plan_graph_version_invalid")
    );
    let mut no_nodes = frozen.clone();
    no_nodes.node_versions.clear();
    assert_eq!(no_nodes.validate(), Err("frozen_plan_graph_nodes_invalid"));
    let mut zero_node = frozen.clone();
    zero_node.node_versions.insert("p1".to_owned(), 0);
    zero_node.digest = zero_node.canonical_digest();
    assert_eq!(
        zero_node.validate(),
        Err("frozen_plan_graph_node_versions_invalid")
    );
    let mut plan_digest_drift = frozen.clone();
    plan_digest_drift.plan_digest = "not-a-digest".to_owned();
    assert_eq!(
        plan_digest_drift.validate(),
        Err("frozen_plan_graph_plan_digest_invalid")
    );
}

#[test]
fn plan_graph_freeze_command_admits_the_exact_revision_once() {
    let plan = two_milestone_plan();
    let command = CompanyCommand::FreezePlanGraph { plan: plan.clone() };
    let state = approved_state();
    let admitted = state
        .clone()
        .transition(&command, &authority("pm-1", "pm"), &CompanyProof::default())
        .expect("pm admits the plan");
    assert_eq!(admitted.plan_graphs.len(), 1);
    assert_eq!(
        admitted.plan_graphs["project-1"],
        FrozenPlanGraph::from_plan(&plan).expect("frozen plan graph")
    );
    // Admission changes no related object: no milestone, packet or run is created and the
    // project revision stays where it was.
    assert_eq!(admitted.milestones, state.milestones);
    assert_eq!(admitted.packets, state.packets);
    assert!(admitted.runs.is_empty());
    assert_eq!(
        admitted.projects["project-1"].version,
        state.projects["project-1"].version
    );

    // Repeating the exact revision is idempotent, whoever of the two allowed roles submits it.
    let repeated = admitted
        .clone()
        .transition(
            &command,
            &authority("sponsor-1", "sponsor"),
            &CompanyProof::default(),
        )
        .expect("idempotent admission");
    assert_eq!(repeated.plan_graphs, admitted.plan_graphs);

    // Builder subjects are denied by the role gate before any state is read.
    assert_eq!(
        state.clone().transition(
            &command,
            &authority("builder-1", "builder"),
            &CompanyProof::default()
        ),
        Err("company_role_denied")
    );

    // The revision binds the server-side frozen Charter baseline.
    let mut without_baseline = state.clone();
    without_baseline.charter_baselines.clear();
    assert_eq!(
        without_baseline.transition(&command, &authority("pm-1", "pm"), &CompanyProof::default()),
        Err("plan_graph_baseline_missing")
    );
    assert_eq!(
        state.clone().transition(
            &CompanyCommand::FreezePlanGraph {
                plan: crafted_revision(2, 2, digest_a()),
            },
            &authority("pm-1", "pm"),
            &CompanyProof::default()
        ),
        Err("plan_graph_baseline_mismatch")
    );

    // A different revision of the same project is refused and the admitted one is untouched.
    let mut other = state.clone();
    other.plan_graphs = admitted.plan_graphs.clone();
    assert_eq!(
        other.transition(
            &CompanyCommand::FreezePlanGraph { plan: revision(3) },
            &authority("pm-1", "pm"),
            &CompanyProof::default()
        ),
        Err("plan_graph_already_frozen")
    );
    assert_eq!(other.plan_graphs, admitted.plan_graphs);

    // An invalid complete graph fails closed at the boundary with its structural reason.
    let empty_milestone = raw_plan(
        vec![
            node("m1", PlanNodeKind::Milestone, true),
            node("p1", PlanNodeKind::Packet, true),
            node("p2", PlanNodeKind::Packet, false),
        ],
        vec![
            edge("m1", "p2", PlanEdgeKind::ParentChild, None),
            edge("m1", "p1", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
        ],
        Vec::new(),
        Vec::new(),
        1,
        1,
        digest_a(),
    );
    assert_eq!(
        state.clone().transition(
            &CompanyCommand::FreezePlanGraph {
                plan: empty_milestone,
            },
            &authority("pm-1", "pm"),
            &CompanyProof::default()
        ),
        Err("plan_required_milestone_empty")
    );

    // When the project has a frozen CO-11 coverage, the admitted plan must bind to it; a drifted
    // coverage identity is refused with the coverage reason, not a generic one.
    let mut coverage_state = state.clone();
    coverage_state
        .criterion_coverages
        .insert("project-1".to_owned(), frozen_coverage());
    let bound_plan =
        plan_for_coverage(&coverage_state.criterion_coverages["project-1"].graph.digest);
    let bound = coverage_state
        .clone()
        .transition(
            &CompanyCommand::FreezePlanGraph {
                plan: bound_plan.clone(),
            },
            &authority("sponsor-1", "sponsor"),
            &CompanyProof::default(),
        )
        .expect("coverage-bound admission");
    assert_eq!(
        bound.plan_graphs["project-1"].plan_digest,
        bound_plan.digest
    );
    assert_eq!(
        coverage_state.transition(
            &CompanyCommand::FreezePlanGraph {
                plan: plan_for_coverage(&digest_b()),
            },
            &authority("sponsor-1", "sponsor"),
            &CompanyProof::default()
        ),
        Err("plan_coverage_digest_mismatch")
    );
}

#[test]
fn two_milestone_plan_preserves_dependency_and_acceptance_boundaries() {
    // The batch draft is frozen once into the exact revision; every boundary stays visible.
    let mut draft = two_milestone_draft();
    draft.nodes.insert(
        "m1".to_owned(),
        node_with(
            "m1",
            PlanNodeKind::Milestone,
            true,
            vec![cid(7)],
            Vec::new(),
        ),
    );
    draft.nodes.insert(
        "p2".to_owned(),
        node_with(
            "p2",
            PlanNodeKind::Packet,
            true,
            Vec::new(),
            vec!["artifact:charter".to_owned()],
        ),
    );
    draft.require_criterion(cid(7)).expect("required criterion");
    draft.permit_read("artifact:charter").expect("read scope");
    let plan = draft
        .freeze_versioned("project-1", 1, 2, digest_a())
        .expect("plan");
    let order = plan.topological_order().expect("topological order");
    assert_eq!(plan.nodes.len(), 4);
    assert_eq!(plan.edges.len(), 4);
    assert_eq!(order.len(), 4);
    assert!(order.iter().position(|id| id == "m1") < order.iter().position(|id| id == "m2"));
    assert!(order.iter().position(|id| id == "m1") < order.iter().position(|id| id == "p1"));
    assert!(order.iter().position(|id| id == "m2") < order.iter().position(|id| id == "p2"));
    assert_eq!(plan.topological_order().expect("stable order"), order);
    assert!(plan.edges.iter().any(|edge| {
        edge.from == "m1" && edge.to == "m2" && edge.kind == PlanEdgeKind::RequiresAcceptance
    }));
    assert!(plan.edges.iter().any(|edge| {
        edge.from == "m1" && edge.to == "p2" && edge.kind == PlanEdgeKind::Related
    }));
    assert_eq!(plan.digest, plan.canonical_digest());

    let report = plan.graph_report();
    assert_eq!(report.rejection, None);
    assert_eq!(report.cycle_path, None);
    assert!(report.missing_references.is_empty());
    assert_eq!(report.topological_order, Some(order));

    // The admitted revision can be pinned to its dependency versions without replaying the DAG.
    let frozen = FrozenPlanGraph::from_plan(&plan).expect("frozen plan graph");
    assert_eq!(frozen.verify(&plan), Ok(()));
    assert_eq!(frozen.plan_digest, plan.digest);
    assert_eq!(frozen.node_versions.len(), plan.nodes.len());
}
