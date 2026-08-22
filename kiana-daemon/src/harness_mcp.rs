//! MCP client capability for the owned harness.
//!
//! Discovery and execution stay on the daemon broker. The model only sees
//! the `mcp` tool. HTTP/SSE/WS wait; this slice is stdio.

use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult};
use kiana_ports::PortError;
use kiana_services::mcp::{McpClient, McpServerConfig, TransportType};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const MCP_OPERATION: &str = "mcp.call";
const MCP_RESULT_SCHEMA: &str = "kiana.mcp-result.v1";
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
        let result = client
            .call_tool(&tool, tool_arguments)
            .await
            .map_err(|error| PortError::Failed(format!("mcp_call_failed:{error}")))?;
        Ok(CapabilityResult::success(
            request_id,
            json!({
                "schema": MCP_RESULT_SCHEMA,
                "server": config.name,
                "tool": tool,
                "transport": "stdio",
                "result": result,
            }),
        ))
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
}

fn tool_arguments(value: Option<&Value>) -> Result<HashMap<String, Value>, PortError> {
    match value {
        None | Some(Value::Null) => Ok(HashMap::new()),
        Some(Value::Object(map)) => Ok(map_to_hash(map)),
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
}
