//! CAP-02 有界 JSON、schema 和 canonical action input 的 Domain 合同测试。
//!
//! 这些 fixture 在领域层直接传入 JSON bytes/Value，锁定 parser、归一化与摘要规则；它们不
//! 调用真实 provider、ControlPlane 或 Broker。跨层 `include_str!` 检查另见 Core source guard。
use kiana_domain::{
    canonical_action_input_digest, normalize_capability_action, parse_bounded_json, CapabilityKind,
    CapabilityRequest, PreparedAction, RequestId, RiskLevel, TOOL_JSON_MAX_DEPTH,
};
use serde_json::{Map, Value};

// 统一构造最小 shell action；请求 ID 每次不同，用来确认 canonical input digest 不依赖顶层 ID。
fn shell(arguments: Value) -> CapabilityRequest {
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        arguments,
    )
    .with_risk(RiskLevel::ReadOnly)
}

/// 键顺序和 null 删除后语义相同的输入应得到相同 canonical digest。
#[test]
fn equivalent_json_inputs_have_the_same_digest() {
    let first = shell(
        serde_json::json!({"command":"printf ok","workdir":".","timeout_ms":1000,"unused":null}),
    );
    let mut ordered = Map::new();
    ordered.insert("timeout_ms".to_owned(), serde_json::json!(1000));
    ordered.insert("unused".to_owned(), Value::Null);
    ordered.insert("workdir".to_owned(), serde_json::json!("."));
    ordered.insert("command".to_owned(), serde_json::json!("printf ok"));
    let second = shell(Value::Object(ordered));
    let mut first = first;
    let mut second = second;
    normalize_capability_action(&mut first).unwrap();
    normalize_capability_action(&mut second).unwrap();
    assert_eq!(
        canonical_action_input_digest(&first).unwrap(),
        canonical_action_input_digest(&second).unwrap()
    );
}

/// 改变实际执行命令后摘要必须变化，避免把不同副作用当成同一输入。
#[test]
fn execution_affecting_input_changes_change_digest() {
    let mut first = shell(serde_json::json!({"command":"printf one"}));
    let mut second = shell(serde_json::json!({"command":"printf two"}));
    normalize_capability_action(&mut first).unwrap();
    normalize_capability_action(&mut second).unwrap();
    assert_ne!(
        canonical_action_input_digest(&first).unwrap(),
        canonical_action_input_digest(&second).unwrap()
    );
}

/// MCP 新旧工具名同时出现但指向不同工具时必须拒绝，不能选择其中一个继续执行。
#[test]
fn conflicting_mcp_tool_aliases_are_rejected() {
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Network,
        "mcp.call",
        serde_json::json!({"tool":"echo","tool_name":"different","arguments":{}}),
    )
    .with_risk(RiskLevel::ExternalSideEffect);
    assert_eq!(
        normalize_capability_action(&mut request).unwrap_err(),
        "action_tool_alias_conflict"
    );
}

/// 过深参数在 dispatch 前拒绝；普通 `$ref` JSON 可解析，但本地 schema 合同不允许外部引用。
#[test]
fn schema_depth_and_reference_limits_fail_before_dispatch() {
    let mut nested = serde_json::json!(true);
    for _ in 0..=TOOL_JSON_MAX_DEPTH {
        nested = serde_json::json!({"nested":nested});
    }
    let deep = serde_json::json!({"command":"printf ok","nested":nested});
    let mut request = shell(deep);
    assert!(normalize_capability_action(&mut request).is_err());
    assert!(parse_bounded_json(br#"{"$ref":"https://example.invalid/schema"}"#).is_ok());
    assert!(kiana_domain::validate_schema_contract(
        &serde_json::json!({"$ref":"https://example.invalid/schema"})
    )
    .is_err());
}

/// 检查互相矛盾的字符串、数组、数值和整数区间均由 schema 合同拒绝。
#[test]
fn contradictory_schema_bounds_are_rejected_before_dispatch() {
    let cases = [
        (
            serde_json::json!({"type":"string","minLength":3,"maxLength":2}),
            "schema_length_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"array","items":true,"minItems":3,"maxItems":2}),
            "schema_item_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"number","minimum":3,"maximum":2}),
            "schema_number_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"number","exclusiveMinimum":2,"maximum":2}),
            "schema_number_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"number","minimum":2,"exclusiveMaximum":2}),
            "schema_number_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"number","exclusiveMinimum":2,"exclusiveMaximum":2}),
            "schema_number_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"number","minimum":1,"exclusiveMinimum":3,"maximum":3}),
            "schema_number_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"number","minimum":3,"maximum":5,"exclusiveMaximum":3}),
            "schema_number_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"integer","minimum":1.1,"maximum":1.9}),
            "schema_integer_bounds_invalid",
        ),
        (
            serde_json::json!({"type":"integer","exclusiveMinimum":1,"exclusiveMaximum":2}),
            "schema_integer_bounds_invalid",
        ),
        (
            serde_json::json!({"type":["integer"],"minimum":1.1,"maximum":1.9}),
            "schema_integer_bounds_invalid",
        ),
    ];

    for (schema, expected_error) in cases {
        assert_eq!(
            kiana_domain::validate_schema_contract(&schema).unwrap_err(),
            expected_error
        );
        assert_eq!(
            kiana_domain::validate_schema_value(&serde_json::json!(2), &schema).unwrap_err(),
            expected_error
        );
    }
}

/// 为边界测试反向确认非空区间可接受，包括 inclusive/exclusive 端点和类型联合。
#[test]
fn nonempty_schema_bound_edges_are_accepted() {
    let cases = [
        (
            serde_json::json!({"type":"string","minLength":2,"maxLength":2}),
            serde_json::json!("ok"),
        ),
        (
            serde_json::json!({"type":"array","items":true,"minItems":2,"maxItems":2}),
            serde_json::json!([1, 2]),
        ),
        (
            serde_json::json!({"type":"number","minimum":2,"maximum":2}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":"number","exclusiveMinimum":1,"maximum":2}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":"number","minimum":2,"exclusiveMaximum":3}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":"number","exclusiveMinimum":1,"exclusiveMaximum":3}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":"number","minimum":1,"exclusiveMinimum":2,"maximum":4,"exclusiveMaximum":3}),
            serde_json::json!(2.5),
        ),
        (
            serde_json::json!({"type":"integer","minimum":2,"maximum":2}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":"integer","exclusiveMinimum":1,"exclusiveMaximum":3}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":"integer","minimum":1.1,"maximum":2}),
            serde_json::json!(2),
        ),
        (
            serde_json::json!({"type":["integer","number"],"minimum":1.1,"maximum":1.9}),
            serde_json::json!(1.5),
        ),
        (
            serde_json::json!({"type":["integer","string"],"minimum":1.1,"maximum":1.9}),
            serde_json::json!("integer alternative"),
        ),
    ];
    for (schema, value) in cases {
        kiana_domain::validate_schema_contract(&schema).unwrap();
        kiana_domain::validate_schema_value(&value, &schema).unwrap();
    }
}

/// 原始 JSON 中重复对象键必须在 serde 折叠成单个 Value 之前被识别。
#[test]
fn duplicate_json_object_fields_are_rejected_before_value_collapse() {
    let duplicate = br#"{"command":"printf ok","command":"rm -rf /"}"#;

    let error = parse_bounded_json(duplicate).unwrap_err();

    assert!(error.contains("json_duplicate_key"));
}

/// caller 伪造 actor/project/role 只改变待准备输入，不会自动成为服务端授权上下文。
#[test]
fn reserved_authority_fields_are_not_a_server_grant() {
    let base = shell(serde_json::json!({"command":"printf ok"}));
    let mut forged = base.clone();
    forged.arguments["actor_id"] = serde_json::json!("attacker");
    forged.arguments["project_root"] = serde_json::json!("/outside");
    forged.arguments["role_id"] = serde_json::json!("pm");
    let prepared = PreparedAction::new(base).unwrap();
    let forged_prepared = PreparedAction::new(forged).unwrap();
    assert_ne!(prepared.input_digest(), forged_prepared.input_digest());
}
