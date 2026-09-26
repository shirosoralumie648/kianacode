#[test]
fn company_read_model_is_scope_bound_read_only_and_evidence_linked() {
    let model = include_str!("../../kiana-domain/src/company_read_model.rs");
    let governance = include_str!("../../kiana-core/src/company_governance.rs");
    let domain_governance = include_str!("../../kiana-domain/src/governance_gate.rs");
    let core = include_str!("../src/company_read_model.rs");
    for marker in [
        "COMPANY_READ_MODEL_SCHEMA",
        "CompanyReadModelSnapshot",
        "CompanyProjectView",
        "CompanyPacketView",
        "CompanyReadModelBlocker",
        "CompanyReadModelFreshness",
        "authorized_project_ids",
        "company_view_scope_denied",
        "allowed_actions",
        "evidence_links",
        "source_cursor",
        "projection_cursor",
        "authority_epoch",
        "CompanyGovernanceSnapshot",
        "project_company_read_model",
    ] {
        assert!(
            model.contains(marker)
                || governance.contains(marker)
                || domain_governance.contains(marker)
                || core.contains(marker),
            "CO-38 marker missing: {marker}"
        );
    }
    for forbidden in [
        "transcript",
        "chat_advance",
        "ModelClient::new",
        "Command::new",
    ] {
        assert!(
            !model.contains(forbidden),
            "CO-38 bypass marker present: {forbidden}"
        );
    }
}
