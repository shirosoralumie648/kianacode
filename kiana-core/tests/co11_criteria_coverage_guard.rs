#[test]
fn criteria_coverage_is_versioned_and_fail_closed() {
    let criteria = include_str!("../../kiana-domain/src/criteria.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "CRITERIA_COVERAGE_SCHEMA",
        "CRITERIA_COVERAGE_APPROVAL_SCHEMA",
        "FROZEN_CRITERIA_COVERAGE_SCHEMA",
        "CriterionCoverageGraph",
        "CriterionCoverageLink",
        "CriterionCoverageApproval",
        "FrozenCriterionCoverage",
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
        "criteria_origin_baseline_mismatch",
        "criteria_identity_missing",
        "criteria_evidence_ref_reused",
        "criteria_evidence_reused",
        "criteria_coverage_approval_sponsor_required",
        "criteria_coverage_approval_decision_ref_invalid",
        "criteria_coverage_approval_digest_mismatch",
        "criteria_coverage_graph_digest_mismatch",
        "criteria_coverage_project_mismatch",
        "criteria_coverage_baseline_mismatch",
        "criteria_coverage_freeze_digest_mismatch",
        "scope_digest",
        "coverage_gaps",
        "resolve_origin",
        "bind_plan",
        "plan_coverage_project_mismatch",
        "plan_coverage_baseline_mismatch",
        "plan_coverage_digest_mismatch",
        "plan_coverage_plan_digest_mismatch",
        "plan_coverage_reference_unknown",
        "plan_coverage_required_criterion_unassigned",
        "frozen_criteria",
        "pub fn trace",
        "artifact:",
        "pending_required",
        "canonical_digest",
        "CriterionId",
        "AcceptanceTarget::Project",
        "AcceptanceTarget::Milestone",
        "AcceptanceTarget::Packet",
        "criterion_refs",
        "coverage_digest",
        "coverage_baseline_version",
    ] {
        assert!(
            criteria.contains(marker) || domain.contains(marker) || core.contains(marker),
            "CO-11 marker missing: {marker}"
        );
    }

    assert!(criteria.contains("self.digest != self.canonical_digest()"));
    assert!(criteria.contains("criteria_pending_already_covered"));
    assert!(criteria.contains("self.entries.is_empty() || self.entries.len() > MAX_CRITERIA"));
    assert!(criteria.contains("self.links.len() > MAX_LINKS"));
    assert!(domain.contains("if !self.criterion_refs.is_empty()"));
    assert!(domain.contains("criteria_snapshot_duplicate_identity"));
    assert!(domain.contains("criteria_snapshot_source_version_drift"));
    assert!(domain.contains("criteria_snapshot_coverage_requires_typed_criteria"));
    assert!(!criteria.contains("CapabilityBroker"));
    assert!(!criteria.contains("EventStorePort"));
    assert!(!core.contains("CriterionCoverageGraph::new"));
    assert!(!criteria.contains("ModelClient"));
}

/// CO-11 completion slice: the frozen coverage is a Sponsor-approved Company fact, the
/// ControlPlane revalidates it before the transition, and the acceptance snapshot reads typed
/// criterion identities instead of inferring them from text.
#[test]
fn typed_coverage_is_wired_into_company_freeze_and_acceptance() {
    let criteria = include_str!("../../kiana-domain/src/criteria.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");
    let fixture = include_str!("../../kiana-domain/tests/co11_criteria_coverage.rs");
    let workflow = include_str!("../../.github/workflows/co11-criteria-coverage.yml");

    // The command, event and state projection are registered on the single Company aggregate.
    for marker in [
        "FreezeCriterionCoverage",
        "CriterionCoverageFrozen",
        "criterion_coverages",
        "coverage_freeze_verdict",
        "criteria_coverage_sponsor_mismatch",
        "criteria_coverage_baseline_missing",
        "criteria_coverage_already_frozen",
        "acceptance_snapshot",
        "frozen_criteria",
    ] {
        assert!(
            domain.contains(marker),
            "CO-11 company marker missing: {marker}"
        );
    }
    assert!(policy.contains("CompanyCommand::FreezeCriterionCoverage { .. } => &[\"sponsor\"]"));
    assert!(policy.contains("CompanyCommand::FreezeCriterionCoverage { .. }"));
    assert!(policy.contains("| CompanyCommand::FreezeCriterionCoverage { .. }"));
    assert!(policy.contains("criteria_coverage_sponsor_mismatch"));
    assert!(policy.contains("criteria_coverage_freeze_digest_mismatch"));

    // The ControlPlane revalidates the freeze against the frozen Charter baseline before the
    // transition, and the cited decision record resolves through the proof.
    let freeze_gate = core
        .find("state.coverage_freeze_verdict(coverage)")
        .expect("the coverage freeze gate must exist in the handler");
    let transition = core
        .find("state.transition(&request.command")
        .expect("the Company transition must stay in ControlPlane");
    assert!(
        freeze_gate < transition,
        "the coverage freeze verdict must run before the transition"
    );
    assert!(core.contains("coverage.approval.decision_ref.clone()"));
    assert!(!core.contains("CriterionCoverageGraph::new"));

    // The acceptance snapshot freeze reads the typed identities and never falls back to text.
    assert!(domain.contains("criteria_snapshot_coverage_requires_typed_criteria"));
    assert!(domain.contains("if self.coverage_digest.is_some() {\n            return Vec::new();"));
    assert!(domain.contains("crate::FrozenCriterionCoverage::frozen_criteria"));

    // The drain-proof fixtures named by the card and the completion slice stay deny-first and are
    // bound by name to the manual lane.
    for fixture_name in [
        "criteria_coverage_rejects_missing_parent_duplicate_identity_and_silent_weakening",
        "criterion_trace_links_project_requirement_to_specific_test_evidence",
        "criteria_coverage_rejects_reused_generic_evidence_and_merged_text_identities",
        "typed_plan_coverage_binds_the_approved_graph_and_rejects_drift",
        "acceptance_snapshot_uses_typed_criteria_and_reports_remaining_gaps",
        "coverage_freeze_command_requires_sponsor_baseline_and_verified_decision",
    ] {
        assert!(
            fixture.contains(fixture_name),
            "CO-11 domain fixture missing: {fixture_name}"
        );
    }
    let deny_first = fixture
        .find("criteria_coverage_rejects_missing_parent_duplicate_identity_and_silent_weakening")
        .expect("deny fixture");
    let success = fixture
        .find("criterion_trace_links_project_requirement_to_specific_test_evidence")
        .expect("success fixture");
    assert!(
        deny_first < success,
        "the deny-first fixture must precede the success fixture"
    );
    assert!(!fixture.contains("#[ignore]"));

    for target in [
        "cargo test -p kiana-core --test co11_criteria_coverage_guard",
        "cargo test -p kiana-domain --test co11_criteria_coverage",
    ] {
        assert!(
            workflow.contains(target),
            "CO-11 lane target missing: {target}"
        );
    }
    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test-threads=1",
        "--no-run",
        "permissions:\n  contents: read",
    ] {
        assert!(
            workflow.contains(marker),
            "CO-11 lane step missing: {marker}"
        );
    }
    assert!(!workflow.contains("push:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("continue-on-error"));
    assert!(!workflow.contains("--ignored"));
    assert!(!criteria.contains("CapabilityBroker"));
    assert!(!criteria.contains("KianaHarness"));
}
