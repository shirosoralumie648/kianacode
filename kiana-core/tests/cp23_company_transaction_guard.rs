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

/// CP-23 completion slice: the protected mutation permission table is the single subject and
/// precondition registration, and the ControlPlane, memory-promotion and data-revocation
/// boundaries all consult it before writing facts.
#[test]
fn cp23_protected_mutation_permissions_are_registered_and_enforced() {
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let company = include_str!("../src/company.rs");
    let memory = include_str!("../src/memory_proposals.rs");
    let governance = include_str!("../src/data_governance.rs");

    for marker in [
        "COMPANY_MUTATION_PERMISSION_SCHEMA",
        "kiana.company-mutation-permission.v1",
        "pub enum CompanyMutationClass",
        "pub struct CompanyMutationPermission",
        "pub fn company_mutation_permissions",
        "pub fn operator_mutation_permission",
        "pub fn authorize_policy",
        "pub fn authorize_operator_context",
        "pub fn authorize_operator_request",
        "pub fn mutation_class",
        "pub fn mutation_permission",
        "company_mutation_permission_drift",
        "company_mutation_permission_invalid",
        "company_mutation_permission_role_invalid",
        "company_operator_mutation_permission_invalid",
        "subject_denied_code",
        "preconditions",
    ] {
        assert!(
            policy.contains(marker),
            "CP-23 permission table marker missing: {marker}"
        );
    }

    // Every mutation class named by the card registers explicit subjects and preconditions.
    for class in [
        "Assignment",
        "PacketClaim",
        "PacketChange",
        "Review",
        "Acceptance",
        "Delivery",
        "Closeout",
        "MemoryPromote",
        "DataRevoke",
    ] {
        assert!(
            policy.contains(&format!("CompanyMutationClass::{class}")),
            "CP-23 mutation class missing: {class}"
        );
    }
    for code in [
        "business_organization_scope_denied",
        "business_assignment_owner_or_state_invalid",
        "company_packet_not_claimed",
        "company_claim_not_expired",
        "company_claim_worker_not_stopped",
        "business_rework_cannot_change_baseline",
        "business_author_cannot_review",
        "business_review_model_result_mismatch",
        "business_independent_review_required",
        "business_acceptance_author_or_evidence_mismatch",
        "delivery_recipient_ack_required",
        "closing_confirmed_delivery_required",
        "closing_role_separation_required",
        "closing_incident_unresolved",
        "memory_review_operator_required",
        "memory_proposal_data_revoked",
        "governance_operator_required",
        "governance_project_required",
        "governance_actor_required",
    ] {
        assert!(
            policy.contains(code),
            "CP-23 precondition code missing from the table: {code}"
        );
    }

    // The reclaim mutation is explicitly registered; without a role entry the empty fallback
    // denies every subject and the documented expired-lease scan can never commit. The exact
    // subject set is asserted behaviourally by the domain fixture.
    assert!(policy.contains("CompanyCommand::ReclaimPacketClaim { .. } => &["));
    assert!(policy.contains("CompanyMutationClass::PacketClaim => &["));

    // The Company command boundary revalidates the class entry against the per-command policy
    // before the state is read, so drift cannot change a related object.
    let handler = company
        .find("pub(crate) async fn handle_company_command")
        .expect("company command handler must exist");
    let handler_body = &company[handler..];
    let permission_gate = handler_body
        .find(".mutation_permission()")
        .expect("the permission table lookup must exist in the handler");
    let policy_drift = handler_body
        .find("authorize_policy(&command_policy)")
        .expect("the class entry must be revalidated against the per-command policy");
    let state_read = handler_body
        .find("self.load_company(&context).await?")
        .expect("company state read must exist in the handler");
    assert!(
        permission_gate < policy_drift && policy_drift < state_read,
        "the permission table must be enforced before the Company state is read"
    );

    // The two operator surfaces consult the same table instead of local ad-hoc checks.
    assert!(memory.contains("operator_mutation_permission("));
    assert!(memory.contains("\"memory.review\""));
    assert!(memory.contains("\"accept_proposal\""));
    assert!(memory.contains("authorize_operator_context(context)"));
    assert!(memory.contains("memory_review_operator_required"));
    assert!(governance.contains("operator_mutation_permission("));
    assert!(governance.contains("\"data.governance\""));
    assert!(governance.contains("authorize_operator_request("));
    assert!(governance.contains("governance_action_unregistered"));

    // No second execution loop or model surface is introduced by the slice.
    for forbidden in ["CapabilityBroker", "KianaHarness", "ModelClient"] {
        assert!(
            !policy.contains(forbidden),
            "the permission table must stay a pure contract: {forbidden}"
        );
    }
}

/// The three acceptance fixtures named by the card exist in the daemon behaviour target and are
/// bound by name to the CP-23 manual lane, and the lane cannot fan out automatically.
#[test]
fn cp23_named_fixtures_and_lane_are_wired() {
    let daemon = include_str!("../../kiana-daemon/tests/cp23_company_transactions.rs");
    let domain_fixture = include_str!("../../kiana-domain/tests/cp23_company_policy.rs");
    let workflow = include_str!("../../.github/workflows/cp23-company-transactions.yml");

    for target in [
        "cargo test -p kiana-domain --test cp23_company_policy",
        "cargo test -p kiana-core --test cp23_company_transaction_guard",
        "cargo test -p kiana-daemon --test cp23_company_transactions",
    ] {
        assert!(
            workflow.contains(target),
            "CP-23 lane target missing: {target}"
        );
    }
    for fixture in [
        "cp_builder_cannot_self_accept_or_promote_authority",
        "cp_company_command_conflict_changes_no_related_object",
        "cp_close_cannot_hide_unresolved_effects",
    ] {
        assert!(
            daemon.contains(fixture),
            "CP-23 daemon fixture missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "CP-23 daemon fixture not wired into the lane by name: {fixture}"
        );
    }
    for fixture in [
        "company_mutation_permissions_register_every_protected_class_once",
        "registered_permissions_never_drift_from_the_command_policy",
        "cp_builder_cannot_self_accept_or_promote_authority",
        "packet_claim_and_reclaim_subjects_are_registered_and_deny_first",
        "operator_mutation_surfaces_deny_cells_and_unregistered_subjects",
    ] {
        assert!(
            domain_fixture.contains(fixture),
            "CP-23 domain fixture missing: {fixture}"
        );
    }

    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test-threads=1",
        "--exact",
        "--no-run",
    ] {
        assert!(
            workflow.contains(marker),
            "CP-23 lane step missing: {marker}"
        );
    }
    assert!(!workflow.contains("push:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("schedule:"));
    assert!(!workflow.contains("continue-on-error"));
    assert!(!workflow.contains("--ignored"));
    assert!(!daemon.contains("#[ignore]"));
    assert!(!domain_fixture.contains("#[ignore]"));
    assert!(workflow.contains("permissions:\n  contents: read"));
}
