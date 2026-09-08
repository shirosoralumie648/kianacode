//! MCP client capability for the owned harness.
//!
//! Discovery and execution stay on the daemon broker. The model only sees
//! the `mcp` tool. HTTP/SSE/WS wait; this slice is stdio.

use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult};
use kiana_ports::PortError;
use kiana_services::mcp::{McpClient, McpServerConfig, McpTool, TransportType};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

const MCP_OPERATION: &str = "mcp.call";
const MCP_RESULT_SCHEMA: &str = "kiana.mcp-result.v1";
const MCP_MAX_CONFIG_BYTES: usize = 64 * 1024;
const MCP_MAX_TOOL_NAME_BYTES: usize = 256;
const MCP_MAX_ARGUMENT_BYTES: usize = 64 * 1024;
const MCP_MAX_RESULT_BYTES: usize = 256 * 1024;
const MCP_MAX_SCHEMA_DEPTH: usize = 32;
pub(crate) const MCP_SERVERS_ENV: &str = "KIANA_MCP_SERVERS_JSON";

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Network,
        MCP_OPERATION,
        std::sync::Arc::new(McpCallHandler),
    )
}

struct McpCallHandler;

#[async_trait::async_trait]
impl CapabilityHandler for McpCallHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != MCP_OPERATION {
            return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
        }
        let request_id = request.request.request_id;
        let arguments = &request.request.arguments;
        let tool = required_tool(arguments)?;
        let servers = load_servers()?;
        let config = select_server(&servers, arguments.get("server"))?;
        if config.transport != TransportType::Stdio {
            return Err(PortError::Failed("mcp_transport_unsupported".to_owned()));
        }
        let tool_arguments = tool_arguments(arguments.get("arguments"))?;
        let mut client = McpClient::new(config.clone());
        client
            .connect()
            .await
            .map_err(|error| PortError::Failed(format!("mcp_connect_failed:{error}")))?;
        let advertised_tools = client
            .list_tools()
            .await
            .map_err(|error| PortError::Failed(format!("mcp_tool_discovery_failed:{error}")))?;
        let advertised_tool = select_advertised_tool(&advertised_tools, &tool)?;
        validate_tool_arguments(&advertised_tool.input_schema, &tool_arguments)?;
        let result = client
            .call_tool(&tool, tool_arguments)
            .await
            .map_err(|error| PortError::Failed(format!("mcp_call_failed:{error}")))?;
        let result_bytes = serde_json::to_vec(&result)
            .map_err(|error| PortError::Failed(format!("mcp_result_invalid:{error}")))?;
        if result_bytes.len() > MCP_MAX_RESULT_BYTES {
            return Err(PortError::Failed("mcp_result_too_large".to_owned()));
        }
        let provider_failed = validate_mcp_result(&result)?;
        Ok(CapabilityResult {
            request_id,
            success: !provider_failed,
            output: json!({
                "schema": MCP_RESULT_SCHEMA,
                "server": config.name,
                "tool": tool,
                "transport": "stdio",
                "result": result,
            }),
            evidence_refs: Vec::new(),
        })
    }
}

fn select_advertised_tool<'a>(
    tools: &'a [McpTool],
    requested: &str,
) -> Result<&'a McpTool, PortError> {
    let mut matches = tools.iter().filter(|tool| tool.name == requested);
    let tool = matches
        .next()
        .ok_or_else(|| PortError::Failed("mcp_tool_unknown".to_owned()))?;
    if matches.next().is_some() {
        return Err(PortError::Failed("mcp_tool_ambiguous".to_owned()));
    }
    validate_tool_schema(&tool.input_schema)?;
    Ok(tool)
}

fn validate_tool_schema(schema: &Value) -> Result<(), PortError> {
    validate_schema_node(schema, true, 0)
}

/// Validate the small, interoperable JSON-Schema subset that the daemon can
/// enforce locally. Unsupported combinators and malformed constraints are
/// rejected instead of being silently ignored by the authorization boundary.
fn validate_schema_node(schema: &Value, root: bool, depth: usize) -> Result<(), PortError> {
    if depth > MCP_MAX_SCHEMA_DEPTH {
        return Err(PortError::Failed("mcp_tool_schema_too_deep".to_owned()));
    }
    let schema = schema
        .as_object()
        .ok_or_else(|| PortError::Failed("mcp_tool_schema_invalid".to_owned()))?;

    const ALLOWED_KEYS: &[&str] = &[
        "type",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "const",
        "description",
        "title",
        "default",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minItems",
        "maxItems",
    ];
    if schema
        .keys()
        .any(|key| !ALLOWED_KEYS.contains(&key.as_str()))
    {
        return Err(PortError::Failed("mcp_tool_schema_unsupported".to_owned()));
    }

    let schema_type = schema.get("type").map(|value| {
        value
            .as_str()
            .ok_or_else(|| PortError::Failed("mcp_tool_schema_invalid".to_owned()))
    });
    let schema_type = match schema_type {
        Some(Ok(value)) => {
            if !matches!(
                value,
                "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
            ) {
                return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
            }
            Some(value)
        }
        Some(Err(error)) => return Err(error),
        None => None,
    };
    if root && schema_type.is_some_and(|kind| kind != "object") {
        return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
    }

    let properties = match schema.get("properties") {
        None => None,
        Some(Value::Object(properties)) => {
            for (name, property) in properties {
                if name.is_empty() || name.len() > MCP_MAX_TOOL_NAME_BYTES {
                    return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
                }
                validate_schema_node(property, false, depth + 1)?;
            }
            Some(properties)
        }
        Some(_) => return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned())),
    };
    if properties.is_some() && schema_type.is_some_and(|kind| kind != "object") {
        return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
    }

    if let Some(required) = schema.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| PortError::Failed("mcp_tool_schema_invalid".to_owned()))?;
        let Some(properties) = properties else {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        };
        if schema_type.is_some_and(|kind| kind != "object") {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
        let mut seen = HashSet::new();
        for field in required {
            let field = field
                .as_str()
                .filter(|field| !field.is_empty())
                .ok_or_else(|| PortError::Failed("mcp_tool_schema_invalid".to_owned()))?;
            if !seen.insert(field) || !properties.contains_key(field) {
                return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
            }
        }
    }

    if let Some(additional) = schema.get("additionalProperties") {
        if schema_type.is_some_and(|kind| kind != "object") {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
        match additional {
            Value::Bool(_) => {}
            Value::Object(_) => validate_schema_node(additional, false, depth + 1)?,
            _ => return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned())),
        }
    }
    if let Some(items) = schema.get("items") {
        if schema_type != Some("array") {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
        validate_schema_node(items, false, depth + 1)?;
    }
    for key in ["minItems", "maxItems"] {
        if schema
            .get(key)
            .is_some_and(|value| value.as_u64().is_none())
        {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if schema_type.is_some_and(|kind| kind != "array")
        && ["minItems", "maxItems"]
            .iter()
            .any(|key| schema.contains_key(*key))
    {
        return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
    }
    for key in ["minLength", "maxLength"] {
        if schema
            .get(key)
            .is_some_and(|value| value.as_u64().is_none())
        {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if schema_type.is_some_and(|kind| kind != "string")
        && ["minLength", "maxLength"]
            .iter()
            .any(|key| schema.contains_key(*key))
    {
        return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
    }
    if let (Some(minimum), Some(maximum)) = (
        schema.get("minItems").and_then(Value::as_u64),
        schema.get("maxItems").and_then(Value::as_u64),
    ) {
        if minimum > maximum {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if let (Some(minimum), Some(maximum)) = (
        schema.get("minLength").and_then(Value::as_u64),
        schema.get("maxLength").and_then(Value::as_u64),
    ) {
        if minimum > maximum {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if schema_type.is_some_and(|kind| !matches!(kind, "number" | "integer"))
        && ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"]
            .iter()
            .any(|key| schema.contains_key(*key))
    {
        return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
    }
    for key in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
        if schema
            .get(key)
            .is_some_and(|value| value.as_f64().is_none())
        {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if let (Some(minimum), Some(maximum)) = (
        schema.get("minimum").and_then(Value::as_f64),
        schema.get("maximum").and_then(Value::as_f64),
    ) {
        if minimum > maximum {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if let (Some(minimum), Some(maximum)) = (
        schema.get("exclusiveMinimum").and_then(Value::as_f64),
        schema.get("exclusiveMaximum").and_then(Value::as_f64),
    ) {
        if minimum >= maximum {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    for key in ["description", "title"] {
        if schema.get(key).is_some_and(|value| !value.is_string()) {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    if let Some(enum_values) = schema.get("enum") {
        if !enum_values.is_array() {
            return Err(PortError::Failed("mcp_tool_schema_invalid".to_owned()));
        }
    }
    Ok(())
}

fn validate_tool_arguments(
    schema: &Value,
    arguments: &HashMap<String, Value>,
) -> Result<(), PortError> {
    validate_schema_node(schema, true, 0)?;
    let schema = schema
        .as_object()
        .ok_or_else(|| PortError::Failed("mcp_tool_schema_invalid".to_owned()))?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            if !arguments.contains_key(field) {
                return Err(PortError::Failed(format!("mcp_argument_required:{field}")));
            }
        }
    }
    let additional_allowed = schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    for (name, value) in arguments {
        match properties.get(name) {
            Some(property) => validate_argument_value(property, value, 0)?,
            None if !additional_allowed => {
                return Err(PortError::Failed("mcp_argument_unknown".to_owned()))
            }
            None => {
                if let Some(additional_schema) = schema
                    .get("additionalProperties")
                    .filter(|value| value.is_object())
                {
                    validate_argument_value(additional_schema, value, 0)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_argument_value(schema: &Value, value: &Value, depth: usize) -> Result<(), PortError> {
    if depth > MCP_MAX_SCHEMA_DEPTH {
        return Err(PortError::Failed("mcp_argument_too_deep".to_owned()));
    }
    let schema = schema
        .as_object()
        .ok_or_else(|| PortError::Failed("mcp_tool_schema_invalid".to_owned()))?;
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => false,
        };
        if !matches {
            return Err(PortError::Failed("mcp_argument_type_invalid".to_owned()));
        }
    }
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|candidate| candidate == value) {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(constant) = schema.get("const") {
        if constant != value {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Err(PortError::Failed(format!("mcp_argument_required:{field}")));
                }
            }
        }
        let additional = schema.get("additionalProperties");
        for (name, nested) in object {
            match properties.and_then(|properties| properties.get(name)) {
                Some(property) => validate_argument_value(property, nested, depth + 1)?,
                None => match additional {
                    Some(Value::Bool(false)) => {
                        return Err(PortError::Failed("mcp_argument_unknown".to_owned()))
                    }
                    Some(Value::Object(property)) => validate_argument_value(
                        &Value::Object(property.clone()),
                        nested,
                        depth + 1,
                    )?,
                    _ => {}
                },
            }
        }
    }
    if value.is_array() {
        if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64) {
            if value
                .as_array()
                .is_some_and(|items| items.len() < minimum as usize)
            {
                return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
            }
        }
        if let Some(maximum) = schema.get("maxItems").and_then(Value::as_u64) {
            if value
                .as_array()
                .is_some_and(|items| items.len() > maximum as usize)
            {
                return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
            }
        }
        if let Some(items_schema) = schema.get("items") {
            for item in value.as_array().expect("checked array") {
                validate_argument_value(items_schema, item, depth + 1)?;
            }
        }
    }
    if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
        if value
            .as_str()
            .is_some_and(|text| text.chars().count() < minimum as usize)
        {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64) {
        if value
            .as_str()
            .is_some_and(|text| text.chars().count() > maximum as usize)
        {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        if value.as_f64().is_some_and(|number| number < minimum) {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
        if value.as_f64().is_some_and(|number| number > maximum) {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(minimum) = schema.get("exclusiveMinimum").and_then(Value::as_f64) {
        if value.as_f64().is_some_and(|number| number <= minimum) {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    if let Some(maximum) = schema.get("exclusiveMaximum").and_then(Value::as_f64) {
        if value.as_f64().is_some_and(|number| number >= maximum) {
            return Err(PortError::Failed("mcp_argument_value_invalid".to_owned()));
        }
    }
    Ok(())
}

fn validate_mcp_result(result: &Value) -> Result<bool, PortError> {
    let result = result
        .as_object()
        .ok_or_else(|| PortError::Failed("mcp_result_invalid".to_owned()))?;
    let has_content = match result.get("content") {
        Some(Value::Array(content)) => {
            if content.iter().any(|item| {
                item.as_object()
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            }) {
                return Err(PortError::Failed("mcp_result_invalid".to_owned()));
            }
            true
        }
        Some(_) => return Err(PortError::Failed("mcp_result_invalid".to_owned())),
        None => false,
    };
    let has_structured_content = match result.get("structuredContent") {
        Some(Value::Object(_)) => true,
        Some(_) => return Err(PortError::Failed("mcp_result_invalid".to_owned())),
        None => false,
    };
    if !has_content && !has_structured_content {
        return Err(PortError::Failed("mcp_result_invalid".to_owned()));
    }
    match result.get("isError") {
        Some(Value::Bool(is_error)) => Ok(*is_error),
        Some(_) => Err(PortError::Failed("mcp_result_invalid".to_owned())),
        None => Ok(false),
    }
}

fn required_tool(arguments: &Value) -> Result<String, PortError> {
    arguments
        .get("tool")
        .or_else(|| arguments.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| PortError::Failed("mcp_tool_required".to_owned()))
        .and_then(|tool| {
            if tool.len() > MCP_MAX_TOOL_NAME_BYTES {
                Err(PortError::Failed("mcp_tool_too_large".to_owned()))
            } else {
                Ok(tool)
            }
        })
}

fn tool_arguments(value: Option<&Value>) -> Result<HashMap<String, Value>, PortError> {
    match value {
        None | Some(Value::Null) => Ok(HashMap::new()),
        Some(Value::Object(map)) => {
            let arguments = map_to_hash(map);
            let bytes = serde_json::to_vec(&arguments)
                .map_err(|error| PortError::Failed(format!("mcp_arguments_invalid:{error}")))?;
            if bytes.len() > MCP_MAX_ARGUMENT_BYTES {
                return Err(PortError::Failed("mcp_arguments_too_large".to_owned()));
            }
            Ok(arguments)
        }
        Some(_) => Err(PortError::Failed(
            "mcp_arguments_object_required".to_owned(),
        )),
    }
}

fn map_to_hash(map: &Map<String, Value>) -> HashMap<String, Value> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn load_servers() -> Result<Vec<McpServerConfig>, PortError> {
    let raw = std::env::var(MCP_SERVERS_ENV)
        .map_err(|_| PortError::Failed("mcp_servers_required".to_owned()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PortError::Failed("mcp_servers_required".to_owned()));
    }
    if trimmed.len() > MCP_MAX_CONFIG_BYTES {
        return Err(PortError::Failed("mcp_config_too_large".to_owned()));
    }
    serde_json::from_str::<Vec<McpServerConfig>>(trimmed)
        .or_else(|_| serde_json::from_str::<McpServerConfig>(trimmed).map(|config| vec![config]))
        .map_err(|error| PortError::Failed(format!("mcp_config_invalid:{error}")))
}

fn select_server(
    servers: &[McpServerConfig],
    requested: Option<&Value>,
) -> Result<McpServerConfig, PortError> {
    let name = requested
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match name {
        Some(name) => servers
            .iter()
            .find(|server| server.name == name)
            .cloned()
            .ok_or_else(|| PortError::Failed("mcp_server_unknown".to_owned())),
        None if servers.len() == 1 => Ok(servers[0].clone()),
        None if servers.is_empty() => Err(PortError::Failed("mcp_servers_required".to_owned())),
        None => Err(PortError::Failed("mcp_server_required".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertised_tool(name: &str, input_schema: Value) -> McpTool {
        McpTool {
            name: name.to_owned(),
            description: String::new(),
            input_schema,
        }
    }

    #[test]
    fn oversized_mcp_inputs_fail_closed() {
        let long_tool = json!({ "tool": "x".repeat(MCP_MAX_TOOL_NAME_BYTES + 1) });
        assert!(matches!(
            required_tool(&long_tool),
            Err(PortError::Failed(message)) if message == "mcp_tool_too_large"
        ));
        let long_arguments =
            json!({ "arguments": { "payload": "x".repeat(MCP_MAX_ARGUMENT_BYTES) } });
        assert!(matches!(
            tool_arguments(long_arguments.get("arguments")),
            Err(PortError::Failed(message)) if message == "mcp_arguments_too_large"
        ));
        let _env = std::env::var(MCP_SERVERS_ENV).ok();
    }

    #[test]
    fn missing_tool_fails_closed() {
        let error = required_tool(&json!({})).unwrap_err();
        assert!(matches!(error, PortError::Failed(message) if message == "mcp_tool_required"));
    }

    #[test]
    fn single_server_is_selected_when_unnamed() {
        let servers = vec![McpServerConfig {
            name: "mock".to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some("python3".to_owned()),
            args: None,
            env: None,
            headers: None,
        }];
        let selected = select_server(&servers, None).unwrap();
        assert_eq!(selected.name, "mock");
    }

    #[test]
    fn unknown_server_fails_closed() {
        let servers = vec![McpServerConfig {
            name: "mock".to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some("python3".to_owned()),
            args: None,
            env: None,
            headers: None,
        }];
        let error = select_server(&servers, Some(&json!("other"))).unwrap_err();
        assert!(matches!(error, PortError::Failed(message) if message == "mcp_server_unknown"));
    }

    #[test]
    fn unknown_ambiguous_and_malformed_tools_fail_closed() {
        let echo = advertised_tool("echo", json!({ "type": "object" }));
        assert!(matches!(
            select_advertised_tool(std::slice::from_ref(&echo), "missing"),
            Err(PortError::Failed(message)) if message == "mcp_tool_unknown"
        ));
        assert!(matches!(
            select_advertised_tool(&[echo.clone(), echo], "echo"),
            Err(PortError::Failed(message)) if message == "mcp_tool_ambiguous"
        ));
        let malformed = advertised_tool("echo", json!({ "type": "array" }));
        assert!(matches!(
            select_advertised_tool(&[malformed], "echo"),
            Err(PortError::Failed(message)) if message == "mcp_tool_schema_invalid"
        ));
    }

    #[test]
    fn advertised_schema_fences_required_types_and_unknown_arguments() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "minLength": 1 },
                "count": { "type": "integer" }
            },
            "required": ["path"],
            "additionalProperties": false
        });
        let valid = HashMap::from([
            ("path".to_owned(), json!("src/lib.rs")),
            ("count".to_owned(), json!(2)),
        ]);
        assert!(validate_tool_arguments(&schema, &valid).is_ok());

        let missing = HashMap::new();
        assert!(matches!(
            validate_tool_arguments(&schema, &missing),
            Err(PortError::Failed(message)) if message == "mcp_argument_required:path"
        ));

        let wrong_type = HashMap::from([("path".to_owned(), json!(42))]);
        assert!(matches!(
            validate_tool_arguments(&schema, &wrong_type),
            Err(PortError::Failed(message)) if message == "mcp_argument_type_invalid"
        ));

        let unknown = HashMap::from([
            ("path".to_owned(), json!("src/lib.rs")),
            ("extra".to_owned(), json!(true)),
        ]);
        assert!(matches!(
            validate_tool_arguments(&schema, &unknown),
            Err(PortError::Failed(message)) if message == "mcp_argument_unknown"
        ));
    }

    #[test]
    fn unsupported_schema_keywords_fail_closed_before_arguments() {
        let schema = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "oneOf": [{ "required": ["path"] }]
        });
        assert!(matches!(
            validate_tool_schema(&schema),
            Err(PortError::Failed(message)) if message == "mcp_tool_schema_unsupported"
        ));
    }

    #[test]
    fn schema_constraints_are_enforced_at_the_boundary() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": {
                    "type": "integer",
                    "exclusiveMinimum": 0,
                    "exclusiveMaximum": 4
                },
                "options": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "mode": { "type": "string" } }
                }
            },
            "additionalProperties": false
        });
        for count in [json!(0), json!(4)] {
            let arguments = HashMap::from([(String::from("count"), count)]);
            assert!(matches!(
                validate_tool_arguments(&schema, &arguments),
                Err(PortError::Failed(message)) if message == "mcp_argument_value_invalid"
            ));
        }
        let nested_unknown =
            HashMap::from([(String::from("options"), json!({ "unexpected": true }))]);
        assert!(matches!(
            validate_tool_arguments(&schema, &nested_unknown),
            Err(PortError::Failed(message)) if message == "mcp_argument_unknown"
        ));
        let valid = HashMap::from([(String::from("options"), json!({ "mode": "safe" }))]);
        assert!(validate_tool_arguments(&schema, &valid).is_ok());
    }

    #[test]
    fn malformed_constraint_placement_fails_closed() {
        let schema = json!({
            "type": "string",
            "minimum": 1
        });
        assert!(matches!(
            validate_tool_schema(&schema),
            Err(PortError::Failed(message)) if message == "mcp_tool_schema_invalid"
        ));
        let reversed = json!({
            "type": "object",
            "properties": { "value": { "type": "string", "minLength": 3, "maxLength": 1 } }
        });
        assert!(matches!(
            validate_tool_schema(&reversed),
            Err(PortError::Failed(message)) if message == "mcp_tool_schema_invalid"
        ));
    }

    #[test]
    fn malformed_provider_results_fail_closed_and_is_error_is_preserved() {
        for malformed in [
            Value::Null,
            json!({}),
            json!({ "content": "not-an-array" }),
            json!({ "content": [{}] }),
            json!({ "content": [], "isError": "yes" }),
        ] {
            assert!(matches!(
                validate_mcp_result(&malformed),
                Err(PortError::Failed(message)) if message == "mcp_result_invalid"
            ));
        }
        assert!(!validate_mcp_result(&json!({
            "content": [{ "type": "text", "text": "ok" }]
        }))
        .unwrap());
        assert!(validate_mcp_result(&json!({
            "content": [{ "type": "text", "text": "denied" }],
            "isError": true
        }))
        .unwrap());
    }
}
