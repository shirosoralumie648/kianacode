use kiana_domain::{
    canonical_action_input_digest, normalize_capability_action, parse_bounded_json, CapabilityKind,
    CapabilityRequest, PreparedAction, RequestId, RiskLevel, TOOL_JSON_MAX_DEPTH,
};
use serde_json::{Map, Value};

fn shell(arguments: Value) -> CapabilityRequest {
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        arguments,
    )
    .with_risk(RiskLevel::ReadOnly)
}

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
