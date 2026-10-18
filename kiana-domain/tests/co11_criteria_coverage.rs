use kiana_domain::*;
use std::collections::BTreeSet;
use uuid::Uuid;

fn scope() -> String {
    json_digest(&serde_json::json!({"project": "project-1"}))
}

fn cid(value: u128) -> CriterionId {
    CriterionId::from_uuid(Uuid::from_u128(value))
}

fn zero_digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}

fn criterion(id: CriterionId, source: &str, required: bool, description: &str) -> Criterion {
    Criterion::new(
        id,
        source,
        1,
        description,
        "ci_fixture",
        vec!["verification_log".to_owned()],
        required,
        scope(),
    )
    .expect("criterion")
}

fn entry(criterion: Criterion, target: AcceptanceTarget) -> CriterionCoverageEntry {
    CriterionCoverageEntry { criterion, target }
}

fn link(
    parent_id: CriterionId,
    child_id: CriterionId,
    relation: CriterionRelation,
) -> CriterionCoverageLink {
    CriterionCoverageLink {
        parent_id,
        child_id,
        relation,
        evidence_refs: Vec::new(),
    }
}

#[test]
fn criteria_coverage_rejects_missing_parent_duplicate_identity_and_silent_weakening() {
    let project = CriterionId::new();
    let milestone = CriterionId::new();
    let child = criterion(milestone, "charter:v1", true, "milestone proves project");
    let parent = criterion(project, "charter:v1", true, "project ships safely");

    let missing_parent = CriterionCoverageLink {
        parent_id: project,
        child_id: CriterionId::new(),
        relation: CriterionRelation::Covers,
        evidence_refs: Vec::new(),
    };
    assert_eq!(
        CriterionCoverageGraph::new(
            "project-1",
            1,
            scope(),
            vec![
                entry(
                    parent.clone(),
                    AcceptanceTarget::Project("project-1".to_owned())
                ),
                entry(
                    child.clone(),
                    AcceptanceTarget::Milestone("milestone-1".to_owned())
                ),
            ],
            vec![missing_parent],
            BTreeSet::new(),
        )
        .unwrap_err(),
        "criteria_link_child_missing"
    );

    assert_eq!(
        CriterionCoverageGraph::new(
            "project-1",
            1,
            scope(),
            vec![
                entry(
                    parent.clone(),
                    AcceptanceTarget::Project("project-1".to_owned())
                ),
                entry(
                    parent.clone(),
                    AcceptanceTarget::Milestone("milestone-1".to_owned())
                ),
            ],
            Vec::new(),
            BTreeSet::from([project]),
        )
        .unwrap_err(),
        "criteria_duplicate_identity"
    );

    let optional_child = criterion(milestone, "charter:v1", false, "optional evidence");
    assert_eq!(
        CriterionCoverageGraph::new(
            "project-1",
            1,
            scope(),
            vec![
                entry(parent, AcceptanceTarget::Project("project-1".to_owned())),
                entry(
                    optional_child,
                    AcceptanceTarget::Milestone("milestone-1".to_owned()),
                ),
            ],
            vec![link(project, milestone, CriterionRelation::Covers)],
            BTreeSet::new(),
        )
        .unwrap_err(),
        "criteria_required_child_not_required"
    );
}

#[test]
fn criterion_trace_links_project_requirement_to_specific_test_evidence() {
    let project = CriterionId::new();
    let milestone = CriterionId::new();
    let packet = CriterionId::new();
    let evidence = CriterionId::new();
    let mut verify = link(packet, evidence, CriterionRelation::Verifies);
    verify.evidence_refs = vec!["artifact:test-log".to_owned()];
    let graph = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope(),
        vec![
            entry(
                criterion(project, "charter:v1", true, "project ships safely"),
                AcceptanceTarget::Project("project-1".to_owned()),
            ),
            entry(
                criterion(milestone, "plan:v1", true, "milestone verifies scope"),
                AcceptanceTarget::Milestone("milestone-1".to_owned()),
            ),
            entry(
                criterion(packet, "packet:v1", true, "packet runs the check"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
            entry(
                criterion(evidence, "packet:v1", false, "test log is recorded"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
        ],
        vec![
            link(project, milestone, CriterionRelation::Covers),
            link(milestone, packet, CriterionRelation::Refines),
            verify,
        ],
        BTreeSet::new(),
    )
    .expect("coverage graph");

    assert!(graph.validate().is_ok());
    assert!(graph.uncovered_required().is_empty());
    assert!(graph.coverage_gaps().is_empty());
    assert_eq!(graph.trace(project).unwrap().len(), 4);
    assert_eq!(graph.digest, graph.canonical_digest());
    assert!(graph
        .links
        .iter()
        .any(|link| link.relation == CriterionRelation::Verifies
            && link.evidence_refs == vec!["artifact:test-log"]));
}

#[test]
fn required_packet_without_verification_stays_uncovered() {
    let packet = CriterionId::new();
    let graph = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope(),
        vec![entry(
            criterion(packet, "packet:v1", true, "packet runs the check"),
            AcceptanceTarget::Packet("packet-1".to_owned()),
        )],
        Vec::new(),
        BTreeSet::new(),
    )
    .expect("unverified packet graph");

    assert_eq!(graph.uncovered_required(), vec![packet]);
}

#[test]
fn verification_evidence_covers_only_its_required_packet() {
    let verified_packet = CriterionId::new();
    let unverified_packet = CriterionId::new();
    let evidence = CriterionId::new();
    let mut verify = link(verified_packet, evidence, CriterionRelation::Verifies);
    verify.evidence_refs = vec!["artifact:test-log".to_owned()];
    let graph = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope(),
        vec![
            entry(
                criterion(
                    verified_packet,
                    "packet:v1",
                    true,
                    "first packet runs the check",
                ),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
            entry(
                criterion(
                    unverified_packet,
                    "packet:v1",
                    true,
                    "second packet runs the check",
                ),
                AcceptanceTarget::Packet("packet-2".to_owned()),
            ),
            entry(
                criterion(evidence, "packet:v1", false, "test log is recorded"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
        ],
        vec![verify],
        BTreeSet::new(),
    )
    .expect("partially verified packet graph");

    assert_eq!(graph.uncovered_required(), vec![unverified_packet]);
}

#[test]
fn invalid_packet_verification_evidence_stays_uncovered() {
    let packet = CriterionId::new();
    let evidence = CriterionId::new();
    let mut verify = link(packet, evidence, CriterionRelation::Verifies);
    verify.evidence_refs = vec!["artifact:test-log".to_owned()];
    let graph = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope(),
        vec![
            entry(
                criterion(packet, "packet:v1", true, "packet runs the check"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
            entry(
                criterion(evidence, "packet:v1", false, "test log is recorded"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
        ],
        vec![verify],
        BTreeSet::new(),
    )
    .expect("verified packet graph");

    for refs in [Vec::new(), vec!["untyped-evidence".to_owned()]] {
        let mut invalid = graph.clone();
        invalid.links[0].evidence_refs = refs;
        invalid.digest = invalid.canonical_digest();
        assert!(invalid.validate().is_err());
        assert_eq!(invalid.uncovered_required(), vec![packet]);
    }

    let mut missing_evidence = graph.clone();
    missing_evidence.entries.remove(&evidence);
    missing_evidence.digest = missing_evidence.canonical_digest();
    assert_eq!(
        missing_evidence.validate(),
        Err("criteria_link_child_missing")
    );
    assert_eq!(missing_evidence.uncovered_required(), vec![packet]);
}

#[test]
fn project_verification_does_not_replace_required_child_coverage() {
    let project = CriterionId::new();
    let evidence = CriterionId::new();
    let mut verify = link(project, evidence, CriterionRelation::Verifies);
    verify.evidence_refs = vec!["artifact:test-log".to_owned()];

    let result = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope(),
        vec![
            entry(
                criterion(project, "charter:v1", true, "project ships safely"),
                AcceptanceTarget::Project("project-1".to_owned()),
            ),
            entry(
                criterion(evidence, "packet:v1", false, "test log is recorded"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
        ],
        vec![verify],
        BTreeSet::new(),
    );

    assert_eq!(result.unwrap_err(), "criteria_required_uncovered");
}

fn verify_link(
    parent_id: CriterionId,
    child_id: CriterionId,
    evidence_ref: &str,
) -> CriterionCoverageLink {
    let mut link = link(parent_id, child_id, CriterionRelation::Verifies);
    link.evidence_refs = vec![evidence_ref.to_owned()];
    link
}

/// Project → Milestone → Packet chain with one verified packet criterion.
fn standard_graph(
    project: CriterionId,
    milestone: CriterionId,
    packet: CriterionId,
    evidence: CriterionId,
    baseline: u64,
    evidence_ref: &str,
) -> CriterionCoverageGraph {
    CriterionCoverageGraph::new(
        "project-1",
        baseline,
        scope(),
        vec![
            entry(
                criterion(project, "charter:v1", true, "用户登录可靠"),
                AcceptanceTarget::Project("project-1".to_owned()),
            ),
            entry(
                criterion(milestone, "charter:v1", true, "milestone verifies scope"),
                AcceptanceTarget::Milestone("milestone-1".to_owned()),
            ),
            entry(
                criterion(packet, "packet:v1", true, "packet runs the check"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
            entry(
                criterion(evidence, "packet:v1", false, "test log is recorded"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
        ],
        vec![
            link(project, milestone, CriterionRelation::Covers),
            link(milestone, packet, CriterionRelation::Refines),
            verify_link(packet, evidence, evidence_ref),
        ],
        BTreeSet::new(),
    )
    .expect("standard coverage graph")
}

#[allow(clippy::too_many_arguments)]
fn frozen_coverage(
    project: CriterionId,
    milestone: CriterionId,
    packet: CriterionId,
    evidence: CriterionId,
    baseline: u64,
    evidence_ref: &str,
    plan_digest: &str,
) -> FrozenCriterionCoverage {
    let graph = standard_graph(project, milestone, packet, evidence, baseline, evidence_ref);
    let approval = CriterionCoverageApproval::new(
        "coverage-approval-1",
        "project-1",
        graph.baseline_version,
        graph.digest.clone(),
        plan_digest,
        "artifact:coverage-decision",
        "assignment-sponsor-1",
        "sponsor",
        42,
    )
    .expect("coverage approval");
    FrozenCriterionCoverage::freeze(graph, approval).expect("frozen coverage")
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
        decision_ref: Some("artifact:coverage-decision".to_owned()),
        milestone_refs: vec!["milestone-1".to_owned()],
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
        budget_digest: zero_digest(),
        charter_draft_digest: None,
        charter_draft_version: None,
        stakeholder_principals: Vec::new(),
    }
}

fn snapshot_state(coverage: Option<&FrozenCriterionCoverage>) -> CompanyState {
    let mut state = CompanyState::default();
    state
        .projects
        .insert("project-1".to_owned(), approved_project());
    state.milestones.insert(
        "milestone-1".to_owned(),
        Milestone {
            milestone_id: "milestone-1".to_owned(),
            project_id: "project-1".to_owned(),
            objective_refs: vec!["objective-1".to_owned()],
            deliverables: vec!["deliverable".to_owned()],
            acceptance_criteria: vec!["milestone verifies scope".to_owned()],
            due_at: 100,
            dependency_refs: Vec::new(),
            status: MilestoneStatus::Planned,
            version: 1,
        },
    );
    state.packets.insert(
        "packet-1".to_owned(),
        CompanyPacket {
            project_id: "project-1".to_owned(),
            milestone_id: "milestone-1".to_owned(),
            packet: WorkPacket::builder_task("packet-1", "run the login check")
                .with_acceptance_tests(["packet runs the check"]),
            version: 1,
        },
    );
    if let Some(coverage) = coverage {
        state
            .criterion_coverages
            .insert("project-1".to_owned(), coverage.clone());
    }
    state
}

fn authority(actor: &str, role: &str) -> CompanyAuthority {
    CompanyAuthority {
        actor_id: actor.to_owned(),
        role_id: role.to_owned(),
        session_id: SessionId::new("coverage-session"),
        now_ms: 42,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

fn sponsor(actor: &str) -> CompanyAuthority {
    authority(actor, "sponsor")
}

fn decision_proof() -> CompanyProof {
    CompanyProof {
        events: vec!["artifact:coverage-decision".to_owned()],
        ..CompanyProof::default()
    }
}

fn approved_state(baseline: Option<ProjectCharterBaseline>) -> CompanyState {
    let mut state = CompanyState::default();
    state
        .projects
        .insert("project-1".to_owned(), approved_project());
    if let Some(baseline) = baseline {
        state
            .charter_baselines
            .insert("project-1".to_owned(), baseline);
    }
    state
}

#[test]
fn criteria_coverage_rejects_reused_generic_evidence_and_merged_text_identities() {
    let project = cid(1);
    let peer = cid(2);
    let milestone = cid(3);
    let peer_milestone = cid(4);
    let packet = cid(5);
    let peer_packet = cid(6);
    let evidence = cid(7);
    let peer_evidence = cid(8);
    let graph = CriterionCoverageGraph::new(
        "project-1",
        1,
        scope(),
        vec![
            entry(
                criterion(project, "charter:v1", true, "用户登录可靠"),
                AcceptanceTarget::Project("project-1".to_owned()),
            ),
            entry(
                criterion(peer, "initiative:v1", true, "用户登录可靠"),
                AcceptanceTarget::Project("project-1".to_owned()),
            ),
            entry(
                criterion(milestone, "charter:v1", true, "milestone verifies login"),
                AcceptanceTarget::Milestone("milestone-1".to_owned()),
            ),
            entry(
                criterion(
                    peer_milestone,
                    "initiative:v1",
                    true,
                    "milestone verifies login",
                ),
                AcceptanceTarget::Milestone("milestone-2".to_owned()),
            ),
            entry(
                criterion(packet, "packet:v1", true, "packet runs the check"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
            entry(
                criterion(peer_packet, "packet:v1", true, "packet runs the peer check"),
                AcceptanceTarget::Packet("packet-2".to_owned()),
            ),
            entry(
                criterion(evidence, "packet:v1", false, "test log one is recorded"),
                AcceptanceTarget::Packet("packet-1".to_owned()),
            ),
            entry(
                criterion(
                    peer_evidence,
                    "packet:v1",
                    false,
                    "test log two is recorded",
                ),
                AcceptanceTarget::Packet("packet-2".to_owned()),
            ),
        ],
        vec![
            link(project, milestone, CriterionRelation::Covers),
            link(peer, peer_milestone, CriterionRelation::Covers),
            link(milestone, packet, CriterionRelation::Covers),
            link(peer_milestone, peer_packet, CriterionRelation::Covers),
            verify_link(packet, evidence, "test:packet-1"),
            verify_link(peer_packet, peer_evidence, "test:packet-2"),
        ],
        BTreeSet::new(),
    )
    .expect("two origins keep separate identities");

    assert!(graph.validate().is_ok());
    assert_eq!(graph.criteria_ids().len(), 8);
    // Same text, different origin: both identities survive and a text index would merge them.
    assert_eq!(
        graph
            .entries
            .values()
            .filter(|entry| entry.criterion.description == "用户登录可靠")
            .count(),
        2
    );
    assert_eq!(
        graph
            .resolve_origin(project, 1)
            .expect("origin")
            .criterion
            .source_ref,
        "charter:v1"
    );
    assert_eq!(
        graph
            .resolve_origin(peer, 1)
            .expect("origin")
            .criterion
            .source_ref,
        "initiative:v1"
    );
    assert_eq!(
        graph.resolve_origin(project, 2),
        Err("criteria_origin_baseline_mismatch")
    );
    assert_eq!(
        graph.resolve_origin(cid(99), 1),
        Err("criteria_identity_missing")
    );
    // The origin participates in the identity: swapping origins changes the graph digest.
    let mut swapped = graph.clone();
    for (id, source) in [(project, "initiative:v1"), (peer, "charter:v1")] {
        let entry = swapped.entries.get_mut(&id).expect("entry");
        entry.criterion.source_ref = source.to_owned();
        entry.criterion.criterion_digest = entry.criterion.digest();
    }
    swapped.digest = swapped.canonical_digest();
    assert!(swapped.validate().is_ok());
    assert_ne!(graph.digest, swapped.digest);

    // One generic verification reference cannot verify every required packet criterion.
    let mut reused_ref = graph.clone();
    reused_ref
        .links
        .push(verify_link(peer_packet, evidence, "test:packet-1"));
    reused_ref.digest = reused_ref.canonical_digest();
    assert_eq!(reused_ref.validate(), Err("criteria_evidence_ref_reused"));

    let mut reused_evidence = graph.clone();
    reused_evidence
        .links
        .push(verify_link(peer_packet, evidence, "test:packet-2-alt"));
    reused_evidence.digest = reused_evidence.canonical_digest();
    assert_eq!(reused_evidence.validate(), Err("criteria_evidence_reused"));
}

#[test]
fn typed_plan_coverage_binds_the_approved_graph_and_rejects_drift() {
    let project = cid(11);
    let milestone = cid(12);
    let packet = cid(13);
    let evidence = cid(14);
    let graph = standard_graph(project, milestone, packet, evidence, 1, "test:packet-1");
    let plan_nodes = vec![
        PlanNode {
            node_id: "milestone-1".to_owned(),
            project_id: "project-1".to_owned(),
            kind: PlanNodeKind::Milestone,
            version: 1,
            required: true,
            criterion_refs: vec![milestone],
        },
        PlanNode {
            node_id: "packet-1".to_owned(),
            project_id: "project-1".to_owned(),
            kind: PlanNodeKind::Packet,
            version: 1,
            required: true,
            criterion_refs: vec![packet],
        },
    ];
    let plan_edges = vec![PlanEdge {
        from: "milestone-1".to_owned(),
        to: "packet-1".to_owned(),
        kind: PlanEdgeKind::ParentChild,
        reference: None,
    }];
    let plan = PlanProposal::new(
        "project-1",
        1,
        graph.digest.clone(),
        plan_nodes.clone(),
        plan_edges.clone(),
    )
    .expect("plan proposal");
    let frozen = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        1,
        "test:packet-1",
        &plan.digest,
    );
    assert!(frozen.bind_plan(&plan).is_ok());
    assert_eq!(frozen.criteria_ids().len(), 4);
    assert_eq!(
        frozen
            .criteria_for(&AcceptanceTarget::Packet("packet-1".to_owned()))
            .len(),
        2
    );
    assert!(frozen.uncovered_required().is_empty());
    assert!(frozen.coverage_gaps().is_empty());
    assert_eq!(
        frozen.trace(project).expect("trace"),
        vec![project, milestone, packet, evidence]
    );

    // The approval itself is deny-first: Sponsor role, typed digest and a resolvable record.
    assert_eq!(
        CriterionCoverageApproval::new(
            "coverage-approval-2",
            "project-1",
            0,
            graph.digest.clone(),
            plan.digest.clone(),
            "artifact:coverage-decision",
            "assignment-sponsor-1",
            "sponsor",
            42,
        ),
        Err("criteria_coverage_approval_invalid")
    );
    assert_eq!(
        CriterionCoverageApproval::new(
            "coverage-approval-2",
            "project-1",
            1,
            "not-a-digest",
            plan.digest.clone(),
            "artifact:coverage-decision",
            "assignment-sponsor-1",
            "sponsor",
            42,
        ),
        Err("criteria_coverage_approval_digest_invalid")
    );
    assert_eq!(
        CriterionCoverageApproval::new(
            "coverage-approval-2",
            "project-1",
            1,
            graph.digest.clone(),
            plan.digest.clone(),
            "summary:model-text",
            "assignment-sponsor-1",
            "sponsor",
            42,
        ),
        Err("criteria_coverage_approval_decision_ref_invalid")
    );
    assert_eq!(
        CriterionCoverageApproval::new(
            "coverage-approval-2",
            "project-1",
            1,
            graph.digest.clone(),
            plan.digest.clone(),
            "artifact:coverage-decision",
            "assignment-sponsor-1",
            "pm",
            42,
        ),
        Err("criteria_coverage_approval_sponsor_required")
    );

    let approval = || {
        CriterionCoverageApproval::new(
            "coverage-approval-2",
            "project-1",
            1,
            graph.digest.clone(),
            plan.digest.clone(),
            "artifact:coverage-decision",
            "assignment-sponsor-1",
            "sponsor",
            42,
        )
        .expect("approval")
    };

    let mut digest_drift = approval();
    digest_drift.graph_digest = zero_digest();
    digest_drift.digest = digest_drift.canonical_digest();
    assert_eq!(
        FrozenCriterionCoverage::freeze(graph.clone(), digest_drift).unwrap_err(),
        "criteria_coverage_graph_digest_mismatch"
    );

    let mut project_drift = approval();
    project_drift.project_id = "project-2".to_owned();
    project_drift.digest = project_drift.canonical_digest();
    assert_eq!(
        FrozenCriterionCoverage::freeze(graph.clone(), project_drift).unwrap_err(),
        "criteria_coverage_project_mismatch"
    );

    let mut baseline_drift = approval();
    baseline_drift.baseline_version = 2;
    baseline_drift.digest = baseline_drift.canonical_digest();
    assert_eq!(
        FrozenCriterionCoverage::freeze(graph.clone(), baseline_drift).unwrap_err(),
        "criteria_coverage_baseline_mismatch"
    );

    let mut tampered = approval();
    tampered.digest = zero_digest();
    assert_eq!(
        FrozenCriterionCoverage::freeze(graph.clone(), tampered).unwrap_err(),
        "criteria_coverage_approval_digest_mismatch"
    );

    let coverage_drift_plan = PlanProposal::new(
        "project-1",
        1,
        zero_digest(),
        plan_nodes.clone(),
        plan_edges.clone(),
    )
    .expect("plan proposal");
    assert_eq!(
        frozen.bind_plan(&coverage_drift_plan).unwrap_err(),
        "plan_coverage_digest_mismatch"
    );

    let unsigned_plan = PlanProposal::new(
        "project-1",
        1,
        graph.digest.clone(),
        vec![
            plan_nodes[0].clone(),
            plan_nodes[1].clone(),
            PlanNode {
                node_id: "packet-2".to_owned(),
                project_id: "project-1".to_owned(),
                kind: PlanNodeKind::Packet,
                version: 1,
                required: false,
                criterion_refs: Vec::new(),
            },
        ],
        vec![
            plan_edges[0].clone(),
            PlanEdge {
                from: "milestone-1".to_owned(),
                to: "packet-2".to_owned(),
                kind: PlanEdgeKind::Related,
                reference: None,
            },
        ],
    )
    .expect("plan proposal");
    assert_eq!(
        frozen.bind_plan(&unsigned_plan).unwrap_err(),
        "plan_coverage_plan_digest_mismatch"
    );

    let unknown_plan = PlanProposal::new(
        "project-1",
        1,
        graph.digest.clone(),
        vec![
            PlanNode {
                node_id: "milestone-1".to_owned(),
                project_id: "project-1".to_owned(),
                kind: PlanNodeKind::Milestone,
                version: 1,
                required: true,
                criterion_refs: vec![milestone, cid(99)],
            },
            plan_nodes[1].clone(),
        ],
        plan_edges.clone(),
    )
    .expect("plan proposal");
    let unknown_frozen = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        1,
        "test:packet-1",
        &unknown_plan.digest,
    );
    assert_eq!(
        unknown_frozen.bind_plan(&unknown_plan).unwrap_err(),
        "plan_coverage_reference_unknown"
    );

    let unassigned_plan = PlanProposal::new(
        "project-1",
        1,
        graph.digest.clone(),
        vec![
            PlanNode {
                node_id: "milestone-1".to_owned(),
                project_id: "project-1".to_owned(),
                kind: PlanNodeKind::Milestone,
                version: 1,
                required: true,
                criterion_refs: Vec::new(),
            },
            plan_nodes[1].clone(),
        ],
        plan_edges.clone(),
    )
    .expect("plan proposal");
    let unassigned_frozen = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        1,
        "test:packet-1",
        &unassigned_plan.digest,
    );
    assert_eq!(
        unassigned_frozen.bind_plan(&unassigned_plan).unwrap_err(),
        "plan_coverage_required_criterion_unassigned"
    );

    let baseline_mismatch_plan = PlanProposal::new(
        "project-1",
        2,
        graph.digest.clone(),
        plan_nodes.clone(),
        plan_edges.clone(),
    )
    .expect("plan proposal");
    assert_eq!(
        frozen.bind_plan(&baseline_mismatch_plan).unwrap_err(),
        "plan_coverage_baseline_mismatch"
    );

    let foreign_plan = PlanProposal::new(
        "project-2",
        1,
        graph.digest.clone(),
        plan_nodes
            .iter()
            .map(|node| PlanNode {
                project_id: "project-2".to_owned(),
                ..node.clone()
            })
            .collect(),
        plan_edges.clone(),
    )
    .expect("plan proposal");
    assert_eq!(
        frozen.bind_plan(&foreign_plan).unwrap_err(),
        "plan_coverage_project_mismatch"
    );
}

#[test]
fn acceptance_snapshot_uses_typed_criteria_and_reports_remaining_gaps() {
    let project = cid(21);
    let milestone = cid(22);
    let packet = cid(23);
    let evidence = cid(24);
    let frozen = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        1,
        "test:packet-1",
        &zero_digest(),
    );
    let state = snapshot_state(Some(&frozen));
    let snapshot = state
        .acceptance_snapshot("project-1", "packet-1")
        .expect("typed snapshot");

    let mut expected = vec![project, milestone, packet, evidence];
    expected.sort();
    let expected = expected.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(snapshot.criteria(), expected);
    assert_eq!(snapshot.criterion_refs.len(), 4);
    assert_eq!(
        snapshot.coverage_digest.as_deref(),
        Some(frozen.graph.digest.as_str())
    );
    assert_eq!(snapshot.coverage_baseline_version, Some(1));
    // The legacy text stays readable, but it never substitutes for the typed identities.
    assert!(snapshot
        .project_criteria
        .contains(&"用户登录可靠".to_owned()));
    assert!(!snapshot.criteria().contains(&"用户登录可靠".to_owned()));

    // Legacy projects keep the readable text inference they were approved with.
    let legacy = snapshot_state(None)
        .acceptance_snapshot("project-1", "packet-1")
        .expect("legacy snapshot");
    let mut legacy_expected = vec![
        "用户登录可靠".to_owned(),
        "milestone verifies scope".to_owned(),
        "packet runs the check".to_owned(),
    ];
    legacy_expected.sort();
    assert_eq!(legacy.criteria(), legacy_expected);
    assert!(legacy.coverage_digest.is_none());
    assert!(legacy.coverage_baseline_version.is_none());

    // A coverage-bound snapshot never falls back to the text lists.
    let mut mixed = snapshot.clone();
    mixed.criterion_refs.clear();
    assert_eq!(
        mixed.validate(),
        Err("criteria_snapshot_coverage_requires_typed_criteria")
    );
    assert!(mixed.criteria().is_empty());

    // Uncovered and explicitly deferred criteria stay visible instead of becoming a silent pass.
    let mut deferred_graph =
        standard_graph(project, milestone, packet, evidence, 1, "test:packet-1");
    deferred_graph
        .links
        .retain(|link| link.relation != CriterionRelation::Verifies);
    let deferred = cid(25);
    deferred_graph.entries.insert(
        deferred,
        entry(
            criterion(deferred, "plan:v1", true, "next milestone criterion"),
            AcceptanceTarget::Project("project-1".to_owned()),
        ),
    );
    deferred_graph.pending_required.insert(deferred);
    deferred_graph.digest = deferred_graph.canonical_digest();
    assert!(deferred_graph.validate().is_ok());
    let deferred_approval = CriterionCoverageApproval::new(
        "coverage-approval-gap",
        "project-1",
        1,
        deferred_graph.digest.clone(),
        zero_digest(),
        "artifact:coverage-decision",
        "assignment-sponsor-1",
        "sponsor",
        42,
    )
    .expect("approval");
    let deferred_frozen =
        FrozenCriterionCoverage::freeze(deferred_graph, deferred_approval).expect("frozen");
    assert_eq!(deferred_frozen.coverage_gaps(), vec![deferred]);
    assert_eq!(deferred_frozen.uncovered_required(), vec![packet]);
    let deferred_snapshot = snapshot_state(Some(&deferred_frozen))
        .acceptance_snapshot("project-1", "packet-1")
        .expect("deferred snapshot");
    assert!(deferred_snapshot.criteria().contains(&deferred.to_string()));
}

#[test]
fn coverage_freeze_command_requires_sponsor_baseline_and_verified_decision() {
    let project = cid(31);
    let milestone = cid(32);
    let packet = cid(33);
    let evidence = cid(34);
    let frozen = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        1,
        "test:packet-1",
        &zero_digest(),
    );
    let command = CompanyCommand::FreezeCriterionCoverage {
        coverage: frozen.clone(),
    };
    let state = approved_state(Some(charter_baseline()));
    let frozen_state = state
        .clone()
        .transition(&command, &sponsor("sponsor-1"), &decision_proof())
        .expect("sponsor freeze");
    assert!(frozen_state.criterion_coverages.contains_key("project-1"));
    assert_eq!(frozen_state.criterion_coverages["project-1"], frozen);

    // Repeating the exact freeze is idempotent.
    let repeated = frozen_state
        .clone()
        .transition(&command, &sponsor("sponsor-1"), &decision_proof())
        .expect("idempotent freeze");
    assert_eq!(
        repeated.criterion_coverages,
        frozen_state.criterion_coverages
    );

    // Only the Sponsor role can freeze, and only this project's recorded Sponsor can sign.
    assert_eq!(
        state
            .clone()
            .transition(&command, &authority("sponsor-1", "pm"), &decision_proof()),
        Err("company_role_denied")
    );
    assert_eq!(
        state
            .clone()
            .transition(&command, &sponsor("pm-1"), &decision_proof()),
        Err("criteria_coverage_sponsor_mismatch")
    );

    // The freeze binds the server-side frozen Charter baseline.
    assert_eq!(
        approved_state(None).transition(&command, &sponsor("sponsor-1"), &decision_proof()),
        Err("criteria_coverage_baseline_missing")
    );
    let drifted = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        2,
        "test:packet-1",
        &zero_digest(),
    );
    assert_eq!(
        state.clone().transition(
            &CompanyCommand::FreezeCriterionCoverage { coverage: drifted },
            &sponsor("sponsor-1"),
            &decision_proof()
        ),
        Err("criteria_coverage_baseline_mismatch")
    );

    // Semantic sufficiency needs a verified planning/review record, not a text summary.
    assert_eq!(
        state
            .clone()
            .transition(&command, &sponsor("sponsor-1"), &CompanyProof::default()),
        Err("company_evidence_unverified")
    );

    // Freezing is only available once the Charter is approved.
    let mut chartering = state.clone();
    chartering
        .projects
        .get_mut("project-1")
        .expect("project")
        .status = ProjectStatus::Chartering;
    assert_eq!(
        chartering.transition(&command, &sponsor("sponsor-1"), &decision_proof()),
        Err("company_project_not_approved_or_active")
    );

    // A different graph for the same project cannot overwrite the approved freeze.
    let alternative = frozen_coverage(
        project,
        milestone,
        packet,
        evidence,
        1,
        "test:packet-1-alt",
        &zero_digest(),
    );
    assert_eq!(
        frozen_state.clone().transition(
            &CompanyCommand::FreezeCriterionCoverage {
                coverage: alternative
            },
            &sponsor("sponsor-1"),
            &decision_proof()
        ),
        Err("criteria_coverage_already_frozen")
    );
}
