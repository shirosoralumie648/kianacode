//! CO-09 source guard.
//!
//! The first test keeps the intake boundary on the ControlPlane command path and asserts the
//! measurement/organization gates. The second test binds the completion slice to the declared
//! fixtures, the manual lane and the automatic shards, and fails closed on a second execution
//! loop or a removed refusal.

#[test]
fn company_objective_initiative_intake_stays_on_control_plane_and_has_measurement_gates() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "pub struct Objective",
        "measurement_method",
        "objective_measurement_method_required",
        "objective_owner_mismatch",
        "pub struct Initiative",
        "initiative_initial_state_invalid",
        "initiative_objective_missing",
        "initiative_objective_approval_required",
        "initiative_project_objective_ancestry_mismatch",
        "CompanyCommand::ProposeObjective",
        "CompanyCommand::SubmitInitiative",
        "CompanyCommandRequest",
        "authorize_context",
        "company_idempotency_conflict",
    ] {
        assert!(
            domain.contains(marker) || policy.contains(marker) || core.contains(marker),
            "CO-09 marker missing: {marker}"
        );
    }

    assert!(domain.contains("measurement_method"));
    assert!(domain.contains("initiative.sponsor_id == a.actor_id"));
    assert!(domain.contains("objective.organization_id == initiative.organization_id"));
    assert!(core.contains("handle_company_command"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("EventStorePort"));
}

#[test]
fn co09_intake_completion_slice_stays_wired_to_fixtures_lane_and_shards() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");
    let fixtures = include_str!("../../kiana-domain/tests/co09_objective_initiative.rs");
    let lane = include_str!("../../.github/workflows/co09-company-intake.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    let baseline = include_str!("../../docs/roadmap/co09-company-intake-baseline.md");

    // Refusal codes the completion slice relies on. Each one is the stable reason the domain (or
    // the ControlPlane boundary) returns before any related object changes.
    for code in [
        "objective_measurement_invalid",
        "objective_measurement_method_required",
        "objective_owner_mismatch",
        "objective_organization_owner_mismatch",
        "initiative_information_gaps_invalid",
        "initiative_information_gaps_required",
        "initiative_objective_approval_required",
        "initiative_decision_required",
        "initiative_conversion_requires_approval",
        "initiative_already_converted",
        "initiative_project_already_linked",
        "initiative_project_objective_ancestry_mismatch",
        "direct_intake_reason_invalid",
        "direct_intake_objective_required",
        "direct_intake_project_not_pending",
        "direct_intake_initiative_ancestry_conflict",
        "direct_intake_conflict",
        "direct_intake_invalid",
        "project_intake_provenance_required",
        "project_intake_objective_ancestry_mismatch",
    ] {
        assert!(
            domain.contains(code) || policy.contains(code) || core.contains(code),
            "CO-09 refusal code missing from source: {code}"
        );
        assert!(
            fixtures.contains(code),
            "CO-09 refusal code is not exercised by the domain fixture: {code}"
        );
    }

    // Accepted contract surface: the explicit direct-intake command, the provenance projection
    // and the ControlPlane boundary gate that refuses go/no-go without intake provenance.
    for marker in [
        "RecordDirectIntake",
        "DirectProjectIntake",
        "ProjectIntakeProvenance",
        "pub fn project_intake_provenance",
        "DirectProjectIntakeRecorded",
    ] {
        assert!(
            domain.contains(marker),
            "CO-09 domain marker missing: {marker}"
        );
    }
    assert!(policy.contains("CompanyCommand::RecordDirectIntake { .. }"));
    assert!(
        core.contains("CompanyCommand::ApproveProject { project_id, .. } = &request.command"),
        "CO-09 approval gate missing from the ControlPlane boundary"
    );
    assert!(
        core.contains("state.project_intake_provenance(project_id)"),
        "CO-09 approval gate must consult the domain provenance projection"
    );

    // Deny-first order: the card names a conversion refusal and a direct-intake/success fixture,
    // and the success fixture is declared last.
    let deny_first = fixtures
        .find("fn initiative_cannot_convert_without_approved_objective_and_sponsor_decision")
        .expect("card-named conversion refusal fixture");
    let success = fixtures
        .find("fn approved_initiative_converts_once_and_keeps_objective_ancestry")
        .expect("card-named conversion success fixture");
    let direct = fixtures
        .find("fn direct_project_intake_records_the_skip_reason_and_keeps_objective_ancestry")
        .expect("direct intake fixture");
    let approval = fixtures
        .find("fn objective_approval_rejects_nan_inverted_window_wrong_owner_and_organization")
        .expect("objective approval refusal fixture");
    assert!(
        deny_first < success,
        "deny fixture must precede the success fixture"
    );
    assert!(
        approval < direct,
        "refusals must precede the direct-intake fixture"
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
            "CO-09 fixtures/domain must stay a pure state machine: {forbidden}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));

    // The manual lane runs exactly the two registered targets and nothing else.
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test co09_objective_initiative --locked -- --test-threads=1",
        "cargo test -p kiana-core --test co09_company_intake_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test co09_objective_initiative --no-run --locked",
        "cargo test -p kiana-core --test co09_company_intake_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "CO-09 lane step missing: {step}");
    }
    for forbidden in ["continue-on-error", "--ignored", "|| true"] {
        assert!(
            !lane.contains(forbidden),
            "CO-09 lane must fail closed: {forbidden}"
        );
    }
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:\n  contents: read"));

    // Automatic coverage: both targets stay registered in the unified shard book.
    assert!(shards.contains("\"co09_objective_initiative\""));
    assert!(shards.contains("\"co09_company_intake_guard\""));
    assert!(shards.contains("\"kiana-domain-s2/4\""));
    assert!(shards.contains("\"kiana-core-s2/6\""));

    // The baseline records the CI-only evidence boundary.
    assert!(baseline.contains("co09-company-intake-baseline"));
    assert!(baseline.contains("deny"));
    assert!(baseline.contains("proof_level: source"));
}
