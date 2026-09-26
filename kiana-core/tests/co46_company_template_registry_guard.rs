#[test]
fn company_template_registry_keeps_version_and_authority_boundaries() {
    let domain = include_str!("../../kiana-domain/src/company_template_registry.rs");
    let process = include_str!("../../kiana-domain/src/company_process.rs");
    let authority = include_str!("../../kiana-domain/src/authority.rs");
    let core = include_str!("../src/company_template_registry.rs");
    for marker in [
        "CompanyTemplateRegistry",
        "CompanyTemplateVersion",
        "CompanyRolePackVersion",
        "CompanyTemplateConfigProposal",
        "CompanyTemplateRollbackRecord",
        "CompanyOutputContract",
        "CodingSourceChange",
        "ResearchReportLocalDelivery",
        "CompanyConfigSource",
        "prompt_allowed_tools",
        "company_template_prompt_tools_not_authority",
        "company_template_untrusted_package_not_authority",
        "company_template_hidden_script_not_authority",
        "company_template_active_process_requires_migration",
        "company_template_acceptance_required",
        "migrate_process",
        "rollback_template",
        "PolicyProfile",
        "CompanyProcessTemplate",
        "install_company_template",
        "approve_company_template_upgrade",
    ] {
        assert!(
            domain.contains(marker)
                || process.contains(marker)
                || authority.contains(marker)
                || core.contains(marker),
            "CO-46 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ModelClient::new",
        "std::process::Command",
        "shell.exec",
        "git push",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "CO-46 authority bypass marker present: {forbidden}"
        );
    }
}
