//! INT-06 Runner boundary: the model tool surface carries no connector credential material.
//!
//! The card's deny list starts with "a raw token reaching Core/Runner/Event/UI".  These fixtures
//! pin the Runner half of that boundary: the fixed five-tool mapper drops every model-supplied
//! credential-shaped argument instead of forwarding it into a `CapabilityRequest`, the published
//! tool schemas declare no credential parameter, and the Runner sources never import or resolve a
//! `SecretRef`/`CredentialLease`.  Connector credentials stay server-owned and are issued by the
//! daemon at the adapter effect boundary, so the model cannot choose, switch or inject one.
//!
//! These tests only read sources and map in-memory tool calls.  They do not run the Harness,
//! reach ControlPlane/Broker, or prove any durable/live credential behavior.

use kiana_domain::{
    tool_schemas, ModelToolCall, TOOL_APPLY_PATCH, TOOL_MCP, TOOL_MEMORY_SEARCH, TOOL_MEMORY_WRITE,
    TOOL_SHELL,
};
use kiana_runner::capability_for_tool;
use serde_json::{json, Value};

const RAW_SENTINEL: &str = "INT06_RUNNER_RAW_SECRET_SENTINEL";

const FIXED_TOOLS: [&str; 5] = [
    TOOL_SHELL,
    TOOL_APPLY_PATCH,
    TOOL_MCP,
    TOOL_MEMORY_SEARCH,
    TOOL_MEMORY_WRITE,
];

fn minimal_arguments(name: &str) -> Value {
    match name {
        TOOL_SHELL => json!({"command": "pwd"}),
        TOOL_APPLY_PATCH => json!({"patch": "*** Begin Patch\n*** End Patch\n"}),
        TOOL_MCP => json!({"tool": "echo"}),
        TOOL_MEMORY_SEARCH => json!({"query": "connector"}),
        TOOL_MEMORY_WRITE => {
            json!({"collection": "instance-scratch", "text": "draft", "source": "explicit:int06"})
        }
        other => panic!("unexpected tool {other}"),
    }
}

fn credential_shaped(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    [
        "credential",
        "secret",
        "lease",
        "token",
        "authorization",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

#[test]
fn runner_mapping_drops_model_supplied_credential_fields() {
    for name in FIXED_TOOLS {
        let mut arguments = minimal_arguments(name);
        let object = arguments
            .as_object_mut()
            .expect("object arguments for the fixed tool surface");
        object.insert(
            "credential_ref".to_owned(),
            json!({"store": "env", "key": RAW_SENTINEL, "generation": 1}),
        );
        object.insert(
            "secret_ref".to_owned(),
            json!({"store": "env", "key": RAW_SENTINEL}),
        );
        object.insert(
            "credential_lease".to_owned(),
            json!({"lease_id": RAW_SENTINEL}),
        );
        object.insert("lease".to_owned(), json!({"raw": RAW_SENTINEL}));
        object.insert(
            "authorization".to_owned(),
            json!(format!("Bearer {RAW_SENTINEL}")),
        );
        let call = ModelToolCall {
            id: format!("int06-{name}"),
            name: name.to_owned(),
            arguments,
        };

        let request = capability_for_tool(&call, "read-only", "/repo").expect("mapped request");
        let serialized = serde_json::to_string(&request).expect("request encoding");
        assert!(
            !serialized.contains(RAW_SENTINEL),
            "{name} forwarded model-supplied credential material into the capability request"
        );
        for key in request
            .arguments
            .as_object()
            .expect("object arguments")
            .keys()
        {
            assert!(
                !credential_shaped(key),
                "{name} capability arguments expose credential-shaped key {key}"
            );
        }
    }
}

#[test]
fn runner_tool_surface_declares_no_credential_parameter() {
    let mut names = tool_schemas()
        .into_iter()
        .map(|schema| schema["name"].as_str().expect("schema name").to_owned())
        .collect::<Vec<_>>();
    names.sort();
    let mut expected = FIXED_TOOLS.map(str::to_owned).to_vec();
    expected.sort();
    assert_eq!(names, expected, "the model-visible tool surface is frozen");

    for schema in tool_schemas() {
        let name = schema["name"].as_str().expect("schema name").to_owned();
        let properties = schema["parameters"]["properties"]
            .as_object()
            .expect("tool parameter properties");
        for property in properties.keys() {
            assert!(
                !credential_shaped(property),
                "{name} publishes credential-shaped parameter {property}"
            );
        }
    }
}

#[test]
fn runner_sources_never_resolve_connector_credentials() {
    let sources = [
        ("lib.rs", include_str!("../src/lib.rs")),
        ("tools.rs", include_str!("../src/tools.rs")),
        ("harness.rs", include_str!("../src/harness.rs")),
        ("model.rs", include_str!("../src/model.rs")),
        ("step_context.rs", include_str!("../src/step_context.rs")),
        ("state_driver.rs", include_str!("../src/state_driver.rs")),
    ];
    for marker in [
        "SecretRef",
        "CredentialLease",
        "CredentialGeneration",
        "CredentialStore",
        "secret_ref",
        "credential_ref",
        "credential_lease",
        "connector_credential",
        "resolve_secret",
    ] {
        for (path, source) in sources {
            assert!(
                !source.contains(marker),
                "kiana-runner/src/{path} must not carry credential material marker {marker}"
            );
        }
    }
    // The only outward path for a tool call is still a capability request that ControlPlane and
    // the Broker re-authorize; the Runner does not execute connectors itself.
    assert!(sources[2].1.contains("CapabilityRequested"));
}
