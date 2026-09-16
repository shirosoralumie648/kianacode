#[test]
fn company_commands_use_one_policy_and_human_decision_boundary() {
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    let business = include_str!("../src/company_business.rs");

    for marker in [
        "CompanyCommandPolicy",
        "DecisionPurpose",
        "HumanDecision",
        "HumanTask",
        "company_human_decision_required",
        "command_policy_roles",
    ] {
        assert!(policy.contains(marker), "policy marker missing: {marker}");
    }
    for marker in ["allowed_roles", "CompanyCommand", "company_role_denied"] {
        assert!(
            domain.contains(marker),
            "command contract marker missing: {marker}"
        );
    }
    for marker in [
        "let command_policy = request.command.policy()",
        "command_policy.authorize_context",
        "proof.human_decision",
        "human_decision",
    ] {
        assert!(
            core.contains(marker),
            "core policy gate marker missing: {marker}"
        );
    }
    assert!(business.contains("actor_is_human"));
}
