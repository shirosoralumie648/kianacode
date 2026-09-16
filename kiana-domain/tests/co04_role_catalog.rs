use kiana_domain::{
    DepartmentCatalog, DepartmentSpec, PromptBundle, RoleCatalog, RoleSpec, ROLE_ANALYST,
    ROLE_LIBRARIAN, ROLE_QA,
};

#[test]
fn versioned_role_and_department_catalogs_are_deterministic() {
    let roles = RoleCatalog::builtin();
    roles.validate().unwrap();
    assert_eq!(roles.roles.len(), 9);
    assert_eq!(roles.catalog_digest, RoleCatalog::builtin().catalog_digest);
    for role_id in [ROLE_ANALYST, ROLE_QA, ROLE_LIBRARIAN] {
        let role = RoleSpec::lookup(role_id).unwrap();
        role.validate().unwrap();
        assert!(!role.prompt_hash.is_empty());
        assert!(role.input_schema.ends_with(".v1"));
        assert!(role.output_schema.ends_with(".v1"));
    }

    let departments = DepartmentCatalog::builtin();
    departments.validate().unwrap();
    assert_eq!(departments.departments.len(), 5);
    assert_eq!(DepartmentSpec::monitoring().roles, ["reviewer", ROLE_QA]);
    assert_eq!(DepartmentSpec::closing().roles, ["closer", ROLE_LIBRARIAN]);
}

#[test]
fn role_pack_metadata_cannot_be_forged_into_permissions_or_unknown_role() {
    let mut role = RoleSpec::analyst();
    role.tools.push("danger-full-access".to_owned());
    assert_eq!(role.validate().unwrap_err(), "role_spec_tool_unknown");

    let mut role = RoleSpec::qa();
    role.model_profile = "planning".to_owned();
    assert_eq!(role.validate().unwrap_err(), "role_model_profile_mismatch");
    assert!(RoleSpec::lookup("risk-scout").is_none());
    assert!(!role.allows_tool("apply_patch"));
}

#[test]
fn prompt_bundle_carries_role_and_io_versions_without_permission_from_text() {
    let role = RoleSpec::qa();
    let encoded = PromptBundle::for_role(&role).encode().unwrap();
    let decoded = PromptBundle::decode(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.role_prompt_hash, role.prompt_hash);
    assert_eq!(decoded.model_profile, "quality");

    let mut forged = serde_json::to_value(decoded).unwrap();
    forged["model_profile"] = serde_json::json!("initiating");
    assert_eq!(
        PromptBundle::decode(&serde_json::to_string(&forged).unwrap()).unwrap_err(),
        "prompt_bundle_role_metadata_mismatch"
    );
}
