//! Broker-owned Streamable HTTP MCP adapter.
//!
//! The legacy service crate supplies the wire client, but this wrapper is the product-path
//! boundary: it validates the configured endpoint before connect, rejects raw credential headers,
//! keeps the call-sent bit explicit, and exposes only bounded JSON values to `harness_mcp`.

use kiana_ports::PortError;
use kiana_services::mcp::{McpClient, McpServerConfig, TransportType};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::watch;

pub(crate) struct HttpMcpClient {
    client: McpClient,
    pub call_started: bool,
    pub server_info: Value,
}

impl HttpMcpClient {
    pub async fn connect(
        config: &McpServerConfig,
        cancellation: watch::Receiver<bool>,
    ) -> Result<Self, PortError> {
        if !matches!(&config.transport, TransportType::Http | TransportType::Sse) {
            return Err(failed("mcp_http_transport_required"));
        }
        let url = config
            .url
            .as_deref()
            .ok_or_else(|| failed("mcp_http_url_required"))?;
        kiana_services::network_policy::validate_http_url(
            kiana_services::network_policy::HttpNetworkSurface::HttpMcp,
            url,
        )
        .map_err(|_| failed("mcp_http_endpoint_denied"))?;
        if config.command.is_some() || config.args.is_some() || config.env.is_some() {
            return Err(failed("mcp_http_process_fields_invalid"));
        }
        if config.headers.as_ref().is_some_and(|headers| {
            headers.iter().any(|(name, _)| {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "authorization" | "proxy-authorization" | "x-api-key" | "api-key"
                )
            })
        }) {
            return Err(failed("mcp_http_raw_credential_header_denied"));
        }
        if *cancellation.borrow() {
            return Err(failed("cancelled:mcp_http_not_started"));
        }
        let transport = config.transport.clone();
        let config = config.clone();
        let mut client = McpClient::new(config);
        tokio::select! {
            biased;
            _ = wait_cancel(cancellation) => return Err(failed("cancelled:mcp_http_not_started")),
            result = client.connect() => result.map_err(|_| failed("mcp_http_connect_failed"))?,
        }
        Ok(Self {
            client,
            call_started: false,
            server_info: json!({
                "protocol_version": "2024-11-05",
                "transport": match transport { TransportType::Sse => "sse", _ => "streamable_http" },
                "session_identity": "server-owned-not-authority"
            }),
        })
    }

    pub async fn list_tools(&mut self) -> Result<Vec<Value>, PortError> {
        let tools = self
            .client
            .list_tools()
            .await
            .map_err(|_| failed("mcp_http_tools_list_failed"))?;
        let values = tools
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": tool.input_schema
                })
            })
            .collect::<Vec<_>>();
        kiana_domain::validate_json_limits(&json!(values))
            .map_err(|_| failed("mcp_http_tool_catalog_limit"))?;
        Ok(values)
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, PortError> {
        let object = arguments
            .as_object()
            .ok_or_else(|| failed("mcp_arguments_object_required"))?;
        let arguments: HashMap<String, Value> = object.clone().into_iter().collect();
        self.call_started = true;
        self.client
            .call_tool(name, arguments)
            .await
            .map_err(|_| failed("result_unknown:mcp_http_call_unconfirmed"))
    }

    pub async fn stop(&mut self) -> Result<Value, PortError> {
        Ok(json!({
            "stop_confirmed": true,
            "transport": "streamable_http",
            "call_started": self.call_started
        }))
    }
}

async fn wait_cancel(mut cancellation: watch::Receiver<bool>) {
    let _ = kiana_ports::wait_for_cancellation(&mut cancellation).await;
}

fn failed(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}
