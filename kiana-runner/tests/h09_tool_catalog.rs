use kiana_domain::{
    current_tool_catalog, CapabilityKind, ModelToolCall, ToolCatalogSnapshot, TOOL_APPLY_PATCH,
    TOOL_MCP, TOOL_MEMORY_SEARCH, TOOL_MEMORY_WRITE, TOOL_SHELL,
};
use kiana_runner::capability_for_tool;
use serde_json::json;

fn minimal_call(name: &str) -> ModelToolCall {
    let arguments = match name {
        TOOL_SHELL => json!({"command":"pwd"}),
        TOOL_APPLY_PATCH => json!({"patch":"*** Begin Patch\n*** End Patch\n"}),
        TOOL_MCP => json!({"tool":"echo"}),
        TOOL_MEMORY_SEARCH => json!({"query":"catalog"}),
        TOOL_MEMORY_WRITE => {
            json!({"collection":"instance-scratch","text":"catalog","source":"explicit:h09"})
        }
        _ => json!({}),
    };
    ModelToolCall {
        id: format!("h09-{name}"),
        name: name.to_owned(),
        arguments,
    }
}

#[test]
fn unadvertised_tool_and_wire_name_collision_are_rejected() {
    let unknown = ModelToolCall {
        id: "unknown".to_owned(),
        name: "shell.exec.injected".to_owned(),
        arguments: json!({"command":"echo nope"}),
    };
    assert_eq!(
        capability_for_tool(&unknown, "read-only", "/h09"),
        Err("tool_unsupported:shell.exec.injected".to_owned())
    );

    let mut catalog = current_tool_catalog();
    let second_wire = catalog.tools[1].wire_name.clone();
    catalog.tools[0].wire_name = second_wire;
    assert_eq!(
        catalog.validate(),
        Err("tool_catalog_snapshot_invalid".to_owned())
    );
}

#[test]
fn catalog_change_cannot_reuse_old_approval() {
    let mut catalog = current_tool_catalog();
    catalog.tools[0].output_limit += 1;
    assert!(catalog.validate().is_err());
    let encoded = serde_json::to_value(ToolCatalogSnapshot::current()).unwrap();
    assert_eq!(encoded["schema"], "kiana.tool-catalog.v1");
    assert!(encoded["digest"].as_str().unwrap().starts_with("sha256:"));
}

#[test]
fn schema_mapper_and_broker_resolve_same_tool_version() {
    let catalog = current_tool_catalog();
    catalog.validate().unwrap();
    for name in [
        TOOL_SHELL,
        TOOL_APPLY_PATCH,
        TOOL_MCP,
        TOOL_MEMORY_SEARCH,
        TOOL_MEMORY_WRITE,
    ] {
        let descriptor = catalog.descriptor(name).unwrap();
        let request = capability_for_tool(&minimal_call(name), "read-only", "/h09").unwrap();
        assert_eq!(request.operation, descriptor.operation);
        assert_eq!(request.capability, descriptor.capability);
    }
    assert_eq!(
        catalog.descriptor(TOOL_SHELL).unwrap().capability,
        CapabilityKind::Process
    );
}
