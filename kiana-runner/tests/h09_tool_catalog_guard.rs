#[test]
fn h09_tool_catalog_is_versioned_and_single_source() {
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let schemas = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let tools = include_str!("../src/tools.rs");
    let provider = include_str!("../../kiana-provider/src/request.rs");
    let harness = include_str!("../src/harness.rs");
    let fixtures = include_str!("h09_tool_catalog.rs");
    for marker in [
        "TOOL_CATALOG_SCHEMA",
        "ToolCatalogSnapshot",
        "ToolDescriptor",
        "tool_catalog_digest",
        "tool_catalog_hash",
        "tool_wire_name",
        "current_tool_catalog",
        "output_limit",
        "execution_mode",
        "replay_class",
        "catalog_change_cannot_reuse_old_approval",
        "catalog_digest",
        "tool_catalog_changed",
        "runner_checkpoint_tool_catalog_changed",
        "capability_action_catalog_digest",
    ] {
        assert!(
            authority.contains(marker)
                || schemas.contains(marker)
                || actions.contains(marker)
                || model.contains(marker)
                || tools.contains(marker)
                || provider.contains(marker)
                || harness.contains(marker)
                || fixtures.contains(marker),
            "H09 marker missing: {marker}"
        );
    }
    assert!(authority.contains("ToolCatalogSnapshot::current().validate()"));
    assert!(tools.contains("catalog.validate()?"));
    assert!(provider.contains("kiana_domain::tool_wire_name"));
    assert!(model.contains("tool_catalog_hash != crate::tool_catalog_hash"));
    assert!(actions.contains("tool_catalog_digest"));
    for forbidden in [
        "TOOL_SPECS.len() != 5",
        "unadvertised_tool_fallback_to_shell",
        "catalog_change_reuses_approval",
        "wire_name_collision_ignored",
    ] {
        assert!(
            !authority.contains(forbidden)
                && !tools.contains(forbidden)
                && !provider.contains(forbidden),
            "forbidden H09 catalog bypass: {forbidden}"
        );
    }
}
