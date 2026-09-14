//! MCP discovery and business calls use the same permit-protected broker.
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AggregateVersion, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    CapabilityResult, CommitOutcome, RequestContext, RequestId, RuntimeEvent, TransitionBatch,
};
use kiana_ports::{EventStorePort, PortError};
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

/// Immutable trusted configuration; discovery facts are read from the journal, not this cache.
pub(crate) struct McpRegistry {
    servers: Vec<McpServerConfig>,
    events: Arc<dyn EventStorePort>,
}
impl McpRegistry {
    pub fn new(events: Arc<dyn EventStorePort>) -> Result<Arc<Self>, PortError> {
        let raw = std::env::var(MCP_SERVERS_ENV).unwrap_or_default();
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
            if server.transport != TransportType::Stdio
                || server.url.is_some()
                || server
                    .headers
                    .as_ref()
                    .is_some_and(|headers| !headers.is_empty())
            {
                return Err(mcp_failed("mcp_transport_unsupported"));
            }
            let command = server
                .command
                .as_ref()
                .filter(|c| !c.is_empty() && !c.contains('\0'))
                .ok_or_else(|| mcp_failed("mcp_command_required"))?;
            let resolved = resolve_command(command)?;
            server.command = Some(resolved.to_string_lossy().into_owned());
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
        Ok(Arc::new(Self { servers, events }))
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
            "config_hash":config_hash,"discovery_key":key,"executable":executable["executable"],"argument_files":executable["argument_files"]});
        if request.operation == MCP_OPERATION {
            let tools = self.discovery(&key).await?;
            if tools["config_pin"] != executable {
                return Err(mcp_failed("mcp_config_drift_requires_discovery"));
            }
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
        }
        let mut prepared = request.clone();
        prepared.arguments["server"] = json!(config.name);
        prepared.arguments["mcp_snapshot"] = snapshot;
        Ok(prepared)
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
        let data = json!({"schema":"kiana.mcp-discovery.v1","parent_request_id":request.request_id,
            "scope":request.arguments["mcp_snapshot"]["scope"],"server":request.arguments["server"],
            "config_pin":config_pin,"catalog_digest":digest,"protocol":protocol,"tools":tools});
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
            "version":version+1,"tool_count":tools.len(),"tools":tools,"evidence_ref":event_id.to_string(),"untrusted_data":true}),
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
        if !discovering && arguments["sandbox"] == "workspace-write" {
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
        let mut client = crate::mcp_stdio::ConfinedMcpClient::connect(
            &config,
            &execution_scope,
            cancellation.clone(),
        )
        .await?;
        let performed = async {
            let tools = client.list_tools().await?;
            for tool in &tools {
                validate_advertised_tool(tool)?;
            }
            if discovering {
                return Ok((Value::Null, tools));
            }
            if catalog_digest(&tools, &client.server_info) != snapshot["catalog_digest"] {
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
        let started = client.call_started;
        let protocol = client.server_info.clone();
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
            return Ok(result);
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
            return Ok(response);
        }
        let failed = result["isError"] == true;
        let mut output = json!({"schema":MCP_RESULT_SCHEMA,"server":config.name,"tool":arguments["tool"],"transport":"stdio",
            "server_info":protocol,"server_config_hash":snapshot["config_hash"],"catalog_digest":snapshot["catalog_digest"],
            "input_schema_hash":kiana_domain::json_digest(&snapshot["tool"]["inputSchema"]),
            "output_schema_hash":snapshot["tool"].get("outputSchema").map(kiana_domain::json_digest),
            "containment":"bwrap","process_scope":"per_invocation","untrusted_data":true,"resource_links_fetched":false,
            "diagnostics":diagnostics,"workspace":publication,"result":result});
        if failed {
            output["error"] = json!("execution_failed:mcp_tool_is_error");
        }
        Ok(CapabilityResult {
            request_id: request.request.request_id,
            success: !failed,
            output,
            evidence_refs: Vec::new(),
        })
    }
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
fn config_pin(config: &McpServerConfig, root: &Path) -> Result<Value, PortError> {
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
    Ok(json!({"executable":executable,"argument_files":files}))
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
