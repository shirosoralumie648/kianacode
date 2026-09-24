#[test]
fn cp23_company_commands_use_the_control_plane_transaction_boundary() {
    let company = include_str!("../src/company.rs");
    let authority = include_str!("../src/authority.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");

    for marker in [
        "handle_company_command",
        "request.command.policy()",
        "company_proof",
        "validate_decision_for",
        "state.transition(&request.command",
        "commit_company",
        "commit_protected_event",
        "TransitionBatch",
        "CompanyCommandReceipt::new",
        "CompanyCommandRequest",
        "CompanyProof",
        "CompanyState",
    ] {
        assert!(
            company.contains(marker) || authority.contains(marker) || domain.contains(marker),
            "CP-23 transaction marker missing: {marker}"
        );
    }

    let proof = company
        .find("company_proof(&context, &state, &request.command)")
        .expect("proof must be derived before transition");
    let decision = company
        .find("validate_decision_for(")
        .expect("human decision must be rebound at the transaction boundary");
    let transition = company
        .find("state.transition(&request.command")
        .expect("state transition must remain in ControlPlane");
    let commit = company
        .find("self.commit_company(&context, event)")
        .expect("Company event must commit through the protected path");
    let receipt = company[commit..]
        .find("CompanyCommandReceipt::new(")
        .map(|offset| commit + offset)
        .expect("receipt must be server-generated");
    assert!(proof < decision && decision < transition && transition < commit && commit < receipt);

    let request_start = domain
        .find("pub struct CompanyCommandRequest")
        .expect("strict command request must exist");
    let request_end = domain[request_start..]
        .find("pub enum CompanyCommand")
        .map(|offset| request_start + offset)
        .expect("command enum follows the request DTO");
    let request_shape = &domain[request_start..request_end];
    assert!(request_shape.contains("expected_revision"));
    assert!(request_shape.contains("idempotency_key"));
    assert!(request_shape.contains("command"));
    assert!(!request_shape.contains("CompanyProof"));
    assert!(!request_shape.contains("CompanyCommandReceipt"));
    assert!(!request_shape.contains("model_conclusion"));

    assert!(!company.contains("CapabilityBroker"));
    assert!(!company.contains("KianaHarness"));
    assert!(!company.contains("ModelClient"));
    assert!(authority.contains("TransitionBatch"));
    assert!(authority.contains("aggregate_type: \"authority\""));
}
