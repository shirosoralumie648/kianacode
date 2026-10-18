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

fn declared(tool_name: &str, required_scope: &str) -> McpDeclaredTool {
    let input_schema = json!({"type":"object","properties":{}});
    McpDeclaredTool {
        tool_name: tool_name.to_owned(),
        operation_id: format!("connector.{tool_name}"),
        required_scope: required_scope.to_owned(),
        input_schema_digest: json_digest(&input_schema),
        output_schema_digest: None,
        conditional: false,
    }
}

/// Every handshake bound is checked before a capability exists: unsupported protocol versions,
/// empty or control-bearing session references, oversized or ambiguous catalogues and unsupported
/// server schemas are all fail-closed.  A remote transport is also refused here, so a handshake
/// deserialized from an HTTP/SSE-shaped payload can never validate as a stdio capability set.
#[test]
fn handshake_bounds_protocol_session_catalog_and_unknown_fields() {
    let server_info = json!({"name":"fixture-server"});
    let scopes = BTreeSet::from(["read".to_owned()]);
    let long_protocol = "2".repeat(MCP_MAX_PROTOCOL_VERSION + 1);
    let long_session = "s".repeat(MCP_MAX_SESSION_REF + 1);
    let negotiate = |protocol: &str, session: &str, advertisements: Vec<McpToolAdvertisement>| {
        McpCapabilityHandshake::negotiate(
            protocol,
            &server_info,
            session,
            advertisements,
            Vec::new(),
            &scopes,
        )
    };

    for protocol in ["", "2024-01-01", long_protocol.as_str(), "2025-06-18\u{7}"] {
        assert_eq!(
            negotiate(protocol, "session-ref", vec![advertisement("lookup")]).unwrap_err(),
            "mcp_capability_handshake_invalid",
            "protocol {protocol:?} must be denied"
        );
    }
    for session in ["", "session\u{7}ref", long_session.as_str()] {
        assert_eq!(
            negotiate("2025-06-18", session, vec![advertisement("lookup")]).unwrap_err(),
            "mcp_capability_handshake_invalid",
            "session reference of {} bytes must be denied",
            session.len()
        );
    }
    assert_eq!(
        McpCapabilityHandshake::negotiate(
            "2025-06-18",
            &json!([1]),
            "session-ref",
            vec![advertisement("lookup")],
            Vec::new(),
            &scopes,
        )
        .unwrap_err(),
        "mcp_capability_handshake_invalid",
        "server metadata must be an object, never a server grant"
    );
    let oversized = (0..=MCP_MAX_HANDSHAKE_TOOLS)
        .map(|index| advertisement(&format!("tool-{index}")))
        .collect::<Vec<_>>();
    assert_eq!(
        negotiate("2025-06-18", "session-ref", oversized).unwrap_err(),
        "mcp_capability_handshake_invalid"
    );
    assert_eq!(
        negotiate(
            "2025-06-18",
            "session-ref",
            vec![advertisement("lookup"), advertisement("lookup")]
        )
        .unwrap_err(),
        "mcp_tool_ambiguous"
    );
    assert_eq!(
        McpCapabilityHandshake::negotiate(
            "2025-06-18",
            &server_info,
            "session-ref",
            vec![advertisement("lookup")],
            vec![declared("lookup", "read"), declared("lookup", "read")],
            &scopes,
        )
        .unwrap_err(),
        "mcp_tool_declaration_ambiguous"
    );

    let mut non_object = advertisement("lookup");
    non_object.input_schema = json!({"type":"string"});
    assert_eq!(
        non_object.validate().unwrap_err(),
        "mcp_tool_schema_object_required"
    );
    let mut unsupported = advertisement("lookup");
    unsupported.input_schema =
        json!({"type":"object","properties":{"value":{"type":"string","pattern":"^a+$"}}});
    assert_eq!(
        unsupported.validate().unwrap_err(),
        "mcp_tool_schema_unsupported"
    );
    let mut bad_output = advertisement("lookup");
    bad_output.output_schema = Some(json!({"type":"string"}));
    assert_eq!(
        bad_output.validate().unwrap_err(),
        "mcp_output_schema_object_required"
    );

    let handshake = negotiate("2025-06-18", "session-ref", vec![advertisement("lookup")])
        .expect("valid handshake");
    let mut unknown_field = serde_json::to_value(&handshake).expect("handshake serializes");
    unknown_field["server_grant"] = json!(true);
    assert!(
        serde_json::from_value::<McpCapabilityHandshake>(unknown_field).is_err(),
        "an unknown server grant field must not deserialize"
    );
    let mut remote = serde_json::to_value(&handshake).expect("handshake serializes");
    remote["transport"] = json!("sse");
    let remote = serde_json::from_value::<McpCapabilityHandshake>(remote).expect("shape is legal");
    assert_eq!(
        remote.validate().unwrap_err(),
        "mcp_capability_handshake_invalid",
        "a remote transport can never be projected as a stdio capability set"
    );
    let mut capability = serde_json::to_value(&handshake.tools[0]).expect("capability serializes");
    capability["scope_grant"] = json!("admin");
    assert!(
        serde_json::from_value::<McpToolCapability>(capability).is_err(),
        "a capability with an unknown grant field must not deserialize"
    );
}

/// Availability is decided by the binding intersection, never by the server: a declared operation
/// whose scope is missing from the binding stays conditional, and no scope string crosses the
/// handshake boundary.
#[test]
fn declared_scope_intersection_decides_availability() {
    let advertisements = vec![advertisement("lookup"), advertisement("restricted_lookup")];
    let handshake = McpCapabilityHandshake::negotiate(
        "2025-06-18",
        &json!({"name":"fixture-server"}),
        "session-ref",
        advertisements,
        vec![
            declared("restricted_lookup", "admin"),
            declared("lookup", "read"),
        ],
        &BTreeSet::from(["read".to_owned()]),
    )
    .expect("scope intersection is server-owned");
    let by_name = |name: &str| {
        handshake
            .tools
            .iter()
            .find(|tool| tool.tool_name == name)
            .expect("advertised tool is projected")
            .clone()
    };
    let lookup = by_name("lookup");
    assert!(lookup.available);
    assert!(!lookup.conditional);
    assert_eq!(lookup.operation_id, "connector.lookup");
    let restricted = by_name("restricted_lookup");
    assert!(!restricted.available);
    assert!(restricted.conditional);
    assert_eq!(restricted.operation_id, "connector.restricted_lookup");

    let rendered = serde_json::to_string(&handshake).expect("handshake serializes");
    for leaked in ["admin", "required_scope", "\"read\""] {
        assert!(
            !rendered.contains(leaked),
            "scope material leaked into the handshake: {leaked} in {rendered}"
        );
    }
    assert!(rendered.contains("binding_scope_digest"));

    // An unbound advertisement is retained as conditional metadata with no operation mapping.
    let unbound = McpCapabilityHandshake::negotiate(
        "2025-06-18",
        &json!({"name":"fixture-server"}),
        "session-ref",
        vec![advertisement("probe")],
        Vec::new(),
        &BTreeSet::from(["read".to_owned()]),
    )
    .expect("unbound discovery is retained as conditional metadata");
    assert_eq!(unbound.tools[0].operation_id, "mcp.unbound");
    assert!(!unbound.tools[0].available);
    assert!(unbound.tools[0].conditional);
    assert!(!unbound.tools[0].retracted);
}

/// A terminal session reclaims every unreachable capability: retraction is bounded, idempotent and
/// digest-bound, a live session cannot reclaim, and a reclaimed tool cannot become available again.
#[test]
fn terminal_session_reclaims_conditional_tools_and_cannot_reopen() {
    let mut handshake = McpCapabilityHandshake::negotiate(
        "2025-06-18",
        &json!({"name":"fixture-server"}),
        "session-ref",
        vec![advertisement("lookup"), advertisement("probe")],
        vec![declared("lookup", "read")],
        &BTreeSet::from(["read".to_owned()]),
    )
    .expect("valid handshake");
    assert_eq!(
        handshake.retract_unavailable_tools().unwrap_err(),
        "mcp_session_not_terminal",
        "a live session has nothing to reclaim yet"
    );

    let live_catalog = handshake.catalog_digest.clone();
    handshake
        .transition(McpSessionState::TimedOut)
        .expect("a timeout is terminal");
    assert_eq!(
        handshake
            .retract_unavailable_tools()
            .expect("terminal reclaim"),
        vec!["probe".to_owned()]
    );
    let reclaimed = handshake.catalog_digest.clone();
    assert_ne!(live_catalog, reclaimed);
    let probe = handshake
        .tools
        .iter()
        .find(|tool| tool.tool_name == "probe")
        .expect("retracted evidence is retained");
    assert!(probe.retracted);
    assert!(!probe.available);
    assert!(probe.conditional);
    let lookup = handshake
        .tools
        .iter()
        .find(|tool| tool.tool_name == "lookup")
        .expect("available operation is retained");
    assert!(!lookup.retracted);
    assert!(lookup.available);
    assert!(handshake.validate().is_ok());

    // Reclaiming twice is a no-op: the evidence and digest stay byte-stable.
    let settled = handshake.handshake_digest.clone();
    assert!(handshake
        .retract_unavailable_tools()
        .expect("idempotent reclaim")
        .is_empty());
    assert_eq!(handshake.handshake_digest, settled);
    assert_eq!(handshake.catalog_digest, reclaimed);

    // A terminal session never reopens, and a retracted capability never becomes available.
    assert_eq!(
        handshake.transition(McpSessionState::Ready).unwrap_err(),
        "mcp_session_transition_invalid"
    );
    let mut forgery = serde_json::to_value(&handshake).expect("handshake serializes");
    forgery["tools"][1]["available"] = json!(true);
    let mut forged =
        serde_json::from_value::<McpCapabilityHandshake>(forgery).expect("shape is legal");
    forged.handshake_digest = forged.digest();
    assert_eq!(
        forged.validate().unwrap_err(),
        "mcp_tool_capability_invalid",
        "a reclaimed capability cannot be flipped back to available"
    );
    let mut reopened = serde_json::to_value(&handshake).expect("handshake serializes");
    reopened["state"] = json!("ready");
    let mut reopened =
        serde_json::from_value::<McpCapabilityHandshake>(reopened).expect("shape is legal");
    reopened.handshake_digest = reopened.digest();
    assert_eq!(
        reopened.validate().unwrap_err(),
        "mcp_capability_handshake_invalid",
        "a retracted catalogue cannot belong to a live session"
    );
}

/// The audited evidence is a bounded digest set: field names are pinned, the session reference and
/// every scope value stay outside the payload, and the digests bind what was observed.
#[test]
fn handshake_evidence_binds_digests_without_session_or_scope_material() {
    let session_ref = "session-live-ref";
    let server_info = json!({"name":"fixture-server","capabilities":{"tools":{}}});
    let input_schema = json!({"type":"object","properties":{}});
    let mut handshake = McpCapabilityHandshake::negotiate(
        "2025-03-26",
        &server_info,
        session_ref,
        vec![advertisement("lookup"), advertisement("probe")],
        vec![declared("lookup", "read")],
        &BTreeSet::from(["read".to_owned()]),
    )
    .expect("valid handshake");
    handshake
        .transition(McpSessionState::Closed)
        .expect("session close is terminal");
    handshake
        .retract_unavailable_tools()
        .expect("close reclaims unreachable tools");

    let value = serde_json::to_value(&handshake).expect("handshake serializes");
    let keys = value
        .as_object()
        .expect("handshake is an object")
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        BTreeSet::from(
            [
                "schema",
                "transport",
                "protocol_version",
                "server_info_digest",
                "catalog_digest",
                "session_digest",
                "state",
                "tools",
                "handshake_digest",
            ]
            .map(str::to_owned),
        )
    );
    assert_eq!(
        value["session_digest"],
        json!(json_digest(&json!({"session_ref":session_ref})))
    );
    assert_eq!(value["state"], json!("closed"));
    assert_eq!(value["transport"], json!(MCP_STDIO_TRANSPORT));
    assert_eq!(value["protocol_version"], json!("2025-03-26"));
    assert_eq!(
        value["server_info_digest"],
        json!(json_digest(&server_info))
    );
    assert_eq!(
        value["tools"][0]["input_schema_digest"],
        json!(json_digest(&input_schema))
    );
    let tool_keys = value["tools"][0]
        .as_object()
        .expect("tool projection is an object")
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tool_keys,
        BTreeSet::from(
            [
                "schema",
                "tool_name",
                "operation_id",
                "input_schema_digest",
                "binding_scope_digest",
                "available",
                "conditional",
                "retracted",
            ]
            .map(str::to_owned),
        )
    );

    let rendered = serde_json::to_string(&handshake).expect("handshake serializes");
    for leaked in [session_ref, "read", "admin", "server_requested_scopes"] {
        assert!(
            !rendered.contains(leaked),
            "handshake evidence leaked {leaked}: {rendered}"
        );
    }
    assert!(handshake.validate().is_ok());
}
