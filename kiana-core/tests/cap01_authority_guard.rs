#[test]
fn capability_catalog_binding_is_single_source_and_sealed_at_composition_root() {
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let handlers = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let runner = include_str!("../../kiana-runner/src/tools.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let baseline = include_str!("../../docs/roadmap/capability-authority-baseline.md");

    assert!(broker.contains("validate_catalog_bindings"));
    assert!(broker.contains("validate_action_catalog"));
    assert!(broker.contains("capability_handler_already_registered"));
    assert!(broker.contains("capability_binding_version_mismatch"));
    assert!(broker.contains("capability_catalog_sealed"));
    assert!(daemon.contains("capabilities.validate_catalog_bindings"));
    assert!(handlers.contains("register(broker"));
    assert!(memory.contains("register(broker"));
    assert!(mcp.contains("register("));
    assert!(runner.contains("tool_schemas"));
    assert!(runner.contains("tool_unsupported"));
    assert!(actions.contains("ACTION_OPERATIONS"));
    assert!(actions.contains("ACTION_HANDLER_BINDING_VERSION"));
    assert!(baseline.contains("duplicate_alias_or_operation_is_rejected"));
    assert!(baseline.contains("descriptor_binding_version_mismatch_never_dispatches"));
    assert!(baseline.contains("tool_authority_covers_every_model_visible_tool"));
    assert!(baseline.contains("Operator-only"));
}
