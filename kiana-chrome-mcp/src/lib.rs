use base64::{engine::general_purpose::STANDARD, Engine as _};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::ScreenshotParams as ChromiumScreenshotParams;
use chromiumoxide::Page;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

pub mod native_host;
pub mod native_install;

#[derive(Debug, Serialize, Deserialize)]
pub struct NavigateParams {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClickParams {
    pub selector: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeParams {
    pub selector: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EvaluateParams {
    pub script: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScreenshotRequest {
    pub full_page: Option<bool>,
}

pub struct ChromeClient {
    browser: Browser,
    current_page: Arc<Mutex<Option<Page>>>,
}

impl ChromeClient {
    pub async fn new() -> anyhow::Result<Self> {
        let config = BrowserConfig::builder()
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build browser config: {}", e))?;
        let (browser, mut handler) = Browser::launch(config).await?;

        tokio::spawn(async move { while let Some(_) = handler.next().await {} });

        Ok(Self {
            browser,
            current_page: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn navigate(&self, url: &str) -> anyhow::Result<String> {
        let page = self.browser.new_page("about:blank").await?;
        page.goto(url).await?;
        *self.current_page.lock().await = Some(page);
        Ok(format!("Navigated to {}", url))
    }

    pub async fn get_content(&self) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let content = page.content().await?;
        Ok(content)
    }

    pub async fn click(&self, selector: &str) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let element = page.find_element(selector).await?;
        element.click().await?;
        Ok(format!("Clicked {}", selector))
    }

    pub async fn type_text(&self, selector: &str, text: &str) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let element = page.find_element(selector).await?;
        element.click().await?;
        element.type_str(text).await?;
        Ok(format!("Typed into {}", selector))
    }

    pub async fn evaluate(&self, script: &str) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let result = page.evaluate(script).await?;
        let value = result.value().cloned().unwrap_or(serde_json::Value::Null);
        Ok(serde_json::to_string(&value)?)
    }

    pub async fn screenshot(&self, full_page: bool) -> anyhow::Result<Vec<u8>> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let params = ChromiumScreenshotParams::builder()
            .full_page(full_page)
            .build();
        let bytes = page.screenshot(params).await?;
        Ok(bytes)
    }

    pub async fn new_tab(&self, url: Option<&str>) -> anyhow::Result<String> {
        let page = self.browser.new_page(url.unwrap_or("about:blank")).await?;
        *self.current_page.lock().await = Some(page);
        Ok("New tab created".to_string())
    }

    pub async fn close_tab(&self) -> anyhow::Result<String> {
        let mut guard = self.current_page.lock().await;
        if let Some(page) = guard.take() {
            page.close().await?;
            Ok("Tab closed".to_string())
        } else {
            Err(anyhow::anyhow!("No active page"))
        }
    }
}

pub struct ChromeMcpServer {
    client: Mutex<Option<ChromeClient>>,
}

impl ChromeMcpServer {
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
        }
    }

    pub async fn serve_stdio<R, W>(&self, reader: R, mut writer: W) -> anyhow::Result<()>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut reader = reader;
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 {
                return Ok(());
            }
            let Ok(request) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(response) = self.handle_json_rpc(request).await {
                writer
                    .write_all(serde_json::to_string(&response)?.as_bytes())
                    .await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
        }
    }

    pub async fn handle_json_rpc(&self, request: Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str)?;

        if id.is_none() {
            return None;
        }
        let id = id.unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "kiana-chrome-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "tools/list" => Ok(json!({ "tools": chrome_tools() })),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.handle_tool_call(name, args).await
            }
            "resources/list" => Ok(json!({ "resources": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            _ => Err(anyhow::anyhow!("Method {method} not found")),
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

    async fn handle_tool_call(&self, name: &str, args: Value) -> anyhow::Result<Value> {
        if !chrome_tools()
            .iter()
            .any(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
        {
            return Ok(mcp_tool_error(format!("Unknown tool: {name}")));
        }
        let call = match ChromeToolCall::parse(name, &args) {
            Ok(call) => call,
            Err(message) => return Ok(mcp_tool_error(message)),
        };

        let mut client = self.client.lock().await;
        if client.is_none() {
            *client = Some(ChromeClient::new().await?);
        }
        let client = client.as_ref().expect("chrome client initialized");
        let result = match call {
            ChromeToolCall::Navigate(url) => client.navigate(&url).await,
            ChromeToolCall::GetContent => client.get_content().await,
            ChromeToolCall::Click(selector) => client.click(&selector).await,
            ChromeToolCall::Type { selector, text } => client.type_text(&selector, &text).await,
            ChromeToolCall::Evaluate(script) => client.evaluate(&script).await,
            ChromeToolCall::Screenshot { full_page } => client
                .screenshot(full_page)
                .await
                .map(|bytes| STANDARD.encode(bytes)),
            ChromeToolCall::NewTab(url) => client.new_tab(url.as_deref()).await,
            ChromeToolCall::CloseTab => client.close_tab().await,
        };

        Ok(match result {
            Ok(text) => json!({
                "content": [{
                    "type": "text",
                    "text": text
                }],
                "structuredContent": {
                    "text": text
                },
                "isError": false
            }),
            Err(error) => mcp_tool_error(error.to_string()),
        })
    }
}

enum ChromeToolCall {
    Navigate(String),
    GetContent,
    Click(String),
    Type { selector: String, text: String },
    Evaluate(String),
    Screenshot { full_page: bool },
    NewTab(Option<String>),
    CloseTab,
}

impl ChromeToolCall {
    fn parse(name: &str, args: &Value) -> Result<Self, String> {
        match name {
            "navigate" => Ok(Self::Navigate(required_string(args, "url")?.to_string())),
            "get_content" => Ok(Self::GetContent),
            "click" => Ok(Self::Click(required_string(args, "selector")?.to_string())),
            "type" => Ok(Self::Type {
                selector: required_string(args, "selector")?.to_string(),
                text: required_string(args, "text")?.to_string(),
            }),
            "evaluate" => Ok(Self::Evaluate(required_string(args, "script")?.to_string())),
            "screenshot" => Ok(Self::Screenshot {
                full_page: args
                    .get("full_page")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }),
            "new_tab" => Ok(Self::NewTab(
                args.get("url").and_then(Value::as_str).map(str::to_string),
            )),
            "close_tab" => Ok(Self::CloseTab),
            _ => Err(format!("Unknown tool: {name}")),
        }
    }
}

impl Default for ChromeMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run_stdio() -> anyhow::Result<()> {
    let server = ChromeMcpServer::new();
    let stdin = BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    server.serve_stdio(stdin, stdout).await
}

pub async fn run_native_host_stdio() -> anyhow::Result<()> {
    native_host::run_native_host_stdio().await
}

fn chrome_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "navigate",
            "description": "Navigate to a URL",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "get_content",
            "description": "Get page HTML content",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "click",
            "description": "Click an element",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": { "type": "string" }
                },
                "required": ["selector"]
            }
        }),
        json!({
            "name": "type",
            "description": "Type text into an element",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["selector", "text"]
            }
        }),
        json!({
            "name": "evaluate",
            "description": "Execute JavaScript",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "script": { "type": "string" }
                },
                "required": ["script"]
            }
        }),
        json!({
            "name": "screenshot",
            "description": "Take a screenshot",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "full_page": { "type": "boolean" }
                }
            }
        }),
        json!({
            "name": "new_tab",
            "description": "Open a new browser tab",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "close_tab",
            "description": "Close current tab",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn tools_list_does_not_launch_browser() {
        let server = ChromeMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            }))
            .await
            .unwrap();

        let names = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"navigate"));
        assert!(names.contains(&"screenshot"));
        assert!(server.client.lock().await.is_none());
    }

    #[tokio::test]
    async fn initialize_returns_mcp_server_info() {
        let server = ChromeMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["serverInfo"]["name"], "kiana-chrome-mcp");
        assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
        assert!(server.client.lock().await.is_none());
    }

    #[tokio::test]
    async fn unknown_tool_returns_mcp_tool_error_without_launching_browser() {
        let server = ChromeMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "missing_tool",
                    "arguments": {}
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
        assert!(server.client.lock().await.is_none());
    }

    #[tokio::test]
    async fn missing_required_argument_returns_tool_error_without_launching_browser() {
        let server = ChromeMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "navigate",
                    "arguments": {}
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("url is required"));
        assert!(server.client.lock().await.is_none());
    }

    #[tokio::test]
    async fn notifications_do_not_emit_responses() {
        let server = ChromeMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }))
            .await;

        assert!(response.is_none());
    }
}
