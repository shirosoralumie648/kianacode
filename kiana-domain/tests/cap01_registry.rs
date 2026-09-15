use kiana_domain::{
    capability_action_descriptor, model_tool_name, operator_only_action, tool_schemas,
    ACTION_HANDLER_BINDING_VERSION, ACTION_OPERATIONS,
};

#[test]
fn tool_authority_covers_every_model_visible_tool() {
    let visible = [
        "shell",
        "apply_patch",
        "mcp",
        "memory.search",
        "memory.write",
    ];
    let schemas = tool_schemas();
    assert_eq!(schemas.len(), visible.len());
    for name in visible {
        let canonical = model_tool_name(name).expect("model tool mapping");
        assert!(schemas.iter().any(|schema| schema["name"] == name));
        let descriptor = capability_action_descriptor(canonical).expect("action descriptor");
        assert_eq!(descriptor.binding_version, ACTION_HANDLER_BINDING_VERSION);
    }
}

#[test]
fn operator_only_capabilities_never_enter_model_schema() {
    for operation in ACTION_OPERATIONS {
        if operator_only_action(operation) {
            assert!(
                model_tool_name(operation).is_none(),
                "operator operation leaked: {operation}"
            );
        }
    }
}
