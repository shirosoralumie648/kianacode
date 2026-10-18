//! CO-10 source guard.
//!
//! The first test keeps the Charter go/no-go boundary on the existing ControlPlane command path
//! and asserts the deny-first gates. The second test binds the completion slice to the declared
//! fixtures, the manual lane and the automatic shards, and fails closed on a second execution
//! loop or a removed refusal.

#[test]
fn project_charter_go_no_go_is_frozen_and_budget_backed() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "ProjectStatus::Chartering",
        "project_charter_not_ready_for_approval",
        "project_budget_required",
        "project_charter_artifact_required",
        "project_charter_changed",
        "project_objective_not_active",
        "project_sponsor_mismatch",
        "charter_digests",
        "charter_baselines",
        "ProjectCharterBaseline",
        "charter_identity_digest",
        "company_budget_scope_mismatch",
        "company_budget_already_configured",
        "company_project_not_budgetable",
        "project_budget_ref",
        "decision_ref",
        "CompanyCommand::ApproveProject",
        "CompanyCommand::RejectProject",
        "CompanyCommand::ConfigureBudget",
        "CompanyCommandPolicy",
        "company_human_decision_missing",
        "handle_company_command",
        "company_idempotency_conflict",
        // CO-10 completion slice: the versioned Charter draft the Sponsor approves.
        "PROJECT_CHARTER_DRAFT_SCHEMA",
        "ProjectCharterDraft",
        "CharterStakeholder",
        "CompanyCommand::DraftCharter",
        "ProjectCharterDrafted",
        "charter_drafts",
        "charter_draft_digests",
        "charter_draft_digest",
        "project_charter_draft_changed",
        "project_charter_draft_exists",
        "project_charter_draft_role_denied",
        "charter_draft_delivery_recipient_required",
        "ProjectCharterBaseline::from_project",
    ] {
        assert!(
            domain.contains(marker) || policy.contains(marker) || core.contains(marker),
            "CO-10 marker missing: {marker}"
        );
    }

    assert!(domain.contains("self.budgets.contains_key(project_id)"));
    assert!(domain.contains("project_charter_artifact_required"));
    assert!(domain.contains("project_charter_changed"));
    assert!(domain.contains("project.objective_refs.iter().all"));
    assert!(domain.contains("self.charter_digests.get(project_id)"));
    assert!(domain.contains("self.charter_baselines.insert"));
    assert!(core.contains("validate_decision_for"));
    // The runtime write path stays closed until the project is approved and active.
    assert!(core.contains("company_approved_packet_required"));
    assert!(core.contains("company_run_not_authorized"));
    assert!(core.contains("company_capability_scope_inactive"));
    assert!(core.contains("project.status != kiana_domain::ProjectStatus::Active"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("EventStorePort"));
}

#[test]
fn co10_charter_completion_slice_stays_wired_to_fixtures_lane_and_shards() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let business = include_str!("../../kiana-domain/src/company_business.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");
    let fixtures = include_str!("../../kiana-domain/tests/co10_project_charter.rs");
    let lane = include_str!("../../.github/workflows/co10-project-charter.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    let baseline = include_str!("../../docs/roadmap/co10-project-charter-baseline.md");

    // Refusal codes the completion slice relies on. Each one is the stable reason the domain (or
    // the ControlPlane boundary) returns before any related object changes.
    for code in [
        "charter_draft_schema_invalid",
        "charter_draft_version_invalid",
        "charter_draft_project_invalid",
        "charter_draft_organization_invalid",
        "charter_draft_author_invalid",
        "charter_draft_budget_ref_invalid",
        "charter_draft_scope_invalid",
        "charter_draft_risk_invalid",
        "charter_draft_non_goals_invalid",
        "charter_draft_success_criteria_invalid",
        "charter_draft_stakeholders_invalid",
        "charter_draft_stakeholder_principal_invalid",
        "charter_draft_stakeholder_responsibility_invalid",
        "charter_draft_delivery_recipient_required",
        "project_charter_draft_not_pending",
        "project_charter_draft_organization_mismatch",
        "project_charter_draft_budget_ref_mismatch",
        "project_charter_draft_author_mismatch",
        "project_charter_draft_role_denied",
        "project_charter_draft_project_mismatch",
        "project_charter_draft_exists",
        "project_charter_draft_missing",
        "project_charter_draft_changed",
        "project_charter_not_ready_for_approval",
        "project_budget_required",
        "project_charter_artifact_required",
        "project_charter_changed",
        "project_objective_not_active",
        "project_sponsor_mismatch",
        "company_evidence_unverified",
        "company_project_not_approved_or_active",
    ] {
        assert!(
            domain.contains(code) || policy.contains(code) || core.contains(code),
            "CO-10 refusal code missing from source: {code}"
        );
    }

    // Every refusal the card names is exercised by the domain fixture, not only declared in the
    // source, and no refusal was dropped from the fixture.
    for code in [
        "charter_draft_schema_invalid",
        "charter_draft_version_invalid",
        "charter_draft_stakeholders_invalid",
        "charter_draft_delivery_recipient_required",
        "charter_draft_stakeholder_responsibility_invalid",
        "charter_draft_non_goals_invalid",
        "charter_draft_scope_invalid",
        "project_charter_draft_project_mismatch",
        "project_charter_draft_budget_ref_mismatch",
        "project_charter_draft_author_mismatch",
        "project_charter_draft_role_denied",
        "project_charter_draft_exists",
        "project_charter_draft_not_pending",
        "project_charter_draft_changed",
        "project_budget_required",
        "project_charter_not_ready_for_approval",
        "project_charter_artifact_required",
        "project_charter_changed",
        "project_objective_not_active",
        "project_sponsor_mismatch",
        "company_evidence_unverified",
        "company_project_not_approved_or_active",
    ] {
        assert!(
            fixtures.contains(code),
            "CO-10 refusal code is not exercised by the domain fixture: {code}"
        );
    }

    // Accepted contract surface: the versioned draft command, the frozen draft identity and the
    // baseline binding, plus the business (typed) path that records the draft it approves.
    for marker in [
        "DraftCharter",
        "ProjectCharterDraft",
        "CharterStakeholder",
        "PROJECT_CHARTER_DRAFT_SCHEMA",
        "pub fn identity_digest",
        "pub fn validate",
    ] {
        assert!(
            domain.contains(marker),
            "CO-10 domain marker missing: {marker}"
        );
    }
    assert!(policy.contains("CompanyCommand::DraftCharter { .. } => &[\"sponsor\", \"pm\"]"));
    assert!(
        policy.contains("CompanyCommand::DraftCharter { .. }")
            && policy.contains("| CompanyCommand::DraftCharter { .. }"),
        "CO-10 draft command must be registered in the mutation class table"
    );
    assert!(
        business.contains("CompanyCommand::DraftCharter { draft }"),
        "CO-10 typed business path must record the Charter draft it approves"
    );
    assert!(
        domain.contains("ProjectCharterBaseline::from_project("),
        "CO-10 baseline must be built from the approved project, budget and draft"
    );
    assert!(
        domain.contains("self.charter_draft_digests") && domain.contains("self.charter_drafts"),
        "CO-10 frozen draft identity and recorded draft must live in CompanyState"
    );

    // Deny-first order: the card names a refusal fixture and a success fixture, and the success
    // fixture is declared last.
    let deny = fixtures
        .find("fn project_approval_rejects_missing_budget_changed_charter_and_inactive_objective")
        .expect("card-named approval refusal fixture");
    let draft_deny = fixtures
        .find("fn project_charter_draft_rejects_role_drift_and_stakeholder_gaps")
        .expect("draft refusal fixture");
    let success = fixtures
        .find("fn project_charter_approval_freezes_scope_criteria_and_budget")
        .expect("card-named approval success fixture");
    assert!(
        deny < success,
        "deny fixture must precede the success fixture"
    );
    assert!(
        draft_deny < success,
        "draft refusals must precede the success fixture"
    );

    // The fixtures never bypass the state machine with a second execution surface.
    for forbidden in [
        "CapabilityBroker",
        "EventStorePort",
        "KianaHarness",
        "DaemonHost",
        "tokio::spawn",
        "std::process::Command",
        "std::fs::",
        "reqwest::",
    ] {
        assert!(
            !fixtures.contains(forbidden) && !domain.contains(forbidden),
            "CO-10 fixtures/domain must stay a pure state machine: {forbidden}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));

    // The manual lane runs exactly the two registered targets and nothing else.
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test co10_project_charter_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test co10_project_charter --locked -- --test-threads=1",
        "cargo test -p kiana-core --test co10_project_charter_guard --no-run --locked",
        "cargo test -p kiana-domain --test co10_project_charter --no-run --locked",
    ] {
        assert!(lane.contains(step), "CO-10 lane step missing: {step}");
    }
    for forbidden in ["continue-on-error", "--ignored", "|| true"] {
        assert!(
            !lane.contains(forbidden),
            "CO-10 lane must fail closed: {forbidden}"
        );
    }
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:\n  contents: read"));

    // Automatic coverage: both targets stay registered in the unified shard book.
    assert!(shards.contains("\"co10_project_charter\""));
    assert!(shards.contains("\"co10_project_charter_guard\""));
    assert!(shards.contains("\"kiana-domain-s2/4\""));
    assert!(shards.contains("\"kiana-core-s2/6\""));

    // The baseline records the CI-only evidence boundary.
    assert!(baseline.contains("co10-project-charter-baseline"));
    assert!(baseline.contains("deny"));
    assert!(baseline.contains("proof_level: source"));
}
