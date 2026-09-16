#[test]
fn role_catalog_and_provenance_stay_on_the_existing_harness_path() {
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let events = include_str!("../src/events.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let skills = include_str!("../../kiana-daemon/src/harness_skills.rs");
    let provider = include_str!("../../kiana-provider/src/config.rs");

    for marker in [
        "ROLE_ANALYST",
        "ROLE_QA",
        "ROLE_LIBRARIAN",
        "RoleCatalog",
        "DepartmentCatalog",
        "input_schema",
        "output_schema",
        "role_model_profile_mismatch",
    ] {
        assert!(
            roles.contains(marker),
            "role contract marker missing: {marker}"
        );
    }
    for marker in [
        "catalog_version",
        "role_version",
        "role_prompt_hash",
        "prompt_bundle_role_metadata_mismatch",
    ] {
        assert!(
            prompts.contains(marker) || model.contains(marker),
            "version marker missing: {marker}"
        );
    }
    for marker in [
        "role_input_schema",
        "role_output_schema",
        "role_catalog_schema",
        "ModelAssignment",
    ] {
        assert!(
            lifecycle.contains(marker),
            "run provenance marker missing: {marker}"
        );
    }
    for marker in [
        "role_spec_schema",
        "role_catalog_version",
        "role_resolution",
    ] {
        assert!(
            events.contains(marker),
            "receipt provenance marker missing: {marker}"
        );
    }
    for marker in [
        "RoleCatalog::builtin",
        "DepartmentCatalog::builtin",
        "model_profiles",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon catalog marker missing: {marker}"
        );
    }
    assert!(skills.contains("if project_trusted"));
    assert!(provider.contains("model_profile_unknown"));
}
