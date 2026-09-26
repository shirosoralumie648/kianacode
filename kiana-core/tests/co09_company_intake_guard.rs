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
