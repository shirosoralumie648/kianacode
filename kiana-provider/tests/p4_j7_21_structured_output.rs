//! P4-J7-21 provider-level fixtures.
//!
//! These fixtures drive the real `ProviderGateway::prepare_call` path (connection resolution →
//! request compiler) while never opening a socket: an undeclared structured-output capability or
//! a protocol that cannot express the requested form must fail before any request is sent, and
//! each supported response form must reach the protocol wire body next to — never merged into —
//! the tool catalog. Reply-side handling of refusal/length/empty/invalid JSON stays in the
//! codec fixtures (`kiana-provider/src/response.rs`, run by the same lib target).

use kiana_domain::{
    tool_schemas, ModelAssignment, ModelCallSpec, ModelMessage, ModelPurpose,
    ModelRecoveryDisposition, ModelRequest, ModelResponseFormat, ModelRetryClass,
    ModelSideEffectState, RequestId, RoleSpec, RunId, SchemaVersion, TurnId,
};
use kiana_ports::ModelClient;
use kiana_provider::{ProviderConfig, ProviderGateway};
use serde_json::{json, Value};
use std::ffi::OsString;

/// Provider configuration is resolved from the process environment, so ambient Kiana provider
/// variables are cleared first: the fixture must always see this checkout's builtin catalog and
/// the explicit `ProviderConfig` below.
struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new() -> Self {
        let values = [
            "KIANA_MODEL_PROFILES_JSON",
            "KIANA_PROVIDER",
            "KIANA_MODEL",
            "KIANA_BASE_URL",
            "KIANA_STREAMING",
            "KIANA_MODEL_MAX_CONCURRENCY",
            "KIANA_MODEL_QUEUE_LIMIT",
            "OPENAI_API_KEY",
            "OPENAI_MODEL",
            "OPENAI_BASE_URL",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_BASE_URL",
            "GEMINI_API_KEY",
            "GEMINI_MODEL",
            "GEMINI_BASE_URL",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect::<Vec<_>>();
        let restore = Self(values);
        for (key, _) in &restore.0 {
            std::env::remove_var(key);
        }
        restore
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// Build a gateway against a loopback endpoint. HTTP on loopback is the only non-TLS form the
/// resolver accepts, and no fixture here reaches transport, so no socket is ever opened.
fn gateway(provider: &str, model: &str) -> ProviderGateway {
    ProviderGateway::from_env(ProviderConfig {
        provider: Some(provider.to_owned()),
        model: Some(model.to_owned()),
        base_url: Some("http://127.0.0.1:9/v1".to_owned()),
        api_key: Some("P4J721_SECRET_SENTINEL".to_owned()),
    })
    .expect("provider gateway")
}

fn assignment() -> ModelAssignment {
    let role = RoleSpec::pm();
    let assignment = ModelAssignment {
        schema: "kiana.model-assignment.v1".to_owned(),
        run_id: RunId::new(),
        turn_id: TurnId::new(),
        role_id: role.role_id.clone(),
        role_version: Some(role.version),
        catalog_version: Some(SchemaVersion::new(1, 0)),
        prompt_hash: Some(role.prompt_hash.clone()),
        input_schema: Some(role.input_schema.clone()),
        output_schema: Some(role.output_schema.clone()),
        profile: role.model_profile.clone(),
        project_root: "/repo".to_owned(),
        project_trusted: true,
        authority_revision: None,
        max_wall_time_ms: 10_000,
        runtime_budget: None,
    };
    assignment.validate().expect("server model assignment");
    assignment
}

fn request(tools: Vec<Value>) -> ModelRequest {
    ModelRequest {
        messages: vec![ModelMessage::user("return the answer object")],
        tools,
        sandbox: "read-only".to_owned(),
    }
}

fn spec(format: ModelResponseFormat) -> ModelCallSpec {
    ModelCallSpec {
        call_id: RequestId::new(),
        attempt_id: RequestId::new(),
        model_attempt_id: None,
        step_id: None,
        step: 1,
        purpose: ModelPurpose::Task,
        assignment: Some(assignment()),
        response_format: format,
        replay: Vec::new(),
        deadline_unix_ms: u64::MAX,
    }
}

fn answer_schema() -> Value {
    json!({
        "type":"object",
        "properties":{"answer":{"type":"string"}},
        "required":["answer"],
        "additionalProperties":false
    })
}

fn schema_format() -> ModelResponseFormat {
    ModelResponseFormat::JsonSchema {
        name: "answer".to_owned(),
        schema: answer_schema(),
    }
}

/// The builtin catalog never guesses structured-output support for an undeclared model, so the
/// requested form is rejected while compiling. The same connection still prepares a `text`
/// request, which proves the rejection is about the requested form and not about the model.
#[tokio::test]
async fn unsupported_response_schema_fails_before_send() {
    let _env = EnvRestore::new();
    let openai_gateway = gateway("openai", "p4-j7-21-undeclared-model");

    for format in [ModelResponseFormat::JsonObject, schema_format()] {
        let error = openai_gateway
            .prepare_call(request(Vec::new()), spec(format.clone()))
            .expect_err("undeclared structured output must fail before send");
        assert_eq!(
            error.code, "model_structured_output_unsupported",
            "{format:?}"
        );
        assert!(!error.request_sent, "{format:?}");
        assert_eq!(error.retry_class, ModelRetryClass::Never, "{format:?}");
        assert_eq!(
            error.recovery_disposition,
            ModelRecoveryDisposition::Terminal,
            "{format:?}"
        );
        assert_eq!(
            error.side_effect_state,
            ModelSideEffectState::None,
            "{format:?}"
        );
    }

    // Anthropic Messages cannot express a generic JSON object: the request is refused instead of
    // being silently downgraded to text.
    let anthropic = gateway("anthropic", "claude-sonnet-4-6");
    let error = anthropic
        .prepare_call(request(Vec::new()), spec(ModelResponseFormat::JsonObject))
        .expect_err("anthropic json_object requires a named schema");
    assert_eq!(error.code, "anthropic_json_object_requires_schema");
    assert!(!error.request_sent);
    assert_eq!(error.side_effect_state, ModelSideEffectState::None);

    let prepared = openai_gateway
        .prepare_call(request(Vec::new()), spec(ModelResponseFormat::Text))
        .expect("text responses do not require structured-output capability");
    assert!(prepared.wire_body.get("response_format").is_none());
    assert!(prepared.wire_body.get("text").is_none());
    assert!(prepared.wire_body.get("output_config").is_none());
    assert!(prepared.wire_body.get("format").is_none());
}

/// The structured response format and the tool catalog are separate contracts on the same wire
/// body: a structured request keeps the full tool catalog, a `text` request keeps it too and
/// leaves no structured key behind, and the requested schema is unchanged byte-for-byte.
#[tokio::test]
async fn structured_output_and_tools_have_distinct_contracts() {
    let _env = EnvRestore::new();
    let schema = answer_schema();

    for (provider, model) in [
        ("openai", "gpt-4o-mini"),
        ("openai-responses", "gpt-4.1"),
        ("anthropic", "claude-sonnet-4-6"),
        ("gemini", "gemini-2.5-flash"),
    ] {
        let gateway = gateway(provider, model);
        let prepared = gateway
            .prepare_call(request(tool_schemas()), spec(schema_format()))
            .unwrap_or_else(|error| panic!("{provider} structured prepare failed: {error:?}"));
        let body = &prepared.wire_body;
        assert!(
            body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()),
            "{provider} structured request keeps the tool catalog"
        );
        match provider {
            "openai" => {
                assert_eq!(body["response_format"]["type"], "json_schema");
                assert_eq!(body["response_format"]["json_schema"]["name"], "answer");
                assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
                assert_eq!(body["response_format"]["json_schema"]["strict"], true);
                assert!(body.get("output_config").is_none());
            }
            "openai-responses" => {
                assert_eq!(body["text"]["format"]["type"], "json_schema");
                assert_eq!(body["text"]["format"]["name"], "answer");
                assert_eq!(body["text"]["format"]["schema"], schema);
                assert_eq!(body["text"]["format"]["strict"], true);
            }
            "anthropic" => {
                assert_eq!(body["output_config"]["format"]["type"], "json_schema");
                assert_eq!(body["output_config"]["format"]["schema"], schema);
                assert!(body["tools"][0].get("input_schema").is_some());
            }
            "gemini" => {
                assert_eq!(body["response_format"]["mime_type"], "application/json");
                assert_eq!(body["response_format"]["schema"], schema);
                assert_eq!(body["tools"][0]["type"], "function");
                assert!(body["tools"][0].get("parameters").is_some());
                assert_eq!(body["store"], false);
            }
            other => panic!("unexpected fixture provider {other}"),
        }

        let text = gateway
            .prepare_call(request(tool_schemas()), spec(ModelResponseFormat::Text))
            .expect("text response prepares on the same connection");
        assert!(
            text.wire_body
                .get("response_format")
                .is_none_or(Value::is_null),
            "{provider} text request carries no response_format"
        );
        assert!(text.wire_body.get("output_config").is_none(), "{provider}");
        assert!(text.wire_body.get("format").is_none(), "{provider}");
        if provider == "openai-responses" {
            assert!(text.wire_body.get("text").is_none(), "{provider}");
        }
        assert!(
            text.wire_body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()),
            "{provider} text request keeps the tool catalog"
        );
    }
}
