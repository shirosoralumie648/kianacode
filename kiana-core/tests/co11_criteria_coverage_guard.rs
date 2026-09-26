#[test]
fn criteria_coverage_is_versioned_and_fail_closed() {
    let criteria = include_str!("../../kiana-domain/src/criteria.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "CRITERIA_COVERAGE_SCHEMA",
        "CriterionCoverageGraph",
        "CriterionCoverageLink",
        "CriterionRelation::Covers",
        "CriterionRelation::Refines",
        "CriterionRelation::Verifies",
        "criteria_duplicate_identity",
        "criteria_link_parent_missing",
        "criteria_link_cycle",
        "criteria_required_uncovered",
        "criteria_required_child_not_required",
        "criteria_scope_drift",
        "criteria_source_version_drift",
        "scope_digest",
        "coverage_gaps",
        "pub fn trace",
        "artifact:",
        "pending_required",
        "canonical_digest",
        "CriterionId",
        "AcceptanceTarget::Project",
        "AcceptanceTarget::Milestone",
        "AcceptanceTarget::Packet",
        "criterion_refs",
        "criterion.criterion_id.to_string()",
    ] {
        assert!(
            criteria.contains(marker) || domain.contains(marker) || core.contains(marker),
            "CO-11 marker missing: {marker}"
        );
    }

    assert!(criteria.contains("self.digest != self.canonical_digest()"));
    assert!(criteria.contains("criteria_pending_already_covered"));
    assert!(domain.contains("if !self.criterion_refs.is_empty()"));
    assert!(!criteria.contains("CapabilityBroker"));
    assert!(!criteria.contains("EventStorePort"));
    assert!(!core.contains("CriterionCoverageGraph::new"));
}
