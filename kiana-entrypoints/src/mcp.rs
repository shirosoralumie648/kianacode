use anyhow::{anyhow, Result};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use kiana_tools::{
    create_default_registry,
    mcp_tool::{MCP_SERVERS_APP_STATE_KEY, MCP_SERVERS_ENV},
    tool_execution::execute_tool_call,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

const PROMPT_STATUS_REPORT: &str = "kiana-status-report";
const PROMPT_TOOL_AUDIT: &str = "kiana-tool-audit";
const PROMPT_PERMISSIONS_REVIEW: &str = "kiana-permissions-review";
const TOOL_RESOURCE_PREFIX: &str = "kiana://tool/";

pub struct McpServer {
    name: String,
    version: String,
    registry: ToolRegistry,
    tool_context: Mutex<ToolContext>,
}

struct McpHttpState {
    server: Arc<McpServer>,
    sse_sessions: Mutex<HashMap<String, mpsc::Sender<Value>>>,
}

impl McpHttpState {
    fn new(server: Arc<McpServer>) -> Self {
        Self {
            server,
            sse_sessions: Mutex::new(HashMap::new()),
        }
    }

    async fn register_sse_session(&self) -> (String, mpsc::Receiver<Value>) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::channel(32);
        self.sse_sessions
            .lock()
            .await
            .insert(session_id.clone(), sender);
        (session_id, receiver)
    }

    async fn send_sse_response(&self, session_id: &str, response: Value) -> StatusCode {
        let sender = {
            let sessions = self.sse_sessions.lock().await;
            sessions.get(session_id).cloned()
        };

        let Some(sender) = sender else {
            return StatusCode::NOT_FOUND;
        };

        if sender.send(response).await.is_err() {
            self.sse_sessions.lock().await.remove(session_id);
            return StatusCode::GONE;
        }

        StatusCode::ACCEPTED
    }
}

impl McpServer {
    pub fn new(name: String, version: String, cwd: String) -> Self {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut app_state = HashMap::new();
        if let Some(mcp_servers) = mcp_servers_from_env() {
            app_state.insert(MCP_SERVERS_APP_STATE_KEY.to_string(), mcp_servers);
        }
        Self {
            name,
            version,
            registry: create_default_registry(),
            tool_context: Mutex::new(ToolContext {
                cwd,
                read_file_state: HashMap::new(),
                app_state,
                abort_signal: abort_rx,
            }),
        }
    }

    pub async fn start(&self, cwd: &str) -> Result<()> {
        {
            let mut context = self.tool_context.lock().await;
            context.cwd = cwd.to_string();
        }
        let stdin = BufReader::new(tokio::io::stdin());
        let stdout = tokio::io::stdout();
        self.serve_stdio(stdin, stdout).await
    }

    pub async fn serve_stdio<R, W>(&self, mut reader: R, mut writer: W) -> Result<()>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 {
                return Ok(());
            }

            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(response) = self.handle_json_rpc(message).await {
                write_json_line(&mut writer, &response).await?;
            }
        }
    }

    pub async fn handle_list_tools(&self) -> Result<Value> {
        let tools: Vec<Value> = self
            .registry
            .list_tools()
            .into_iter()
            .map(|tool| {
                let mut tool_json = json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": tool.input_schema(),
                });
                if let Some(workbench) = tool.workbench() {
                    tool_json["workbench"] = json!(workbench);
                }
                tool_json
            })
            .collect();
        Ok(json!({ "tools": tools }))
    }

    pub async fn handle_list_resources(&self) -> Result<Value> {
        Ok(json!({
            "resources": [
                {
                    "uri": "kiana://tools",
                    "name": "Kiana tools",
                    "description": "Registered local tools exposed by this MCP server",
                    "mimeType": "application/json"
                },
                {
                    "uri": "kiana://status",
                    "name": "Kiana status",
                    "description": "Runtime status for the local Kiana MCP server",
                    "mimeType": "application/json"
                },
                {
                    "uri": "kiana://permissions",
                    "name": "Kiana permissions",
                    "description": "Effective permission mode and tool allow/deny rules",
                    "mimeType": "application/json"
                }
            ]
        }))
    }

    pub async fn handle_list_resource_templates(&self) -> Result<Value> {
        Ok(json!({
            "resourceTemplates": [
                {
                    "uriTemplate": "kiana://tool/{name}",
                    "name": "Kiana tool schema",
                    "description": "Schema and metadata for one registered Kiana tool by name",
                    "mimeType": "application/json"
                }
            ]
        }))
    }

    pub async fn handle_list_prompts(&self) -> Result<Value> {
        Ok(json!({
            "prompts": [
                {
                    "name": PROMPT_STATUS_REPORT,
                    "description": "Summarize the current Kiana MCP runtime status",
                    "arguments": []
                },
                {
                    "name": PROMPT_TOOL_AUDIT,
                    "description": "Audit the available Kiana tools and identify what can be used next",
                    "arguments": [
                        {
                            "name": "focus",
                            "description": "Optional area to focus the audit on, such as files, tasks, permissions, or MCP",
                            "required": false
                        }
                    ]
                },
                {
                    "name": PROMPT_PERMISSIONS_REVIEW,
                    "description": "Review effective tool permissions and explain why a tool may be allowed or denied",
                    "arguments": [
                        {
                            "name": "tool",
                            "description": "Optional tool name or permission pattern to inspect",
                            "required": false
                        }
                    ]
                }
            ]
        }))
    }

    pub async fn handle_read_resource(&self, uri: &str) -> Result<Value> {
        let resource = match uri {
            "kiana://tools" => self.handle_list_tools().await?,
            "kiana://status" => {
                let context = self.tool_context.lock().await;
                json!({
                    "cwd": context.cwd,
                    "tools": self.registry.list_tools().len(),
                    "read_files": context.read_file_state.len(),
                    "app_state_keys": context.app_state.keys().cloned().collect::<Vec<_>>()
                })
            }
            "kiana://permissions" => {
                let context = self.tool_context.lock().await;
                let permissions =
                    kiana_tools::permissions::effective_tool_permissions(&context.app_state);
                json!({
                    "mode": permissions.mode,
                    "allowed_tools": permissions.allowed_tools,
                    "disallowed_tools": permissions.disallowed_tools,
                    "sources": permissions.sources,
                    "file": permissions.file_path.display().to_string(),
                    "file_loaded": permissions.file_loaded,
                    "file_error": permissions.file_error,
                })
            }
            _ => {
                if let Some(tool_name) = uri.strip_prefix(TOOL_RESOURCE_PREFIX) {
                    let tool = self
                        .registry
                        .get(tool_name)
                        .ok_or_else(|| anyhow!("Tool resource {} not found", tool_name))?;
                    let mut tool_json = json!({
                        "name": tool.name(),
                        "description": tool.description(),
                        "inputSchema": tool.input_schema(),
                        "readOnly": tool.is_read_only(),
                        "concurrencySafe": tool.is_concurrency_safe(),
                    });
                    if let Some(workbench) = tool.workbench() {
                        tool_json["workbench"] = json!(workbench);
                    }
                    tool_json
                } else {
                    return Err(anyhow!("Resource {} not found", uri));
                }
            }
        };

        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&resource)?
            }]
        }))
    }

    pub async fn handle_get_prompt(&self, name: &str, args: Value) -> Result<Value> {
        let args = args.as_object().cloned().unwrap_or_default();
        let prompt_text = match name {
            PROMPT_STATUS_REPORT => {
                let context = self.tool_context.lock().await;
                format!(
                    "Summarize this Kiana MCP server status for the user.\n\ncwd: {}\nregistered_tools: {}\nread_files: {}\napp_state_keys: {}",
                    context.cwd,
                    self.registry.list_tools().len(),
                    context.read_file_state.len(),
                    context
                        .app_state
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            PROMPT_TOOL_AUDIT => {
                let focus = args
                    .get("focus")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("overall usability");
                let tools = self
                    .registry
                    .list_tools()
                    .into_iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name(),
                            "description": tool.description(),
                            "read_only": tool.is_read_only(),
                            "concurrency_safe": tool.is_concurrency_safe(),
                        })
                    })
                    .collect::<Vec<_>>();
                format!(
                    "Audit the available Kiana tools with focus on {focus}. Identify which tools are ready to use, which require permissions or state, and the next concrete action.\n\n{}",
                    serde_json::to_string_pretty(&tools)?
                )
            }
            PROMPT_PERMISSIONS_REVIEW => {
                let tool = args
                    .get("tool")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("all tools");
                let context = self.tool_context.lock().await;
                let permissions =
                    kiana_tools::permissions::effective_tool_permissions(&context.app_state);
                format!(
                    "Review Kiana permissions for {tool}. Explain the effective mode, allow rules, deny rules, and what a caller should change if execution is blocked.\n\n{}",
                    serde_json::to_string_pretty(&json!({
                        "mode": permissions.mode,
                        "allowed_tools": permissions.allowed_tools,
                        "disallowed_tools": permissions.disallowed_tools,
                        "sources": permissions.sources,
                        "file": permissions.file_path.display().to_string(),
                        "file_loaded": permissions.file_loaded,
                        "file_error": permissions.file_error,
                    }))?
                )
            }
            _ => return Err(anyhow!("Prompt {} not found", name)),
        };

        Ok(json!({
            "description": prompt_description(name),
            "messages": [{
                "role": "user",
                "content": {
                    "type": "text",
                    "text": prompt_text
                }
            }]
        }))
    }

    pub async fn handle_call_tool(&self, name: &str, args: Value) -> Result<Value> {
        let mut context = self.tool_context.lock().await;
        let result = execute_tool_call(&self.registry, None, &mut context, name, &args, None).await;
        if result.is_error {
            return Ok(mcp_tool_error(
                result
                    .content
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| stringify_tool_output(&result.content)),
            ));
        }
        Ok(json!({
                "content": [{
                    "type": "text",
                    "text": stringify_tool_output(&result.content)
                }],
                "structuredContent": result.content,
                "isError": false
        }))
    }

    async fn handle_json_rpc(&self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str)?;

        if id.is_none() {
            return None;
        }
        let id = id.unwrap_or(Value::Null);

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {},
                    "prompts": {}
                },
                "serverInfo": {
                    "name": self.name,
                    "version": self.version
                }
            })),
            "tools/list" => self.handle_list_tools().await,
            "tools/call" => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.handle_call_tool(name, args).await
            }
            "resources/list" => self.handle_list_resources().await,
            "resources/templates/list" => self.handle_list_resource_templates().await,
            "resources/read" => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                let uri = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.handle_read_resource(uri).await
            }
            "prompts/list" => self.handle_list_prompts().await,
            "prompts/get" => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.handle_get_prompt(name, args).await
            }
            _ => Err(anyhow!("Method {} not found", method)),
        };

        Some(match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32603,
                    "message": error.to_string()
                }
            }),
        })
    }
}

fn mcp_servers_from_env() -> Option<Value> {
    let raw = std::env::var(MCP_SERVERS_ENV).ok()?;
    serde_json::from_str::<Value>(&raw).ok()
}

pub async fn start_mcp_server(cwd: &str, _debug: bool, _verbose: bool) -> Result<()> {
    let server = McpServer::new(
        "kiana/tengu".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        cwd.to_string(),
    );
    server.start(cwd).await
}

pub async fn start_mcp_http_server(
    cwd: &str,
    addr: SocketAddr,
    _debug: bool,
    _verbose: bool,
) -> Result<()> {
    let server = Arc::new(McpServer::new(
        "kiana/tengu".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        cwd.to_string(),
    ));
    let app = mcp_http_router(server);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn mcp_http_router(server: Arc<McpServer>) -> Router {
    let state = Arc::new(McpHttpState::new(server));
    Router::new()
        .route("/", post(handle_http_json_rpc))
        .route("/mcp", post(handle_http_json_rpc))
        .route("/sse", get(handle_sse_connect))
        .route("/message", post(handle_sse_json_rpc))
        .route("/ws", get(handle_ws_connect))
        .with_state(state)
}

async fn handle_http_json_rpc(
    State(state): State<Arc<McpHttpState>>,
    Json(message): Json<Value>,
) -> axum::response::Response {
    match state.server.handle_json_rpc(message).await {
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn handle_sse_connect(State(state): State<Arc<McpHttpState>>) -> axum::response::Response {
    let (session_id, receiver) = state.register_sse_session().await;
    let endpoint = format!("/message?sessionId={session_id}");
    let endpoint_event = tokio_stream::once(Ok::<_, Infallible>(
        Event::default().event("endpoint").data(endpoint),
    ));
    let message_events = ReceiverStream::new(receiver).map(|message| {
        Ok::<_, Infallible>(Event::default().event("message").data(message.to_string()))
    });

    Sse::new(endpoint_event.chain(message_events))
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn handle_sse_json_rpc(
    State(state): State<Arc<McpHttpState>>,
    Query(query): Query<HashMap<String, String>>,
    Json(message): Json<Value>,
) -> axum::response::Response {
    let Some(session_id) = query.get("sessionId") else {
        return (StatusCode::BAD_REQUEST, "missing sessionId").into_response();
    };

    let Some(response) = state.server.handle_json_rpc(message).await else {
        return StatusCode::ACCEPTED.into_response();
    };

    state
        .send_sse_response(session_id, response)
        .await
        .into_response()
}

async fn handle_ws_connect(
    State(state): State<Arc<McpHttpState>>,
    websocket: WebSocketUpgrade,
) -> axum::response::Response {
    websocket
        .on_upgrade(move |socket| handle_ws_session(socket, state))
        .into_response()
}

async fn handle_ws_session(mut socket: WebSocket, state: Arc<McpHttpState>) {
    while let Some(message) = socket.recv().await {
        let Ok(message) = message else {
            break;
        };

        let value = match message {
            Message::Text(text) => serde_json::from_str::<Value>(text.as_str()).ok(),
            Message::Binary(bytes) => serde_json::from_slice::<Value>(&bytes).ok(),
            Message::Close(_) => break,
            _ => None,
        };

        let Some(value) = value else {
            continue;
        };
        let Some(response) = state.server.handle_json_rpc(value).await else {
            continue;
        };

        if socket
            .send(Message::Text(response.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn write_json_line<W>(writer: &mut W, value: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

fn stringify_tool_output(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn mcp_tool_error(message: String) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": message
        }],
        "isError": true
    })
}

fn prompt_description(name: &str) -> &'static str {
    match name {
        PROMPT_STATUS_REPORT => "Summarize the current Kiana MCP runtime status",
        PROMPT_TOOL_AUDIT => "Audit the available Kiana tools and identify what can be used next",
        PROMPT_PERMISSIONS_REVIEW => {
            "Review effective tool permissions and explain why a tool may be allowed or denied"
        }
        _ => "Kiana MCP prompt",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle_http_json_rpc, handle_sse_json_rpc, mcp_http_router, McpHttpState, McpServer,
    };
    use crate::test_support::env_lock;
    use axum::body::to_bytes;
    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::Json;
    use kiana_services::mcp::{McpClient, McpServerConfig, TransportType};
    use kiana_tools::{
        mcp_tool::{MCP_SERVERS_APP_STATE_KEY, MCP_SERVERS_ENV},
        tool_execution::PERMISSION_PROMPT_TOOL_APP_STATE_KEY,
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    fn test_server() -> McpServer {
        McpServer::new(
            "kiana/test".to_string(),
            "0.0.0".to_string(),
            ".".to_string(),
        )
    }

    fn isolate_permission_env(label: &str) {
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_ALLOWED_TOOLS");
        std::env::remove_var("KIANA_DISALLOWED_TOOLS");
        std::env::remove_var("KIANA_ASK_TOOLS");
        std::env::remove_var(kiana_tools::tool_execution::PERMISSION_PROMPT_TOOL_ENV);
        let path = std::env::temp_dir().join(format!(
            "kiana-mcp-permissions-{label}-{}.json",
            std::process::id()
        ));
        std::env::set_var("KIANA_PERMISSIONS_FILE", path);
    }

    fn test_http_state() -> Arc<McpHttpState> {
        Arc::new(McpHttpState::new(Arc::new(test_server())))
    }

    #[tokio::test]
    async fn list_tools_exposes_default_registry() {
        let server = test_server();
        let tools = server.handle_list_tools().await.unwrap();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.contains(&"Read"));
        assert!(names.contains(&"TaskCreate"));
        assert!(tools["tools"][0].get("inputSchema").is_some());
        let mcp = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "MCP")
            .expect("MCP tool should be exposed");
        assert_eq!(mcp["workbench"], "mcp");
        let read = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "Read")
            .expect("Read tool should be exposed");
        assert!(read.get("workbench").is_none());
    }

    #[tokio::test]
    async fn resources_list_and_read_expose_local_status() {
        let server = test_server();
        let resources = server.handle_list_resources().await.unwrap();
        let uris: Vec<&str> = resources["resources"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|resource| resource["uri"].as_str())
            .collect();
        assert!(uris.contains(&"kiana://tools"));
        assert!(uris.contains(&"kiana://status"));
        assert!(uris.contains(&"kiana://permissions"));

        let status = server.handle_read_resource("kiana://status").await.unwrap();
        assert_eq!(status["contents"][0]["uri"], "kiana://status");
        assert!(status["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"tools\""));

        let mcp = server
            .handle_read_resource("kiana://tool/MCP")
            .await
            .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(mcp["contents"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["workbench"], "mcp");
    }

    #[tokio::test]
    async fn resources_templates_list_expose_local_tool_template() {
        let server = test_server();
        let templates = server.handle_list_resource_templates().await.unwrap();
        let uris: Vec<&str> = templates["resourceTemplates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|template| template["uriTemplate"].as_str())
            .collect();
        assert!(uris.contains(&"kiana://tool/{name}"));

        let resource = server
            .handle_read_resource("kiana://tool/Read")
            .await
            .unwrap();
        assert_eq!(resource["contents"][0]["uri"], "kiana://tool/Read");
        assert!(resource["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"name\": \"Read\""));
    }

    #[tokio::test]
    async fn prompts_list_and_get_expose_actionable_prompt_messages() {
        let server = test_server();
        let prompts = server.handle_list_prompts().await.unwrap();
        let names: Vec<&str> = prompts["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|prompt| prompt["name"].as_str())
            .collect();
        assert!(names.contains(&"kiana-status-report"));
        assert!(names.contains(&"kiana-tool-audit"));
        assert!(names.contains(&"kiana-permissions-review"));

        let prompt = server
            .handle_get_prompt(
                "kiana-tool-audit",
                json!({
                    "focus": "MCP"
                }),
            )
            .await
            .unwrap();
        assert_eq!(prompt["messages"][0]["role"], "user");
        let text = prompt["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("focus on MCP"));
        assert!(text.contains("\"name\""));
    }

    #[tokio::test]
    async fn call_tool_runs_registered_tool() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env("call-tool-runs");
        let server = test_server();
        let result = server
            .handle_call_tool("TaskCreate", json!({ "title": "Inspect MCP server" }))
            .await
            .unwrap();

        assert_eq!(result["isError"], false);
        assert_eq!(
            result["structuredContent"]["task"]["title"],
            "Inspect MCP server"
        );
    }

    #[tokio::test]
    async fn call_tool_enforces_permission_rules() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env("call-tool-permissions");
        let server = test_server();
        {
            let mut context = server.tool_context.lock().await;
            context
                .app_state
                .insert("disallowed_tools".to_string(), json!(["TaskCreate"]));
        }

        let result = server
            .handle_call_tool("TaskCreate", json!({ "title": "Blocked MCP task" }))
            .await
            .unwrap();

        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("denied"));
    }

    #[tokio::test]
    async fn call_tool_ask_mode_returns_noninteractive_denial_without_hanging() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env("call-tool-ask");
        let server = test_server();
        {
            let mut context = server.tool_context.lock().await;
            context
                .app_state
                .insert("permission_mode".to_string(), json!("ask"));
        }

        let result = server
            .handle_call_tool("TaskCreate", json!({ "title": "Needs approval" }))
            .await
            .unwrap();

        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not attached to an interactive terminal"));
        assert!(!text.contains("not wired"));
    }

    #[tokio::test]
    async fn new_server_loads_mcp_servers_from_env() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(
            MCP_SERVERS_ENV,
            r#"{"perm":{"transport":"http","url":"http://127.0.0.1/mcp"}}"#,
        );
        let server = test_server();
        std::env::remove_var(MCP_SERVERS_ENV);

        let context = server.tool_context.lock().await;
        assert_eq!(
            context.app_state[MCP_SERVERS_APP_STATE_KEY]["perm"]["url"],
            "http://127.0.0.1/mcp"
        );
    }

    #[tokio::test]
    async fn call_tool_uses_mcp_permission_prompt_tool_on_ask() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env("call-tool-mcp-approval");
        let (url, state, mock_server) =
            start_mock_permission_mcp_server(json!({"decision":"allow"})).await;
        let server = test_server();
        {
            let mut context = server.tool_context.lock().await;
            context
                .app_state
                .insert("permission_mode".to_string(), json!("ask"));
            context.app_state.insert(
                PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
                json!("perm.approve"),
            );
            context.app_state.insert(
                MCP_SERVERS_APP_STATE_KEY.to_string(),
                json!({
                    "perm": {
                        "transport": "http",
                        "url": url
                    }
                }),
            );
        }

        let input = json!({ "title": "Approved through MCP server" });
        let result = server
            .handle_call_tool("TaskCreate", input.clone())
            .await
            .unwrap();

        assert_eq!(result["isError"], false);
        assert_eq!(
            result["structuredContent"]["task"]["title"],
            "Approved through MCP server"
        );
        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool_name"], "TaskCreate");
        assert_eq!(calls[0]["input"], input);
        mock_server.abort();
    }

    #[tokio::test]
    async fn json_rpc_dispatches_initialize_and_tool_call() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env("json-rpc-tool-call");
        let server = test_server();
        let init = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            }))
            .await
            .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "kiana/test");
        assert!(init["result"]["capabilities"].get("prompts").is_some());

        let call = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "TaskCreate",
                    "arguments": { "title": "From JSON-RPC" }
                }
            }))
            .await
            .unwrap();
        assert_eq!(call["result"]["isError"], false);
        assert_eq!(
            call["result"]["structuredContent"]["task"]["title"],
            "From JSON-RPC"
        );

        let notification = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }))
            .await;
        assert!(notification.is_none());

        let resources = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "resources/list",
                "params": {}
            }))
            .await
            .unwrap();
        assert!(resources["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["uri"] == "kiana://tools"));

        let templates = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "resources/templates/list",
                "params": {}
            }))
            .await
            .unwrap();
        assert!(templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|template| template["uriTemplate"] == "kiana://tool/{name}"));

        let resource = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "resources/read",
                "params": { "uri": "kiana://permissions" }
            }))
            .await
            .unwrap();
        assert_eq!(
            resource["result"]["contents"][0]["uri"],
            "kiana://permissions"
        );

        let prompts = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "prompts/list",
                "params": {}
            }))
            .await
            .unwrap();
        assert!(prompts["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|prompt| prompt["name"] == "kiana-status-report"));

        let prompt = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "prompts/get",
                "params": {
                    "name": "kiana-permissions-review",
                    "arguments": { "tool": "Bash(git:*)" }
                }
            }))
            .await
            .unwrap();
        assert!(prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("Bash(git:*)"));
    }

    #[tokio::test]
    async fn http_json_rpc_handler_returns_json_response() {
        let response = handle_http_json_rpc(
            State(test_http_state()),
            Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["id"], 1);
        assert!(value["result"]["tools"].as_array().unwrap().len() > 1);
    }

    #[tokio::test]
    async fn http_json_rpc_handler_returns_no_content_for_notifications() {
        let response = handle_http_json_rpc(
            State(test_http_state()),
            Json(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn sse_json_rpc_handler_sends_response_to_session_stream() {
        let state = test_http_state();
        let (session_id, mut receiver) = state.register_sse_session().await;
        let response = handle_sse_json_rpc(
            State(state),
            Query(HashMap::from([("sessionId".to_string(), session_id)])),
            Json(json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/list",
                "params": {}
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let message = receiver.recv().await.unwrap();
        assert_eq!(message["id"], 7);
        assert!(message["result"]["tools"].as_array().unwrap().len() > 1);
    }

    #[tokio::test]
    async fn websocket_route_dispatches_json_rpc() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = mcp_http_router(Arc::new(test_server()));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let mut client = McpClient::new(McpServerConfig {
            name: "entrypoint-ws".to_string(),
            transport: TransportType::Ws,
            url: Some(format!("ws://{}/ws", addr)),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert!(tools.iter().any(|tool| tool.name == "Read"));
        assert!(tools.iter().any(|tool| tool.name == "TaskCreate"));

        let resource = client.read_resource("kiana://tools").await.unwrap();
        assert_eq!(resource["contents"][0]["uri"], "kiana://tools");
        let prompts = client.list_prompts().await.unwrap();
        assert!(prompts
            .iter()
            .any(|prompt| prompt.name == "kiana-status-report"));
        let prompt = client
            .get_prompt("kiana-status-report", std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(prompt["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("Kiana MCP server status"));

        server.abort();
    }

    #[tokio::test]
    async fn serve_stdio_writes_json_rpc_lines() {
        let server = test_server();
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"prompts/list\",\"params\":{}}\n"
        );
        let reader = tokio::io::BufReader::new(input.as_bytes());
        let mut output = Vec::new();

        server.serve_stdio(reader, &mut output).await.unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["result"]["serverInfo"]["name"], "kiana/test");
        assert!(lines[1]["result"]["tools"].as_array().unwrap().len() > 5);
        assert!(lines[2]["result"]["prompts"].as_array().unwrap().len() >= 3);
    }

    #[derive(Debug)]
    struct MockPermissionMcpState {
        decision: Value,
        calls: Vec<Value>,
    }

    async fn start_mock_permission_mcp_server(
        decision: Value,
    ) -> (String, Arc<Mutex<MockPermissionMcpState>>, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(MockPermissionMcpState {
            decision,
            calls: Vec::new(),
        }));
        let app = axum::Router::new()
            .route("/", post(handle_mock_permission_mcp_request))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_permission_mcp_request(
        State(state): State<Arc<Mutex<MockPermissionMcpState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let method = body.get("method").and_then(Value::as_str);
        let id = body.get("id").cloned().unwrap_or(Value::Null);

        match method {
            Some("notifications/initialized") => StatusCode::NO_CONTENT.into_response(),
            Some("initialize") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": "permission-mock", "version": "1.0.0"}
                }
            }))
            .into_response(),
            Some("tools/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "approve",
                        "description": "Approve a Kiana tool call",
                        "inputSchema": {"type": "object"}
                    }]
                }
            }))
            .into_response(),
            Some("resources/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resources": []}
            }))
            .into_response(),
            Some("prompts/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"prompts": []}
            }))
            .into_response(),
            Some("tools/call") => {
                let args = body
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let decision = {
                    let mut state = state.lock().unwrap();
                    state.calls.push(args);
                    state.decision.clone()
                };
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": decision.to_string()
                        }],
                        "isError": false
                    }
                }))
                .into_response()
            }
            _ => (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "method not found"}
                })),
            )
                .into_response(),
        }
    }
}
