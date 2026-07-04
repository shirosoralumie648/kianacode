use crate::tool::*;
use async_trait::async_trait;
use kiana_services::mcp::{McpClient, McpServerConfig, TransportType};
use kiana_types::{project_trust_from_app_state, ProjectTrust};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MCP_INVOCATIONS_KEY: &str = "mcp_invocations";
pub const MCP_SERVERS_APP_STATE_KEY: &str = "mcp_servers";
pub const MCP_SERVERS_ENV: &str = "KIANA_MCP_SERVERS_JSON";

#[derive(Debug, Deserialize)]
struct McpInput {
    tool_name: String,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    config: Option<McpServerConfigInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpServerConfigInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, alias = "type")]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ListMcpResourcesInput {
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    config: Option<McpServerConfigInput>,
}

#[derive(Debug, Deserialize)]
struct ListMcpResourceTemplatesInput {
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    config: Option<McpServerConfigInput>,
}

#[derive(Debug, Deserialize)]
struct ListMcpPromptsInput {
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    config: Option<McpServerConfigInput>,
}

#[derive(Debug, Deserialize)]
struct ReadMcpResourceInput {
    server: String,
    uri: String,
    #[serde(default)]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    config: Option<McpServerConfigInput>,
}

#[derive(Debug, Deserialize)]
struct GetMcpPromptInput {
    server: String,
    name: String,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    transport: Option<TransportType>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "commandArgs")]
    command_args: Option<Vec<String>>,
    #[serde(default)]
    config: Option<McpServerConfigInput>,
}

pub struct McpTool;
pub struct ListMcpResourcesTool;
pub struct ListMcpResourceTemplatesTool;
pub struct ListMcpPromptsTool;
pub struct ReadMcpResourceTool;
pub struct GetMcpPromptTool;

pub fn configured_mcp_servers() -> Option<Value> {
    configured_mcp_servers_with_trust(ProjectTrust::Trusted)
}

pub fn configured_mcp_servers_with_trust(project_trust: ProjectTrust) -> Option<Value> {
    let mut merged = serde_json::Map::new();
    merge_mcp_servers_value(&mut merged, &Value::Object(load_plugin_mcp_servers()));
    if let Some(value) = read_user_mcp_config() {
        merge_mcp_servers_value(&mut merged, &value);
    }
    for value in read_project_mcp_configs_with_trust(project_trust) {
        merge_mcp_servers_value(&mut merged, &value);
    }
    if let Some(value) = read_local_mcp_config_with_trust(project_trust) {
        merge_mcp_servers_value(&mut merged, &value);
    }
    if let Ok(raw) = env::var(MCP_SERVERS_ENV) {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            merge_mcp_servers_value(&mut merged, &expand_env_vars_in_value(&value));
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(Value::Object(merged))
    }
}

fn read_user_mcp_config() -> Option<Value> {
    read_json_file(&kiana_home_dir().join("mcp.json")).map(|value| expand_env_vars_in_value(&value))
}

fn read_project_mcp_configs_with_trust(project_trust: ProjectTrust) -> Vec<Value> {
    if !project_trust.allows_project_resources() {
        return Vec::new();
    }

    let Some(cwd) = env::current_dir().ok() else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut current = Some(cwd.as_path());
    while let Some(dir) = current {
        dirs.push(dir.to_path_buf());
        current = dir.parent();
    }
    dirs.reverse();
    dirs.into_iter()
        .filter_map(|dir| read_json_file(&dir.join(".mcp.json")))
        .map(|value| expand_env_vars_in_value(&value))
        .collect()
}

fn read_local_mcp_config_with_trust(project_trust: ProjectTrust) -> Option<Value> {
    if !project_trust.allows_project_resources() {
        return None;
    }
    let cwd = env::current_dir().ok()?;
    read_json_file(&cwd.join(".kiana").join("mcp.local.json"))
        .map(|value| expand_env_vars_in_value(&value))
}

fn kiana_home_dir() -> PathBuf {
    if let Ok(path) = env::var("KIANA_HOME") {
        return PathBuf::from(path);
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".kiana");
    }
    PathBuf::from(".kiana")
}

impl McpTool {
    pub fn new() -> Self {
        Self
    }
}

impl ListMcpResourcesTool {
    pub fn new() -> Self {
        Self
    }
}

impl ListMcpResourceTemplatesTool {
    pub fn new() -> Self {
        Self
    }
}

impl ListMcpPromptsTool {
    pub fn new() -> Self {
        Self
    }
}

impl ReadMcpResourceTool {
    pub fn new() -> Self {
        Self
    }
}

impl GetMcpPromptTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        "MCP"
    }

    fn description(&self) -> &str {
        "Call a configured MCP tool, or record the invocation when no MCP server config is provided"
    }

    fn workbench(&self) -> Option<&str> {
        Some("mcp")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_name": { "type": "string" },
                "server": { "type": "string" },
                "args": { "type": "object" },
                "transport": { "type": "string", "enum": ["stdio", "http", "sse", "ws"] },
                "url": { "type": "string" },
                "command": { "type": "string" },
                "command_args": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "config": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "transport": { "type": "string", "enum": ["stdio", "http", "sse", "ws"] },
                        "url": { "type": "string" },
                        "command": { "type": "string" },
                        "command_args": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "args": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }
            },
            "required": ["tool_name"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "invocation": { "type": "object" },
                "result": { "type": ["object", "array", "string", "number", "boolean", "null"] }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: McpInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };
        if input.tool_name.trim().is_empty() {
            return ValidationResult::err("tool_name cannot be empty".to_string(), 2);
        }
        if input.args.as_ref().is_some_and(|args| !args.is_object()) {
            return ValidationResult::err("args must be an object".to_string(), 3);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: McpInput = serde_json::from_value(input.clone())?;
        let tool_name = input.tool_name.trim();
        if tool_name.is_empty() {
            return Err(ToolError::ValidationError(
                "tool_name cannot be empty".to_string(),
            ));
        }

        let args = input.args.clone().unwrap_or_else(|| json!({}));
        let mut invocation = json!({
            "id": Uuid::new_v4().to_string(),
            "server": input.server.clone(),
            "tool_name": tool_name,
            "args": args,
            "created_at": now_unix_seconds()
        });

        if let Some(config) = resolve_server_config(&input, &context.app_state)? {
            invocation["server"] = json!(config.name.clone());
            invocation["transport"] = json!(config.transport);
            let mut client = McpClient::new(config);
            client
                .connect()
                .await
                .map_err(|error| ToolError::Other(error.to_string()))?;
            let result = client
                .call_tool(tool_name, value_object_to_hashmap(args)?)
                .await
                .map_err(|error| ToolError::Other(error.to_string()))?;
            let metadata = if mcp_result_is_error(&result) {
                Some(HashMap::from([("is_error".to_string(), json!(true))]))
            } else {
                None
            };
            record_invocation(context, invocation.clone())?;
            return Ok(ToolOutput {
                data: json!({
                    "status": "executed",
                    "invocation": invocation,
                    "result": result
                }),
                metadata,
            });
        }

        record_invocation(context, invocation.clone())?;

        Ok(ToolOutput {
            data: json!({
                "status": "recorded",
                "invocation": invocation,
                "message": "MCP transport execution is handled by the MCP service layer; this tool records the requested invocation for the current session."
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let content = if let Some(result) = output.data.get("result") {
            mcp_result_to_model_text(result)
        } else if let Some(message) = output.data.get("message").and_then(Value::as_str) {
            message.to_string()
        } else {
            json_value_to_model_text(&output.data)
        };

        let mut api_result = json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        });
        if output.data.get("result").is_some_and(mcp_result_is_error) {
            api_result["is_error"] = json!(true);
        }
        api_result
    }
}

#[async_trait]
impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &str {
        "ListMcpResourcesTool"
    }

    fn description(&self) -> &str {
        "List resources from a configured MCP server"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("list resources from an MCP server")
    }

    fn workbench(&self) -> Option<&str> {
        Some("mcp")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        mcp_server_input_schema(false)
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "uri": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "mimeType": { "type": "string" },
                    "server": { "type": "string" }
                }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match serde_json::from_value::<ListMcpResourcesInput>(input.clone()) {
            Ok(_) => ValidationResult::ok(),
            Err(error) => ValidationResult::err(format!("Invalid input: {error}"), 1),
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ListMcpResourcesInput = serde_json::from_value(input.clone())?;
        let config = resolve_server_config_fields(
            input.server.as_deref(),
            &input.transport,
            input.url.as_deref(),
            input.command.as_deref(),
            input.command_args.as_ref(),
            input.config.as_ref(),
            &context.app_state,
        )?
        .ok_or_else(|| {
            ToolError::ValidationError(
                "ListMcpResourcesTool requires an MCP server config".to_string(),
            )
        })?;

        let server_name = config.name.clone();
        let mut client = McpClient::new(config);
        client
            .connect()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let resources = client
            .list_resources()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?
            .into_iter()
            .map(|resource| {
                json!({
                    "uri": resource.uri,
                    "name": resource.name,
                    "description": resource.description,
                    "mimeType": resource.mime_type,
                    "server": server_name
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput {
            data: json!(resources),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let content = match output.data.as_array() {
            Some(resources) if resources.is_empty() => {
                "No resources found. MCP servers may still provide tools even if they have no resources."
                    .to_string()
            }
            _ => json_value_to_model_text(&output.data),
        };

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}

#[async_trait]
impl Tool for ListMcpResourceTemplatesTool {
    fn name(&self) -> &str {
        "ListMcpResourceTemplatesTool"
    }

    fn description(&self) -> &str {
        "List resource templates from a configured MCP server"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("list parameterized resource templates from an MCP server")
    }

    fn workbench(&self) -> Option<&str> {
        Some("mcp")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        mcp_server_input_schema(false)
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "uriTemplate": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "mimeType": { "type": "string" },
                    "server": { "type": "string" }
                }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match serde_json::from_value::<ListMcpResourceTemplatesInput>(input.clone()) {
            Ok(_) => ValidationResult::ok(),
            Err(error) => ValidationResult::err(format!("Invalid input: {error}"), 1),
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ListMcpResourceTemplatesInput = serde_json::from_value(input.clone())?;
        let config = resolve_server_config_fields(
            input.server.as_deref(),
            &input.transport,
            input.url.as_deref(),
            input.command.as_deref(),
            input.command_args.as_ref(),
            input.config.as_ref(),
            &context.app_state,
        )?
        .ok_or_else(|| {
            ToolError::ValidationError(
                "ListMcpResourceTemplatesTool requires an MCP server config".to_string(),
            )
        })?;

        let server_name = config.name.clone();
        let mut client = McpClient::new(config);
        client
            .connect()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let templates = client
            .list_resource_templates()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?
            .into_iter()
            .map(|template| {
                json!({
                    "uriTemplate": template.uri_template,
                    "name": template.name,
                    "description": template.description,
                    "mimeType": template.mime_type,
                    "server": server_name
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput {
            data: json!(templates),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let content = match output.data.as_array() {
            Some(templates) if templates.is_empty() => {
                "No resource templates found. MCP servers may still provide fixed resources or tools."
                    .to_string()
            }
            _ => json_value_to_model_text(&output.data),
        };

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}

#[async_trait]
impl Tool for ListMcpPromptsTool {
    fn name(&self) -> &str {
        "ListMcpPromptsTool"
    }

    fn description(&self) -> &str {
        "List prompts from a configured MCP server"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("list reusable prompts from an MCP server")
    }

    fn workbench(&self) -> Option<&str> {
        Some("mcp")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        mcp_server_input_schema(false)
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "arguments": { "type": "array" },
                    "server": { "type": "string" }
                }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match serde_json::from_value::<ListMcpPromptsInput>(input.clone()) {
            Ok(_) => ValidationResult::ok(),
            Err(error) => ValidationResult::err(format!("Invalid input: {error}"), 1),
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ListMcpPromptsInput = serde_json::from_value(input.clone())?;
        let config = resolve_server_config_fields(
            input.server.as_deref(),
            &input.transport,
            input.url.as_deref(),
            input.command.as_deref(),
            input.command_args.as_ref(),
            input.config.as_ref(),
            &context.app_state,
        )?
        .ok_or_else(|| {
            ToolError::ValidationError(
                "ListMcpPromptsTool requires an MCP server config".to_string(),
            )
        })?;

        let server_name = config.name.clone();
        let mut client = McpClient::new(config);
        client
            .connect()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let prompts = client
            .list_prompts()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?
            .into_iter()
            .map(|prompt| {
                json!({
                    "name": prompt.name,
                    "description": prompt.description,
                    "arguments": prompt.arguments.into_iter().map(|argument| {
                        json!({
                            "name": argument.name,
                            "description": argument.description,
                            "required": argument.required
                        })
                    }).collect::<Vec<_>>(),
                    "server": server_name
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput {
            data: json!(prompts),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let content = match output.data.as_array() {
            Some(prompts) if prompts.is_empty() => {
                "No prompts found. MCP servers may still provide tools or resources.".to_string()
            }
            _ => json_value_to_model_text(&output.data),
        };

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}

#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &str {
        "ReadMcpResourceTool"
    }

    fn description(&self) -> &str {
        "Read a resource from a configured MCP server by URI"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("read an MCP resource by URI")
    }

    fn workbench(&self) -> Option<&str> {
        Some("mcp")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        let mut schema = mcp_server_input_schema(true);
        if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
            required.push(json!("uri"));
        }
        if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
            properties.insert("uri".to_string(), json!({ "type": "string" }));
        }
        schema
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "contents": { "type": "array" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: ReadMcpResourceInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.server.trim().is_empty() {
            return ValidationResult::err("server cannot be empty".to_string(), 2);
        }
        if input.uri.trim().is_empty() {
            return ValidationResult::err("uri cannot be empty".to_string(), 3);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ReadMcpResourceInput = serde_json::from_value(input.clone())?;
        let uri = input.uri.trim();
        if uri.is_empty() {
            return Err(ToolError::ValidationError(
                "uri cannot be empty".to_string(),
            ));
        }
        let config = resolve_server_config_fields(
            Some(input.server.as_str()),
            &input.transport,
            input.url.as_deref(),
            input.command.as_deref(),
            input.command_args.as_ref(),
            input.config.as_ref(),
            &context.app_state,
        )?
        .ok_or_else(|| {
            ToolError::ValidationError(
                "ReadMcpResourceTool requires an MCP server config".to_string(),
            )
        })?;

        let mut client = McpClient::new(config);
        client
            .connect()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let resource = client
            .read_resource(uri)
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;

        Ok(ToolOutput {
            data: resource,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": json_value_to_model_text(&output.data)
        })
    }
}

#[async_trait]
impl Tool for GetMcpPromptTool {
    fn name(&self) -> &str {
        "GetMcpPromptTool"
    }

    fn description(&self) -> &str {
        "Get a prompt from a configured MCP server by name"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("get an MCP prompt by name")
    }

    fn workbench(&self) -> Option<&str> {
        Some("mcp")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        let mut schema = mcp_server_input_schema(true);
        if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
            required.push(json!("name"));
        }
        if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
            properties.insert("name".to_string(), json!({ "type": "string" }));
            properties.insert(
                "args".to_string(),
                json!({
                    "type": "object",
                    "additionalProperties": true
                }),
            );
        }
        schema
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": { "type": "string" },
                "messages": { "type": "array" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: GetMcpPromptInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.server.trim().is_empty() {
            return ValidationResult::err("server cannot be empty".to_string(), 2);
        }
        if input.name.trim().is_empty() {
            return ValidationResult::err("name cannot be empty".to_string(), 3);
        }
        if input.args.as_ref().is_some_and(|args| !args.is_object()) {
            return ValidationResult::err("args must be an object".to_string(), 4);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: GetMcpPromptInput = serde_json::from_value(input.clone())?;
        let prompt_name = input.name.trim();
        if prompt_name.is_empty() {
            return Err(ToolError::ValidationError(
                "name cannot be empty".to_string(),
            ));
        }
        let args = input.args.unwrap_or_else(|| json!({}));
        let config = resolve_server_config_fields(
            Some(input.server.as_str()),
            &input.transport,
            input.url.as_deref(),
            input.command.as_deref(),
            input.command_args.as_ref(),
            input.config.as_ref(),
            &context.app_state,
        )?
        .ok_or_else(|| {
            ToolError::ValidationError("GetMcpPromptTool requires an MCP server config".to_string())
        })?;

        let mut client = McpClient::new(config);
        client
            .connect()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let prompt = client
            .get_prompt(prompt_name, value_object_to_hashmap(args)?)
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;

        Ok(ToolOutput {
            data: prompt,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": json_value_to_model_text(&output.data)
        })
    }
}

fn mcp_result_to_model_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }

    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text_blocks = content
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();
        if !text_blocks.is_empty() {
            return text_blocks.join("\n");
        }
    }

    json_value_to_model_text(value)
}

fn mcp_result_is_error(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) == Some(true)
        || value.get("is_error").and_then(Value::as_bool) == Some(true)
}

fn json_value_to_model_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn resolve_server_config(
    input: &McpInput,
    app_state: &HashMap<String, Value>,
) -> ToolResult<Option<McpServerConfig>> {
    resolve_server_config_fields(
        input.server.as_deref(),
        &input.transport,
        input.url.as_deref(),
        input.command.as_deref(),
        input.command_args.as_ref(),
        input.config.as_ref(),
        app_state,
    )
}

fn resolve_server_config_fields(
    server: Option<&str>,
    transport: &Option<TransportType>,
    url: Option<&str>,
    command: Option<&str>,
    command_args: Option<&Vec<String>>,
    config: Option<&McpServerConfigInput>,
    app_state: &HashMap<String, Value>,
) -> ToolResult<Option<McpServerConfig>> {
    let server = server.map(str::trim).filter(|server| !server.is_empty());
    if let Some(config) = config.cloned() {
        return Ok(Some(config.into_config(server.unwrap_or("inline"))?));
    }

    if transport.is_some() || url.is_some() || command.is_some() {
        return Ok(Some(
            McpServerConfigInput {
                name: server.map(str::to_string),
                transport: transport.clone(),
                url: url.map(str::to_string),
                command: command.map(str::to_string),
                command_args: command_args.cloned(),
                args: None,
                env: None,
                headers: None,
            }
            .into_config(server.unwrap_or("inline"))?,
        ));
    }

    let Some(server_name) = server else {
        return Ok(None);
    };

    find_app_state_server_config(app_state, server_name)
        .map(|config| config.into_config(server_name))
        .transpose()
}

fn mcp_server_input_schema(require_server: bool) -> Value {
    let mut required = Vec::new();
    if require_server {
        required.push(json!("server"));
    }
    json!({
        "type": "object",
        "properties": {
            "server": { "type": "string" },
            "transport": { "type": "string", "enum": ["stdio", "http", "sse", "ws"] },
            "url": { "type": "string" },
            "command": { "type": "string" },
            "command_args": {
                "type": "array",
                "items": { "type": "string" }
            },
            "config": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "transport": { "type": "string", "enum": ["stdio", "http", "sse", "ws"] },
                    "url": { "type": "string" },
                    "command": { "type": "string" },
                    "command_args": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "env": {
                        "type": "object",
                        "additionalProperties": { "type": "string" }
                    },
                    "headers": {
                        "type": "object",
                        "additionalProperties": { "type": "string" }
                    }
                }
            }
        },
        "required": required
    })
}

fn find_app_state_server_config(
    app_state: &HashMap<String, Value>,
    server_name: &str,
) -> Option<McpServerConfigInput> {
    let servers = merged_mcp_servers(app_state)?;
    if let Some(server) = servers.get(server_name) {
        let mut config: McpServerConfigInput = serde_json::from_value(server.clone()).ok()?;
        if config.name.is_none() {
            config.name = Some(server_name.to_string());
        }
        return Some(config);
    }

    servers.as_array()?.iter().find_map(|server| {
        let config: McpServerConfigInput = serde_json::from_value(server.clone()).ok()?;
        if config.name.as_deref() == Some(server_name) {
            Some(config)
        } else {
            None
        }
    })
}

fn merged_mcp_servers(app_state: &HashMap<String, Value>) -> Option<Value> {
    let mut merged = serde_json::Map::new();
    let project_trust = project_trust_from_app_state(app_state);
    if let Some(configured) = configured_mcp_servers_with_trust(project_trust) {
        merge_mcp_servers_value(&mut merged, &configured);
    }
    if let Some(servers) = app_state.get(MCP_SERVERS_APP_STATE_KEY) {
        merge_mcp_servers_value(&mut merged, servers);
    }
    if merged.is_empty() {
        None
    } else {
        Some(Value::Object(merged))
    }
}

fn load_plugin_mcp_servers() -> serde_json::Map<String, Value> {
    let mut all_servers = serde_json::Map::new();
    for plugin_root in installed_plugin_roots() {
        let Some(manifest_path) = find_plugin_manifest_path(&plugin_root) else {
            continue;
        };
        let manifest = read_json_file(&manifest_path);
        let mut plugin_servers = serde_json::Map::new();
        merge_plugin_mcp_file(&mut plugin_servers, &plugin_root, ".mcp.json");
        if let Some(manifest) = manifest.as_ref() {
            merge_manifest_mcp_servers(&mut plugin_servers, &plugin_root, manifest);
        }
        for (name, config) in plugin_servers {
            all_servers.insert(name, config);
        }
    }
    all_servers
}

fn merge_manifest_mcp_servers(
    servers: &mut serde_json::Map<String, Value>,
    plugin_root: &Path,
    manifest: &Value,
) {
    let Some(spec) = manifest.get("mcpServers") else {
        return;
    };
    match spec {
        Value::String(path) => merge_safe_plugin_mcp_file(servers, plugin_root, path),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(path) => merge_safe_plugin_mcp_file(servers, plugin_root, path),
                    Value::Object(_) => {
                        let substituted = substitute_plugin_mcp_variables(item, plugin_root);
                        merge_mcp_servers_value(servers, &substituted);
                    }
                    _ => {}
                }
            }
        }
        Value::Object(_) => {
            let substituted = substitute_plugin_mcp_variables(spec, plugin_root);
            merge_mcp_servers_value(servers, &substituted);
        }
        _ => {}
    }
}

fn merge_safe_plugin_mcp_file(
    servers: &mut serde_json::Map<String, Value>,
    plugin_root: &Path,
    path: &str,
) {
    if path.ends_with(".mcpb") || path.ends_with(".dxt") {
        return;
    }
    let Some(path) = safe_relative_plugin_path(path) else {
        return;
    };
    merge_plugin_mcp_file(servers, plugin_root, path);
}

fn merge_plugin_mcp_file(
    servers: &mut serde_json::Map<String, Value>,
    plugin_root: &Path,
    relative_path: impl AsRef<Path>,
) {
    let path = plugin_root.join(relative_path);
    let Some(value) = read_json_file(&path) else {
        return;
    };
    let value = substitute_plugin_mcp_variables(&value, plugin_root);
    merge_mcp_servers_value(servers, &value);
}

fn merge_mcp_servers_value(target: &mut serde_json::Map<String, Value>, value: &Value) {
    let servers = value
        .get("mcpServers")
        .or_else(|| value.get(MCP_SERVERS_APP_STATE_KEY))
        .unwrap_or(value);
    match servers {
        Value::Object(object) => {
            for (name, config) in object {
                if !name.trim().is_empty() {
                    target.insert(name.clone(), config.clone());
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                let Some(name) = item
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                else {
                    continue;
                };
                target.insert(name.to_string(), item.clone());
            }
        }
        _ => {}
    }
}

fn substitute_plugin_mcp_variables(value: &Value, plugin_root: &Path) -> Value {
    match value {
        Value::String(value) => {
            let plugin_root = plugin_root.to_string_lossy();
            Value::String(
                value
                    .replace("${CLAUDE_PLUGIN_ROOT}", &plugin_root)
                    .replace("${KIANA_PLUGIN_ROOT}", &plugin_root),
            )
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| substitute_plugin_mcp_variables(value, plugin_root))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        substitute_plugin_mcp_variables(value, plugin_root),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn expand_env_vars_in_value(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(expand_env_vars_in_string(value)),
        Value::Array(values) => Value::Array(values.iter().map(expand_env_vars_in_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), expand_env_vars_in_value(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn expand_env_vars_in_string(value: &str) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        expanded.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            expanded.push_str(&rest[start..]);
            return expanded;
        };
        let content = &after_start[..end];
        let (name, default) = content
            .split_once(":-")
            .map(|(name, default)| (name, Some(default)))
            .unwrap_or((content, None));
        if let Ok(env_value) = env::var(name) {
            expanded.push_str(&env_value);
        } else if let Some(default) = default {
            expanded.push_str(default);
        } else {
            expanded.push_str("${");
            expanded.push_str(content);
            expanded.push('}');
        }
        rest = &after_start[end + 1..];
    }
    expanded.push_str(rest);
    expanded
}

fn installed_plugin_roots() -> Vec<PathBuf> {
    kiana_types::plugin::installed_plugin_roots()
}

fn safe_relative_plugin_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path)
}

fn read_json_file(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
}

fn find_plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

impl McpServerConfigInput {
    fn into_config(self, default_name: &str) -> ToolResult<McpServerConfig> {
        let url = self.url.map(|value| value.trim().to_string());
        let command = self.command.map(|value| value.trim().to_string());
        let transport = self
            .transport
            .or_else(|| infer_transport(url.as_deref(), command.as_deref()))
            .ok_or_else(|| {
                ToolError::ValidationError(
                    "MCP server config requires transport, url, or command".to_string(),
                )
            })?;

        Ok(McpServerConfig {
            name: self
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| default_name.to_string()),
            transport,
            url,
            command,
            args: self.command_args.or(self.args),
            env: self.env,
            headers: self.headers,
        })
    }
}

fn infer_transport(url: Option<&str>, command: Option<&str>) -> Option<TransportType> {
    if command.is_some_and(|command| !command.trim().is_empty()) {
        return Some(TransportType::Stdio);
    }

    let url = url?.trim().to_ascii_lowercase();
    if url.starts_with("ws://") || url.starts_with("wss://") {
        Some(TransportType::Ws)
    } else if url.contains("/sse") {
        Some(TransportType::Sse)
    } else if !url.is_empty() {
        Some(TransportType::Http)
    } else {
        None
    }
}

fn value_object_to_hashmap(value: Value) -> ToolResult<HashMap<String, Value>> {
    let Some(object) = value.as_object() else {
        return Err(ToolError::ValidationError(
            "args must be an object".to_string(),
        ));
    };
    Ok(object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect())
}

fn record_invocation(context: &mut ToolContext, invocation: Value) -> ToolResult<()> {
    let mut invocations = context
        .app_state
        .get(MCP_INVOCATIONS_KEY)
        .cloned()
        .unwrap_or_else(|| json!([]));
    let Some(invocations_array) = invocations.as_array_mut() else {
        return Err(ToolError::Other(
            "mcp_invocations state is not an array".to_string(),
        ));
    };

    invocations_array.push(invocation);
    context
        .app_state
        .insert(MCP_INVOCATIONS_KEY.to_string(), invocations);
    Ok(())
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        configured_mcp_servers, configured_mcp_servers_with_trust, resolve_server_config_fields,
        GetMcpPromptTool, ListMcpPromptsTool, ListMcpResourceTemplatesTool, ListMcpResourcesTool,
        McpTool, ReadMcpResourceTool, MCP_SERVERS_APP_STATE_KEY, MCP_SERVERS_ENV,
    };
    use crate::{Tool, ToolContext};
    use kiana_services::mcp::TransportType;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kiana-mcp-{label}-{}-{unique}", std::process::id()))
    }

    fn write_plugin_manifest(plugin_root: &Path, name: &str, extra: Value) {
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        let mut manifest = serde_json::Map::new();
        manifest.insert("name".to_string(), json!(name));
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                manifest.insert(key.clone(), value.clone());
            }
        }
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            Value::Object(manifest).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn configured_mcp_servers_merges_parent_project_mcp_json_files() {
        let _guard = crate::test_support::lock_env();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_root("project-parent");
        let child = root.join("packages").join("app");
        fs::create_dir_all(&child).unwrap();
        fs::write(
            root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "docs": {
                        "command": "root-docs",
                        "args": ["--root"]
                    },
                    "shared": {
                        "command": "root-shared"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            child.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "shared": {
                        "command": "child-shared"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_current_dir(&child).unwrap();
        std::env::remove_var(MCP_SERVERS_ENV);
        std::env::remove_var("KIANA_PLUGINS_DIR");

        let servers = configured_mcp_servers().unwrap();
        assert_eq!(servers["docs"]["command"], "root-docs");
        assert_eq!(servers["shared"]["command"], "child-shared");

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_mcp_servers_reads_user_mcp_config_when_project_is_untrusted() {
        let _guard = crate::test_support::lock_env();
        let previous_cwd = std::env::current_dir().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("user-scope");
        let project = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&kiana_home).unwrap();
        fs::write(
            kiana_home.join("mcp.json"),
            json!({
                "mcpServers": {
                    "user-docs": {
                        "command": "node",
                        "args": ["user-server.js"]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            project.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "project-docs": {
                        "command": "project-server"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_current_dir(&project).unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::remove_var(MCP_SERVERS_ENV);
        std::env::remove_var("KIANA_PLUGINS_DIR");

        let servers = configured_mcp_servers_with_trust(kiana_types::ProjectTrust::Untrusted)
            .expect("user MCP config should be loaded");
        assert_eq!(servers["user-docs"]["command"], "node");
        assert!(servers.get("project-docs").is_none(), "{servers:?}");

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_mcp_servers_reads_local_scope_only_when_project_is_trusted() {
        let _guard = crate::test_support::lock_env();
        let previous_cwd = std::env::current_dir().unwrap();
        let previous_home = std::env::var_os("KIANA_HOME");
        let root = temp_root("local-scope");
        let project = root.join("project");
        let kiana_home = root.join("home").join(".kiana");
        fs::create_dir_all(project.join(".kiana")).unwrap();
        fs::create_dir_all(&kiana_home).unwrap();
        fs::write(
            project.join(".kiana").join("mcp.local.json"),
            json!({
                "mcpServers": {
                    "local-shell": {
                        "command": "bash",
                        "args": ["local-server.sh"]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_current_dir(&project).unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::remove_var(MCP_SERVERS_ENV);
        std::env::remove_var("KIANA_PLUGINS_DIR");

        let untrusted = configured_mcp_servers_with_trust(kiana_types::ProjectTrust::Untrusted);
        assert!(
            untrusted
                .as_ref()
                .and_then(|servers| servers.get("local-shell"))
                .is_none(),
            "{untrusted:?}"
        );

        let trusted = configured_mcp_servers_with_trust(kiana_types::ProjectTrust::Trusted)
            .expect("local MCP config should be loaded for trusted projects");
        assert_eq!(trusted["local-shell"]["command"], "bash");

        match previous_home {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_server_config_uses_project_mcp_json_without_app_state() {
        let _guard = crate::test_support::lock_env();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_root("project-resolve");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "docs": {
                        "command": "node",
                        "args": ["server.js"],
                        "env": {
                            "API_KEY": "from-config"
                        },
                        "headers": {
                            "Authorization": "Bearer from-config"
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(MCP_SERVERS_ENV);
        std::env::remove_var("KIANA_PLUGINS_DIR");

        let config = resolve_server_config_fields(
            Some("docs"),
            &None,
            None,
            None,
            None,
            None,
            &HashMap::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(config.command.as_deref(), Some("node"));
        assert_eq!(config.args, Some(vec!["server.js".to_string()]));
        assert_eq!(
            config
                .env
                .as_ref()
                .and_then(|env| env.get("API_KEY"))
                .map(String::as_str),
            Some("from-config")
        );
        assert_eq!(
            config
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization"))
                .map(String::as_str),
            Some("Bearer from-config")
        );

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn untrusted_project_mcp_json_is_ignored_for_server_resolution() {
        let _guard = crate::test_support::lock_env();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_root("project-trust");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "project-docs": {
                        "command": "node",
                        "args": ["project-server.js"]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::set_var(
            MCP_SERVERS_ENV,
            json!({
                "mcpServers": {
                    "env-docs": {
                        "command": "env-server"
                    }
                }
            })
            .to_string(),
        );
        std::env::remove_var("KIANA_PLUGINS_DIR");
        let app_state = HashMap::from([("project_trusted".to_string(), json!(false))]);

        let project_config = resolve_server_config_fields(
            Some("project-docs"),
            &None,
            None,
            None,
            None,
            None,
            &app_state,
        )
        .unwrap();
        assert!(project_config.is_none());

        let env_config = resolve_server_config_fields(
            Some("env-docs"),
            &None,
            None,
            None,
            None,
            None,
            &app_state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(env_config.command.as_deref(), Some("env-server"));

        std::env::remove_var(MCP_SERVERS_ENV);
        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_mcp_json_expands_environment_variables_and_defaults() {
        let _guard = crate::test_support::lock_env();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_root("project-env-expansion");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "docs": {
                        "command": "${KIANA_TEST_MCP_COMMAND}",
                        "args": ["${KIANA_TEST_MCP_ARG:-fallback.js}"]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(MCP_SERVERS_ENV);
        std::env::remove_var("KIANA_PLUGINS_DIR");
        std::env::set_var("KIANA_TEST_MCP_COMMAND", "node");
        std::env::remove_var("KIANA_TEST_MCP_ARG");

        let config = resolve_server_config_fields(
            Some("docs"),
            &None,
            None,
            None,
            None,
            None,
            &HashMap::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(config.command.as_deref(), Some("node"));
        assert_eq!(config.args, Some(vec!["fallback.js".to_string()]));

        std::env::remove_var("KIANA_TEST_MCP_COMMAND");
        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn records_mcp_invocation_in_context_state() {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let output = McpTool::new()
            .call(
                &json!({
                    "server": "local",
                    "tool_name": "inspect",
                    "args": { "path": "src" }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(output.data["status"], "recorded");
        assert_eq!(
            context.app_state["mcp_invocations"][0]["tool_name"],
            "inspect"
        );
    }

    #[test]
    fn maps_mcp_tool_result_content_to_model_facing_text() {
        let output = crate::tool::ToolOutput {
            data: json!({
                "status": "executed",
                "invocation": {
                    "tool_name": "echo"
                },
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": "hello from mcp"
                        }
                    ]
                }
            }),
            metadata: None,
        };

        let result = McpTool::new().map_to_api_result(&output, "toolu_mcp");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_mcp");
        assert_eq!(result["content"], "hello from mcp");
        assert!(result["content"]["invocation"].is_null());
    }

    #[test]
    fn maps_mcp_resource_lists_and_reads_to_model_facing_json_text() {
        let resources = crate::tool::ToolOutput {
            data: json!([{
                "uri": "file://note",
                "name": "note",
                "server": "mock"
            }]),
            metadata: None,
        };
        let list_result =
            ListMcpResourcesTool::new().map_to_api_result(&resources, "toolu_list_resources");

        assert_eq!(list_result["type"], "tool_result");
        assert_eq!(list_result["tool_use_id"], "toolu_list_resources");
        let list_content = list_result["content"].as_str().unwrap();
        assert!(list_content.contains("\"uri\": \"file://note\""));
        assert!(list_result["content"][0]["uri"].is_null());

        let empty = crate::tool::ToolOutput {
            data: json!([]),
            metadata: None,
        };
        let empty_result =
            ListMcpResourcesTool::new().map_to_api_result(&empty, "toolu_empty_resources");
        assert!(empty_result["content"]
            .as_str()
            .unwrap()
            .contains("No resources found"));

        let resource = crate::tool::ToolOutput {
            data: json!({
                "contents": [
                    {
                        "uri": "file://note",
                        "text": "resource body"
                    }
                ]
            }),
            metadata: None,
        };
        let read_result =
            ReadMcpResourceTool::new().map_to_api_result(&resource, "toolu_read_resource");
        let read_content = read_result["content"].as_str().unwrap();
        assert!(read_content.contains("\"text\": \"resource body\""));
        assert!(read_result["content"]["contents"].is_null());
    }

    #[test]
    fn maps_mcp_resource_template_lists_to_model_facing_json_text() {
        let templates = crate::tool::ToolOutput {
            data: json!([{
                "uriTemplate": "file:///logs/{date}.log",
                "name": "Daily logs",
                "server": "mock"
            }]),
            metadata: None,
        };
        let result = ListMcpResourceTemplatesTool::new()
            .map_to_api_result(&templates, "toolu_list_templates");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_list_templates");
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("\"uriTemplate\": \"file:///logs/{date}.log\""));
        assert!(result["content"][0]["uriTemplate"].is_null());
    }

    #[test]
    fn maps_mcp_prompt_lists_and_get_results_to_model_facing_json_text() {
        let prompts = crate::tool::ToolOutput {
            data: json!([{
                "name": "summarize",
                "description": "Summarize a topic",
                "arguments": [{
                    "name": "topic",
                    "description": "Topic to summarize",
                    "required": true
                }],
                "server": "mock"
            }]),
            metadata: None,
        };
        let list_result =
            ListMcpPromptsTool::new().map_to_api_result(&prompts, "toolu_list_prompts");

        assert_eq!(list_result["type"], "tool_result");
        assert_eq!(list_result["tool_use_id"], "toolu_list_prompts");
        let list_content = list_result["content"].as_str().unwrap();
        assert!(list_content.contains("\"name\": \"summarize\""));
        assert!(list_content.contains("\"required\": true"));
        assert!(list_result["content"][0]["name"].is_null());

        let empty = crate::tool::ToolOutput {
            data: json!([]),
            metadata: None,
        };
        let empty_result =
            ListMcpPromptsTool::new().map_to_api_result(&empty, "toolu_empty_prompts");
        assert!(empty_result["content"]
            .as_str()
            .unwrap()
            .contains("No prompts found"));

        let prompt = crate::tool::ToolOutput {
            data: json!({
                "description": "Summarize a topic",
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Summarize release readiness"
                    }
                }]
            }),
            metadata: None,
        };
        let get_result = GetMcpPromptTool::new().map_to_api_result(&prompt, "toolu_get_prompt");
        let get_content = get_result["content"].as_str().unwrap();
        assert!(get_content.contains("\"text\": \"Summarize release readiness\""));
        assert!(get_result["content"]["messages"].is_null());
    }

    #[tokio::test]
    async fn executes_mcp_tool_when_server_config_is_provided() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = McpTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "transport": "http",
                    "url": url,
                    "tool_name": "echo",
                    "args": { "message": "hello from MCP" }
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["status"], "executed");
        assert_eq!(
            output.data["result"]["content"][0]["text"],
            "hello from MCP"
        );
        assert_eq!(context.app_state["mcp_invocations"][0]["transport"], "http");
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn executes_mcp_tool_from_app_state_server_config() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([(
                MCP_SERVERS_APP_STATE_KEY.to_string(),
                json!({
                    "mock-http": {
                        "transport": "http",
                        "url": url
                    }
                }),
            )]),
            abort_signal: abort_rx,
        };

        let output = McpTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "tool_name": "echo",
                    "args": { "message": "hello from app_state" }
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["status"], "executed");
        assert_eq!(
            output.data["result"]["content"][0]["text"],
            "hello from app_state"
        );
        assert_eq!(
            context.app_state["mcp_invocations"][0]["server"],
            "mock-http"
        );
        assert_eq!(context.app_state["mcp_invocations"][0]["transport"], "http");
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn mcp_tool_error_result_marks_tool_execution_error() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let result = crate::tool_execution::execute_tool_call(
            &crate::create_default_registry(),
            None,
            &mut context,
            "MCP",
            &json!({
                "server": "mock-http",
                "transport": "http",
                "url": url,
                "tool_name": "fail",
                "args": { "message": "denied by MCP fixture" }
            }),
            Some("toolu_mcp_error"),
        )
        .await;

        assert!(result.is_error, "{:?}", result);
        assert_eq!(result.api_result["is_error"], true);
        assert_eq!(result.api_result["tool_use_id"], "toolu_mcp_error");
        assert!(result.api_result["content"]
            .as_str()
            .unwrap()
            .contains("denied by MCP fixture"));
        assert_eq!(result.content["result"]["isError"], true);
        handle.join().unwrap();
    }

    #[test]
    fn resolves_plugin_mcp_servers_with_reference_priority() {
        let _guard = crate::test_support::lock_env();
        let root = temp_root("plugin-priority");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-mcp");
        fs::create_dir_all(plugin_root.join("extra")).unwrap();
        fs::create_dir_all(plugin_root.join("bin")).unwrap();
        write_plugin_manifest(
            &plugin_root,
            "review-mcp",
            json!({
                "mcpServers": [
                    "extra/mcp.json",
                    {
                        "inline-server": {
                            "command": "${CLAUDE_PLUGIN_ROOT}/bin/inline-mcp",
                            "args": ["--inline"]
                        },
                        "overridden": {
                            "command": "manifest-mcp"
                        }
                    },
                    "../outside.json"
                ]
            }),
        );
        fs::write(
            plugin_root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "default-server": {
                        "command": "default-mcp",
                        "args": ["--default"]
                    },
                    "overridden": {
                        "command": "default-mcp"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            plugin_root.join("extra").join("mcp.json"),
            json!({
                "file-server": {
                    "type": "http",
                    "url": "http://127.0.0.1/mcp"
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            plugins_dir.join("outside.json"),
            json!({
                "outside-server": {
                    "command": "outside-mcp"
                }
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let empty_state = HashMap::new();
        let default_server = resolve_server_config_fields(
            Some("default-server"),
            &None,
            None,
            None,
            None,
            None,
            &empty_state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(default_server.command.as_deref(), Some("default-mcp"));
        assert_eq!(default_server.args, Some(vec!["--default".to_string()]));

        let file_server = resolve_server_config_fields(
            Some("file-server"),
            &None,
            None,
            None,
            None,
            None,
            &empty_state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(file_server.transport, TransportType::Http);
        assert_eq!(file_server.url.as_deref(), Some("http://127.0.0.1/mcp"));

        let inline_server = resolve_server_config_fields(
            Some("inline-server"),
            &None,
            None,
            None,
            None,
            None,
            &empty_state,
        )
        .unwrap()
        .unwrap();
        let inline_command = plugin_root.join("bin").join("inline-mcp");
        assert_eq!(
            inline_server.command.as_deref().map(Path::new),
            Some(inline_command.as_path())
        );

        let manifest_override = resolve_server_config_fields(
            Some("overridden"),
            &None,
            None,
            None,
            None,
            None,
            &empty_state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(manifest_override.command.as_deref(), Some("manifest-mcp"));

        let app_state = HashMap::from([(
            MCP_SERVERS_APP_STATE_KEY.to_string(),
            json!({
                "overridden": {
                    "command": "app-state-mcp"
                }
            }),
        )]);
        let app_state_override = resolve_server_config_fields(
            Some("overridden"),
            &None,
            None,
            None,
            None,
            None,
            &app_state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(app_state_override.command.as_deref(), Some("app-state-mcp"));
        assert!(resolve_server_config_fields(
            Some("outside-server"),
            &None,
            None,
            None,
            None,
            None,
            &empty_state,
        )
        .unwrap()
        .is_none());

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_plugin_mcp_servers_are_not_resolved() {
        let _guard = crate::test_support::lock_env();
        let root = temp_root("plugin-disabled");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-mcp");
        write_plugin_manifest(&plugin_root, "review-mcp", json!({}));
        fs::write(
            plugin_root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "review-server": {
                        "command": "review-mcp"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "review-mcp", false).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        assert!(resolve_server_config_fields(
            Some("review-server"),
            &None,
            None,
            None,
            None,
            None,
            &HashMap::new(),
        )
        .unwrap()
        .is_none());

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn lists_mcp_resources_when_server_config_is_provided() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = ListMcpResourcesTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "transport": "http",
                    "url": url
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data[0]["uri"], "mock://status");
        assert_eq!(output.data[0]["server"], "mock-http");
        assert_eq!(output.data[0]["mimeType"], "text/plain");
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn lists_mcp_resource_templates_when_server_config_is_provided() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = ListMcpResourceTemplatesTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "transport": "http",
                    "url": url
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data[0]["uriTemplate"], "mock://logs/{date}");
        assert_eq!(output.data[0]["server"], "mock-http");
        assert_eq!(output.data[0]["mimeType"], "text/plain");
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn lists_mcp_prompts_when_server_config_is_provided() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = ListMcpPromptsTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "transport": "http",
                    "url": url
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data[0]["name"], "summarize");
        assert_eq!(output.data[0]["server"], "mock-http");
        assert_eq!(output.data[0]["arguments"][0]["name"], "topic");
        assert_eq!(output.data[0]["arguments"][0]["required"], true);
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn gets_mcp_prompt_when_server_config_is_provided() {
        let (url, handle) = start_mock_mcp_tool_server(8);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = GetMcpPromptTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "transport": "http",
                    "url": url,
                    "name": "summarize",
                    "args": { "topic": "release readiness" }
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["description"], "Summarize a topic");
        assert_eq!(
            output.data["messages"][0]["content"]["text"],
            "Summarize release readiness"
        );
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn reads_mcp_resource_when_server_config_is_provided() {
        let (url, handle) = start_mock_mcp_tool_server(7);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let output = ReadMcpResourceTool::new()
            .call(
                &json!({
                    "server": "mock-http",
                    "transport": "http",
                    "url": url,
                    "uri": "mock://status"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["contents"][0]["uri"], "mock://status");
        assert_eq!(output.data["contents"][0]["text"], "mock ready");
        handle.join().unwrap();
    }

    fn start_mock_mcp_tool_server(requests: usize) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut handled = 0usize;
            let mut last_request = Instant::now();
            let deadline = Instant::now() + Duration::from_secs(5);
            while handled < requests && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        handled += 1;
                        last_request = Instant::now();
                        handle_mock_mcp_tool_request(&mut stream);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if handled > 0 && last_request.elapsed() > Duration::from_secs(2) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn handle_mock_mcp_tool_request(stream: &mut TcpStream) {
        let body = read_http_body(stream);
        let message: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let response = mock_mcp_tool_response(&message);
        let body = response.map(|value| value.to_string()).unwrap_or_default();
        let status = if body.is_empty() {
            "HTTP/1.1 204 No Content"
        } else {
            "HTTP/1.1 200 OK"
        };
        let response = format!(
            "{status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn read_http_body(stream: &mut TcpStream) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        let header_end;
        loop {
            let read = stream.read(&mut chunk).unwrap();
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = position;
                break;
            }
        }

        let headers = String::from_utf8_lossy(&buffer[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        buffer[body_start..body_start + content_length].to_vec()
    }

    fn mock_mcp_tool_response(message: &serde_json::Value) -> Option<serde_json::Value> {
        let id = message.get("id").cloned().unwrap_or(json!(null));
        match message.get("method").and_then(serde_json::Value::as_str) {
            Some("notifications/initialized") => None,
            Some("initialize") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": "mock-http", "version": "1.0.0"}
                }
            })),
            Some("tools/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "echo",
                        "description": "Echo a message",
                        "inputSchema": {"type": "object"}
                    }]
                }
            })),
            Some("resources/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "resources": [{
                        "uri": "mock://status",
                        "name": "Mock status",
                        "description": "Mock server status",
                        "mimeType": "text/plain"
                    }]
                }
            })),
            Some("resources/templates/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "resourceTemplates": [{
                        "uriTemplate": "mock://logs/{date}",
                        "name": "Mock daily logs",
                        "description": "Mock logs for a date",
                        "mimeType": "text/plain"
                    }]
                }
            })),
            Some("resources/read") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "contents": [{
                        "uri": message
                            .get("params")
                            .and_then(|params| params.get("uri"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("mock://status"),
                        "mimeType": "text/plain",
                        "text": "mock ready"
                    }]
                }
            })),
            Some("prompts/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "prompts": [{
                        "name": "summarize",
                        "description": "Summarize a topic",
                        "arguments": [{
                            "name": "topic",
                            "description": "Topic to summarize",
                            "required": true
                        }]
                    }]
                }
            })),
            Some("prompts/get") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "description": "Summarize a topic",
                    "messages": [{
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "Summarize {}",
                                message
                                    .get("params")
                                    .and_then(|params| params.get("arguments"))
                                    .and_then(|args| args.get("topic"))
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("the topic")
                            )
                        }
                    }]
                }
            })),
            Some("tools/call") => {
                let tool_name = message
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let text = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .and_then(|args| args.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let is_error = tool_name == "fail";
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": text }],
                        "isError": is_error
                    }
                }))
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"}
            })),
        }
    }
}
