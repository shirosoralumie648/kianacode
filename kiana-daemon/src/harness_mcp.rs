//! MCP discovery and business calls use the same permit-protected broker.
use crate::mcp_http::HttpMcpClient;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AdapterCommitState, AdapterResultKind, AggregateVersion, AuthorizedCapabilityRequest,
    CapabilityKind, CapabilityRequest, CapabilityResult, CommitOutcome, McpCapabilityHandshake,
    RequestContext, RequestId, RuntimeEvent, TransitionBatch,
};
use kiana_ports::{ConnectorAdapter, EventStorePort, McpCapabilityHandshakeRequest, PortError};
#[cfg(test)]
use kiana_services::mcp::McpTool;
use kiana_services::mcp::{McpServerConfig, TransportType};
#[cfg(test)]
use serde_json::Map;
use serde_json::{json, Value};
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::watch;

const MCP_OPERATION: &str = "mcp.call";
const MCP_RESULT_SCHEMA: &str = "kiana.mcp-result.v1";
const MCP_MAX_CONFIG_BYTES: usize = 64 * 1024;
const MCP_MAX_TOOL_NAME_BYTES: usize = 256;
const MCP_MAX_ARGUMENT_BYTES: usize = 64 * 1024;
const MCP_MAX_RESULT_BYTES: usize = 256 * 1024;
#[cfg(test)]
const MCP_MAX_SCHEMA_DEPTH: usize = 32;
pub(crate) const MCP_SERVERS_ENV: &str = "KIANA_MCP_SERVERS_JSON";

enum McpInvocationClient {
    Stdio(crate::mcp_stdio::ConfinedMcpClient),
    Http(HttpMcpClient),
}

impl McpInvocationClient {
    async fn connect(
        config: &McpServerConfig,
        scope: &Value,
        cancellation: watch::Receiver<bool>,
    ) -> Result<Self, PortError> {
        match &config.transport {
            TransportType::Stdio => Ok(Self::Stdio(
                crate::mcp_stdio::ConfinedMcpClient::connect(config, scope, cancellation).await?,
            )),
            TransportType::Http | TransportType::Sse => Ok(Self::Http(
                HttpMcpClient::connect(config, cancellation).await?,
            )),
            _ => Err(mcp_failed("mcp_transport_unsupported")),
        }
    }

    async fn list_tools(&mut self) -> Result<Vec<Value>, PortError> {
        match self {
            Self::Stdio(client) => client.list_tools().await,
            Self::Http(client) => client.list_tools().await,
        }
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, PortError> {
        match self {
            Self::Stdio(client) => client.call_tool(name, arguments).await,
            Self::Http(client) => client.call_tool(name, arguments).await,
        }
    }

    async fn stop(&mut self) -> Result<Value, PortError> {
        match self {
            Self::Stdio(client) => client.stop().await,
            Self::Http(client) => client.stop().await,
        }
    }

    fn call_started(&self) -> bool {
        match self {
            Self::Stdio(client) => client.call_started,
            Self::Http(client) => client.call_started,
        }
    }

    fn result_boundary(
        &self,
        transport: &TransportType,
    ) -> Result<serde_json::Map<String, Value>, PortError> {
        match (self, transport) {
            (Self::Stdio(_), TransportType::Stdio)
            | (Self::Http(_), TransportType::Http | TransportType::Sse) => {
                mcp_result_boundary(transport)
            }
            _ => Err(mcp_failed("mcp_client_transport_mismatch")),
        }
    }

    fn server_info(&self) -> Value {
        match self {
            Self::Stdio(client) => client.server_info.clone(),
            Self::Http(client) => client.server_info.clone(),
        }
    }
}

/// Immutable trusted configuration; discovery facts are read from the journal, not this cache.
pub(crate) struct McpRegistry {
    servers: Vec<McpServerConfig>,
    events: Arc<dyn EventStorePort>,
}
impl McpRegistry {
    pub fn new(events: Arc<dyn EventStorePort>) -> Result<Arc<Self>, PortError> {
        let raw = std::env::var(MCP_SERVERS_ENV).unwrap_or_default();
        Ok(Arc::new(Self {
            servers: parse_servers(&raw)?,
            events,
        }))
    }

    /// Test-only constructor that injects already-validated servers, so behavioural fixtures do
    /// not depend on process-global environment mutation. It performs no additional validation;
    /// production paths always go through `new`/`parse_servers`.
    #[cfg(test)]
    fn with_servers(servers: Vec<McpServerConfig>, events: Arc<dyn EventStorePort>) -> Arc<Self> {
        Arc::new(Self { servers, events })
    }

    /// Read-only preparation: no MCP process, writes, or RPCs occur before the permit.
    pub async fn prepare(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<CapabilityRequest, PortError> {
        if !matches!(request.operation.as_str(), MCP_OPERATION | "mcp.discover") {
            return Ok(request.clone());
        }
        if !context.project_trusted {
            return Err(mcp_failed("mcp_project_untrusted"));
        }
        let config = select_server(&self.servers, request.arguments.get("server"))?;
        ensure_supported_transport(&config)?;
        let root = PathBuf::from(&context.project_root);
        let pin_config = config.clone();
        let pin_root = root.clone();
        let executable = tokio::task::spawn_blocking(move || config_pin(&pin_config, &pin_root))
            .await
            .map_err(|_| mcp_failed("mcp_config_pin_failed"))??;
        let config_hash = kiana_domain::json_digest(
            &serde_json::to_value(&config).map_err(|_| mcp_failed("mcp_config_invalid"))?,
        );
        let owner = context
            .actor_id
            .as_deref()
            .ok_or_else(|| mcp_failed("actor_identity_required"))?;
        let scope = json!({"actor_id":owner,"project_root":context.project_root,"role_id":context.role_id,"department_id":context.department_id});
        let key = kiana_domain::json_digest(
            &json!({"scope":scope,"server":config.name,"config_hash":config_hash}),
        );
        let mut snapshot = json!({"schema":"kiana.mcp-snapshot.v1","scope":scope,"server":config.name,
            "config_source":MCP_SERVERS_ENV,"transport":executable["transport"],
            "env_keys":executable["env_keys"],"credential_refs":executable["credential_refs"],
            "config_hash":config_hash,"discovery_key":key,"executable":executable["executable"],"argument_files":executable["argument_files"]});
        if request.operation == MCP_OPERATION {
            let tools = self.discovery(&key).await?;
            if tools["config_pin"] != executable {
                return Err(mcp_failed("mcp_config_drift_requires_discovery"));
            }
            let health = tools
                .get("health")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| mcp_failed("mcp_health_unavailable"))?;
            let tool = required_tool(&request.arguments)?;
            let advertised = tools["tools"]
                .as_array()
                .and_then(|tools| tools.iter().find(|value| value["name"] == tool))
                .ok_or_else(|| mcp_failed("mcp_tool_unknown"))?;
            validate_advertised_tool(advertised)?;
            let arguments = request
                .arguments
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if serde_json::to_vec(&arguments)
                .map_err(|_| mcp_failed("mcp_arguments_invalid"))?
                .len()
                > MCP_MAX_ARGUMENT_BYTES
            {
                return Err(mcp_failed("mcp_arguments_too_large"));
            }
            kiana_domain::validate_schema_value(&arguments, &advertised["inputSchema"])
                .map_err(|_| mcp_failed("mcp_arguments_schema_mismatch"))?;
            snapshot["tool"] = advertised.clone();
            snapshot["catalog_digest"] = tools["catalog_digest"].clone();
            snapshot["protocol"] = tools["protocol"].clone();
            snapshot["discovery_version"] = tools["version"].clone();
            snapshot["health"] = health;
        }
        let mut prepared = request.clone();
        prepared.arguments["server"] = json!(config.name);
        prepared.arguments["mcp_snapshot"] = snapshot;
        Ok(prepared)
    }

    /// Connector-owned read-only handshake. The config is resolved from the trusted registry and
    /// the process is always confined and per invocation; no HTTP/SSE/WS transport is accepted.
    pub(crate) async fn connector_handshake(
        &self,
        request: &McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        let selector = json!(request.server);
        let config = select_server(&self.servers, Some(&selector))?;
        if config.transport != TransportType::Stdio {
            return Err(mcp_failed("mcp_http_unsupported"));
        }
        let root = PathBuf::from(&request.binding.project_root);
        let pin_config = config.clone();
        let pin_root = root.clone();
        let pin = tokio::task::spawn_blocking(move || config_pin(&pin_config, &pin_root))
            .await
            .map_err(|_| mcp_failed("mcp_config_pin_failed"))??;
        let scope = json!({
            "project_trusted": true,
            "project_root": request.binding.project_root,
            "sandbox": "read-only",
            "path_allow": ["."],
            "mcp_snapshot": pin,
        });
        let adapter =
            crate::mcp_connector::StdioMcpConnectorAdapter::new(config, scope, Vec::new())?;
        adapter.capability_handshake_checked(request.clone()).await
    }

    async fn discovery(&self, key: &str) -> Result<Value, PortError> {
        let mut stream = self.events.read_stream("mcp_discovery", key).await?;
        stream.sort_by_key(|event| event.stream_version);
        let event = stream
            .last()
            .filter(|event| event.kind == "mcp.discovery_committed")
            .ok_or_else(|| mcp_failed("mcp_discovery_required:use_operator_mcp.discover"))?;
        let mut value = event.data.clone();
        value["version"] = json!(event
            .stream_version
            .ok_or_else(|| mcp_failed("mcp_discovery_invalid"))?);
        Ok(value)
    }

    async fn commit_discovery(
        &self,
        request: &CapabilityRequest,
        config_pin: Value,
        tools: Vec<Value>,
        protocol: Value,
    ) -> Result<Value, PortError> {
        let key = request.arguments["mcp_snapshot"]["discovery_key"]
            .as_str()
            .ok_or_else(|| mcp_failed("mcp_snapshot_required"))?;
        let prior = self.events.read_stream("mcp_discovery", key).await?;
        let version = prior
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let digest = catalog_digest(&tools, &protocol);
        let health = health_snapshot(&protocol, &tools);
        let data = json!({"schema":"kiana.mcp-discovery.v1","parent_request_id":request.request_id,
            "scope":request.arguments["mcp_snapshot"]["scope"],"server":request.arguments["server"],
            "config_pin":config_pin,"catalog_digest":digest,"protocol":protocol,"tools":tools,"health":health.clone()});
        let command_id = RequestId::new();
        let event = RuntimeEvent::new(command_id, 1, "mcp.discovery_committed", data.clone())
            .map_err(|_| mcp_failed("mcp_discovery_event_invalid"))?
            .with_stream_metadata("mcp_discovery", key, version + 1);
        let event_id = event.event_id;
        let batch = TransitionBatch {
            command_id,
            command_digest: kiana_domain::journal_sha256(
                &kiana_domain::canonical_journal_bytes(&data).map_err(mcp_failed)?,
            ),
            expected_versions: vec![AggregateVersion::new("mcp_discovery", key, version)],
            events: vec![event],
        };
        let outcome = self.events.commit_transition(batch).await?;
        match outcome {
            CommitOutcome::Committed { .. } | CommitOutcome::Replayed { .. } => (),
            CommitOutcome::Conflict { .. } => return Err(mcp_failed("mcp_discovery_conflict")),
            CommitOutcome::Unknown { .. } => {
                if self.events.read_command(&command_id).await?.is_none() {
                    return Err(mcp_failed(
                        "result_unknown:mcp_discovery_commit_unconfirmed",
                    ));
                }
            }
        }
        Ok(
            json!({"schema":"kiana.mcp-discovery.v1","server":request.arguments["server"],"catalog_digest":digest,
            "version":version+1,"tool_count":tools.len(),"tools":tools,"health":health,"evidence_ref":event_id.to_string(),"untrusted_data":true}),
        )
    }
}

pub(crate) fn register(
    broker: &mut CapabilityBroker,
    registry: Arc<McpRegistry>,
) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Network,
        MCP_OPERATION,
        Arc::new(McpHandler {
            registry: registry.clone(),
        }),
    )?;
    broker.register_static(
        CapabilityKind::Network,
        "mcp.discover",
        Arc::new(McpHandler { registry }),
    )
}
struct McpHandler {
    registry: Arc<McpRegistry>,
}
#[async_trait::async_trait]
impl CapabilityHandler for McpHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let (_sender, cancellation) = watch::channel(false);
        self.run(request, cancellation).await
    }
    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        self.run(request, cancellation).await
    }
}
impl McpHandler {
    async fn run(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        let arguments = &request.request.arguments;
        let discovering = request.request.operation == "mcp.discover";
        if !discovering && request.request.operation != MCP_OPERATION {
            return Err(mcp_failed("harness_operation_mismatch"));
        }
        if discovering
            && (request.request.cell_id.is_some() || arguments["operator_authorized"] != true)
        {
            return Err(mcp_failed("mcp_discovery_operator_required"));
        }
        let config = select_server(&self.registry.servers, arguments.get("server"))?;
        let snapshot = &arguments["mcp_snapshot"];
        if snapshot["schema"] != "kiana.mcp-snapshot.v1"
            || snapshot["config_hash"]
                != kiana_domain::json_digest(
                    &serde_json::to_value(&config).map_err(|_| mcp_failed("mcp_config_invalid"))?,
                )
        {
            return Err(mcp_failed("mcp_config_changed"));
        }
        for key in ["actor_id", "project_root", "role_id", "department_id"] {
            if snapshot["scope"][key] != arguments[key] {
                return Err(mcp_failed("mcp_snapshot_scope_changed"));
            }
        }
        let pin_config = config.clone();
        let pin_root = PathBuf::from(
            arguments["project_root"]
                .as_str()
                .ok_or_else(|| mcp_failed("mcp_project_required"))?,
        );
        let pin = tokio::task::spawn_blocking(move || config_pin(&pin_config, &pin_root))
            .await
            .map_err(|_| mcp_failed("mcp_config_pin_failed"))??;
        if snapshot["executable"] != pin["executable"]
            || snapshot["argument_files"] != pin["argument_files"]
        {
            return Err(mcp_failed("mcp_config_changed"));
        }
        if !discovering {
            let current = self
                .registry
                .discovery(
                    snapshot["discovery_key"]
                        .as_str()
                        .ok_or_else(|| mcp_failed("mcp_snapshot_required"))?,
                )
                .await?;
            if current["catalog_digest"] != snapshot["catalog_digest"]
                || current["version"] != snapshot["discovery_version"]
                || current["config_pin"] != pin
                || current["health"] != snapshot["health"]
            {
                return Err(mcp_failed("mcp_catalog_changed"));
            }
        }
        if *cancellation.borrow() {
            return Err(mcp_failed("cancelled:mcp_not_started"));
        }
        let mut execution_scope = arguments.clone();
        if discovering {
            execution_scope["sandbox"] = json!("read-only");
        }
        let mut workspace = None;
        if !discovering
            && arguments["sandbox"] == "workspace-write"
            && config.transport == TransportType::Stdio
        {
            let owned = request.clone();
            let root = PathBuf::from(
                arguments["project_root"]
                    .as_str()
                    .ok_or_else(|| mcp_failed("mcp_project_required"))?,
            );
            let copied = tokio::task::spawn_blocking(move || {
                crate::execution_workspace::ExecutionWorkspace::prepare(&owned, &root, &root)
            })
            .await
            .map_err(|_| mcp_failed("mcp_workspace_prepare_failed"))??;
            execution_scope["project_root"] = json!(copied.root);
            execution_scope["logical_project_root"] = arguments["project_root"].clone();
            execution_scope["path_allow"] = json!(["."]);
            workspace = Some(copied);
        }
        let mut client =
            McpInvocationClient::connect(&config, &execution_scope, cancellation.clone()).await?;
        let boundary = client.result_boundary(&config.transport)?;
        let performed = async {
            let tools = client.list_tools().await?;
            for tool in &tools {
                validate_advertised_tool(tool)?;
            }
            if discovering {
                return Ok((Value::Null, tools));
            }
            if catalog_digest(&tools, &client.server_info()) != snapshot["catalog_digest"] {
                return Err(mcp_failed("mcp_catalog_changed"));
            }
            let tool = required_tool(arguments)?;
            let advertised = tools
                .iter()
                .find(|value| value["name"] == tool)
                .ok_or_else(|| mcp_failed("mcp_tool_unknown"))?;
            if advertised != &snapshot["tool"] {
                return Err(mcp_failed("mcp_tool_schema_changed"));
            }
            let input = arguments
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            kiana_domain::validate_schema_value(&input, &advertised["inputSchema"])
                .map_err(|_| mcp_failed("mcp_arguments_schema_mismatch"))?;
            let result = client.call_tool(&tool, input).await?;
            validate_call_result(&result, advertised)?;
            Ok((result, tools))
        }
        .await;
        let started = client.call_started();
        let protocol = client.server_info();
        let stopped = client.stop().await;
        let stop_confirmed = stopped.is_ok();
        let cancelled = *cancellation.borrow();
        let successful = performed
            .as_ref()
            .is_ok_and(|(result, _)| result["isError"] != true)
            && stop_confirmed
            && !cancelled;
        // A sent call without a response, or an unconfirmed child stop, retains the private
        // workspace for operator reconciliation. It must never be silently discarded.
        let needs_reconciliation = !stop_confirmed || (started && performed.is_err());
        let publication = if let Some(workspace) = workspace {
            let result = if needs_reconciliation {
                let reason = if !stop_confirmed {
                    "mcp_stop_unconfirmed"
                } else {
                    "mcp_call_unconfirmed"
                };
                tokio::task::spawn_blocking(move || workspace.retain_for_reconciliation(reason))
                    .await
                    .map_err(|_| mcp_failed("result_unknown:mcp_workspace_retain_unconfirmed"))??
            } else {
                tokio::task::spawn_blocking(move || workspace.finish(successful))
                    .await
                    .map_err(|_| mcp_failed("result_unknown:mcp_workspace_finish_unconfirmed"))??
            };
            Some(result)
        } else {
            None
        };
        let diagnostics = stopped
            .unwrap_or_else(|error| json!({"stop_confirmed":false,"error":error.to_string()}));
        if cancelled || needs_reconciliation {
            let code = if cancelled {
                kiana_domain::CapabilityErrorCode::Cancelled
            } else {
                kiana_domain::CapabilityErrorCode::ResultUnknown
            };
            let mut result = CapabilityResult::failure_with_code(
                request.request.request_id,
                code,
                Some(if cancelled {
                    "mcp_stopped"
                } else {
                    "mcp_call_unconfirmed"
                }),
            );
            result.output["cancelled"] = json!(cancelled);
            result.output["stop_confirmed"] = json!(stop_confirmed);
            result.output["not_executed"] = json!(!started);
            result.output["workspace"] = json!(publication);
            result.output["diagnostics"] = diagnostics;
            let commit = if !started && stop_confirmed {
                AdapterCommitState::NotStarted
            } else {
                AdapterCommitState::Unknown
            };
            return kiana_domain::attach_adapter_result(result, AdapterResultKind::Mcp, commit)
                .map_err(|error| mcp_failed(format!("adapter_result_invalid:{error}")));
        }
        let (result, tools) = performed.map_err(|error| {
            if started {
                mcp_failed(format!("result_unknown:mcp_call_unconfirmed:{error}"))
            } else {
                error
            }
        })?;
        if discovering {
            let output = self
                .registry
                .commit_discovery(&request.request, pin, tools, protocol)
                .await?;
            let mut response =
                CapabilityResult::success(request.request.request_id, output.clone());
            if let Some(reference) = output["evidence_ref"].as_str() {
                response.evidence_refs.push(reference.to_owned());
            }
            return kiana_domain::attach_adapter_result(
                response,
                AdapterResultKind::Mcp,
                AdapterCommitState::Committed,
            )
            .map_err(|error| mcp_failed(format!("adapter_result_invalid:{error}")));
        }
        let failed = result["isError"] == true;
        let mut output = json!({"schema":MCP_RESULT_SCHEMA,"server":config.name,"tool":arguments["tool"],
            "server_info":protocol,"server_config_hash":snapshot["config_hash"],"catalog_digest":snapshot["catalog_digest"],
            "health":health_snapshot(&protocol, &tools),
            "input_schema_hash":kiana_domain::json_digest(&snapshot["tool"]["inputSchema"]),
            "output_schema_hash":snapshot["tool"].get("outputSchema").map(kiana_domain::json_digest),
            "untrusted_data":true,"resource_links_fetched":false,
            "diagnostics":diagnostics,"workspace":publication,"result":result});
        output
            .as_object_mut()
            .ok_or_else(|| mcp_failed("mcp_result_metadata_invalid"))?
            .extend(boundary);
        if failed {
            output["error"] = json!("execution_failed:mcp_tool_is_error");
        }
        let response = CapabilityResult {
            request_id: request.request.request_id,
            success: !failed,
            output,
            evidence_refs: Vec::new(),
        };
        kiana_domain::attach_adapter_result(
            response,
            AdapterResultKind::Mcp,
            AdapterCommitState::Committed,
        )
        .map_err(|error| mcp_failed(format!("adapter_result_invalid:{error}")))
    }
}

fn mcp_result_boundary(
    transport: &TransportType,
) -> Result<serde_json::Map<String, Value>, PortError> {
    let boundary = match transport {
        TransportType::Stdio => {
            json!({"transport":"stdio","containment":"bwrap","process_scope":"per_invocation"})
        }
        TransportType::Http | TransportType::Sse => json!({
            "transport": if *transport == TransportType::Sse { "sse" } else { "streamable_http" },
            "containment": "remote_unconfined",
            "process_scope": "remote_session",
            "remote_effect_locally_confined": false,
            "remote_stop_confirmed": false
        }),
        _ => return Err(mcp_failed("mcp_transport_unsupported")),
    };
    boundary
        .as_object()
        .cloned()
        .ok_or_else(|| mcp_failed("mcp_result_metadata_invalid"))
}

fn validate_advertised_tool(tool: &Value) -> Result<(), PortError> {
    if !tool.is_object()
        || tool["name"]
            .as_str()
            .is_none_or(|name| name.is_empty() || name.len() > 256)
    {
        return Err(mcp_failed("mcp_tool_invalid"));
    }
    if tool
        .get("description")
        .is_some_and(|value| !value.is_string())
    {
        return Err(mcp_failed("mcp_tool_description_invalid"));
    }
    if tool["inputSchema"]["type"] != "object" {
        return Err(mcp_failed("mcp_tool_schema_object_required"));
    }
    kiana_domain::validate_schema_contract(&tool["inputSchema"])
        .map_err(|_| mcp_failed("mcp_tool_schema_unsupported"))?;
    if let Some(output) = tool.get("outputSchema") {
        if output["type"] != "object" {
            return Err(mcp_failed("mcp_output_schema_object_required"));
        }
        kiana_domain::validate_schema_contract(output)
            .map_err(|_| mcp_failed("mcp_output_schema_unsupported"))?;
    }
    if tool
        .get("annotations")
        .is_some_and(|value| !value.is_object())
        || tool
            .get("execution")
            .is_some_and(|value| !value.is_object())
    {
        return Err(mcp_failed("mcp_tool_metadata_invalid"));
    }
    // Task-augmented operations require their own lifecycle; this invocation remains synchronous.
    if tool["execution"]["taskSupport"] == "required" {
        return Err(mcp_failed("mcp_async_task_unsupported"));
    }
    Ok(())
}
fn validate_call_result(result: &Value, tool: &Value) -> Result<(), PortError> {
    kiana_domain::validate_json_limits(result).map_err(|_| mcp_failed("mcp_result_limit"))?;
    if serde_json::to_vec(result)
        .map_err(|_| mcp_failed("mcp_result_invalid"))?
        .len()
        > MCP_MAX_RESULT_BYTES
    {
        return Err(mcp_failed("mcp_result_too_large"));
    }
    validate_mcp_result(result)?;
    if let Some(content) = result["content"].as_array() {
        if content.len() > 128 {
            return Err(mcp_failed("mcp_content_limit"));
        }
        for item in content {
            match item["type"].as_str() {
                Some("text") if item["text"].is_string() => (),
                Some("image" | "audio")
                    if item["data"].as_str().is_some_and(valid_base64)
                        && item["mimeType"]
                            .as_str()
                            .is_some_and(|value| value.len() <= 128 && value.contains('/')) =>
                {
                    ()
                }
                Some("resource_link")
                    if valid_resource_uri(&item["uri"]) && item["name"].is_string() =>
                {
                    ()
                }
                Some("resource")
                    if item["resource"].is_object()
                        && valid_resource_uri(&item["resource"]["uri"])
                        && (item["resource"]["text"].is_string()
                            || item["resource"]["blob"].as_str().is_some_and(valid_base64)) =>
                {
                    ()
                }
                _ => return Err(mcp_failed("mcp_content_invalid")),
            }
        }
    }
    if let Some(schema) = tool.get("outputSchema") {
        let structured = result
            .get("structuredContent")
            .ok_or_else(|| mcp_failed("mcp_structured_content_required"))?;
        kiana_domain::validate_schema_value(structured, schema)
            .map_err(|_| mcp_failed("mcp_output_schema_mismatch"))?;
    }
    Ok(())
}
fn valid_resource_uri(value: &Value) -> bool {
    value.as_str().is_some_and(|uri| {
        uri.len() <= 4096
            && !uri.chars().any(char::is_control)
            && uri.split_once(':').is_some_and(|(scheme, _)| {
                !scheme.is_empty()
                    && scheme
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
            })
    })
}
fn valid_base64(value: &str) -> bool {
    let unpadded = value.trim_end_matches('=');
    value.len() % 4 == 0
        && value.len() - unpadded.len() <= 2
        && unpadded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
}
fn catalog_digest(tools: &[Value], protocol: &Value) -> String {
    kiana_domain::json_digest(
        &json!({"schema_dialect":kiana_domain::TOOL_SCHEMA_DIALECT,"tools":tools,"protocol":protocol}),
    )
}

fn health_snapshot(protocol: &Value, tools: &[Value]) -> Value {
    json!({
        "status": "ready",
        "initialize": protocol.is_object(),
        "tools_list": true,
        "protocol_version": protocol["protocol_version"],
        "tool_count": tools.len()
    })
}

fn config_pin(config: &McpServerConfig, root: &Path) -> Result<Value, PortError> {
    // Provenance every trust/snapshot decision records: which server, which transport, and the
    // explicit environment/credential references only. Values are never copied here.
    let mut env_keys: Vec<String> = config
        .env
        .as_ref()
        .map(|env| env.keys().cloned().collect())
        .unwrap_or_default();
    env_keys.sort();
    if matches!(&config.transport, TransportType::Http | TransportType::Sse) {
        let url = config
            .url
            .as_deref()
            .ok_or_else(|| mcp_failed("mcp_http_url_required"))?;
        kiana_services::network_policy::validate_http_url(
            kiana_services::network_policy::HttpNetworkSurface::HttpMcp,
            url,
        )
        .map_err(|_| mcp_failed("mcp_http_endpoint_denied"))?;
        let header_names = config
            .headers
            .as_ref()
            .map(|headers| {
                let mut names = headers.keys().cloned().collect::<Vec<_>>();
                names.sort();
                names
            })
            .unwrap_or_default();
        let transport = if config.transport == TransportType::Sse {
            "sse"
        } else {
            "streamable_http"
        };
        return Ok(json!({
            "server_id": config.name,
            "transport": transport,
            "env_keys": env_keys,
            "credential_refs": header_names,
            "url": url,
            "endpoint_digest": kiana_domain::json_digest(&json!(url)),
            "header_names": header_names
        }));
    }
    let executable = file_pin(Path::new(
        config
            .command
            .as_deref()
            .ok_or_else(|| mcp_failed("mcp_command_required"))?,
    ))?;
    let mut files = Vec::new();
    for argument in config.args.as_ref().into_iter().flatten() {
        if argument.starts_with('-') || argument.is_empty() {
            continue;
        }
        let path = if Path::new(argument).is_absolute() {
            PathBuf::from(argument)
        } else {
            root.join(argument)
        };
        if path.is_file() {
            let canonical = path
                .canonicalize()
                .map_err(|_| mcp_failed("mcp_config_file_unavailable"))?;
            if let Ok(relative) = canonical.strip_prefix(root) {
                if relative.components().any(|part| {
                    part.as_os_str()
                        .to_str()
                        .is_none_or(crate::harness_sandbox::private_component)
                }) {
                    return Err(mcp_failed("mcp_config_private_file_denied"));
                }
            } else if !["/usr", "/bin", "/lib", "/lib64"]
                .iter()
                .any(|directory| canonical.starts_with(directory))
            {
                return Err(mcp_failed("mcp_argument_file_outside_scope"));
            }
            files.push(file_pin(&canonical)?);
        }
    }
    if files
        .iter()
        .filter_map(|pin| pin["bytes"].as_u64())
        .sum::<u64>()
        > 32 * 1024 * 1024
    {
        return Err(mcp_failed("mcp_argument_file_limit"));
    }
    Ok(json!({
        "server_id": config.name,
        "transport": "stdio",
        "env_keys": env_keys,
        "credential_refs": Vec::<String>::new(),
        "executable": executable,
        "argument_files": files
    }))
}

fn ensure_supported_transport(config: &McpServerConfig) -> Result<(), PortError> {
    if config.transport == TransportType::Stdio {
        Ok(())
    } else {
        Err(mcp_failed("mcp_transport_unsupported"))
    }
}
fn file_pin(path: &Path) -> Result<Value, PortError> {
    use std::io::Read;
    let path = path
        .canonicalize()
        .map_err(|_| mcp_failed("mcp_config_file_unavailable"))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options
        .open(&path)
        .map_err(|_| mcp_failed("mcp_config_file_unavailable"))?;
    if !file
        .metadata()
        .map_err(|_| mcp_failed("mcp_config_file_invalid"))?
        .is_file()
    {
        return Err(mcp_failed("mcp_config_file_invalid"));
    }
    let mut bytes = Vec::new();
    file.take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| mcp_failed("mcp_config_file_unavailable"))?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(mcp_failed("mcp_config_file_too_large"));
    }
    Ok(json!({"path":path,"bytes":bytes.len(),"sha256":kiana_domain::journal_sha256(&bytes)}))
}
fn resolve_command(command: &str) -> Result<PathBuf, PortError> {
    let path = Path::new(command);
    if path.is_absolute() {
        return path
            .canonicalize()
            .map_err(|_| mcp_failed("mcp_command_unavailable"));
    }
    if path.components().count() != 1 {
        return Err(mcp_failed("mcp_command_absolute_required"));
    }
    // Toolchains come from a fixed trusted search path, never a project's PATH or cwd.
    for directory in ["/usr/bin", "/bin", "/usr/local/bin"] {
        let candidate = Path::new(directory).join(command);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|_| mcp_failed("mcp_command_unavailable"));
        }
    }
    Err(mcp_failed("mcp_command_unavailable"))
}
fn mcp_failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}

#[cfg(test)]
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

#[cfg(test)]
fn validate_tool_schema(schema: &Value) -> Result<(), PortError> {
    validate_schema_node(schema, true, 0)
}

/// Validate the small, interoperable JSON-Schema subset that the daemon can
/// enforce locally. Unsupported combinators and malformed constraints are
/// rejected instead of being silently ignored by the authorization boundary.
#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn map_to_hash(map: &Map<String, Value>) -> HashMap<String, Value> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
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

/// Trusted startup-only configuration parse. It runs once, before any call, and rejects every
/// shape that would otherwise be re-read or reinterpreted at call time: duplicate/oversized
/// names, process fields on a remote transport, non-absolute or unresolvable stdio commands,
/// oversized arguments, and implicit credential/environment keys. Callers pass the raw
/// `KIANA_MCP_SERVERS_JSON` body; the function never reads the environment itself.
fn parse_servers(raw: &str) -> Result<Vec<McpServerConfig>, PortError> {
    let mut servers: Vec<McpServerConfig> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        if raw.len() > MCP_MAX_CONFIG_BYTES {
            return Err(mcp_failed("mcp_config_too_large"));
        }
        let value = kiana_domain::parse_bounded_json(raw.as_bytes())
            .map_err(|_| mcp_failed("mcp_config_invalid"))?;
        if value.is_array() {
            serde_json::from_value(value)
        } else {
            serde_json::from_value(value).map(|server| vec![server])
        }
        .map_err(|_| mcp_failed("mcp_config_invalid"))?
    };
    if servers.len() > 32 {
        return Err(mcp_failed("mcp_server_limit"));
    }
    let mut names = HashSet::new();
    for server in &mut servers {
        if server.name.trim().is_empty()
            || server.name.len() > 256
            || !names.insert(server.name.clone())
        {
            return Err(mcp_failed("mcp_server_name_invalid"));
        }
        match &server.transport {
            TransportType::Stdio => {
                if server.url.is_some()
                    || server
                        .headers
                        .as_ref()
                        .is_some_and(|headers| !headers.is_empty())
                {
                    return Err(mcp_failed("mcp_transport_config_invalid"));
                }
                let command = server
                    .command
                    .as_ref()
                    .filter(|c| !c.is_empty() && !c.contains('\0'))
                    .ok_or_else(|| mcp_failed("mcp_command_required"))?;
                let resolved = resolve_command(command)?;
                server.command = Some(resolved.to_string_lossy().into_owned());
            }
            TransportType::Http | TransportType::Sse => {
                let url = server
                    .url
                    .as_deref()
                    .ok_or_else(|| mcp_failed("mcp_http_url_required"))?;
                kiana_services::network_policy::validate_http_url(
                    kiana_services::network_policy::HttpNetworkSurface::HttpMcp,
                    url,
                )
                .map_err(|_| mcp_failed("mcp_http_endpoint_denied"))?;
                if server.command.is_some() || server.args.is_some() || server.env.is_some() {
                    return Err(mcp_failed("mcp_http_process_fields_invalid"));
                }
            }
            _ => return Err(mcp_failed("mcp_transport_unsupported")),
        }
        if server.args.as_ref().is_some_and(|args| {
            args.len() > 128
                || args
                    .iter()
                    .any(|arg| arg.len() > 16_384 || arg.contains('\0'))
        }) {
            return Err(mcp_failed("mcp_config_arguments_invalid"));
        }
        for (key, value) in server.env.as_ref().into_iter().flat_map(|env| env.iter()) {
            if !matches!(key.as_str(), "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ")
                || value.len() > 1024
                || value.contains('\0')
            {
                return Err(mcp_failed("mcp_config_env_not_supported"));
            }
        }
    }
    Ok(servers)
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
    fn cap29_http_receipt_never_claims_stdio_local_containment() {
        for (transport, expected_transport) in [
            (TransportType::Http, "streamable_http"),
            (TransportType::Sse, "sse"),
        ] {
            let boundary = mcp_result_boundary(&transport).expect("supported metadata");
            assert_eq!(boundary["transport"], expected_transport);
            assert_ne!(boundary["transport"], "stdio");
            assert_eq!(boundary["containment"], "remote_unconfined");
            assert_ne!(boundary["containment"], "bwrap");
            assert_eq!(boundary["process_scope"], "remote_session");
            assert_eq!(boundary["remote_effect_locally_confined"], false);
            assert_eq!(boundary["remote_stop_confirmed"], false);
            assert_eq!(boundary.len(), 5, "only explicit non-authorizing metadata");
        }
    }

    #[test]
    fn cap29_stdio_receipt_keeps_confined_process_boundary() {
        let boundary = mcp_result_boundary(&TransportType::Stdio).expect("stdio metadata");
        assert_eq!(
            json!(boundary),
            json!({
                "transport": "stdio",
                "containment": "bwrap",
                "process_scope": "per_invocation"
            })
        );
    }

    #[test]
    fn cap29_unsupported_transport_cannot_get_success_receipt_boundary() {
        assert!(matches!(
            mcp_result_boundary(&TransportType::Ws),
            Err(PortError::Failed(code)) if code == "mcp_transport_unsupported"
        ));
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
    fn http_and_sse_transports_are_rejected_before_connect() {
        for transport in [TransportType::Http, TransportType::Sse] {
            let config = McpServerConfig {
                name: "remote".to_owned(),
                transport,
                url: Some("https://example.invalid/mcp".to_owned()),
                command: None,
                args: None,
                env: None,
                headers: None,
            };
            assert!(matches!(
                ensure_supported_transport(&config),
                Err(PortError::Failed(message)) if message == "mcp_transport_unsupported"
            ));
        }
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

    // -----------------------------------------------------------------------------------
    // CAP-20 behavioural fixtures: trusted MCP configuration and discovery snapshot.
    //
    // These fixtures exercise the production boundary — `parse_servers` (the startup parse),
    // `McpRegistry::prepare`/`McpHandler::run` (snapshot binding and the deny paths) and the
    // real confined `ConfinedMcpClient` against a stdlib-only Python stdio server. They are
    // deny-first: an untrusted project, a swapped binary, an unknown server, an unsupported
    // transport and a cancelled discovery all fail before any process starts, while a
    // legitimate read-only discovery negotiates, lists tools and proves it has no implicit
    // host or network authority. A missing `bwrap`/`python3` fails the fixture instead of
    // degrading to an unsandboxed call; the CAP-20 workflow installs and verifies both.
    // -----------------------------------------------------------------------------------

    /// Minimal stdio MCP server used by the CAP-20 fixtures. It speaks single-line JSON-RPC over
    /// stdin/stdout using only the Python standard library, and exposes an `echo` tool plus a
    /// `probe_network` tool that reports whether an outbound TCP connection succeeds.
    #[cfg(target_os = "linux")]
    const CAP20_MCP_SERVER: &str = r##"#!/usr/bin/python3
import json
import socket
import sys

TOOLS = [
    {
        "name": "echo",
        "description": "Echo the supplied message",
        "inputSchema": {
            "type": "object",
            "properties": {"message": {"type": "string", "minLength": 1}},
            "required": ["message"],
            "additionalProperties": False,
        },
    },
    {
        "name": "probe_network",
        "description": "Report whether an outbound TCP connection succeeds",
        "inputSchema": {
            "type": "object",
            "properties": {"port": {"type": "integer"}},
            "required": ["port"],
            "additionalProperties": False,
        },
    },
]


def send(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def probe(port):
    sock = None
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(3)
        sock.connect(("127.0.0.1", int(port)))
        return "connected"
    except OSError:
        return "denied"
    finally:
        if sock is not None:
            sock.close()


while True:
    line = sys.stdin.readline()
    if not line:
        break
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "cap20-fixture", "version": "1.0.0"},
            },
        })
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": TOOLS}})
    elif method == "tools/call":
        params = message.get("params") or {}
        name = params.get("name")
        arguments = params.get("arguments") or {}
        if name == "echo":
            text = str(arguments.get("message", ""))
        elif name == "probe_network":
            text = probe(arguments.get("port", 0))
        else:
            send({
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": -32601, "message": "unknown tool"},
            })
            continue
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {"content": [{"type": "text", "text": text}]},
        })
    else:
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "error": {"code": -32601, "message": "unsupported"},
        })
"##;

    #[cfg(target_os = "linux")]
    fn cap20_temp_root() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-cap20-mcp-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// Write the stdio MCP fixture server so its canonical path is its own executable.
    #[cfg(target_os = "linux")]
    fn cap20_server(root: &Path) -> PathBuf {
        let path = root.join("cap20_mcp_server.py");
        std::fs::write(&path, CAP20_MCP_SERVER).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.canonicalize().unwrap()
    }

    #[cfg(target_os = "linux")]
    fn cap20_config(command: &Path) -> McpServerConfig {
        McpServerConfig {
            name: "cap20-fixture".to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some(command.to_string_lossy().into_owned()),
            args: None,
            env: Some(HashMap::from([("LANG".to_owned(), "C.UTF-8".to_owned())])),
            headers: None,
        }
    }

    #[cfg(target_os = "linux")]
    fn cap20_registry(servers: Vec<McpServerConfig>) -> Arc<McpRegistry> {
        McpRegistry::with_servers(servers, Arc::new(kiana_eventlog::MemoryEventLog::new()))
    }

    #[cfg(target_os = "linux")]
    fn cap20_context(root: &Path) -> RequestContext {
        let mut context = RequestContext::local("cap20", root.to_string_lossy().into_owned());
        context.project_trusted = true;
        context
    }

    #[cfg(target_os = "linux")]
    fn cap20_request(server: &str, root: &Path, context: &RequestContext) -> CapabilityRequest {
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Network,
            "mcp.discover",
            json!({
                "server": server,
                "operator_authorized": true,
                "actor_id": context.actor_id,
                "project_root": root.to_string_lossy(),
                "role_id": context.role_id,
                "department_id": context.department_id,
            }),
        )
    }

    #[cfg(target_os = "linux")]
    fn cap20_authorized(request: CapabilityRequest) -> AuthorizedCapabilityRequest {
        AuthorizedCapabilityRequest::new("policy:cap20-test", request).unwrap()
    }

    /// The startup parse is the single trusted configuration authority: it rejects duplicate
    /// names, the server cap, remote process fields and implicit environment/credential keys,
    /// and it resolves the executable from a fixed trusted path rather than the caller's PATH.
    #[cfg(target_os = "linux")]
    #[test]
    fn startup_config_parse_rejects_untrusted_shapes() {
        assert!(matches!(
            parse_servers(""),
            Ok(servers) if servers.is_empty()
        ));
        let duplicate = json!([
            { "name": "same", "transport": "stdio", "command": "/bin/sh" },
            { "name": "same", "transport": "stdio", "command": "/bin/sh" }
        ]);
        assert!(matches!(
            parse_servers(&duplicate.to_string()),
            Err(PortError::Failed(code)) if code == "mcp_server_name_invalid"
        ));
        let mut too_many = Vec::new();
        for index in 0..33 {
            let name = format!("s{index}");
            too_many.push(json!({ "name": name, "transport": "stdio", "command": "/bin/sh" }));
        }
        assert!(matches!(
            parse_servers(&Value::Array(too_many).to_string()),
            Err(PortError::Failed(code)) if code == "mcp_server_limit"
        ));
        let remote_with_process = json!({
            "name": "remote",
            "transport": "http",
            "url": "https://example.com/mcp",
            "command": "/bin/sh"
        });
        assert!(matches!(
            parse_servers(&remote_with_process.to_string()),
            Err(PortError::Failed(code)) if code == "mcp_http_process_fields_invalid"
        ));
        let implicit_credential = json!({
            "name": "stdio",
            "transport": "stdio",
            "command": "/bin/sh",
            "env": { "API_TOKEN": "secret" }
        });
        assert!(matches!(
            parse_servers(&implicit_credential.to_string()),
            Err(PortError::Failed(code)) if code == "mcp_config_env_not_supported"
        ));
        let unresolvable = json!({
            "name": "stdio",
            "transport": "stdio",
            "command": "definitely-not-a-real-tool"
        });
        assert!(matches!(
            parse_servers(&unresolvable.to_string()),
            Err(PortError::Failed(code)) if code == "mcp_command_unavailable"
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn untrusted_or_swapped_mcp_binary_cannot_start() {
        let root = cap20_temp_root();
        let script = cap20_server(&root);

        // An untrusted project never reaches a process: the confined client refuses first.
        let scope = json!({
            "project_trusted": false,
            "project_root": root.to_string_lossy(),
            "sandbox": "read-only",
            "path_allow": ["."],
            "mcp_snapshot": config_pin(&cap20_config(&script), &root).unwrap(),
        });
        let (_sender, cancellation) = watch::channel(false);
        let refused = crate::mcp_stdio::ConfinedMcpClient::connect(
            &cap20_config(&script),
            &scope,
            cancellation.clone(),
        )
        .await;
        assert!(matches!(
            refused,
            Err(PortError::Failed(code)) if code == "mcp_project_untrusted"
        ));

        // A trusted project whose pinned binary changed after preparation is refused too.
        let registry = cap20_registry(vec![cap20_config(&script)]);
        let context = cap20_context(&root);
        let prepared = registry
            .prepare(&context, &cap20_request("cap20-fixture", &root, &context))
            .await
            .expect("discovery prepare pins the executable");
        assert_eq!(
            prepared.arguments["mcp_snapshot"]["schema"],
            "kiana.mcp-snapshot.v1"
        );
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        let handler = McpHandler {
            registry: registry.clone(),
        };
        let denied = handler.run(cap20_authorized(prepared), cancellation).await;
        assert!(matches!(
            denied,
            Err(PortError::Failed(code)) if code == "mcp_config_changed"
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn unknown_server_or_ambiguous_tool_is_rejected() {
        let root = cap20_temp_root();
        let script = cap20_server(&root);
        let registry = cap20_registry(vec![cap20_config(&script)]);
        let context = cap20_context(&root);

        let refused = registry
            .prepare(&context, &cap20_request("not-configured", &root, &context))
            .await;
        assert!(matches!(
            refused,
            Err(PortError::Failed(code)) if code == "mcp_server_unknown"
        ));

        let echo = advertised_tool("echo", json!({ "type": "object" }));
        assert!(matches!(
            select_advertised_tool(&[echo.clone(), echo], "echo"),
            Err(PortError::Failed(code)) if code == "mcp_tool_ambiguous"
        ));
        assert!(matches!(
            select_advertised_tool(
                &[advertised_tool("echo", json!({ "type": "object" }))],
                "missing"
            ),
            Err(PortError::Failed(code)) if code == "mcp_tool_unknown"
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn discovery_has_no_implicit_host_or_network_authority() {
        let root = cap20_temp_root();
        let script = cap20_server(&root);
        let context = cap20_context(&root);

        // A remote transport is refused, never silently downgraded to a local call.
        let remote = McpServerConfig {
            name: "remote".to_owned(),
            transport: TransportType::Http,
            url: Some("https://example.com/mcp".to_owned()),
            command: None,
            args: None,
            env: None,
            headers: None,
        };
        for transport in [TransportType::Http, TransportType::Sse, TransportType::Ws] {
            let mut server = remote.clone();
            server.transport = transport;
            assert!(matches!(
                ensure_supported_transport(&server),
                Err(PortError::Failed(code)) if code == "mcp_transport_unsupported"
            ));
        }
        let registry = cap20_registry(vec![remote]);
        assert!(matches!(
            registry
                .prepare(&context, &cap20_request("remote", &root, &context))
                .await,
            Err(PortError::Failed(code)) if code == "mcp_transport_unsupported"
        ));

        // A cancelled discovery never starts the pinned process.
        let (_sender, cancellation) = watch::channel(true);
        let scope = json!({
            "project_trusted": true,
            "project_root": root.to_string_lossy(),
            "sandbox": "read-only",
            "path_allow": ["."],
            "timeout_ms": 60000,
            "mcp_snapshot": config_pin(&cap20_config(&script), &root).unwrap(),
        });
        let cancelled = crate::mcp_stdio::ConfinedMcpClient::connect(
            &cap20_config(&script),
            &scope,
            cancellation,
        )
        .await;
        assert!(matches!(
            cancelled,
            Err(PortError::Failed(code)) if code == "cancelled:mcp_not_started"
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn stdio_fixture_negotiates_lists_tools_and_has_no_network() {
        let root = cap20_temp_root();
        let script = cap20_server(&root);
        let config = cap20_config(&script);
        let pin = config_pin(&config, &root).expect("config pin");
        assert_eq!(pin["server_id"], "cap20-fixture");
        assert_eq!(pin["transport"], "stdio");
        assert_eq!(pin["env_keys"], json!(["LANG"]));
        assert_eq!(pin["credential_refs"], json!([]));
        let scope = json!({
            "project_trusted": true,
            "project_root": root.to_string_lossy(),
            "sandbox": "read-only",
            "path_allow": ["."],
            "timeout_ms": 60000,
            "mcp_snapshot": pin,
        });
        let (_sender, cancellation) = watch::channel(false);
        let mut client =
            crate::mcp_stdio::ConfinedMcpClient::connect(&config, &scope, cancellation)
                .await
                .expect("confined stdio MCP fixture must negotiate");
        assert_eq!(client.server_info["protocol_version"], "2025-06-18");

        let tools = client.list_tools().await.expect("tools/list");
        for tool in &tools {
            validate_advertised_tool(tool).expect("advertised tool passes the boundary");
        }
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(names, vec!["echo", "probe_network"], "{tools:?}");

        let echoed = client
            .call_tool("echo", json!({ "message": "cap20-ok" }))
            .await
            .expect("tools/call echo");
        assert_eq!(echoed["content"][0]["text"], "cap20-ok");

        // The confined discovery has no implicit host or network authority: a real outbound
        // connection to a host listener is denied even though the tool itself runs.
        let host = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("host listener");
        let port = host.local_addr().unwrap().port();
        let probe = client
            .call_tool("probe_network", json!({ "port": port }))
            .await
            .expect("tools/call probe_network");
        assert_eq!(probe["content"][0]["text"], "denied");

        let stopped = client.stop().await.expect("confirmed stop");
        assert_eq!(stopped["stop_confirmed"], true);

        let _ = std::fs::remove_dir_all(root);
    }

    // -----------------------------------------------------------------------------------
    // CAP-21 behavioural fixtures: bounded stdio MCP protocol and stop lifecycle.
    //
    // These fixtures drive the production `ConfinedMcpClient` — the same bwrap-confined,
    // per-invocation stdio transport that `McpHandler` uses — against a stdlib-only Python
    // fixture server that selects an adversarial behaviour per fixture. They are deny-first: an
    // unread stdin, a foreign or duplicated response id, an invalid envelope, a repeated
    // pagination cursor, a notification/byte flood and any unsupported server-initiated request
    // all fail closed. The regression fixtures then cover bounded notifications, fragmented
    // frames, paginated tool lists, initialize failure, EOF, child crash, stderr diagnostics and
    // cancellation, and the handler-level fixture proves that a sent business call without a
    // response is reported as Unknown instead of being replayed. Every exit path stops the
    // confined child, and the host proves that no marked descendant survives it. A missing
    // `bwrap`/`python3` fails the fixture instead of degrading to an unsandboxed call; the
    // CAP-21 workflow installs and verifies both.
    // -----------------------------------------------------------------------------------

    #[cfg(target_os = "linux")]
    use std::time::Duration;

    /// Minimal stdio MCP server used by the CAP-21 fixtures. It speaks single-line JSON-RPC over
    /// stdin/stdout using only the Python standard library, takes a per-fixture marker as
    /// `sys.argv[1]` (the host scans for surviving descendants) and selects one adversarial
    /// behaviour from `sys.argv[2]`. The `cap21-trace.jsonl` journal is written relative to the
    /// working directory: the read-only boundary rejects it, while the writable fixture scope
    /// records what the confined server actually received.
    #[cfg(target_os = "linux")]
    const CAP21_MCP_SERVER: &str = r##"#!/usr/bin/python3
import json
import os
import sys
import time

MARKER = sys.argv[1] if len(sys.argv) > 1 else "cap21"
MODE = sys.argv[2] if len(sys.argv) > 2 else "echo"

ECHO_TOOL = {
    "name": "echo",
    "description": "Echo the supplied message",
    "inputSchema": {
        "type": "object",
        "properties": {"message": {"type": "string", "minLength": 1}},
        "required": ["message"],
        "additionalProperties": False,
    },
}

PAGE_ORDER = ["gamma", "beta", "alpha"]
PAGE_TOOLS = {
    name: dict(ECHO_TOOL, name=name, description="Paged tool " + name) for name in PAGE_ORDER
}


def trace(method):
    try:
        with open("cap21-trace.jsonl", "a", encoding="utf-8") as handle:
            handle.write(str(method) + "\n")
            handle.flush()
    except OSError:
        pass


def send(payload, chunks=1):
    text = json.dumps(payload) + "\n"
    if chunks <= 1:
        sys.stdout.write(text)
        sys.stdout.flush()
        return
    size = max(1, len(text) // chunks)
    for index in range(0, len(text), size):
        sys.stdout.write(text[index:index + size])
        sys.stdout.flush()
        time.sleep(0.02)


def notify():
    return {
        "jsonrpc": "2.0",
        "method": "notifications/message",
        "params": {"level": "info", "data": MARKER},
    }


def read_message():
    while True:
        line = sys.stdin.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            continue
        try:
            return json.loads(line)
        except ValueError:
            continue


def initialize(message):
    if MODE == "initialize_version":
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "protocolVersion": "1999-01-01",
                "capabilities": {},
                "serverInfo": {"name": "cap21-fixture", "version": "1.0.0"},
            },
        })
        sys.exit(0)
    if MODE == "initialize_invalid":
        send({"jsonrpc": "2.0", "id": message["id"],
              "result": {"protocolVersion": "2025-06-18"}})
        sys.exit(0)
    if MODE == "bad_version":
        send({
            "jsonrpc": "1.0",
            "id": message["id"],
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": {"name": "cap21-fixture", "version": "1.0.0"},
            },
        })
        sys.exit(0)
    if MODE == "bad_envelope":
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "error": {"code": -32600, "message": "both result and error"},
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": {"name": "cap21-fixture", "version": "1.0.0"},
            },
        })
        sys.exit(0)
    if MODE == "eof":
        sys.stdout.close()
        sys.exit(0)
    send({
        "jsonrpc": "2.0",
        "id": message["id"],
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "cap21-fixture", "version": "1.0.0"},
        },
    }, chunks=3 if MODE == "fragments" else 1)
    if MODE == "eof_after_init":
        # Give the client time to write notifications/initialized into the pipe before closing
        # stdout, so the failure is a closed stream and not a racing write error.
        time.sleep(0.15)
        sys.stdout.close()
        sys.exit(0)


def tools_list(message):
    if MODE == "foreign_id":
        send({"jsonrpc": "2.0", "id": message["id"] + 1000, "result": {"tools": [ECHO_TOOL]}})
    elif MODE == "duplicate_id":
        response = {"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [ECHO_TOOL]}}
        send(response)
        send(response)
    elif MODE == "cursor_cycle":
        send({"jsonrpc": "2.0", "id": message["id"],
              "result": {"tools": [ECHO_TOOL], "nextCursor": "loop"}})
    elif MODE == "flood":
        flood = "".join(json.dumps(notify()) + "\n" for _ in range(200))
        sys.stdout.write(flood)
        sys.stdout.flush()
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [ECHO_TOOL]}})
    elif MODE == "oversized_frame":
        sys.stdout.write("x" * 600000 + "\n")
        sys.stdout.flush()
    elif MODE == "odd_notification":
        send({"jsonrpc": "2.0", "method": "cap21/unsolicited", "params": {}})
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [ECHO_TOOL]}})
    elif MODE == "list_changed":
        send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [ECHO_TOOL]}})
    elif MODE == "paginate":
        cursor = (message.get("params") or {}).get("cursor")
        index = 0 if cursor is None else {"p1": 1, "p2": 2}.get(cursor, len(PAGE_ORDER))
        if index >= len(PAGE_ORDER):
            send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": []}})
            return
        page = {"tools": [PAGE_TOOLS[PAGE_ORDER[index]]]}
        if index + 1 < len(PAGE_ORDER):
            page["nextCursor"] = "p1" if index == 0 else "p2"
        send({"jsonrpc": "2.0", "id": message["id"], "result": page})
    elif MODE == "page_limit":
        cursor = (message.get("params") or {}).get("cursor")
        index = int(cursor) if cursor else 0
        tool = dict(ECHO_TOOL, name="tool%03d" % (index + 1))
        send({"jsonrpc": "2.0", "id": message["id"],
              "result": {"tools": [tool], "nextCursor": str(index + 1)}})
    elif MODE == "tool_count_limit":
        tools = [dict(ECHO_TOOL, name="t%04d" % index, description="")
                 for index in range(1025)]
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": tools}})
    elif MODE == "fragments":
        for _ in range(3):
            send(notify())
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [ECHO_TOOL]}},
             chunks=3)
    else:
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [ECHO_TOOL]}})


def tools_call(message):
    params = message.get("params") or {}
    if MODE == "server_requests":
        answers = {}
        for request_id, method in ((7, "sampling/createMessage"), (8, "roots/list"),
                                   (9, "elicitation/create")):
            send({"jsonrpc": "2.0", "id": request_id, "method": method,
                  "params": {"cap21_marker": MARKER}})
            reply = read_message()
            while reply is not None and reply.get("id") != request_id:
                reply = read_message()
            answers[method] = reply
        send({"jsonrpc": "2.0", "id": message["id"], "result": {
            "content": [{"type": "text", "text": json.dumps(answers)}]}})
        return
    if MODE in ("silent", "read_then_stall"):
        time.sleep(600)
        return
    arguments = params.get("arguments") or {}
    if params.get("name") == "echo":
        text = str(arguments.get("message", ""))
    else:
        text = str(params.get("name", ""))
    send({"jsonrpc": "2.0", "id": message["id"],
          "result": {"content": [{"type": "text", "text": text}]}})


def main():
    if MODE == "stderr":
        sys.stderr.write("cap21-stderr-" + MARKER + "\n")
        sys.stderr.flush()
    while True:
        message = read_message()
        if message is None:
            break
        method = message.get("method")
        if MODE == "crash" and method not in ("initialize", "notifications/initialized"):
            os._exit(3)
        if method == "initialize":
            initialize(message)
        elif method == "notifications/initialized":
            continue
        elif method == "tools/list":
            trace("tools/list")
            tools_list(message)
            if MODE == "read_then_stall":
                time.sleep(600)
        elif method == "tools/call":
            trace("tools/call")
            tools_call(message)
        elif method is not None:
            send({"jsonrpc": "2.0", "id": message.get("id"),
                  "error": {"code": -32601, "message": "unsupported"}})


main()
"##;

    #[cfg(target_os = "linux")]
    fn cap21_temp_root() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-cap21-mcp-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        // The handler fixture stages this root through `ExecutionWorkspace`, which compares the
        // canonical source with the caller-supplied workdir, so the fixture path must already be
        // canonical.
        root.canonicalize().unwrap()
    }

    /// Every confined fixture process carries a unique marker argument, so the host can prove
    /// that no descendant survives a stop path instead of trusting the child's own report.
    #[cfg(target_os = "linux")]
    fn cap21_marker(role: &str) -> String {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("cap21-{role}-{stamp}")
    }

    #[cfg(target_os = "linux")]
    fn cap21_server(root: &Path) -> PathBuf {
        let path = root.join("cap21_mcp_server.py");
        std::fs::write(&path, CAP21_MCP_SERVER).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.canonicalize().unwrap()
    }

    #[cfg(target_os = "linux")]
    fn cap21_config(command: &Path, marker: &str, mode: &str) -> McpServerConfig {
        McpServerConfig {
            name: "cap21-fixture".to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some(command.to_string_lossy().into_owned()),
            args: Some(vec![marker.to_owned(), mode.to_owned()]),
            env: None,
            headers: None,
        }
    }

    /// The confined execution scope. `read-only` keeps the transport as the only observable
    /// channel; `workspace-write` is used only where the host needs the server's own trace of
    /// what it received.
    #[cfg(target_os = "linux")]
    fn cap21_scope(root: &Path, sandbox: &str, timeout_ms: u64, pin: Value) -> Value {
        json!({
            "project_trusted": true,
            "project_root": root.to_string_lossy(),
            "sandbox": sandbox,
            "path_allow": ["."],
            "timeout_ms": timeout_ms,
            "mcp_snapshot": pin,
        })
    }

    #[cfg(target_os = "linux")]
    async fn cap21_start(
        config: &McpServerConfig,
        root: &Path,
        sandbox: &str,
        timeout_ms: u64,
        cancellation: watch::Receiver<bool>,
    ) -> Result<crate::mcp_stdio::ConfinedMcpClient, PortError> {
        let pin = config_pin(config, root).expect("fixture configuration pins");
        let scope = cap21_scope(root, sandbox, timeout_ms, pin);
        crate::mcp_stdio::ConfinedMcpClient::connect(config, &scope, cancellation).await
    }

    /// Build a capability request with the scope fields the ControlPlane injects in production.
    #[cfg(target_os = "linux")]
    fn cap21_request(
        operation: &str,
        root: &Path,
        context: &RequestContext,
        arguments: Value,
    ) -> CapabilityRequest {
        let mut arguments = arguments;
        arguments["actor_id"] = json!(context.actor_id);
        arguments["project_root"] = json!(root.to_string_lossy());
        arguments["role_id"] = json!(context.role_id);
        arguments["department_id"] = json!(context.department_id);
        arguments["project_trusted"] = json!(true);
        arguments["path_allow"] = json!(["."]);
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Network,
            operation,
            arguments,
        )
    }

    /// PIDs whose command line still carries the fixture marker. This is a bounded host
    /// observation of the current namespace, not a claim about unreachable descendants.
    #[cfg(target_os = "linux")]
    fn cap21_marker_pids(marker: &str) -> Vec<u32> {
        let mut pids = Vec::new();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return pids;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
                continue;
            };
            if cmdline
                .split(|byte| *byte == 0)
                .any(|part| part == marker.as_bytes())
            {
                pids.push(pid);
            }
        }
        pids
    }

    /// Positive control: the survivor check must be able to see a live fixture process, or the
    /// later "no survivor" assertion would be vacuous.
    #[cfg(target_os = "linux")]
    async fn cap21_assert_marker_running(marker: &str) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            if !cap21_marker_pids(marker).is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("fixture process {marker} was never observed as a live descendant");
    }

    #[cfg(target_os = "linux")]
    async fn cap21_assert_no_survivor(marker: &str, context: &str) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let survivors = cap21_marker_pids(marker);
            if survivors.is_empty() {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("{context}: {marker} descendants survived the stop path: {survivors:?}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Stop the confined child and prove it is gone: a transport failure or cancel is only a
    /// stop *request* until the supervisor confirms that no process from the group remains.
    #[cfg(target_os = "linux")]
    async fn cap21_stop(
        client: &mut crate::mcp_stdio::ConfinedMcpClient,
        marker: &str,
        context: &str,
    ) {
        let stopped = client
            .stop()
            .await
            .unwrap_or_else(|error| panic!("{context}: stop must be confirmed: {error}"));
        assert_eq!(stopped["stop_confirmed"], true, "{context}: {stopped}");
        cap21_assert_no_survivor(marker, context).await;
    }

    /// The fixture configuration is a legal startup configuration: the behaviour selector stays a
    /// plain argument, so no fixture-only config shape is being smuggled in through `with_servers`.
    #[cfg(target_os = "linux")]
    #[test]
    fn cap21_fixture_configuration_parses_at_startup() {
        let selector = McpServerConfig {
            name: "cap21-fixture".to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some("/bin/sh".to_owned()),
            args: Some(vec!["cap21-marker".to_owned(), "silent".to_owned()]),
            env: None,
            headers: None,
        };
        let raw = serde_json::to_string(&vec![selector]).expect("fixture config encodes");
        let parsed = parse_servers(&raw).expect("the fixture configuration parses at startup");
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].args.as_deref().unwrap_or_default().len(),
            2,
            "the fixture behaviour selector must stay a plain argument"
        );
    }

    /// Deny-first: an unread stdin must not block the transport, and a sent call that never gets
    /// a response must stay bounded and unreplayed.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_stdin_backpressure_is_bounded() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);

        // Phase 1: the fixture answers tools/list and then stops reading stdin, so a large
        // request can only fill the pipe. The write must fail at the transport deadline instead
        // of blocking the invocation.
        let stall_marker = cap21_marker("stall");
        let stall = cap21_config(&script, &stall_marker, "read_then_stall");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&stall, &root, "workspace-write", 1_500, cancellation)
            .await
            .expect("the stalling fixture completes the handshake");
        cap21_assert_marker_running(&stall_marker).await;
        client
            .list_tools()
            .await
            .expect("tools/list is answered before the stall");
        let started = std::time::Instant::now();
        let blocked = client
            .call_tool("echo", json!({ "message": "x".repeat(200_000) }))
            .await;
        let elapsed = started.elapsed();
        assert!(
            matches!(&blocked, Err(PortError::Failed(code)) if code == "mcp_write_timeout"),
            "an unread stdin must fail the write at the deadline: {blocked:?}"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "a blocked write must stay bounded by the deadline: {elapsed:?}"
        );
        cap21_stop(&mut client, &stall_marker, "unread stdin").await;
        let stalled_trace =
            std::fs::read_to_string(root.join("cap21-trace.jsonl")).expect("fixture trace");
        assert!(
            !stalled_trace.lines().any(|line| line == "tools/call"),
            "a write that never completed must not deliver a request: {stalled_trace}"
        );

        // Phase 2: the fixture reads the call and never answers it. The request fails at the
        // deadline and the trace proves the sent call was not replayed.
        let silent_marker = cap21_marker("silent");
        let silent = cap21_config(&script, &silent_marker, "silent");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&silent, &root, "workspace-write", 1_500, cancellation)
            .await
            .expect("the silent fixture completes the handshake");
        client.list_tools().await.expect("tools/list is answered");
        let started = std::time::Instant::now();
        let unanswered = client
            .call_tool("echo", json!({ "message": "cap21" }))
            .await;
        let elapsed = started.elapsed();
        assert!(
            matches!(&unanswered, Err(PortError::Failed(code)) if code == "mcp_response_timeout"),
            "an unanswered call must fail at the deadline: {unanswered:?}"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "an unanswered call must stay bounded: {elapsed:?}"
        );
        cap21_stop(&mut client, &silent_marker, "unanswered call").await;
        let trace = std::fs::read_to_string(root.join("cap21-trace.jsonl")).expect("fixture trace");
        assert_eq!(
            trace.lines().filter(|line| *line == "tools/call").count(),
            1,
            "a sent call must not be replayed: {trace}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    /// Deny-first: a foreign, duplicated or malformed response envelope is never accepted as this
    /// request's result.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_duplicate_or_foreign_response_id_is_rejected() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);

        let marker = cap21_marker("foreign-id");
        let config = cap21_config(&script, &marker, "foreign_id");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let foreign = client.list_tools().await;
        assert!(
            matches!(&foreign, Err(PortError::Failed(code)) if code == "mcp_response_id_mismatch"),
            "a foreign response id must be rejected: {foreign:?}"
        );
        cap21_stop(&mut client, &marker, "foreign response id").await;

        // A duplicated response is not a second result: the stale copy is rejected when the next
        // request reads it.
        let marker = cap21_marker("duplicate-id");
        let config = cap21_config(&script, &marker, "duplicate_id");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let first = client
            .list_tools()
            .await
            .expect("the first response is accepted");
        assert_eq!(first.len(), 1);
        let stale = client
            .call_tool("echo", json!({ "message": "cap21" }))
            .await;
        assert!(
            matches!(&stale, Err(PortError::Failed(code)) if code == "mcp_response_id_mismatch"),
            "a duplicated response must not answer a later request: {stale:?}"
        );
        cap21_stop(&mut client, &marker, "duplicated response id").await;

        // `result` and `error` are mutually exclusive, and the wire envelope must be JSON-RPC 2.0.
        let marker = cap21_marker("envelope");
        let config = cap21_config(&script, &marker, "bad_envelope");
        let (_sender, cancellation) = watch::channel(false);
        let envelope = cap21_start(&config, &root, "read-only", 30_000, cancellation).await;
        assert!(
            matches!(&envelope, Err(PortError::Failed(code)) if code == "mcp_response_envelope_invalid"),
            "result and error are mutually exclusive: {envelope:?}"
        );
        cap21_assert_no_survivor(&marker, "invalid result/error envelope").await;

        let marker = cap21_marker("version");
        let config = cap21_config(&script, &marker, "bad_version");
        let (_sender, cancellation) = watch::channel(false);
        let version = cap21_start(&config, &root, "read-only", 30_000, cancellation).await;
        assert!(
            matches!(&version, Err(PortError::Failed(code)) if code == "mcp_response_version_invalid"),
            "the wire envelope must be JSON-RPC 2.0: {version:?}"
        );
        cap21_assert_no_survivor(&marker, "invalid JSON-RPC version").await;

        let _ = std::fs::remove_dir_all(root);
    }

    /// Deny-first: unbounded pagination, notification floods and unmetered frames all fail closed,
    /// and a changed catalog invalidates the rest of the invocation.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_pagination_cycle_and_notification_flood_fail_closed() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);

        for (mode, marker_role, expected, context) in [
            (
                "cursor_cycle",
                "cursor-cycle",
                "mcp_cursor_cycle",
                "a repeated pagination cursor",
            ),
            (
                "flood",
                "flood",
                "mcp_notification_budget_exceeded",
                "a notification flood",
            ),
            (
                "oversized_frame",
                "oversized-frame",
                "mcp_response_byte_limit",
                "an oversized frame",
            ),
            (
                "odd_notification",
                "odd-notification",
                "mcp_notification_invalid",
                "an unsolicited method",
            ),
        ] {
            let marker = cap21_marker(marker_role);
            let config = cap21_config(&script, &marker, mode);
            let (_sender, cancellation) = watch::channel(false);
            let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
                .await
                .unwrap_or_else(|error| panic!("{context}: handshake failed: {error}"));
            let denied = client.list_tools().await;
            assert!(
                matches!(&denied, Err(PortError::Failed(code)) if code == expected),
                "{context} must fail closed with {expected}: {denied:?}"
            );
            cap21_stop(&mut client, &marker, context).await;
        }

        let marker = cap21_marker("list-changed");
        let config = cap21_config(&script, &marker, "list_changed");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let changed = client.list_tools().await;
        assert!(
            matches!(&changed, Err(PortError::Failed(code)) if code == "mcp_catalog_changed"),
            "list_changed must invalidate the catalog: {changed:?}"
        );
        let refused = client
            .call_tool("echo", json!({ "message": "cap21" }))
            .await;
        assert!(
            matches!(&refused, Err(PortError::Failed(code)) if code == "mcp_catalog_changed"),
            "a changed catalog must refuse further calls: {refused:?}"
        );
        cap21_stop(&mut client, &marker, "catalog change").await;

        let _ = std::fs::remove_dir_all(root);
    }

    /// Deny-first: server-initiated sampling/elicitation/roots requests are refused with a
    /// JSON-RPC error and never execute against a model or a host path.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_server_requests_cannot_bypass_control_plane() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);
        let marker = cap21_marker("server-requests");
        // The fixture sends the three unsupported server requests while a business call is in
        // flight and reports back, in band, exactly what the client answered.
        let config = cap21_config(&script, &marker, "server_requests");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let echoed = client
            .call_tool("echo", json!({ "message": "cap21" }))
            .await
            .expect("a refused server request must not fail the client's own call");
        let reported = echoed["content"][0]["text"]
            .as_str()
            .expect("the fixture reports the answers as text");
        let answers: Value = serde_json::from_str(reported).expect("answers are JSON");
        for (id, method) in [
            (7, "sampling/createMessage"),
            (8, "roots/list"),
            (9, "elicitation/create"),
        ] {
            let answer = &answers[method];
            assert_eq!(answer["jsonrpc"], "2.0", "{method}");
            assert_eq!(answer["id"], id, "{method}: the request id is echoed");
            assert!(
                answer.get("result").is_none(),
                "{method}: no result may be produced"
            );
            assert_eq!(answer["error"]["code"], -32601, "{method}");
            let message = answer["error"]["message"].as_str().unwrap_or_default();
            assert!(
                message.contains("disabled"),
                "{method}: unsupported server requests are refused, not implemented: {message}"
            );
        }
        assert!(
            !reported.contains(root.to_string_lossy().as_ref()),
            "a refusal must not leak the project path: {reported}"
        );
        assert!(
            !reported.contains("model"),
            "a refusal must not fabricate a model answer: {reported}"
        );
        cap21_stop(&mut client, &marker, "server-initiated requests").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Regression: bounded notifications are consumed while the response is awaited, and a
    /// response split across several writes is still one frame.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_bounded_notifications_and_fragmented_frames_are_parsed() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);
        let marker = cap21_marker("fragments");
        let config = cap21_config(&script, &marker, "fragments");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("a fragmented handshake is still a handshake");
        assert_eq!(client.server_info["protocol_version"], "2025-06-18");
        let tools = client.list_tools().await.expect("fragmented tools/list");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
        let echoed = client
            .call_tool("echo", json!({ "message": "cap21-fragments" }))
            .await
            .expect("tools/call");
        assert_eq!(echoed["content"][0]["text"], "cap21-fragments");
        cap21_stop(&mut client, &marker, "fragmented frames").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Regression and bound: three cursored pages become one sorted catalog, while an unlimited
    /// page count or tool count is refused.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_paginated_tool_list_is_bounded() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);

        let marker = cap21_marker("paginate");
        let config = cap21_config(&script, &marker, "paginate");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let tools = client.list_tools().await.expect("paginated tools/list");
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
        cap21_stop(&mut client, &marker, "paginated catalog").await;

        let marker = cap21_marker("page-limit");
        let config = cap21_config(&script, &marker, "page_limit");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let pages = client.list_tools().await;
        assert!(
            matches!(&pages, Err(PortError::Failed(code)) if code == "mcp_tool_catalog_page_limit"),
            "the page count must be bounded: {pages:?}"
        );
        cap21_stop(&mut client, &marker, "pagination page limit").await;

        let marker = cap21_marker("tool-count");
        let config = cap21_config(&script, &marker, "tool_count_limit");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let catalog = client.list_tools().await;
        assert!(
            matches!(&catalog, Err(PortError::Failed(code)) if code == "mcp_tool_catalog_limit"),
            "the tool count must be bounded: {catalog:?}"
        );
        cap21_stop(&mut client, &marker, "tool count limit").await;

        let _ = std::fs::remove_dir_all(root);
    }

    /// Regression: every non-success exit path of the handshake and of an established transport
    /// still stops the confined child before it returns.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_initialize_failure_eof_and_child_crash_all_clean_up() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);

        for (mode, marker_role, expected, context) in [
            (
                "initialize_version",
                "init-version",
                "mcp_protocol_version_unsupported",
                "an unsupported protocol version",
            ),
            (
                "initialize_invalid",
                "init-invalid",
                "mcp_initialize_invalid",
                "an incomplete initialize result",
            ),
            (
                "eof",
                "eof",
                "mcp_stream_closed",
                "a stream closed during initialize",
            ),
        ] {
            let marker = cap21_marker(marker_role);
            let config = cap21_config(&script, &marker, mode);
            let (_sender, cancellation) = watch::channel(false);
            let failed = cap21_start(&config, &root, "read-only", 30_000, cancellation).await;
            assert!(
                matches!(&failed, Err(PortError::Failed(code)) if code == expected),
                "{context} must fail closed with {expected}: {failed:?}"
            );
            cap21_assert_no_survivor(&marker, context).await;
        }

        for (mode, marker_role, context) in [
            (
                "eof_after_init",
                "eof-after-init",
                "the child closed stdout after initialize",
            ),
            ("crash", "crash", "the child crashed during tools/list"),
        ] {
            let marker = cap21_marker(marker_role);
            let config = cap21_config(&script, &marker, mode);
            let (_sender, cancellation) = watch::channel(false);
            let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
                .await
                .unwrap_or_else(|error| panic!("{context}: handshake failed: {error}"));
            let lost = client.list_tools().await;
            assert!(
                matches!(&lost, Err(PortError::Failed(code)) if code == "mcp_stream_closed"),
                "{context}: {lost:?}"
            );
            cap21_stop(&mut client, &marker, context).await;
        }

        let _ = std::fs::remove_dir_all(root);
    }

    /// Regression: stderr is retained only as bounded, digested diagnostics.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_stderr_diagnostics_are_bounded_evidence() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);
        let marker = cap21_marker("stderr");
        let config = cap21_config(&script, &marker, "stderr");
        let (_sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        client.list_tools().await.expect("tools/list");
        let stopped = client.stop().await.expect("confirmed stop");
        assert_eq!(stopped["stop_confirmed"], true);
        let retained = stopped["stderr_bytes_retained"].as_u64().unwrap_or(0);
        assert!(
            retained > 0,
            "stderr diagnostics must be retained: {stopped}"
        );
        assert_eq!(stopped["stderr_truncated"], false);
        let digest = stopped["stderr_digest"].as_str().unwrap_or_default();
        assert_eq!(digest.len(), 64, "stderr evidence is a digest: {stopped}");
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(
            !stopped.to_string().contains("cap21-stderr"),
            "diagnostics must not echo raw stderr: {stopped}"
        );
        cap21_assert_no_survivor(&marker, "stderr diagnostics").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Regression: cancellation stops the in-flight request, refuses new business work until the
    /// transport is stopped, and never leaks the confined child.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_cancelled_request_never_leaves_the_child_running() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);
        let marker = cap21_marker("cancel");
        let config = cap21_config(&script, &marker, "silent");
        let (sender, cancellation) = watch::channel(false);
        let mut client = cap21_start(&config, &root, "read-only", 30_000, cancellation)
            .await
            .expect("handshake");
        let trigger = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = sender.send(true);
        });
        let cancelled = client
            .call_tool("echo", json!({ "message": "cap21" }))
            .await;
        trigger.await.expect("cancel trigger");
        assert!(
            matches!(&cancelled, Err(PortError::Failed(code)) if code == "cancelled:mcp_request_stopped"),
            "a cancelled request must stop instead of retrying: {cancelled:?}"
        );
        let refused = client
            .call_tool("echo", json!({ "message": "cap21" }))
            .await;
        assert!(
            matches!(&refused, Err(PortError::Failed(code)) if code == "cancelled:mcp_not_sent"),
            "a cancelled transport must refuse new requests: {refused:?}"
        );
        cap21_stop(&mut client, &marker, "cancelled request").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// A sent business call that never gets a response keeps the Unknown classification through
    /// the real handler, retains its private workspace for reconciliation, and is not re-issued.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_unanswered_call_stays_unknown_and_is_not_replayed() {
        let root = cap21_temp_root();
        let script = cap21_server(&root);
        let marker = cap21_marker("unanswered");
        let config = cap21_config(&script, &marker, "silent");
        let registry = cap20_registry(vec![config]);
        let context = cap20_context(&root);
        let handler = McpHandler {
            registry: registry.clone(),
        };
        let (_sender, cancellation) = watch::channel(false);

        let discovery = cap21_request(
            "mcp.discover",
            &root,
            &context,
            json!({ "server": "cap21-fixture", "operator_authorized": true }),
        );
        let prepared = registry
            .prepare(&context, &discovery)
            .await
            .expect("discovery preparation pins the configured server");
        let discovered = handler
            .run(cap20_authorized(prepared), cancellation.clone())
            .await
            .expect("discovery commits a catalog snapshot");
        assert!(discovered.success);

        let call = cap21_request(
            MCP_OPERATION,
            &root,
            &context,
            json!({
                "server": "cap21-fixture",
                "tool": "echo",
                "arguments": { "message": "cap21" },
                "sandbox": "workspace-write",
                "timeout_ms": 3_000,
            }),
        );
        let prepared = registry
            .prepare(&context, &call)
            .await
            .expect("call preparation binds the discovery snapshot");
        let result = handler
            .run(cap20_authorized(prepared), cancellation)
            .await
            .expect("an unanswered call is classified instead of becoming an I/O error");
        assert!(!result.success);
        assert_eq!(
            result.failure_code(),
            Some(kiana_domain::CapabilityErrorCode::ResultUnknown),
            "a sent call without a response stays Unknown: {:?}",
            result.output
        );
        assert_eq!(result.output["not_executed"], json!(false));
        assert_eq!(result.output["stop_confirmed"], json!(true));
        let envelope = kiana_domain::AdapterResult::from_json(&result.output["adapter_result"])
            .expect("the adapter envelope is valid");
        assert_eq!(envelope.adapter, AdapterResultKind::Mcp);
        assert_eq!(envelope.commit, AdapterCommitState::Unknown);
        assert_eq!(
            envelope.effect,
            kiana_domain::CapabilityEffectState::Unknown
        );
        assert!(envelope.reconciliation_required);

        // The private workspace is retained for reconciliation instead of being published.
        let publication = &result.output["workspace"];
        assert_eq!(publication["retained"], json!(true));
        assert_eq!(publication["reason"], json!("mcp_call_unconfirmed"));
        let staging = PathBuf::from(
            publication["workspace"]
                .as_str()
                .expect("the retained staging root is reported"),
        );
        let record = std::fs::read_to_string(staging.join("reconciliation.json"))
            .expect("reconciliation record");
        assert!(record.contains("mcp_call_unconfirmed"), "{record}");

        // Exactly one business call reached the server: an unanswered call is never re-issued.
        let trace = std::fs::read_to_string(staging.join("workspace").join("cap21-trace.jsonl"))
            .expect("fixture trace");
        assert_eq!(
            trace.lines().filter(|line| *line == "tools/call").count(),
            1,
            "a sent call must not be replayed: {trace}"
        );

        cap21_assert_no_survivor(&marker, "unanswered call").await;
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(staging);
    }

    // -----------------------------------------------------------------------------------
    // CAP-22 behavioural fixtures: MCP result/schema drift and per-invocation isolation.
    //
    // These fixtures drive the real `McpRegistry::prepare` + `McpHandler::run` path and the
    // same bwrap-confined, per-invocation stdio transport the product uses, against a
    // stdlib-only Python server (`CAP22_MCP_SERVER`) whose catalog generation is written to
    // `cap22-catalog.json` and whose behaviour is selected by `sys.argv[2]`. They are
    // deny-first: an invalid structured result after a sent call stays Unknown and is not
    // reissued; catalog drift invalidates a pending authorization before any process starts;
    // a discovery key and prepared scope never cross owner or server, and two owners run in
    // two simultaneous confined processes instead of one pooled session; config/env/binary
    // drift and untrusted projects are refused before dispatch. The regression fixtures then
    // cover legal structured/`isError`/resource-link results and the result validator's own
    // bounds. A missing `bwrap`/`python3` fails the fixture instead of degrading to an
    // unsandboxed call; the CAP-22 workflow installs and verifies both.
    // -----------------------------------------------------------------------------------

    /// Minimal stdio MCP server used by the CAP-22 fixtures. It speaks single-line JSON-RPC
    /// over stdin/stdout using only the Python standard library, takes a per-fixture marker as
    /// `sys.argv[1]` (the host scans for surviving descendants) and selects one behaviour from
    /// `sys.argv[2]`. `cap22-catalog.json` in the working directory selects catalog generation
    /// 1 or 2, so re-discovery can commit a drifted catalog under the same discovery key. The
    /// `cap22-trace.jsonl` journal is written relative to the working directory: the read-only
    /// boundary rejects it, while the writable fixture scope records what the confined server
    /// actually received.
    #[cfg(target_os = "linux")]
    const CAP22_MCP_SERVER: &str = r##"#!/usr/bin/python3
import json
import sys
import time

MARKER = sys.argv[1] if len(sys.argv) > 1 else "cap22"
MODE = sys.argv[2] if len(sys.argv) > 2 else "normal"


def catalog_generation():
    try:
        with open("cap22-catalog.json", "r", encoding="utf-8") as handle:
            payload = json.load(handle)
        generation = payload.get("generation", 1)
    except (OSError, ValueError):
        generation = 1
    return generation if generation in (1, 2) else 1


GENERATION = catalog_generation()
SUFFIX = "|v" + str(GENERATION)

ECHO_TOOL = {
    "name": "echo",
    "description": "Echo v1" if GENERATION == 1 else "Echo v2",
    "inputSchema": {
        "type": "object",
        "properties": (
            {"message": {"type": "string", "minLength": 1}}
            if GENERATION == 1
            else {"message": {"type": "string", "minLength": 1}, "note": {"type": "string"}}
        ),
        "required": ["message"],
        "additionalProperties": False,
    },
}
STRUCTURED_TOOL = {
    "name": "structured",
    "description": "Return structured content",
    "inputSchema": {
        "type": "object",
        "properties": {"message": {"type": "string"}},
        "additionalProperties": False,
    },
    "outputSchema": {
        "type": "object",
        "properties": {"value": {"type": "string"}},
        "required": ["value"],
        "additionalProperties": False,
    },
}
ERROR_TOOL = {
    "name": "error_tool",
    "description": "Report a tool business failure",
    "inputSchema": {
        "type": "object",
        "properties": {"message": {"type": "string"}},
        "additionalProperties": False,
    },
}
LINK_TOOL = {
    "name": "link_tool",
    "description": "Return a resource link without fetching it",
    "inputSchema": {
        "type": "object",
        "properties": {"message": {"type": "string"}},
        "additionalProperties": False,
    },
}
# Deliberately unsorted: the client sorts the catalog, the server does not.
TOOLS = [LINK_TOOL, ERROR_TOOL, STRUCTURED_TOOL, ECHO_TOOL]


def trace(method):
    try:
        with open("cap22-trace.jsonl", "a", encoding="utf-8") as handle:
            handle.write(str(method) + "\n")
            handle.flush()
    except OSError:
        pass


def send(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def read_message():
    while True:
        line = sys.stdin.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            continue
        try:
            return json.loads(line)
        except ValueError:
            continue


def initialize(message):
    send({
        "jsonrpc": "2.0",
        "id": message["id"],
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "cap22-fixture", "version": "1.0.0"},
        },
    })


def tools_call(message):
    params = message.get("params") or {}
    name = params.get("name")
    arguments = params.get("arguments") or {}
    if MODE == "slow_call":
        time.sleep(1.0)
    if name == "error_tool":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {
            "content": [{"type": "text", "text": "execution_failed:cap22"}],
            "isError": True,
        }})
        return
    if name == "link_tool":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {
            "content": [{"type": "resource_link", "uri": "cap22://note/1", "name": "cap22-note"}],
        }})
        return
    if name == "structured":
        if MODE == "structured_missing":
            send({"jsonrpc": "2.0", "id": message["id"], "result": {
                "content": [{"type": "text", "text": "ok"}],
            }})
        elif MODE == "structured_wrong":
            send({"jsonrpc": "2.0", "id": message["id"], "result": {
                "content": [{"type": "text", "text": "ok"}],
                "structuredContent": {"value": 7},
            }})
        elif MODE == "bad_content":
            send({"jsonrpc": "2.0", "id": message["id"], "result": {
                "content": [{"type": "image", "data": "not base64!!", "mimeType": "image/png"}],
            }})
        else:
            send({"jsonrpc": "2.0", "id": message["id"], "result": {
                "content": [{"type": "text", "text": "ok"}],
                "structuredContent": {"value": "ok"},
            }})
        return
    text = str(arguments.get("message", "")) + SUFFIX
    send({"jsonrpc": "2.0", "id": message["id"], "result": {
        "content": [{"type": "text", "text": text}],
    }})


def main():
    while True:
        message = read_message()
        if message is None:
            break
        method = message.get("method")
        if method == "initialize":
            initialize(message)
        elif method == "notifications/initialized":
            continue
        elif method == "tools/list":
            trace("tools/list")
            send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": TOOLS}})
        elif method == "tools/call":
            trace("tools/call")
            tools_call(message)
        elif method is not None and message.get("id") is not None:
            send({"jsonrpc": "2.0", "id": message.get("id"),
                  "error": {"code": -32601, "message": "unsupported"}})


main()
"##;

    #[cfg(target_os = "linux")]
    fn cap22_server(root: &Path) -> PathBuf {
        let path = root.join("cap22_mcp_server.py");
        std::fs::write(&path, CAP22_MCP_SERVER).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.canonicalize().unwrap()
    }

    #[cfg(target_os = "linux")]
    fn cap22_config(command: &Path, name: &str, marker: &str, mode: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some(command.to_string_lossy().into_owned()),
            args: Some(vec![marker.to_owned(), mode.to_owned()]),
            env: None,
            headers: None,
        }
    }

    #[cfg(target_os = "linux")]
    fn cap22_context(actor: &str, root: &Path) -> RequestContext {
        let mut context = RequestContext::local("cap22", root.to_string_lossy().into_owned());
        context.project_trusted = true;
        context.actor_id = Some(actor.to_owned());
        context
    }

    #[cfg(target_os = "linux")]
    fn cap22_catalog(root: &Path, generation: u32) {
        std::fs::write(
            root.join("cap22-catalog.json"),
            json!({ "generation": generation }).to_string(),
        )
        .unwrap();
    }

    #[cfg(target_os = "linux")]
    fn cap22_call_request(
        server: &str,
        tool: &str,
        root: &Path,
        context: &RequestContext,
        sandbox: &str,
        arguments: Value,
    ) -> CapabilityRequest {
        cap21_request(
            MCP_OPERATION,
            root,
            context,
            json!({
                "server": server,
                "tool": tool,
                "arguments": arguments,
                "sandbox": sandbox,
                "timeout_ms": 30_000,
            }),
        )
    }

    /// Commit a discovery snapshot through the real `McpRegistry::prepare` + `McpHandler::run`
    /// path so later fixture calls are backed by an operator-authorized catalog.
    #[cfg(target_os = "linux")]
    async fn cap22_discover(
        registry: &Arc<McpRegistry>,
        handler: &McpHandler,
        context: &RequestContext,
        root: &Path,
        server: &str,
        cancellation: watch::Receiver<bool>,
    ) -> CapabilityResult {
        let request = cap21_request(
            "mcp.discover",
            root,
            context,
            json!({ "server": server, "operator_authorized": true }),
        );
        let prepared = registry
            .prepare(context, &request)
            .await
            .unwrap_or_else(|error| panic!("discovery preparation for {server}: {error}"));
        handler
            .run(cap20_authorized(prepared), cancellation)
            .await
            .unwrap_or_else(|error| panic!("discovery run for {server}: {error}"))
    }

    #[cfg(target_os = "linux")]
    fn cap22_adapter(result: &CapabilityResult) -> kiana_domain::AdapterResult {
        kiana_domain::AdapterResult::from_json(&result.output["adapter_result"])
            .expect("the adapter envelope is valid")
    }

    /// An invalid structured result received after a call was sent is not a safe retry: it stays
    /// Unknown, keeps its reconciliation evidence and reaches the server exactly once.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_output_schema_failure_after_call_is_not_safe_retry() {
        for mode in ["structured_missing", "structured_wrong", "bad_content"] {
            let root = cap21_temp_root();
            let script = cap22_server(&root);
            cap22_catalog(&root, 1);
            let marker = cap21_marker(&format!("result-{mode}"));
            let config = cap22_config(&script, "cap22-fixture", &marker, mode);
            let registry = cap20_registry(vec![config]);
            let context = cap22_context("cap22-owner", &root);
            let handler = McpHandler {
                registry: registry.clone(),
            };
            let (_sender, cancellation) = watch::channel(false);

            let discovered = cap22_discover(
                &registry,
                &handler,
                &context,
                &root,
                "cap22-fixture",
                cancellation.clone(),
            )
            .await;
            assert!(
                discovered.success,
                "{mode}: discovery commits before the call: {:?}",
                discovered.output
            );

            let call = cap22_call_request(
                "cap22-fixture",
                "structured",
                &root,
                &context,
                "workspace-write",
                json!({}),
            );
            let prepared = registry
                .prepare(&context, &call)
                .await
                .unwrap_or_else(|error| panic!("{mode}: call preparation: {error}"));
            let result = handler
                .run(cap20_authorized(prepared), cancellation)
                .await
                .unwrap_or_else(|error| panic!("{mode}: a received but invalid result is classified, not an I/O error: {error}"));

            assert!(!result.success, "{mode}: {:?}", result.output);
            assert_eq!(
                result.failure_code(),
                Some(kiana_domain::CapabilityErrorCode::ResultUnknown),
                "{mode}: an invalid result after a sent call stays Unknown: {:?}",
                result.output
            );
            assert_eq!(
                result.output["error"],
                json!("result_unknown:mcp_call_unconfirmed")
            );
            assert_eq!(result.output["not_executed"], json!(false));
            assert_eq!(result.output["stop_confirmed"], json!(true));
            assert_eq!(result.output["cancelled"], json!(false));
            let envelope = cap22_adapter(&result);
            assert_eq!(envelope.adapter, AdapterResultKind::Mcp);
            assert_eq!(envelope.commit, AdapterCommitState::Unknown);
            assert_eq!(
                envelope.effect,
                kiana_domain::CapabilityEffectState::Unknown
            );
            assert!(envelope.reconciliation_required);

            // The private workspace is retained for reconciliation instead of being published.
            let publication = &result.output["workspace"];
            assert_eq!(publication["retained"], json!(true));
            assert_eq!(publication["reason"], json!("mcp_call_unconfirmed"));
            let staging = PathBuf::from(
                publication["workspace"]
                    .as_str()
                    .expect("the retained staging root is reported"),
            );
            let record = std::fs::read_to_string(staging.join("reconciliation.json"))
                .expect("reconciliation record");
            assert!(record.contains("mcp_call_unconfirmed"), "{record}");

            // Exactly one business call reached the server: an invalid result is never reissued.
            let trace =
                std::fs::read_to_string(staging.join("workspace").join("cap22-trace.jsonl"))
                    .expect("fixture trace");
            assert_eq!(
                trace.lines().filter(|line| *line == "tools/call").count(),
                1,
                "{mode}: a sent call with an invalid result must not be replayed: {trace}"
            );

            cap21_assert_no_survivor(&marker, mode).await;
            let _ = std::fs::remove_dir_all(root);
            let _ = std::fs::remove_dir_all(staging);
        }
    }

    /// Catalog drift commits a new discovery version under the same discovery key and invalidates
    /// every pending authorization prepared against the old catalog, before any process starts.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_schema_drift_invalidates_pending_approval() {
        let root = cap21_temp_root();
        let script = cap22_server(&root);
        cap22_catalog(&root, 1);
        let marker = cap21_marker("catalog");
        let config = cap22_config(&script, "cap22-catalog", &marker, "normal");
        let registry = cap20_registry(vec![config]);
        let context = cap22_context("cap22-owner", &root);
        let handler = McpHandler {
            registry: registry.clone(),
        };
        let (_sender, cancellation) = watch::channel(false);

        let discovered = cap22_discover(
            &registry,
            &handler,
            &context,
            &root,
            "cap22-catalog",
            cancellation.clone(),
        )
        .await;
        assert!(discovered.success);
        let call_v1 = cap22_call_request(
            "cap22-catalog",
            "echo",
            &root,
            &context,
            "read-only",
            json!({ "message": "cap22-catalog" }),
        );
        let prepared_v1 = registry
            .prepare(&context, &call_v1)
            .await
            .expect("v1 call preparation binds the committed catalog");
        let key_v1 = prepared_v1.arguments["mcp_snapshot"]["discovery_key"]
            .as_str()
            .expect("discovery key")
            .to_owned();
        let digest_v1 = prepared_v1.arguments["mcp_snapshot"]["catalog_digest"].clone();
        let action_v1 = kiana_domain::capability_action_digest(&prepared_v1);
        assert_eq!(
            prepared_v1.arguments["mcp_snapshot"]["discovery_version"],
            json!(1)
        );

        // The catalog drifts: the same server and scope must re-discover, not reuse the tool.
        cap22_catalog(&root, 2);
        let rediscovered = cap22_discover(
            &registry,
            &handler,
            &context,
            &root,
            "cap22-catalog",
            cancellation.clone(),
        )
        .await;
        assert!(rediscovered.success);
        let call_v2 = cap22_call_request(
            "cap22-catalog",
            "echo",
            &root,
            &context,
            "read-only",
            json!({ "message": "cap22-catalog" }),
        );
        let prepared_v2 = registry
            .prepare(&context, &call_v2)
            .await
            .expect("v2 call preparation binds the drifted catalog");
        assert_eq!(
            prepared_v2.arguments["mcp_snapshot"]["discovery_key"],
            json!(key_v1),
            "the discovery stream is still keyed to the same owner/server/config"
        );
        assert_ne!(
            prepared_v2.arguments["mcp_snapshot"]["catalog_digest"], digest_v1,
            "catalog drift changes the digest"
        );
        assert_eq!(
            prepared_v2.arguments["mcp_snapshot"]["discovery_version"],
            json!(2)
        );
        assert_ne!(
            prepared_v2.arguments["mcp_snapshot"]["tool"],
            prepared_v1.arguments["mcp_snapshot"]["tool"],
            "the pinned tool metadata changed with the catalog"
        );
        assert_ne!(
            kiana_domain::capability_action_digest(&prepared_v2),
            action_v1,
            "a drifted catalog is a different action, so pending authorization does not carry over"
        );

        // The stale authorization is refused before any process starts.
        let stale = handler
            .run(cap20_authorized(prepared_v1.clone()), cancellation.clone())
            .await;
        assert!(
            matches!(&stale, Err(PortError::Failed(code)) if code == "mcp_catalog_changed"),
            "the stale v1 authorization must be refused: {stale:?}"
        );
        assert!(
            cap21_marker_pids(&marker).is_empty(),
            "a stale catalog is refused before connect"
        );

        let committed = handler
            .run(cap20_authorized(prepared_v2), cancellation.clone())
            .await
            .expect("the refreshed authorization runs against the drifted catalog");
        assert!(committed.success, "{:?}", committed.output);
        assert_eq!(
            committed.output["result"]["content"][0]["text"],
            "cap22-catalog|v2"
        );

        // Restoring the old catalog commits a new discovery version: the old authorization stays
        // invalidated even though its catalog digest matches again.
        cap22_catalog(&root, 1);
        let restored = cap22_discover(
            &registry,
            &handler,
            &context,
            &root,
            "cap22-catalog",
            cancellation.clone(),
        )
        .await;
        assert!(restored.success);
        let stale_again = handler
            .run(cap20_authorized(prepared_v1.clone()), cancellation.clone())
            .await;
        assert!(
            matches!(&stale_again, Err(PortError::Failed(code)) if code == "mcp_catalog_changed"),
            "a re-discovered stream invalidates older authorizations: {stale_again:?}"
        );
        let call_v3 = cap22_call_request(
            "cap22-catalog",
            "echo",
            &root,
            &context,
            "read-only",
            json!({ "message": "cap22-catalog" }),
        );
        let prepared_v3 = registry
            .prepare(&context, &call_v3)
            .await
            .expect("v3 call preparation binds the restored catalog");
        assert_eq!(
            prepared_v3.arguments["mcp_snapshot"]["discovery_version"],
            json!(3)
        );
        let replayed = handler
            .run(cap20_authorized(prepared_v3), cancellation.clone())
            .await
            .expect("the refreshed authorization runs against the restored catalog");
        assert!(replayed.success, "{:?}", replayed.output);
        assert_eq!(
            replayed.output["result"]["content"][0]["text"],
            "cap22-catalog|v1"
        );

        cap21_assert_no_survivor(&marker, "catalog drift").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Owner and scope are part of the discovery key: two owners never share a discovery stream,
    /// a tampered scope is refused before dispatch, and concurrent invocations each own a
    /// separate confined process (`per_invocation`, no pooled reuse).
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_pool_never_crosses_owner_or_scope() {
        let root = cap21_temp_root();
        let script = cap22_server(&root);
        cap22_catalog(&root, 1);
        let alpha_marker = cap21_marker("alpha");
        let beta_marker = cap21_marker("beta");
        let alpha = cap22_config(&script, "cap22-alpha", &alpha_marker, "slow_call");
        let beta = cap22_config(&script, "cap22-beta", &beta_marker, "slow_call");
        let registry = cap20_registry(vec![alpha, beta]);
        let owner_a = cap22_context("cap22-owner-a", &root);
        let owner_b = cap22_context("cap22-owner-b", &root);
        let handler = Arc::new(McpHandler {
            registry: registry.clone(),
        });
        let (_sender, cancellation) = watch::channel(false);

        // Owner A discovers and prepares a call to alpha.
        let discovered_a = cap22_discover(
            &registry,
            handler.as_ref(),
            &owner_a,
            &root,
            "cap22-alpha",
            cancellation.clone(),
        )
        .await;
        assert!(discovered_a.success);
        let call_a = cap22_call_request(
            "cap22-alpha",
            "echo",
            &root,
            &owner_a,
            "read-only",
            json!({ "message": "owner-a" }),
        );
        let prepared_a = registry
            .prepare(&owner_a, &call_a)
            .await
            .expect("owner A binds its own discovery snapshot");
        let key_a = prepared_a.arguments["mcp_snapshot"]["discovery_key"]
            .as_str()
            .expect("discovery key")
            .to_owned();

        // Owner B cannot reuse A's discovery: the key binds the owner scope, so B must discover.
        let call_b_alpha = cap22_call_request(
            "cap22-alpha",
            "echo",
            &root,
            &owner_b,
            "read-only",
            json!({ "message": "owner-b" }),
        );
        let refused = registry.prepare(&owner_b, &call_b_alpha).await;
        assert!(
            matches!(&refused, Err(PortError::Failed(code)) if code == "mcp_discovery_required:use_operator_mcp.discover"),
            "another owner cannot ride on A's discovery: {refused:?}"
        );
        let discovery_b_alpha = cap21_request(
            "mcp.discover",
            &root,
            &owner_b,
            json!({ "server": "cap22-alpha", "operator_authorized": true }),
        );
        let prepared_b_alpha = registry
            .prepare(&owner_b, &discovery_b_alpha)
            .await
            .expect("owner B prepares its own discovery");
        assert_ne!(
            prepared_b_alpha.arguments["mcp_snapshot"]["discovery_key"],
            json!(key_a),
            "the discovery key binds the owner scope"
        );
        assert_eq!(
            prepared_b_alpha.arguments["mcp_snapshot"]["scope"]["actor_id"],
            json!("cap22-owner-b")
        );

        // Any scope field altered after preparation is refused before dispatch.
        for field in ["actor_id", "role_id", "department_id", "project_root"] {
            let mut tampered = prepared_a.clone();
            tampered.arguments[field] = json!("cap22-tampered");
            let denied = handler
                .run(cap20_authorized(tampered), cancellation.clone())
                .await;
            assert!(
                matches!(&denied, Err(PortError::Failed(code)) if code == "mcp_snapshot_scope_changed"),
                "{field}: a prepared scope is not reusable for another identity: {denied:?}"
            );
        }

        // Owner B discovers and prepares its own call to beta; the keys differ by owner/server.
        let discovered_b = cap22_discover(
            &registry,
            handler.as_ref(),
            &owner_b,
            &root,
            "cap22-beta",
            cancellation.clone(),
        )
        .await;
        assert!(discovered_b.success);
        let call_b = cap22_call_request(
            "cap22-beta",
            "echo",
            &root,
            &owner_b,
            "read-only",
            json!({ "message": "owner-b" }),
        );
        let prepared_b = registry
            .prepare(&owner_b, &call_b)
            .await
            .expect("owner B binds its own beta discovery snapshot");
        let key_b = prepared_b.arguments["mcp_snapshot"]["discovery_key"]
            .as_str()
            .expect("discovery key")
            .to_owned();
        assert_ne!(key_b, key_a, "different owner/server must not share a key");

        // Both owners invoke at the same time: each invocation owns its confined process.
        let run_a = tokio::spawn({
            let handler = handler.clone();
            let cancellation = cancellation.clone();
            async move {
                handler
                    .run(cap20_authorized(prepared_a), cancellation)
                    .await
            }
        });
        let run_b = tokio::spawn({
            let handler = handler.clone();
            let cancellation = cancellation.clone();
            async move {
                handler
                    .run(cap20_authorized(prepared_b), cancellation)
                    .await
            }
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut overlapped = false;
        while tokio::time::Instant::now() < deadline {
            if !cap21_marker_pids(&alpha_marker).is_empty()
                && !cap21_marker_pids(&beta_marker).is_empty()
            {
                overlapped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            overlapped,
            "two owners must run in two simultaneous confined processes, not one pooled session"
        );
        let result_a = run_a
            .await
            .expect("join alpha")
            .expect("alpha invocation runs");
        let result_b = run_b
            .await
            .expect("join beta")
            .expect("beta invocation runs");
        for (result, owner, text) in [
            (&result_a, "owner-a", "owner-a|v1"),
            (&result_b, "owner-b", "owner-b|v1"),
        ] {
            assert!(result.success, "{owner}: {:?}", result.output);
            assert_eq!(
                result.output["process_scope"], "per_invocation",
                "{owner}: each invocation owns its process"
            );
            assert_eq!(result.output["containment"], "bwrap", "{owner}");
            assert_eq!(
                result.output["result"]["content"][0]["text"], text,
                "{owner}: each owner receives its own result"
            );
        }
        cap21_assert_no_survivor(&alpha_marker, "alpha call").await;
        cap21_assert_no_survivor(&beta_marker, "beta call").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Legal results stay bounded: a matching structured result keeps the schema binding, an
    /// `isError` result is a tool business failure rather than invalid data, and a resource link
    /// remains a reference.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_legal_results_errors_and_resource_links_stay_bounded() {
        let root = cap21_temp_root();
        let script = cap22_server(&root);
        cap22_catalog(&root, 1);
        let marker = cap21_marker("legal");
        let config = cap22_config(&script, "cap22-legal", &marker, "normal");
        let registry = cap20_registry(vec![config]);
        let context = cap22_context("cap22-owner", &root);
        let handler = McpHandler {
            registry: registry.clone(),
        };
        let (_sender, cancellation) = watch::channel(false);

        let discovered = cap22_discover(
            &registry,
            &handler,
            &context,
            &root,
            "cap22-legal",
            cancellation.clone(),
        )
        .await;
        assert!(discovered.success);
        let tools = discovered.output["tools"]
            .as_array()
            .expect("the discovery catalog is a list");
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec!["echo", "error_tool", "link_tool", "structured"],
            "the catalog is sorted by the client: {:?}",
            discovered.output["tools"]
        );

        let structured_call = cap22_call_request(
            "cap22-legal",
            "structured",
            &root,
            &context,
            "read-only",
            json!({}),
        );
        let prepared = registry
            .prepare(&context, &structured_call)
            .await
            .expect("structured call preparation");
        let structured = handler
            .run(cap20_authorized(prepared), cancellation.clone())
            .await
            .expect("a legal structured result is a successful call");
        assert!(structured.success, "{:?}", structured.output);
        assert_eq!(
            structured.output["result"]["structuredContent"],
            json!({ "value": "ok" })
        );
        assert_eq!(structured.output["untrusted_data"], json!(true));
        assert_eq!(structured.output["resource_links_fetched"], json!(false));
        let output_hash = structured.output["output_schema_hash"]
            .as_str()
            .expect("a structured tool binds its output schema");
        assert!(output_hash.starts_with("sha256:"), "{output_hash}");
        assert_eq!(output_hash.len(), 71, "{output_hash}");
        assert!(output_hash[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
        let envelope = cap22_adapter(&structured);
        assert_eq!(envelope.commit, AdapterCommitState::Committed);
        assert_eq!(
            envelope.effect,
            kiana_domain::CapabilityEffectState::Succeeded
        );
        assert!(!envelope.reconciliation_required);

        // `isError` is a tool business failure: committed, failed effect, no reconciliation.
        let error_call = cap22_call_request(
            "cap22-legal",
            "error_tool",
            &root,
            &context,
            "read-only",
            json!({}),
        );
        let prepared = registry
            .prepare(&context, &error_call)
            .await
            .expect("error_tool call preparation");
        let failed = handler
            .run(cap20_authorized(prepared), cancellation.clone())
            .await
            .expect("an isError result is a classified tool failure, not a transport error");
        assert!(!failed.success);
        assert_eq!(
            failed.failure_code(),
            Some(kiana_domain::CapabilityErrorCode::ExecutionFailed)
        );
        assert_eq!(
            failed.output["error"],
            json!("execution_failed:mcp_tool_is_error")
        );
        let envelope = cap22_adapter(&failed);
        assert_eq!(envelope.commit, AdapterCommitState::Committed);
        assert_eq!(envelope.effect, kiana_domain::CapabilityEffectState::Failed);
        assert!(!envelope.reconciliation_required);

        // A resource link is a reference: it is returned without any implicit fetch.
        let link_call = cap22_call_request(
            "cap22-legal",
            "link_tool",
            &root,
            &context,
            "read-only",
            json!({}),
        );
        let prepared = registry
            .prepare(&context, &link_call)
            .await
            .expect("link_tool call preparation");
        let linked = handler
            .run(cap20_authorized(prepared), cancellation.clone())
            .await
            .expect("a resource link result is a successful call");
        assert!(linked.success, "{:?}", linked.output);
        assert_eq!(
            linked.output["result"]["content"][0]["type"],
            "resource_link"
        );
        assert_eq!(
            linked.output["result"]["content"][0]["uri"],
            "cap22://note/1"
        );
        assert_eq!(linked.output["resource_links_fetched"], json!(false));
        assert!(
            linked.output["result"]["content"][0]
                .get("resource")
                .is_none(),
            "a resource link must not embed fetched content"
        );

        cap21_assert_no_survivor(&marker, "legal results").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Untrusted projects, changed env references and swapped binaries all fence a prepared
    /// authorization before dispatch; re-using the snapshot requires a fresh discovery.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_explicit_revoke_and_config_change_fence_prepared_authorization() {
        let root = cap21_temp_root();
        let script = cap22_server(&root);
        cap22_catalog(&root, 1);
        let marker = cap21_marker("revoke");
        let config = cap22_config(&script, "cap22-revoke", &marker, "normal");
        let registry = cap20_registry(vec![config.clone()]);
        let handler = McpHandler {
            registry: registry.clone(),
        };
        let (_sender, cancellation) = watch::channel(false);

        // An untrusted project cannot even prepare a discovery: no process may start.
        let mut untrusted = cap22_context("cap22-owner", &root);
        untrusted.project_trusted = false;
        let discovery_request = cap21_request(
            "mcp.discover",
            &root,
            &untrusted,
            json!({ "server": "cap22-revoke", "operator_authorized": true }),
        );
        let refused = registry.prepare(&untrusted, &discovery_request).await;
        assert!(
            matches!(&refused, Err(PortError::Failed(code)) if code == "mcp_project_untrusted"),
            "an untrusted project is refused before any process: {refused:?}"
        );
        assert!(
            cap21_marker_pids(&marker).is_empty(),
            "an untrusted project never reaches a process"
        );

        let context = cap22_context("cap22-owner", &root);
        let discovered = cap22_discover(
            &registry,
            &handler,
            &context,
            &root,
            "cap22-revoke",
            cancellation.clone(),
        )
        .await;
        assert!(discovered.success);
        let call = cap22_call_request(
            "cap22-revoke",
            "echo",
            &root,
            &context,
            "read-only",
            json!({ "message": "cap22-revoke" }),
        );
        let prepared_old = registry
            .prepare(&context, &call)
            .await
            .expect("call preparation binds the committed snapshot");

        // An added explicit env reference changes the config hash: the old authorization and the
        // old discovery key are both fenced.
        let mut changed_config = config.clone();
        changed_config.env = Some(HashMap::from([
            ("LANG".to_owned(), "C.UTF-8".to_owned()),
            ("TZ".to_owned(), "UTC".to_owned()),
        ]));
        let registry_changed = cap20_registry(vec![changed_config]);
        let handler_changed = McpHandler {
            registry: registry_changed.clone(),
        };
        let changed = handler_changed
            .run(cap20_authorized(prepared_old.clone()), cancellation.clone())
            .await;
        assert!(
            matches!(&changed, Err(PortError::Failed(code)) if code == "mcp_config_changed"),
            "an env reference change fences the prepared authorization: {changed:?}"
        );
        let refused = registry_changed.prepare(&context, &call).await;
        assert!(
            matches!(&refused, Err(PortError::Failed(code)) if code == "mcp_discovery_required:use_operator_mcp.discover"),
            "a changed configuration has no committed discovery: {refused:?}"
        );

        // The pinned binary changed after preparation: the snapshot executable is fenced and a
        // new call preparation requires a fresh discovery.
        std::fs::write(&script, format!("{}\n# cap22 drift\n", CAP22_MCP_SERVER)).unwrap();
        let stale = handler
            .run(cap20_authorized(prepared_old.clone()), cancellation.clone())
            .await;
        assert!(
            matches!(&stale, Err(PortError::Failed(code)) if code == "mcp_config_changed"),
            "a swapped binary fences the prepared authorization: {stale:?}"
        );
        let refused = registry.prepare(&context, &call).await;
        assert!(
            matches!(&refused, Err(PortError::Failed(code)) if code == "mcp_config_drift_requires_discovery"),
            "a swapped binary requires a fresh discovery: {refused:?}"
        );

        cap21_assert_no_survivor(&marker, "config drift").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// The result validator itself bounds structured content, content types/encodings and the
    /// content list, while legal structured and `isError` results stay legal.
    #[cfg(target_os = "linux")]
    #[test]
    fn mcp_result_validator_bounds_structured_content_and_content_types() {
        let structured_tool = json!({
            "name": "structured",
            "inputSchema": { "type": "object", "properties": {} },
            "outputSchema": {
                "type": "object",
                "properties": { "value": { "type": "string" } },
                "required": ["value"],
                "additionalProperties": false,
            },
        });
        let echo_tool = json!({
            "name": "echo",
            "inputSchema": { "type": "object", "properties": {} },
        });

        let missing = validate_call_result(
            &json!({ "content": [{ "type": "text", "text": "ok" }] }),
            &structured_tool,
        );
        assert!(
            matches!(&missing, Err(PortError::Failed(code)) if code == "mcp_structured_content_required"),
            "{missing:?}"
        );

        let mismatch = validate_call_result(
            &json!({
                "content": [{ "type": "text", "text": "ok" }],
                "structuredContent": { "value": 7 },
            }),
            &structured_tool,
        );
        assert!(
            matches!(&mismatch, Err(PortError::Failed(code)) if code == "mcp_output_schema_mismatch"),
            "{mismatch:?}"
        );

        for (result, context) in [
            (
                json!({ "content": [{ "type": "image", "data": "not base64!!", "mimeType": "image/png" }] }),
                "an image payload must be valid base64",
            ),
            (
                json!({ "content": [{ "type": "video", "data": "AAAA" }] }),
                "an unknown content type is refused",
            ),
            (
                json!({ "content": [{ "type": "resource_link", "uri": "no-scheme", "name": "n" }] }),
                "a resource link needs an absolute URI",
            ),
        ] {
            let invalid = validate_call_result(&result, &echo_tool);
            assert!(
                matches!(&invalid, Err(PortError::Failed(code)) if code == "mcp_content_invalid"),
                "{context}: {invalid:?}"
            );
        }

        let items: Vec<Value> = (0..129)
            .map(|index| json!({ "type": "text", "text": format!("item-{index}") }))
            .collect();
        let flood = validate_call_result(&json!({ "content": items }), &echo_tool);
        assert!(
            matches!(&flood, Err(PortError::Failed(code)) if code == "mcp_content_limit"),
            "{flood:?}"
        );

        let business_failure = validate_call_result(
            &json!({
                "content": [{ "type": "text", "text": "failed" }],
                "isError": true,
            }),
            &echo_tool,
        );
        assert!(
            business_failure.is_ok(),
            "isError is a tool business failure, not an invalid result: {business_failure:?}"
        );

        let valid = validate_call_result(
            &json!({
                "content": [{ "type": "text", "text": "ok" }],
                "structuredContent": { "value": "ok" },
            }),
            &structured_tool,
        );
        assert!(
            valid.is_ok(),
            "a schema-matching structured result stays legal: {valid:?}"
        );
    }
}
