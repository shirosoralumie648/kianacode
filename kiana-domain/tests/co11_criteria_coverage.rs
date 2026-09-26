use kiana_domain::*;
use std::collections::BTreeSet;

fn scope() -> String {
    json_digest(&serde_json::json!({"project": "project-1"}))
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
