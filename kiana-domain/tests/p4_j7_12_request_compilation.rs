use kiana_domain::{
    tool_catalog_hash, tool_schemas, validate_model_history, ModelMessage, TokenBudget, ToolNameMap,
};
use serde_json::json;

#[test]
fn five_tools_round_trip_through_one_reversible_map() {
    let tools = tool_schemas();
    let map = ToolNameMap::from_tools(&tools, true).expect("server tool map");
    assert_eq!(map.internal_to_wire.len(), 5);
    assert_eq!(map.internal_to_wire.len(), map.wire_to_internal.len());
    for tool in tools {
        let internal = tool["name"].as_str().unwrap();
        let wire = map.internal_to_wire.get(internal).unwrap();
        assert_eq!(map.internal_name(wire).unwrap(), internal);
        assert_eq!(map.wire_name(internal).unwrap(), wire);
    }
    map.validate().unwrap();
    assert!(map.digest().starts_with("sha256:"));
}

#[test]
fn wire_tool_name_collision_is_rejected() {
    let error = ToolNameMap::from_entries(
        vec![
            ("first.tool".to_owned(), "same_wire".to_owned()),
            ("second.tool".to_owned(), "same_wire".to_owned()),
        ],
        tool_catalog_hash(&[]),
        true,
    )
    .unwrap_err();
    assert_eq!(error, "tool_name_map_wire_collision");
}

#[test]
fn unknown_tool_is_rejected_before_protocol_encoding() {
    let error = ToolNameMap::from_tools(
        &[json!({
            "name": "provider.injected_tool",
            "parameters": {"type": "object"}
        })],
        true,
    )
    .unwrap_err();
    assert_eq!(error.code, "model_tool_unadvertised");
}

#[test]
fn orphan_tool_result_fails_before_send() {
    let error = validate_model_history(&[ModelMessage::tool("missing-call", "{}")]).unwrap_err();
    assert_eq!(error.code, "model_history_orphan_tool_result");
}

#[test]
fn final_wire_budget_keeps_schema_and_output_reserves_visible() {
    let budget = TokenBudget::new(512, 128, 256, 64, 2_000);
    assert_eq!(budget.accounting, "utf8_wire_bytes_with_framing_reserve");
    assert_eq!(
        budget.total,
        budget.messages + budget.system_prompt + budget.tool_schemas + budget.reserved_output
    );
    assert!(budget.validate().is_ok());
    assert_eq!(
        TokenBudget::new(512, 128, 256, 64, 900).validate(),
        Err("context_budget_exceeded")
    );
}
