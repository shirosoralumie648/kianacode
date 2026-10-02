use kiana_domain::{
    capability_action_descriptor, canonical_action_operation, model_tool_name,
    operator_only_action, tool_schemas, tool_spec, validate_tool_action_binding,
    validate_tool_authority, ACTION_HANDLER_BINDING_VERSION, ACTION_OPERATIONS, TOOL_SPECS,
    CapabilityKind, RiskLevel,
};

#[test]
fn tool_authority_covers_every_model_visible_tool() {
    validate_tool_authority().unwrap();
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
        let spec = tool_spec(name).unwrap();
        assert_eq!(spec.name, name);
        assert_eq!(canonical_action_operation(canonical), Some(spec.operation));
        assert!(ACTION_OPERATIONS.contains(&spec.operation));
        assert!(schemas.iter().any(|schema| schema["name"] == name));
        let descriptor = capability_action_descriptor(canonical).expect("action descriptor");
        assert_eq!(descriptor.operation, spec.operation);
        assert_eq!(descriptor.capability, spec.capability);
        assert_eq!(descriptor.minimum_risk, spec.risk_policy);
        assert_eq!(descriptor.binding_version, ACTION_HANDLER_BINDING_VERSION);
        for alias in spec.aliases {
            assert_eq!(model_tool_name(alias), Some(spec.name));
            assert_eq!(canonical_action_operation(alias), Some(spec.operation));
        }
    }
    assert_eq!(TOOL_SPECS.len(), visible.len());
}

#[test]
fn tool_action_binding_rejects_controlled_metadata_drift() {
    let canonical = tool_spec("shell").unwrap();

    let mut operation_mismatch = (*canonical).clone();
    operation_mismatch.operation = "apply_patch";
    assert_eq!(
        validate_tool_action_binding(&operation_mismatch).unwrap_err(),
        "tool_action_operation_mismatch"
    );

    let mut capability_mismatch = (*canonical).clone();
    capability_mismatch.capability = CapabilityKind::Filesystem;
    assert_eq!(
        validate_tool_action_binding(&capability_mismatch).unwrap_err(),
        "tool_action_metadata_mismatch"
    );

    let mut risk_mismatch = (*canonical).clone();
    risk_mismatch.risk_policy = RiskLevel::Critical;
    assert_eq!(
        validate_tool_action_binding(&risk_mismatch).unwrap_err(),
        "tool_action_metadata_mismatch"
    );
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
