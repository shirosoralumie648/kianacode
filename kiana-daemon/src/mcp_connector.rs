//! Stdio-only MCP connector adapter.
//!
//! This adapter is reached after a connector capability permit.  It reuses the confined
//! per-invocation MCP process owned by the daemon and returns only the typed handshake.  HTTP,
//! SSE, WebSocket and remote configuration are rejected before a child or network client starts.

use crate::mcp_stdio::ConfinedMcpClient;
use async_trait::async_trait;
use kiana_domain::{
    McpCapabilityHandshake, McpDeclaredTool, McpToolAdvertisement, MCP_STDIO_TRANSPORT,
};
use kiana_ports::{
    ConnectorAdapter, ConnectorAdapterCapabilities, ConnectorAdapterCapability,
    McpCapabilityHandshakeRequest, PortError,
};
use kiana_services::mcp::{McpServerConfig, TransportType};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use tokio::sync::watch;

pub(crate) struct StdioMcpConnectorAdapter {
    config: McpServerConfig,
    scope: Value,
    declared: Vec<McpDeclaredTool>,
}

impl StdioMcpConnectorAdapter {
    pub(crate) fn new(
        config: McpServerConfig,
        scope: Value,
        declared: Vec<McpDeclaredTool>,
    ) -> Result<Self, PortError> {
        if config.transport != TransportType::Stdio
            || config.url.is_some()
            || config
                .headers
                .as_ref()
                .is_some_and(|headers| !headers.is_empty())
        {
            return Err(failed("mcp_http_unsupported"));
        }
        if config.name.trim().is_empty() {
            return Err(failed("mcp_server_name_invalid"));
        }
        Ok(Self {
            config,
            scope,
            declared,
        })
    }

    async fn run_handshake(
        &self,
        request: &McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        if request.server != self.config.name {
            return Err(failed("mcp_server_binding_mismatch"));
        }
        if request.binding.definition.adapter != "stdio_mcp" {
            return Err(failed("mcp_connector_transport_unsupported"));
        }
        let (_sender, cancellation) = watch::channel(false);
        let mut client = ConfinedMcpClient::connect(&self.config, &self.scope, cancellation)
            .await
            .map_err(|error| map_handshake_error(error, "mcp_startup_failed"))?;
        let tools = match client.list_tools().await {
            Ok(tools) => tools,
            Err(error) => {
                let outcome = client.stop().await;
                return Err(match outcome {
                    Ok(_) => map_handshake_error(error, "mcp_tools_list_failed"),
                    Err(stop_error) => map_handshake_error(
                        stop_error,
                        "result_unknown:mcp_stop_unconfirmed",
                    ),
                });
            }
        };
        let server_info = client.server_info.clone();
        let protocol_version = server_info["protocol_version"]
            .as_str()
            .ok_or_else(|| failed("mcp_protocol_version_required"))?
            .to_owned();
        let advertisements = tools
            .iter()
            .map(advertisement_from_tool)
            .collect::<Result<Vec<_>, _>>()?;
        let binding_scopes = request.binding.binding.read_scopes.clone();
        let handshake = McpCapabilityHandshake::negotiate(
            protocol_version,
            &server_info,
            &request.session_ref,
            advertisements,
            self.declared.clone(),
            &binding_scopes,
        )
        .map_err(failed);
        let stopped = client.stop().await;
        if let Err(error) = stopped {
            return Err(map_handshake_error(
                error,
                "result_unknown:mcp_stop_unconfirmed",
            ));
        }
        handshake
    }
}

#[async_trait]
impl ConnectorAdapter for StdioMcpConnectorAdapter {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::from_iter([ConnectorAdapterCapability::CapabilityHandshake])
    }

    async fn capability_handshake(
        &self,
        request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        self.run_handshake(&request).await
    }
}

fn advertisement_from_tool(tool: &Value) -> Result<McpToolAdvertisement, PortError> {
    let name = tool["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| failed("mcp_tool_name_invalid"))?;
    let mut requested_scopes = BTreeSet::new();
    if let Some(scopes) = tool.get("requiredScopes") {
        let values = scopes
            .as_array()
            .ok_or_else(|| failed("mcp_server_scope_not_authority"))?;
        for scope in values {
            requested_scopes.insert(
                scope
                    .as_str()
                    .filter(|scope| !scope.trim().is_empty())
                    .ok_or_else(|| failed("mcp_server_scope_not_authority"))?
                    .to_owned(),
            );
        }
    }
    let advertisement = McpToolAdvertisement {
        schema: kiana_domain::MCP_TOOL_ADVERTISEMENT_SCHEMA.to_owned(),
        name: name.to_owned(),
        input_schema: tool["inputSchema"].clone(),
        output_schema: tool.get("outputSchema").cloned(),
        description: tool
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        conditional: tool["execution"]["taskSupport"] == "optional",
        server_requested_scopes: requested_scopes,
    };
    advertisement.validate().map_err(failed)?;
    Ok(advertisement)
}

fn map_handshake_error(error: PortError, fallback: &str) -> PortError {
    let text = error.to_string();
    if text.contains("mcp_response_timeout") || text.contains("mcp_write_timeout") {
        failed("mcp_startup_timeout")
    } else if text.contains("mcp_stream_closed") {
        failed("mcp_disconnected")
    } else if text.contains("mcp_stop_unconfirmed") {
        failed("result_unknown:mcp_stop_unconfirmed")
    } else {
        failed(if text.trim().is_empty() {
            fallback
        } else {
            fallback
        })
    }
}

fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
