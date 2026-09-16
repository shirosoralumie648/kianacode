#[test]
fn role_model_profile_is_server_selected_and_reaches_provider_route() {
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    for marker in [
        "pub fn catalog() -> [RoleSpec; 9]",
        "model_profile",
        "role_model_profile_mismatch",
        "ROLE_PM",
        "ROLE_BUILDER",
        "ROLE_QA",
    ] {
        assert!(
            roles.contains(marker),
            "role catalog marker missing: {marker}"
        );
    }
    for marker in [
        "profile: role.model_profile.clone()",
        "ModelAssignment",
        "authority_revision",
    ] {
        assert!(
            lifecycle.contains(marker),
            "lifecycle routing marker missing: {marker}"
        );
    }
    assert!(model.contains("self.profile != role.model_profile"));
    for marker in [
        "assignment.profile",
        "model_profile_unconfigured",
        "fn connection(&self, spec: &ModelCallSpec)",
    ] {
        assert!(
            provider.contains(marker),
            "provider route marker missing: {marker}"
        );
    }
    assert!(config.contains("RoleSpec::catalog()"));
}
