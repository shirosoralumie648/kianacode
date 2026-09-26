use kiana_domain::*;

fn profile() -> PolicyProfile {
    PolicyProfile::new(
        PolicyProfileId::new(),
        vec!["company.read".to_owned(), "company.review".to_owned()],
        DataBoundaryId::new(),
        7,
    )
    .expect("profile")
}

fn role_pack(role_id: &str, profile: &PolicyProfile) -> CompanyRolePackVersion {
    let mut value = CompanyRolePackVersion {
        schema: COMPANY_ROLE_PACK_SCHEMA.to_owned(),
        role_id: role_id.to_owned(),
        version: 1,
        prompt_hash: format!("sha256:{}", "p".repeat(64)),
        input_schema: format!("kiana.company.input.{role_id}.v1"),
        output_schema: format!("kiana.company.output.{role_id}.v1"),
        model_profile: format!("{role_id}-profile"),
        policy_profile_id: profile.profile_id.to_string(),
        policy_profile_version: profile.version,
        policy_profile_digest: profile.profile_digest.clone(),
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn role_ref(role: &CompanyRolePackVersion) -> CompanyRolePackRef {
    CompanyRolePackRef {
        role_id: role.role_id.clone(),
        version: role.version,
        digest: role.digest.clone(),
    }
}

fn coding_template(profile: &PolicyProfile, version: u64) -> CompanyTemplateVersion {
    let roles = ["sponsor", "pm", "builder", "reviewer", "closer"]
        .into_iter()
        .map(|role| role_pack(role, profile))
        .collect::<Vec<_>>();
    let mut template = CompanyTemplateVersion::coding_v1(
        "coding",
        roles.iter().map(role_ref).collect(),
        profile.profile_id.to_string(),
        profile.profile_digest.clone(),
    );
    template.version = version;
    if version > 1 {
        template.required_gates.push("human_acceptance".to_owned());
    }
    template.template_hash = template.canonical_hash();
    template
}

fn research_template(profile: &PolicyProfile) -> CompanyTemplateVersion {
    let roles = ["sponsor", "pm", "analyst", "reviewer", "closer"]
        .into_iter()
        .map(|role| role_pack(role, profile))
        .collect::<Vec<_>>();
    CompanyTemplateVersion::research_report_v1(
        "research-report",
        roles.iter().map(role_ref).collect(),
        profile.profile_id.to_string(),
        profile.profile_digest.clone(),
    )
}

fn proposal(
    template: &CompanyTemplateVersion,
    current: &CompanyTemplateVersion,
    process_ids: Vec<String>,
    migration_required: bool,
) -> CompanyTemplateConfigProposal {
    let mut value = CompanyTemplateConfigProposal {
        schema: COMPANY_TEMPLATE_PROPOSAL_SCHEMA.to_owned(),
        proposal_id: format!("proposal-{}", template.version),
        template_id: template.template_id.clone(),
        current_version: current.version,
        current_template_hash: current.template_hash.clone(),
        new_template: template.clone(),
        active_process_ids: process_ids,
        migration_required,
        source: CompanyConfigSource::TrustedOperator,
        prompt_allowed_tools: Vec::new(),
        approval_ref: None,
        acceptance_ref: None,
        status: CompanyConfigProposalStatus::Proposed,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn install_coding_registry() -> (
    CompanyTemplateRegistry,
    PolicyProfile,
    CompanyTemplateVersion,
) {
    let profile = profile();
    let template = coding_template(&profile, 1);
    let mut registry = CompanyTemplateRegistry::default();
    registry
        .install_policy_profile(profile.clone())
        .expect("policy profile");
    for role in ["sponsor", "pm", "builder", "reviewer", "closer"] {
        registry
            .install_role_pack(role_pack(role, &profile))
            .expect("role pack");
    }
    registry
        .install_template(template.clone())
        .expect("template");
    (registry, profile, template)
}

#[test]
fn template_upgrade_requires_acceptance_and_explicit_migration_for_active_processes() {
    let (mut registry, _profile, current) = install_coding_registry();
    registry.pin_process("process-1", "coding").expect("pin");
    let next = coding_template(
        registry
            .policy_profiles
            .values()
            .next()
            .and_then(|versions| versions.values().next())
            .expect("profile"),
        2,
    );
    let blocked = proposal(&next, &current, vec!["process-1".to_owned()], false);
    let mut no_migration = registry.clone();
    no_migration.propose_upgrade(blocked).expect("proposal");
    assert_eq!(
        no_migration
            .approve_upgrade("proposal-2", "approval:upgrade", "acceptance:upgrade")
            .unwrap_err(),
        "company_template_active_process_requires_migration"
    );

    let mut upgrade = proposal(&next, &current, vec!["process-1".to_owned()], true);
    registry.propose_upgrade(upgrade.clone()).expect("proposal");
    assert_eq!(
        registry
            .approve_upgrade("proposal-2", "", "acceptance:upgrade")
            .unwrap_err(),
        "company_template_approval_required"
    );
    registry
        .approve_upgrade("proposal-2", "approval:upgrade", "acceptance:upgrade")
        .expect("approved upgrade");
    assert_eq!(registry.active_template("coding").unwrap().version, 2);
    assert_eq!(registry.pinned_template("process-1").unwrap().version, 1);

    let migrated = registry
        .migrate_process("process-1", "proposal-2")
        .expect("explicit migration");
    assert_eq!(migrated.template_version, 2);
    assert_eq!(registry.pinned_template("process-1").unwrap().version, 2);

    let mut rollback = CompanyTemplateRollbackRecord {
        schema: COMPANY_TEMPLATE_ROLLBACK_SCHEMA.to_owned(),
        rollback_id: "rollback-2-to-1".to_owned(),
        template_id: "coding".to_owned(),
        from_version: 2,
        from_template_hash: next.template_hash.clone(),
        to_version: 1,
        to_template_hash: current.template_hash.clone(),
        approval_ref: "approval:rollback".to_owned(),
        reason: "restore reviewed process version".to_owned(),
        active_process_ids: vec!["process-1".to_owned()],
        digest: String::new(),
    };
    rollback.digest = rollback.canonical_digest();
    registry.rollback_template(rollback).expect("rollback");
    assert_eq!(registry.active_template("coding").unwrap().version, 1);
    assert_eq!(registry.pinned_template("process-1").unwrap().version, 2);
    assert!(registry.proposals.contains_key("proposal-2"));
    assert_eq!(registry.rollback_records.len(), 1);

    upgrade.status = CompanyConfigProposalStatus::Approved;
    assert!(upgrade.validate().is_err());
}

#[test]
fn coding_and_research_templates_share_policy_but_keep_distinct_outputs() {
    let profile = profile();
    let coding = coding_template(&profile, 1);
    let research = research_template(&profile);
    assert_eq!(coding.policy_profile_digest, research.policy_profile_digest);
    assert_ne!(coding.output_contract, research.output_contract);
    assert!(coding.role_order.iter().any(|role| role == "builder"));
    assert!(!research.role_order.iter().any(|role| role == "builder"));
    assert!(research
        .required_gates
        .iter()
        .any(|gate| gate == "independent_check"));
    assert!(research
        .required_artifacts
        .iter()
        .any(|artifact| artifact == "local_report"));
    coding.validate().expect("coding contract");
    research.validate().expect("research contract");
}

#[test]
fn prompt_tools_untrusted_packages_and_hidden_scripts_cannot_form_authority() {
    let (registry, _profile, current) = install_coding_registry();
    let next = coding_template(
        registry
            .policy_profiles
            .values()
            .next()
            .and_then(|versions| versions.values().next())
            .expect("profile"),
        2,
    );
    let mut prompt = proposal(&next, &current, Vec::new(), false);
    prompt.prompt_allowed_tools = vec!["shell".to_owned()];
    prompt.digest = prompt.canonical_digest();
    assert_eq!(
        prompt.validate().unwrap_err(),
        "company_template_prompt_tools_not_authority"
    );

    let mut package = proposal(&next, &current, Vec::new(), false);
    package.source = CompanyConfigSource::UntrustedPackage;
    package.digest = package.canonical_digest();
    assert_eq!(
        package.validate().unwrap_err(),
        "company_template_untrusted_package_not_authority"
    );

    let mut script = proposal(&next, &current, Vec::new(), false);
    script.source = CompanyConfigSource::HiddenScript;
    script.digest = script.canonical_digest();
    assert_eq!(
        script.validate().unwrap_err(),
        "company_template_hidden_script_not_authority"
    );
}

#[test]
fn direct_second_template_install_requires_a_config_proposal() {
    let (mut registry, profile, current) = install_coding_registry();
    let next = coding_template(&profile, 2);
    assert_eq!(
        registry.install_template(next).unwrap_err(),
        "company_template_upgrade_requires_proposal"
    );
    assert_eq!(registry.active_template("coding").unwrap(), &current);
}
