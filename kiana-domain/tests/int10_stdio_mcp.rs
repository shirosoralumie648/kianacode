use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeSet;

fn advertisement(name: &str) -> McpToolAdvertisement {
    McpToolAdvertisement::new(name, json!({"type":"object","properties":{}}))
        .expect("valid MCP advertisement")
}

#[test]
fn server_tool_schema_cannot_grant_scope() {
    let mut tool = advertisement("lookup");
    tool.server_requested_scopes.insert("admin".to_owned());
    assert_eq!(
        tool.validate().unwrap_err(),
        "mcp_server_scope_not_authority"
    );
}

#[test]
fn unbound_tool_is_conditional_and_scope_free() {
    let handshake = McpCapabilityHandshake::negotiate(
        "2025-06-18",
        &json!({"name":"fixture-server"}),
        "request-session-secret-shaped-but-hashed",
        vec![advertisement("lookup")],
        Vec::new(),
        &BTreeSet::from(["read".to_owned()]),
    )
    .expect("handshake should retain conditional discovery metadata");
    assert_eq!(handshake.transport, MCP_STDIO_TRANSPORT);
    assert_eq!(handshake.state, McpSessionState::Ready);
    assert_eq!(handshake.tools.len(), 1);
    assert_eq!(handshake.tools[0].operation_id, "mcp.unbound");
    assert!(!handshake.tools[0].available);
    assert!(handshake.tools[0].conditional);
    assert!(!serde_json::to_string(&handshake)
        .expect("handshake serializes")
        .contains("request-session-secret-shaped-but-hashed"));
}

#[test]
fn conditional_tool_can_be_retracted_after_disconnect() {
    let mut handshake = McpCapabilityHandshake::negotiate(
        "2024-11-05",
        &json!({"name":"fixture-server"}),
        "session-ref",
        vec![advertisement("lookup")],
        Vec::new(),
        &BTreeSet::new(),
    )
    .expect("valid handshake");
    handshake
        .transition(McpSessionState::Disconnected)
        .expect("disconnect is terminal");
    handshake
        .transition(McpSessionState::Closed)
        .expect("disconnect cleanup closes the session");
    assert_eq!(handshake.state, McpSessionState::Closed);
    assert!(handshake.validate().is_ok());
    assert!(handshake.transition(McpSessionState::Ready).is_err());
}

#[test]
fn declared_schema_drift_is_denied_before_capability_projection() {
    let tool = advertisement("lookup");
    let declared = McpDeclaredTool {
        tool_name: "lookup".to_owned(),
        operation_id: "connector.lookup".to_owned(),
        required_scope: "read".to_owned(),
        input_schema_digest: json_digest(&json!({"type":"object"})),
        output_schema_digest: None,
        conditional: false,
    };
    assert_eq!(
        McpCapabilityHandshake::negotiate(
            "2025-03-26",
            &json!({"name":"fixture-server"}),
            "session-ref",
            vec![tool],
            vec![declared],
            &BTreeSet::from(["read".to_owned()]),
        )
        .unwrap_err(),
        "mcp_tool_schema_changed"
    );
}
