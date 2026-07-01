use crate::errors::{ServiceError, ServiceResult};
use crate::network_policy::{validate_http_redirect, validate_http_url, HttpNetworkSurface};
use eventsource_stream::Eventsource;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: TransportType,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    Stdio,
    Sse,
    Http,
    Ws,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<McpPromptArgument>,
}

pub struct McpClient {
    config: McpServerConfig,
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
    resource_templates: Vec<McpResourceTemplate>,
    prompts: Vec<McpPrompt>,
    stdio: Option<Arc<Mutex<StdioMcpTransport>>>,
    http: Option<Arc<Mutex<HttpMcpTransport>>>,
    sse: Option<Arc<Mutex<SseMcpTransport>>>,
    ws: Option<Arc<Mutex<WsMcpTransport>>>,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            tools: Vec::new(),
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
            stdio: None,
            http: None,
            sse: None,
            ws: None,
        }
    }

    pub async fn connect(&mut self) -> ServiceResult<()> {
        match self.config.transport {
            TransportType::Stdio => {
                return self.connect_stdio().await;
            }
            TransportType::Http => {
                return self.connect_http().await;
            }
            TransportType::Sse => {
                return self.connect_sse().await;
            }
            TransportType::Ws => {
                return self.connect_ws().await;
            }
        }
    }

    async fn connect_ws(&mut self) -> ServiceResult<()> {
        let Some(url) = self.config.url.as_deref() else {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses ws but has no url",
                self.config.name
            )));
        };
        let url = url.trim();
        if url.is_empty() {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses ws but url is empty",
                self.config.name
            )));
        }

        let mut transport = WsMcpTransport::connect(url.to_string()).await?;
        let _initialize = transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "kiana-code",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        transport
            .notify("notifications/initialized", json!({}))
            .await?;
        let tools_result = transport.request("tools/list", json!({})).await?;
        self.tools = parse_tools(&tools_result);
        self.resources = match transport.request("resources/list", json!({})).await {
            Ok(result) => parse_resources(&result),
            Err(_) => Vec::new(),
        };
        self.resource_templates = match transport
            .request("resources/templates/list", json!({}))
            .await
        {
            Ok(result) => parse_resource_templates(&result),
            Err(_) => Vec::new(),
        };
        self.prompts = match transport.request("prompts/list", json!({})).await {
            Ok(result) => parse_prompts(&result),
            Err(_) => Vec::new(),
        };
        self.ws = Some(Arc::new(Mutex::new(transport)));
        Ok(())
    }

    async fn connect_http(&mut self) -> ServiceResult<()> {
        let Some(url) = self.config.url.as_deref() else {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses http but has no url",
                self.config.name
            )));
        };
        let url = url.trim();
        if url.is_empty() {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses http but url is empty",
                self.config.name
            )));
        }

        let mut transport = HttpMcpTransport::new(
            url.to_string(),
            self.config.headers.clone().unwrap_or_default(),
        )?;
        let _initialize = transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "kiana-code",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        transport
            .notify("notifications/initialized", json!({}))
            .await?;
        let tools_result = transport.request("tools/list", json!({})).await?;
        self.tools = parse_tools(&tools_result);
        self.resources = match transport.request("resources/list", json!({})).await {
            Ok(result) => parse_resources(&result),
            Err(_) => Vec::new(),
        };
        self.resource_templates = match transport
            .request("resources/templates/list", json!({}))
            .await
        {
            Ok(result) => parse_resource_templates(&result),
            Err(_) => Vec::new(),
        };
        self.prompts = match transport.request("prompts/list", json!({})).await {
            Ok(result) => parse_prompts(&result),
            Err(_) => Vec::new(),
        };
        self.http = Some(Arc::new(Mutex::new(transport)));
        Ok(())
    }

    async fn connect_sse(&mut self) -> ServiceResult<()> {
        let Some(url) = self.config.url.as_deref() else {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses sse but has no url",
                self.config.name
            )));
        };
        let url = url.trim();
        if url.is_empty() {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses sse but url is empty",
                self.config.name
            )));
        }

        let mut transport = SseMcpTransport::connect(
            url.to_string(),
            self.config.headers.clone().unwrap_or_default(),
        )
        .await?;
        let _initialize = transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "kiana-code",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        transport
            .notify("notifications/initialized", json!({}))
            .await?;
        let tools_result = transport.request("tools/list", json!({})).await?;
        self.tools = parse_tools(&tools_result);
        self.resources = match transport.request("resources/list", json!({})).await {
            Ok(result) => parse_resources(&result),
            Err(_) => Vec::new(),
        };
        self.resource_templates = match transport
            .request("resources/templates/list", json!({}))
            .await
        {
            Ok(result) => parse_resource_templates(&result),
            Err(_) => Vec::new(),
        };
        self.prompts = match transport.request("prompts/list", json!({})).await {
            Ok(result) => parse_prompts(&result),
            Err(_) => Vec::new(),
        };
        self.sse = Some(Arc::new(Mutex::new(transport)));
        Ok(())
    }

    async fn connect_stdio(&mut self) -> ServiceResult<()> {
        let Some(command) = self.config.command.as_deref() else {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses stdio but has no command",
                self.config.name
            )));
        };
        if command.trim().is_empty() {
            return Err(ServiceError::Connection(format!(
                "MCP server '{}' uses stdio but command is empty",
                self.config.name
            )));
        }

        let mut transport = StdioMcpTransport::spawn(
            command,
            self.config.args.clone().unwrap_or_default(),
            self.config.env.clone().unwrap_or_default(),
        )
        .await?;
        let _initialize = transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "kiana-code",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        transport
            .notify("notifications/initialized", json!({}))
            .await?;
        let tools_result = transport.request("tools/list", json!({})).await?;
        self.tools = parse_tools(&tools_result);
        self.resources = match transport.request("resources/list", json!({})).await {
            Ok(result) => parse_resources(&result),
            Err(_) => Vec::new(),
        };
        self.resource_templates = match transport
            .request("resources/templates/list", json!({}))
            .await
        {
            Ok(result) => parse_resource_templates(&result),
            Err(_) => Vec::new(),
        };
        self.prompts = match transport.request("prompts/list", json!({})).await {
            Ok(result) => parse_prompts(&result),
            Err(_) => Vec::new(),
        };
        self.stdio = Some(Arc::new(Mutex::new(transport)));
        Ok(())
    }

    pub fn server_name(&self) -> &str {
        &self.config.name
    }

    pub async fn list_tools(&self) -> ServiceResult<Vec<McpTool>> {
        Ok(self.tools.clone())
    }

    pub async fn list_resources(&self) -> ServiceResult<Vec<McpResource>> {
        Ok(self.resources.clone())
    }

    pub async fn list_resource_templates(&self) -> ServiceResult<Vec<McpResourceTemplate>> {
        Ok(self.resource_templates.clone())
    }

    pub async fn list_prompts(&self) -> ServiceResult<Vec<McpPrompt>> {
        Ok(self.prompts.clone())
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        args: HashMap<String, Value>,
    ) -> ServiceResult<Value> {
        match (&self.stdio, &self.http, &self.sse, &self.ws) {
            (Some(stdio), _, _, _) => {
                let mut transport = stdio.lock().await;
                transport
                    .request(
                        "prompts/get",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            (_, Some(http), _, _) => {
                let mut transport = http.lock().await;
                transport
                    .request(
                        "prompts/get",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            (_, _, Some(sse), _) => {
                let mut transport = sse.lock().await;
                transport
                    .request(
                        "prompts/get",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            (_, _, _, Some(ws)) => {
                let mut transport = ws.lock().await;
                transport
                    .request(
                        "prompts/get",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            _ => Err(ServiceError::Connection(format!(
                "MCP prompt '{}' cannot be read before a connected MCP transport is available",
                name
            ))),
        }
    }

    pub async fn read_resource(&self, uri: &str) -> ServiceResult<Value> {
        match (&self.stdio, &self.http, &self.sse, &self.ws) {
            (Some(stdio), _, _, _) => {
                let mut transport = stdio.lock().await;
                transport
                    .request("resources/read", json!({ "uri": uri }))
                    .await
            }
            (_, Some(http), _, _) => {
                let mut transport = http.lock().await;
                transport
                    .request("resources/read", json!({ "uri": uri }))
                    .await
            }
            (_, _, Some(sse), _) => {
                let mut transport = sse.lock().await;
                transport
                    .request("resources/read", json!({ "uri": uri }))
                    .await
            }
            (_, _, _, Some(ws)) => {
                let mut transport = ws.lock().await;
                transport
                    .request("resources/read", json!({ "uri": uri }))
                    .await
            }
            _ => Err(ServiceError::Connection(format!(
                "MCP resource '{}' cannot be read before a connected MCP transport is available",
                uri
            ))),
        }
    }

    pub async fn call_tool(
        &self,
        name: &str,
        args: HashMap<String, Value>,
    ) -> ServiceResult<Value> {
        match (&self.stdio, &self.http, &self.sse, &self.ws) {
            (Some(stdio), _, _, _) => {
                let mut transport = stdio.lock().await;
                transport
                    .request(
                        "tools/call",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            (_, Some(http), _, _) => {
                let mut transport = http.lock().await;
                transport
                    .request(
                        "tools/call",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            (_, _, Some(sse), _) => {
                let mut transport = sse.lock().await;
                transport
                    .request(
                        "tools/call",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            (_, _, _, Some(ws)) => {
                let mut transport = ws.lock().await;
                transport
                    .request(
                        "tools/call",
                        json!({
                            "name": name,
                            "arguments": args,
                        }),
                    )
                    .await
            }
            _ => Err(ServiceError::Connection(format!(
                "MCP tool '{}' cannot be called before a connected MCP transport is available",
                name
            ))),
        }
    }
}

type PendingSseResponses = Arc<Mutex<HashMap<u64, oneshot::Sender<ServiceResult<Value>>>>>;

struct StdioMcpTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioMcpTransport {
    async fn spawn(
        command: &str,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> ServiceResult<Self> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                ServiceError::Connection(format!("failed to spawn MCP stdio server: {}", e))
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ServiceError::Connection("MCP stdio server stdin was not available".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ServiceError::Connection("MCP stdio server stdout was not available".to_string())
        })?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> ServiceResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.write_message(&request).await?;

        timeout(Duration::from_secs(10), self.read_response(id))
            .await
            .map_err(|_| ServiceError::Connection(format!("MCP request '{}' timed out", method)))?
    }

    async fn notify(&mut self, method: &str, params: Value) -> ServiceResult<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_message(&notification).await
    }

    async fn write_message(&mut self, message: &Value) -> ServiceResult<()> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        self.stdin.write_all(&line).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_response(&mut self, id: u64) -> ServiceResult<Value> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.stdout.read_line(&mut line).await?;
            if bytes == 0 {
                return Err(ServiceError::Connection(
                    "MCP stdio server closed stdout".to_string(),
                ));
            }

            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(ServiceError::Connection(format!(
                    "MCP request failed: {}",
                    error
                )));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

impl Drop for StdioMcpTransport {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

struct HttpMcpTransport {
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
    next_id: u64,
}

struct SseMcpTransport {
    client: reqwest::Client,
    post_url: String,
    headers: HashMap<String, String>,
    pending: PendingSseResponses,
    next_id: u64,
}

struct WsMcpTransport {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl WsMcpTransport {
    async fn connect(url: String) -> ServiceResult<Self> {
        let (stream, _) = connect_async(url.as_str()).await.map_err(|error| {
            ServiceError::Connection(format!(
                "failed to connect MCP WebSocket '{}': {}",
                url, error
            ))
        })?;
        Ok(Self { stream, next_id: 1 })
    }

    async fn request(&mut self, method: &str, params: Value) -> ServiceResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.write_message(&request).await?;

        timeout(Duration::from_secs(10), self.read_response(id))
            .await
            .map_err(|_| {
                ServiceError::Connection(format!("MCP WS request '{}' timed out", method))
            })?
    }

    async fn notify(&mut self, method: &str, params: Value) -> ServiceResult<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_message(&notification).await
    }

    async fn write_message(&mut self, message: &Value) -> ServiceResult<()> {
        self.stream
            .send(Message::Text(message.to_string()))
            .await
            .map_err(|error| {
                ServiceError::Connection(format!("failed to write MCP WS message: {}", error))
            })
    }

    async fn read_response(&mut self, id: u64) -> ServiceResult<Value> {
        loop {
            let message = self.stream.next().await.ok_or_else(|| {
                ServiceError::Connection("MCP WS server closed the connection".to_string())
            })?;
            let message = message.map_err(|error| {
                ServiceError::Connection(format!("MCP WS stream failed: {}", error))
            })?;

            let parsed = match message {
                Message::Text(text) => serde_json::from_str::<Value>(&text).ok(),
                Message::Binary(bytes) => serde_json::from_slice::<Value>(&bytes).ok(),
                Message::Ping(payload) => {
                    self.stream
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| {
                            ServiceError::Connection(format!(
                                "failed to write MCP WS pong: {}",
                                error
                            ))
                        })?;
                    None
                }
                Message::Close(_) => {
                    return Err(ServiceError::Connection(
                        "MCP WS server closed the connection".to_string(),
                    ));
                }
                _ => None,
            };

            let Some(message) = parsed else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            return json_rpc_result(message, id);
        }
    }
}

impl SseMcpTransport {
    async fn connect(url: String, headers: HashMap<String, String>) -> ServiceResult<Self> {
        let client = reqwest::Client::builder()
            .redirect(mcp_redirect_policy())
            .build()?;
        let base_url = validate_http_url(HttpNetworkSurface::HttpMcp, &url).map_err(|error| {
            ServiceError::Connection(format!("invalid MCP SSE url '{}': {}", url, error))
        })?;

        let response = apply_headers(
            client
                .get(base_url.clone())
                .header(reqwest::header::ACCEPT, "text/event-stream"),
            &headers,
        )?
        .send()
        .await
        .map_err(map_mcp_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(mcp_http_status_error("SSE", status, body));
        }

        let mut events = response.bytes_stream().eventsource();
        let post_url = timeout(Duration::from_secs(10), async {
            while let Some(event) = events.next().await {
                let event = event.map_err(|error| {
                    ServiceError::Connection(format!("MCP SSE stream failed: {}", error))
                })?;
                if event.event == "endpoint" {
                    return resolve_sse_endpoint(&base_url, &event.data);
                }
            }
            Err(ServiceError::Connection(
                "MCP SSE stream ended before sending an endpoint event".to_string(),
            ))
        })
        .await
        .map_err(|_| ServiceError::Connection("MCP SSE endpoint timed out".to_string()))??;

        let pending: PendingSseResponses = Arc::new(Mutex::new(HashMap::new()));
        spawn_sse_reader(events, pending.clone());

        Ok(Self {
            client,
            post_url,
            headers,
            pending,
            next_id: 1,
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> ServiceResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let response = apply_headers(
            self.client.post(&self.post_url).json(&request),
            &self.headers,
        )?
        .send()
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.pending.lock().await.remove(&id);
                return Err(map_mcp_reqwest_error(error));
            }
        };
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            self.pending.lock().await.remove(&id);
            return Err(mcp_http_status_error("SSE", status, body));
        }

        if !body.trim().is_empty() {
            self.pending.lock().await.remove(&id);
            return json_rpc_result(serde_json::from_str(&body)?, id);
        }

        match timeout(Duration::from_secs(10), receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ServiceError::Connection(format!(
                "MCP SSE response channel closed for request '{}'",
                method
            ))),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(ServiceError::Connection(format!(
                    "MCP SSE request '{}' timed out",
                    method
                )))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> ServiceResult<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let response = self
            .post_json_request(&notification)?
            .send()
            .await
            .map_err(map_mcp_reqwest_error)?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(mcp_http_status_error("SSE", status, body));
        }
        Ok(())
    }

    fn post_json_request(&self, message: &Value) -> ServiceResult<reqwest::RequestBuilder> {
        apply_headers(
            self.client.post(&self.post_url).json(message),
            &self.headers,
        )
    }
}

fn spawn_sse_reader<S>(mut events: S, pending: PendingSseResponses)
where
    S: futures::Stream<
            Item = Result<
                eventsource_stream::Event,
                eventsource_stream::EventStreamError<reqwest::Error>,
            >,
        > + Send
        + Unpin
        + 'static,
{
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            match event {
                Ok(event) => {
                    if event.event != "message" {
                        continue;
                    }
                    let Ok(message) = serde_json::from_str::<Value>(&event.data) else {
                        continue;
                    };
                    let Some(id) = message.get("id").and_then(Value::as_u64) else {
                        continue;
                    };
                    if let Some(sender) = pending.lock().await.remove(&id) {
                        let _ = sender.send(json_rpc_result(message, id));
                    }
                }
                Err(error) => {
                    let mut pending = pending.lock().await;
                    for (_, sender) in pending.drain() {
                        let _ = sender.send(Err(ServiceError::Connection(format!(
                            "MCP SSE stream failed: {}",
                            error
                        ))));
                    }
                    break;
                }
            }
        }
    });
}

fn resolve_sse_endpoint(base_url: &reqwest::Url, endpoint: &str) -> ServiceResult<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err(ServiceError::Connection(
            "MCP SSE endpoint event was empty".to_string(),
        ));
    }
    let url = base_url.join(endpoint).map_err(|error| {
        ServiceError::Connection(format!(
            "invalid MCP SSE endpoint '{}': {}",
            endpoint, error
        ))
    })?;
    validate_http_redirect(HttpNetworkSurface::HttpMcp, base_url, &url).map_err(|error| {
        ServiceError::Connection(format!(
            "invalid MCP SSE endpoint '{}': {}",
            endpoint, error
        ))
    })?;
    Ok(url.to_string())
}

fn mcp_http_status_error(surface: &str, status: reqwest::StatusCode, body: String) -> ServiceError {
    let status_code = status.as_u16();
    if matches!(status_code, 401 | 403) {
        return ServiceError::Auth(format!(
            "MCP {surface} authentication failed: status={status_code}, body={body}"
        ));
    }
    ServiceError::Http {
        status: status_code,
        body,
    }
}

impl HttpMcpTransport {
    fn new(url: String, headers: HashMap<String, String>) -> ServiceResult<Self> {
        let url = validate_http_url(HttpNetworkSurface::HttpMcp, &url).map_err(|error| {
            ServiceError::Connection(format!("invalid MCP HTTP url '{}': {}", url, error))
        })?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(mcp_redirect_policy())
            .build()?;
        Ok(Self {
            client,
            url: url.to_string(),
            headers,
            next_id: 1,
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> ServiceResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let message = self.post_json(&request).await?;
        json_rpc_result(message, id)
    }

    async fn notify(&self, method: &str, params: Value) -> ServiceResult<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let _ = self.post_json(&notification).await?;
        Ok(())
    }

    async fn post_json(&self, message: &Value) -> ServiceResult<Value> {
        let response = apply_headers(self.client.post(&self.url).json(message), &self.headers)?
            .send()
            .await
            .map_err(map_mcp_reqwest_error)?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(mcp_http_status_error("HTTP", status, body));
        }
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).map_err(ServiceError::from)
    }
}

fn mcp_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let Some(previous_url) = attempt.previous().last() else {
            return attempt.follow();
        };
        if let Err(reason) =
            validate_http_redirect(HttpNetworkSurface::HttpMcp, previous_url, attempt.url())
        {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                reason,
            ));
        }
        attempt.follow()
    })
}

fn apply_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HashMap<String, String>,
) -> ServiceResult<reqwest::RequestBuilder> {
    for (name, value) in headers {
        let header_name =
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                ServiceError::Connection(format!("invalid MCP header name '{}': {}", name, error))
            })?;
        let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|error| {
            ServiceError::Connection(format!(
                "invalid MCP header value for '{}': {}",
                name, error
            ))
        })?;
        request = request.header(header_name, header_value);
    }
    Ok(request)
}

fn map_mcp_reqwest_error(error: reqwest::Error) -> ServiceError {
    let detailed = reqwest_error_with_sources(&error);
    if detailed.contains("network policy denied URL")
        || detailed.contains("network policy denied redirect")
    {
        ServiceError::Connection(detailed)
    } else {
        ServiceError::Reqwest(error)
    }
}

fn reqwest_error_with_sources(error: &reqwest::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        let detail = error.to_string();
        if !message.contains(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = error.source();
    }
    message
}

fn json_rpc_result(message: Value, id: u64) -> ServiceResult<Value> {
    if message.is_null() {
        return Ok(Value::Null);
    }
    if message.get("id").and_then(Value::as_u64) != Some(id) {
        return Err(ServiceError::Connection(format!(
            "MCP response id mismatch: expected {}, got {}",
            id,
            message
                .get("id")
                .map(Value::to_string)
                .unwrap_or_else(|| "null".to_string())
        )));
    }
    if let Some(error) = message.get("error") {
        return Err(ServiceError::Connection(format!(
            "MCP request failed: {}",
            error
        )));
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

fn parse_tools(result: &Value) -> Vec<McpTool> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    let description = tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let input_schema = tool
                        .get("inputSchema")
                        .or_else(|| tool.get("input_schema"))
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object" }));
                    Some(McpTool {
                        name,
                        description,
                        input_schema,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_resources(result: &Value) -> Vec<McpResource> {
    result
        .get("resources")
        .and_then(Value::as_array)
        .map(|resources| {
            resources
                .iter()
                .filter_map(|resource| {
                    let uri = resource.get("uri")?.as_str()?.to_string();
                    let name = resource
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&uri)
                        .to_string();
                    let description = resource
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let mime_type = resource
                        .get("mimeType")
                        .or_else(|| resource.get("mime_type"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Some(McpResource {
                        uri,
                        name,
                        description,
                        mime_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_resource_templates(result: &Value) -> Vec<McpResourceTemplate> {
    result
        .get("resourceTemplates")
        .or_else(|| result.get("resource_templates"))
        .and_then(Value::as_array)
        .map(|templates| {
            templates
                .iter()
                .filter_map(|template| {
                    let uri_template = template
                        .get("uriTemplate")
                        .or_else(|| template.get("uri_template"))?
                        .as_str()?
                        .to_string();
                    let name = template
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&uri_template)
                        .to_string();
                    let description = template
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let mime_type = template
                        .get("mimeType")
                        .or_else(|| template.get("mime_type"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Some(McpResourceTemplate {
                        uri_template,
                        name,
                        description,
                        mime_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_prompts(result: &Value) -> Vec<McpPrompt> {
    result
        .get("prompts")
        .and_then(Value::as_array)
        .map(|prompts| {
            prompts
                .iter()
                .filter_map(|prompt| {
                    let name = prompt.get("name")?.as_str()?.to_string();
                    let description = prompt
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let arguments = prompt
                        .get("arguments")
                        .and_then(Value::as_array)
                        .map(|arguments| {
                            arguments
                                .iter()
                                .filter_map(|argument| {
                                    let name = argument.get("name")?.as_str()?.to_string();
                                    let description = argument
                                        .get("description")
                                        .and_then(Value::as_str)
                                        .map(str::to_string);
                                    let required = argument
                                        .get("required")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false);
                                    Some(McpPromptArgument {
                                        name,
                                        description,
                                        required,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(McpPrompt {
                        name,
                        description,
                        arguments,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{McpClient, McpServerConfig, TransportType};
    use futures::{SinkExt, StreamExt};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{mpsc, Mutex as TokioMutex};
    use tokio::task::JoinHandle;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    #[tokio::test]
    async fn connect_rejects_stdio_server_without_command() {
        let mut client = McpClient::new(McpServerConfig {
            name: "local".to_string(),
            transport: TransportType::Stdio,
            url: None,
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err().to_string();
        assert!(err.contains("has no command"), "{err}");
    }

    #[tokio::test]
    async fn connect_rejects_http_server_without_url() {
        let mut client = McpClient::new(McpServerConfig {
            name: "remote".to_string(),
            transport: TransportType::Http,
            url: None,
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err().to_string();
        assert!(err.contains("has no url"), "{err}");
    }

    #[tokio::test]
    async fn connect_rejects_ws_server_without_url() {
        let mut client = McpClient::new(McpServerConfig {
            name: "remote-ws".to_string(),
            transport: TransportType::Ws,
            url: None,
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err().to_string();
        assert!(err.contains("has no url"), "{err}");
    }

    #[tokio::test]
    async fn stdio_client_lists_and_calls_tools() {
        if Command::new("python3").arg("--version").output().is_err() {
            eprintln!("skipping stdio MCP test because python3 is unavailable");
            return;
        }

        let script = write_mock_mcp_server();
        let mut client = McpClient::new(McpServerConfig {
            name: "mock".to_string(),
            transport: TransportType::Stdio,
            url: None,
            command: Some("python3".to_string()),
            args: Some(vec!["-u".to_string(), script.display().to_string()]),
            env: None,
            headers: None,
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].input_schema["type"], "object");
        let resources = client.list_resources().await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "mock://status");
        assert_mock_prompt_roundtrip(&client, "stdio prompt").await;

        let result = client
            .call_tool(
                "echo",
                HashMap::from([("message".to_string(), json!("hello"))]),
            )
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "hello");
        let resource = client.read_resource("mock://status").await.unwrap();
        assert_eq!(resource["contents"][0]["text"], "mock ready");

        let _ = fs::remove_file(script);
    }

    #[tokio::test]
    async fn stdio_client_passes_configured_env_to_child() {
        if Command::new("python3").arg("--version").output().is_err() {
            eprintln!("skipping stdio MCP env test because python3 is unavailable");
            return;
        }

        let script = write_env_probe_mcp_server();
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-env".to_string(),
            transport: TransportType::Stdio,
            url: None,
            command: Some("python3".to_string()),
            args: Some(vec!["-u".to_string(), script.display().to_string()]),
            env: Some(HashMap::from([(
                "KIANA_TEST_MCP_ENV".to_string(),
                "from-config".to_string(),
            )])),
            headers: None,
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools[0].description, "from-config");

        let _ = fs::remove_file(script);
    }

    #[tokio::test]
    async fn http_client_lists_and_calls_tools() {
        let (url, server) = start_mock_http_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-http".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let resources = client.list_resources().await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].mime_type.as_deref(), Some("text/plain"));
        assert_mock_prompt_roundtrip(&client, "http prompt").await;

        let result = client
            .call_tool(
                "echo",
                HashMap::from([("message".to_string(), json!("hello http"))]),
            )
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "hello http");
        let resource = client.read_resource("mock://status").await.unwrap();
        assert_eq!(resource["contents"][0]["text"], "mock ready");

        server.abort();
    }

    #[tokio::test]
    async fn http_client_lists_resource_templates() {
        let (url, server) = start_mock_http_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-http".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        client.connect().await.unwrap();
        let templates = client.list_resource_templates().await.unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].uri_template, "mock://logs/{date}");
        assert_eq!(templates[0].name, "Mock daily logs");
        assert_eq!(templates[0].mime_type.as_deref(), Some("text/plain"));

        server.abort();
    }

    #[tokio::test]
    async fn http_client_blocks_cross_origin_redirects() {
        let (url, server) = start_mock_http_redirect_mcp_server("http://127.0.0.1:9/mcp").await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-http-redirect".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err().to_string();
        assert!(err.contains("network policy denied redirect"), "{err}");

        server.abort();
    }

    #[tokio::test]
    async fn http_client_sends_configured_headers() {
        let (url, server) =
            start_mock_http_mcp_server_requiring_header("authorization", "Bearer test").await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-http-headers".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer test".to_string(),
            )])),
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);

        server.abort();
    }

    #[tokio::test]
    async fn http_client_reports_missing_auth_as_auth_error() {
        let (url, server) =
            start_mock_http_mcp_server_requiring_header("authorization", "Bearer test").await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-http-auth".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err();
        assert_eq!(err.classify(), "auth_error");
        let message = err.to_string();
        assert!(message.contains("MCP HTTP authentication failed"));
        assert!(message.contains("status=401"));
        assert!(message.contains("missing required header"));

        server.abort();
    }

    #[tokio::test]
    async fn sse_client_lists_and_calls_tools() {
        let (url, server) = start_mock_sse_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-sse".to_string(),
            transport: TransportType::Sse,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let resources = client.list_resources().await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "mock://status");
        assert_mock_prompt_roundtrip(&client, "sse prompt").await;

        let result = client
            .call_tool(
                "echo",
                HashMap::from([("message".to_string(), json!("hello sse"))]),
            )
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "hello sse");
        let resource = client.read_resource("mock://status").await.unwrap();
        assert_eq!(resource["contents"][0]["text"], "mock ready");

        server.abort();
    }

    #[tokio::test]
    async fn sse_client_blocks_cross_origin_endpoint_events() {
        let (url, server) =
            start_mock_sse_mcp_server_with_endpoint("http://127.0.0.1:9/message".to_string()).await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-sse-redirect".to_string(),
            transport: TransportType::Sse,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err().to_string();
        assert!(err.contains("network policy denied redirect"), "{err}");

        server.abort();
    }

    #[tokio::test]
    async fn sse_client_sends_configured_headers() {
        let (url, server) =
            start_mock_sse_mcp_server_requiring_header("authorization", "Bearer sse").await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-sse-headers".to_string(),
            transport: TransportType::Sse,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer sse".to_string(),
            )])),
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);

        server.abort();
    }

    #[tokio::test]
    async fn sse_client_reports_missing_auth_as_auth_error() {
        let (url, server) =
            start_mock_sse_mcp_server_requiring_header("authorization", "Bearer sse").await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-sse-auth".to_string(),
            transport: TransportType::Sse,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        let err = client.connect().await.unwrap_err();
        assert_eq!(err.classify(), "auth_error");
        let message = err.to_string();
        assert!(message.contains("MCP SSE authentication failed"));
        assert!(message.contains("status=401"));
        assert!(message.contains("missing required header"));

        server.abort();
    }

    #[tokio::test]
    async fn ws_client_lists_calls_and_reads_resources() {
        let (url, server) = start_mock_ws_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-ws".to_string(),
            transport: TransportType::Ws,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });

        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let resources = client.list_resources().await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "mock://status");
        assert_mock_prompt_roundtrip(&client, "ws prompt").await;

        let result = client
            .call_tool(
                "echo",
                HashMap::from([("message".to_string(), json!("hello ws"))]),
            )
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "hello ws");
        let resource = client.read_resource("mock://status").await.unwrap();
        assert_eq!(resource["contents"][0]["text"], "mock ready");

        server.abort();
    }

    #[tokio::test]
    async fn mock_mcp_transports_surface_tool_error_results() {
        if Command::new("python3").arg("--version").output().is_ok() {
            let script = write_mock_mcp_server();
            let mut client = McpClient::new(McpServerConfig {
                name: "mock".to_string(),
                transport: TransportType::Stdio,
                url: None,
                command: Some("python3".to_string()),
                args: Some(vec!["-u".to_string(), script.display().to_string()]),
                env: None,
                headers: None,
            });
            client.connect().await.unwrap();
            assert_mock_tool_error_roundtrip(&client, "stdio").await;
            let _ = fs::remove_file(script);
        } else {
            eprintln!("skipping stdio MCP error fixture because python3 is unavailable");
        }

        let (url, server) = start_mock_http_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-http".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });
        client.connect().await.unwrap();
        assert_mock_tool_error_roundtrip(&client, "http").await;
        server.abort();

        let (url, server) = start_mock_sse_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-sse".to_string(),
            transport: TransportType::Sse,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });
        client.connect().await.unwrap();
        assert_mock_tool_error_roundtrip(&client, "sse").await;
        server.abort();

        let (url, server) = start_mock_ws_mcp_server().await;
        let mut client = McpClient::new(McpServerConfig {
            name: "mock-ws".to_string(),
            transport: TransportType::Ws,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });
        client.connect().await.unwrap();
        assert_mock_tool_error_roundtrip(&client, "ws").await;
        server.abort();
    }

    async fn assert_mock_prompt_roundtrip(client: &McpClient, topic: &str) {
        let prompts = client.list_prompts().await.unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "summarize");
        assert_eq!(prompts[0].arguments[0].name, "topic");

        let result = client
            .get_prompt(
                "summarize",
                HashMap::from([("topic".to_string(), json!(topic))]),
            )
            .await
            .unwrap();
        assert!(result["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains(topic));
    }

    async fn assert_mock_tool_error_roundtrip(client: &McpClient, transport: &str) {
        let result = client
            .call_tool(
                "fail",
                HashMap::from([("message".to_string(), json!("denied by fake server"))]),
            )
            .await
            .unwrap();
        assert_eq!(result["isError"], true, "{transport}: {result}");
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));
    }

    fn write_mock_mcp_server() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kiana-mock-mcp-{}-{}.py",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::write(
            &path,
            r#"
import json
import sys

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                "serverInfo": {"name": "mock", "version": "1.0.0"}
            }
        }), flush=True)
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "tools": [{
                    "name": "echo",
                    "description": "Echo a message",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"message": {"type": "string"}},
                        "required": ["message"]
                    }
                }]
            }
        }), flush=True)
    elif method == "tools/call":
        args = msg.get("params", {}).get("arguments", {})
        tool_name = msg.get("params", {}).get("name", "")
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "content": [{"type": "text", "text": args.get("message", "")}],
                "isError": tool_name == "fail"
            }
        }), flush=True)
    elif method == "resources/list":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "resources": [{
                    "uri": "mock://status",
                    "name": "Mock status",
                    "description": "Mock server status",
                    "mimeType": "text/plain"
                }]
            }
        }), flush=True)
    elif method == "resources/templates/list":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "resourceTemplates": [{
                    "uriTemplate": "mock://logs/{date}",
                    "name": "Mock daily logs",
                    "description": "Mock logs for a date",
                    "mimeType": "text/plain"
                }]
            }
        }), flush=True)
    elif method == "resources/read":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "contents": [{
                    "uri": msg.get("params", {}).get("uri", ""),
                    "mimeType": "text/plain",
                    "text": "mock ready"
                }]
            }
        }), flush=True)
    elif method == "prompts/list":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "prompts": [{
                    "name": "summarize",
                    "description": "Summarize a topic",
                    "arguments": [{
                        "name": "topic",
                        "description": "Topic to summarize",
                        "required": False
                    }]
                }]
            }
        }), flush=True)
    elif method == "prompts/get":
        args = msg.get("params", {}).get("arguments", {})
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "description": "Summarize a topic",
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Summarize " + args.get("topic", "something")
                    }
                }]
            }
        }), flush=True)
    else:
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        }), flush=True)
"#,
        )
        .unwrap();
        path
    }

    fn write_env_probe_mcp_server() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kiana-env-mcp-{}-{}.py",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::write(
            &path,
            r#"
import json
import os
import sys

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "env-probe", "version": "1.0.0"}
            }
        }), flush=True)
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "tools": [{
                    "name": "env",
                    "description": os.environ.get("KIANA_TEST_MCP_ENV", ""),
                    "inputSchema": {"type": "object"}
                }]
            }
        }), flush=True)
    elif method == "resources/list":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"resources": []}}), flush=True)
    elif method == "resources/templates/list":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"resourceTemplates": []}}), flush=True)
    elif method == "prompts/list":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"prompts": []}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": msg.get("id"), "result": {}}), flush=True)
"#,
        )
        .unwrap();
        path
    }

    async fn start_mock_http_mcp_server() -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let _ = handle_mock_http_mcp_request(stream).await;
                });
            }
        });
        (format!("http://{}", addr), handle)
    }

    async fn start_mock_http_mcp_server_requiring_header(
        header_name: &'static str,
        header_value: &'static str,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let _ = handle_mock_http_mcp_request_requiring_header(
                        stream,
                        header_name,
                        header_value,
                    )
                    .await;
                });
            }
        });
        (format!("http://{}", addr), handle)
    }

    async fn start_mock_http_redirect_mcp_server(
        location: &'static str,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let _ = handle_mock_http_redirect_mcp_request(stream, location).await;
                });
            }
        });
        (format!("http://{}", addr), handle)
    }

    async fn handle_mock_http_mcp_request(mut stream: TcpStream) -> std::io::Result<()> {
        let request = read_http_request(&mut stream).await?;
        let message: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let Some(response) = mock_mcp_response(&message, "mock-http") else {
            write_http_response(&mut stream, 204, "").await?;
            return Ok(());
        };

        write_http_response(&mut stream, 200, &response.to_string()).await
    }

    async fn handle_mock_http_redirect_mcp_request(
        mut stream: TcpStream,
        location: &str,
    ) -> std::io::Result<()> {
        let _request = read_http_request(&mut stream).await?;
        let response = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n",
            location
        );
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await
    }

    async fn handle_mock_http_mcp_request_requiring_header(
        mut stream: TcpStream,
        header_name: &str,
        header_value: &str,
    ) -> std::io::Result<()> {
        let request = read_http_request(&mut stream).await?;
        if request.headers.get(header_name) != Some(&header_value.to_string()) {
            write_http_response(&mut stream, 401, "missing required header").await?;
            return Ok(());
        }
        let message: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let Some(response) = mock_mcp_response(&message, "mock-http-headers") else {
            write_http_response(&mut stream, 204, "").await?;
            return Ok(());
        };

        write_http_response(&mut stream, 200, &response.to_string()).await
    }

    async fn start_mock_sse_mcp_server() -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint = format!("http://{}/message?sessionId=test-session", addr);
        let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>(32);
        let event_rx = Arc::new(TokioMutex::new(Some(event_rx)));

        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let endpoint = endpoint.clone();
                let event_tx = event_tx.clone();
                let event_rx = event_rx.clone();
                tokio::spawn(async move {
                    let _ = handle_mock_sse_mcp_request(stream, endpoint, event_tx, event_rx).await;
                });
            }
        });
        (format!("http://{}/sse", addr), handle)
    }

    async fn start_mock_sse_mcp_server_with_endpoint(endpoint: String) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>(32);
        let event_rx = Arc::new(TokioMutex::new(Some(event_rx)));

        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let endpoint = endpoint.clone();
                let event_tx = event_tx.clone();
                let event_rx = event_rx.clone();
                tokio::spawn(async move {
                    let _ = handle_mock_sse_mcp_request(stream, endpoint, event_tx, event_rx).await;
                });
            }
        });
        (format!("http://{}/sse", addr), handle)
    }

    async fn start_mock_sse_mcp_server_requiring_header(
        header_name: &'static str,
        header_value: &'static str,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint = format!("http://{}/message?sessionId=test-session", addr);
        let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>(32);
        let event_rx = Arc::new(TokioMutex::new(Some(event_rx)));

        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let endpoint = endpoint.clone();
                let event_tx = event_tx.clone();
                let event_rx = event_rx.clone();
                tokio::spawn(async move {
                    let _ = handle_mock_sse_mcp_request_requiring_header(
                        stream,
                        endpoint,
                        event_tx,
                        event_rx,
                        header_name,
                        header_value,
                    )
                    .await;
                });
            }
        });
        (format!("http://{}/sse", addr), handle)
    }

    async fn start_mock_ws_mcp_server() -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let _ = handle_mock_ws_mcp_request(stream).await;
                });
            }
        });
        (format!("ws://{}", addr), handle)
    }

    async fn handle_mock_ws_mcp_request(stream: TcpStream) -> std::io::Result<()> {
        let mut websocket = accept_async(stream).await.map_err(std::io::Error::other)?;

        while let Some(message) = websocket.next().await {
            let message = message.map_err(std::io::Error::other)?;
            let value = match message {
                Message::Text(text) => serde_json::from_str::<serde_json::Value>(&text).ok(),
                Message::Binary(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes).ok(),
                Message::Ping(payload) => {
                    websocket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(std::io::Error::other)?;
                    None
                }
                Message::Close(_) => break,
                _ => None,
            };

            let Some(value) = value else {
                continue;
            };
            if let Some(response) = mock_mcp_response(&value, "mock-ws") {
                websocket
                    .send(Message::Text(response.to_string()))
                    .await
                    .map_err(std::io::Error::other)?;
            }
        }

        Ok(())
    }

    async fn handle_mock_sse_mcp_request(
        mut stream: TcpStream,
        endpoint: String,
        event_tx: mpsc::Sender<serde_json::Value>,
        event_rx: Arc<TokioMutex<Option<mpsc::Receiver<serde_json::Value>>>>,
    ) -> std::io::Result<()> {
        let request = read_http_request(&mut stream).await?;
        if request.method == "GET" && request.path == "/sse" {
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\nevent: endpoint\ndata: {}\n\n",
                endpoint
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.flush().await?;

            let Some(mut receiver) = event_rx.lock().await.take() else {
                return Ok(());
            };
            while let Some(message) = receiver.recv().await {
                let event = format!("event: message\ndata: {}\n\n", message);
                stream.write_all(event.as_bytes()).await?;
                stream.flush().await?;
            }
            return Ok(());
        }

        if request.method == "POST" && request.path.starts_with("/message") {
            let message: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            if let Some(response) = mock_mcp_response(&message, "mock-sse") {
                let _ = event_tx.send(response).await;
            }
            write_http_response(&mut stream, 202, "").await?;
            return Ok(());
        }

        write_http_response(&mut stream, 404, "").await
    }

    async fn handle_mock_sse_mcp_request_requiring_header(
        mut stream: TcpStream,
        endpoint: String,
        event_tx: mpsc::Sender<serde_json::Value>,
        event_rx: Arc<TokioMutex<Option<mpsc::Receiver<serde_json::Value>>>>,
        header_name: &str,
        header_value: &str,
    ) -> std::io::Result<()> {
        let request = read_http_request(&mut stream).await?;
        if request.headers.get(header_name) != Some(&header_value.to_string()) {
            write_http_response(&mut stream, 401, "missing required header").await?;
            return Ok(());
        }
        if request.method == "GET" && request.path == "/sse" {
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\nevent: endpoint\ndata: {}\n\n",
                endpoint
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.flush().await?;

            let Some(mut receiver) = event_rx.lock().await.take() else {
                return Ok(());
            };
            while let Some(message) = receiver.recv().await {
                let event = format!("event: message\ndata: {}\n\n", message);
                stream.write_all(event.as_bytes()).await?;
                stream.flush().await?;
            }
            return Ok(());
        }

        if request.method == "POST" && request.path.starts_with("/message") {
            let message: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            if let Some(response) = mock_mcp_response(&message, "mock-sse-headers") {
                let _ = event_tx.send(response).await;
            }
            write_http_response(&mut stream, 202, "").await?;
            return Ok(());
        }

        write_http_response(&mut stream, 404, "").await
    }

    struct MockHttpRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<MockHttpRequest> {
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        let header_end;
        loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Ok(MockHttpRequest {
                    method: String::new(),
                    path: String::new(),
                    headers: HashMap::new(),
                    body: Vec::new(),
                });
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(position) = find_header_end(&buffer) {
                header_end = position;
                break;
            }
        }

        let headers = String::from_utf8_lossy(&buffer[..header_end]);
        let request_line = headers.lines().next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let header_map = headers
            .lines()
            .skip(1)
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
            })
            .collect::<HashMap<_, _>>();
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
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        Ok(MockHttpRequest {
            method,
            path,
            headers: header_map,
            body: buffer[body_start..body_start + content_length].to_vec(),
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: u16,
        body: &str,
    ) -> std::io::Result<()> {
        let reason = match status {
            202 => "Accepted",
            204 => "No Content",
            404 => "Not Found",
            _ => "OK",
        };
        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            reason,
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await
    }

    fn mock_mcp_response(
        message: &serde_json::Value,
        server_name: &str,
    ) -> Option<serde_json::Value> {
        let method = message.get("method").and_then(|value| value.as_str());
        let id = message.get("id").cloned().unwrap_or(json!(null));

        match method {
            Some("notifications/initialized") => None,
            Some("initialize") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": server_name, "version": "1.0.0"}
                }
            })),
            Some("tools/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "echo",
                        "description": "Echo a message",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"message": {"type": "string"}},
                            "required": ["message"]
                        }
                    }]
                }
            })),
            Some("tools/call") => {
                let tool_name = message
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let args = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let is_error = tool_name == "fail";
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": args.get("message").and_then(|value| value.as_str()).unwrap_or("")
                        }],
                        "isError": is_error
                    }
                }))
            }
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
                            .and_then(|value| value.as_str())
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
                            "required": false
                        }]
                    }]
                }
            })),
            Some("prompts/get") => {
                let topic = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .and_then(|args| args.get("topic"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("something");
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "description": "Summarize a topic",
                        "messages": [{
                            "role": "user",
                            "content": {
                                "type": "text",
                                "text": format!("Summarize {topic}")
                            }
                        }]
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
