//! # 文件职责
//!
//! 本文件实现一个独立的 Chrome 浏览器模型上下文协议（Model Context Protocol，MCP）服务：
//! 将标准输入/输出（stdio）上的 JSON-RPC 请求转换为 chromiumoxide 浏览器操作，再把结果编码回响应。
//! `chromiumoxide` 是通过 Chrome DevTools Protocol 驱动 Chromium 的 Rust 库；Tokio 提供异步运行时。
//! `serde` 的 `Serialize` / `Deserialize` derive 只负责 Rust 值与 JSON 间的数据转换，不做输入授权。
//! `main.rs` 默认进入 `run_stdio()`；
//! `kiana-entrypoints/src/cli.rs` 还提供显式的 `--claude-in-chrome-mcp` 与
//! `--chrome-native-host` 分支。
//!
//! # 调用位置与边界
//!
//! ```text
//! 独立 CLI / MCP Host
//!   -> run_stdio() -> serve_stdio() -> handle_json_rpc()
//!   -> tools/call -> 参数解析 -> ChromeClient -> chromiumoxide / Chrome
//!   -> MCP tool result -> stdio
//! ```
//!
//! 这条路径没有进入 KianaHarness、ControlPlane、Policy/Gate 或 CapabilityBroker；它不是产品
//! Harness 的五种模型可见工具之一。`docs/coding-pack-matrix.md` 将 Chrome automation 标为
//! 非 Harness 且冻结。本注释描述当前可见接线，不代表该独立入口已纳入主执行授权链。
//!
//! `native_host` 子模块处理浏览器扩展的 Native Messaging 帧和本机 socket 桥接；
//! `native_install` 子模块规划/写入 wrapper 与浏览器 manifest。浏览器自动化本身留在
//! `ChromeClient`，MCP 请求/响应映射留在 `ChromeMcpServer`。

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
/// `navigate` 工具的参数形状；当前实际 dispatcher 通过 `ChromeToolCall::parse()` 解析 JSON `Value`。
pub struct NavigateParams {
    /// 要交给浏览器打开的 URL 原文。
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
/// `click` 工具的参数形状；selector 会交给 Chromium 页面查询。
pub struct ClickParams {
    /// CSS 选择器。
    pub selector: String,
}

#[derive(Debug, Serialize, Deserialize)]
/// `type` 工具的参数形状；文本将输入到指定页面元素。
pub struct TypeParams {
    /// 目标元素的 CSS 选择器。
    pub selector: String,
    /// 要输入的文本。
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
/// `evaluate` 工具的参数形状；脚本会在当前页面的 JavaScript 上下文中执行。
pub struct EvaluateParams {
    /// 浏览器页面脚本原文。
    pub script: String,
}

#[derive(Debug, Serialize, Deserialize)]
/// 截图请求参数；缺省 `full_page` 由实际 parser 解释为 `false`。
pub struct ScreenshotRequest {
    /// 是否截取整个页面，而不是当前视口。
    pub full_page: Option<bool>,
}

/// 管理 Chromium 浏览器连接以及服务当前选中的页面。
///
/// `browser` 由 chromiumoxide 启动并通过异步 DevTools 协议驱动；`current_page` 是所有调用共用的
/// 页面槽位。互斥锁在页面操作的 await 期间仍被持有，因此同一个 client 的浏览器调用会串行化，
/// 页面替换也不会与另一个正在执行的操作同时改写该槽位。
///
/// 这是独立 Chrome MCP 路径中的浏览器适配器，不自行做 Kiana Policy、文件权限或网页信任判断。
pub struct ChromeClient {
    /// chromiumoxide 管理的浏览器实例。
    browser: Browser,
    /// 当前页面句柄；还未导航/建 tab 时为 None。
    current_page: Arc<Mutex<Option<Page>>>,
}

impl ChromeClient {
    /// 启动一个 Chromium 实例并创建尚无当前页面的 client。
    ///
    /// chromiumoxide 还需要持续消费 Browser handler 事件，因此这里把 handler 放进 Tokio 后台任务；
    /// 本方法返回 client，不等待该任务结束。配置构造或浏览器启动失败会向 MCP server 上抛错误。
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

    /// 新建一个空白页面，导航到给定 URL，并把它设为共享的当前页面。
    ///
    /// URL 与导航错误由 chromiumoxide 返回；已有 current_page 不会在此处显式关闭。
    pub async fn navigate(&self, url: &str) -> anyhow::Result<String> {
        let page = self.browser.new_page("about:blank").await?;
        page.goto(url).await?;
        *self.current_page.lock().await = Some(page);
        Ok(format!("Navigated to {}", url))
    }

    /// 读取当前页面的 HTML 内容；还没有当前页面时返回 `No active page`。
    ///
    /// 页面句柄锁会保持到 Chromium 返回内容，因此同一个 client 的其他页面操作需等待。
    pub async fn get_content(&self) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let content = page.content().await?;
        Ok(content)
    }

    /// 在当前页面查找第一个匹配元素并点击。
    ///
    /// 选择器无匹配元素、页面不存在或浏览器协议调用失败都会返回错误；这里不做 selector 白名单。
    pub async fn click(&self, selector: &str) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let element = page.find_element(selector).await?;
        element.click().await?;
        Ok(format!("Clicked {}", selector))
    }

    /// 先点击当前页面中匹配的元素，再把文本输入该元素。
    ///
    /// 点击和输入属于同一个串行持锁操作，避免同一 client 的另一条工具请求中途切换 current_page。
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

    /// 在当前页面上下文执行调用方传入的 JavaScript，并把返回值序列化为 JSON 字符串。
    ///
    /// 页面不存在、Chromium 求值失败或 JSON 编码失败时返回 Err。该接口可运行任意脚本；本文件
    /// 没有额外的脚本权限或来源检查，因此调用入口必须按独立浏览器自动化能力对待。
    pub async fn evaluate(&self, script: &str) -> anyhow::Result<String> {
        let guard = self.current_page.lock().await;
        let page = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active page"))?;
        let result = page.evaluate(script).await?;
        let value = result.value().cloned().unwrap_or(serde_json::Value::Null);
        Ok(serde_json::to_string(&value)?)
    }

    /// 截取当前页面并返回图片原始字节；外层 MCP dispatcher 再把字节编码成 Base64 文本。
    ///
    /// `full_page=true` 请求整个页面，false 使用 Chromium 的默认可见范围；无当前页面或截图失败返回 Err。
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

    /// 新建 tab，并让它成为整个 client 共享的 current_page。
    ///
    /// `url=None` 时打开 `about:blank`。这不是“创建独立 session”：后续所有工具仍访问这个共享槽位。
    pub async fn new_tab(&self, url: Option<&str>) -> anyhow::Result<String> {
        let page = self.browser.new_page(url.unwrap_or("about:blank")).await?;
        *self.current_page.lock().await = Some(page);
        Ok("New tab created".to_string())
    }

    /// 取出并关闭共享 current_page；没有选中页面时返回错误。
    ///
    /// 页面句柄先从槽位移走，再等待 Chromium 关闭，所以关闭失败后槽位仍为空，不会自动恢复旧句柄。
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

/// 将 stdio JSON-RPC/MCP 消息映射到一个懒启动的 `ChromeClient`。
///
/// 该 server 仅实现本文件列出的 initialize、tools 与空 resources/prompts 列表；它不使用通用 MCP
/// SDK，也不把每个 tool call 送入 ControlPlane。`client` 锁覆盖整个浏览器请求处理过程，所以同一
/// server 实例一次只处理一个 Chrome 操作。
pub struct ChromeMcpServer {
    /// 首个有效且已知的工具调用到来时才初始化浏览器；列表/协议查询不启动 Chromium。
    client: Mutex<Option<ChromeClient>>,
}

impl ChromeMcpServer {
    /// 创建尚未启动 Chromium 的 MCP server。
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
        }
    }

    /// 按行读取 stdio 上的 JSON-RPC 请求，并把需要响应的结果写回 stdout。
    ///
    /// 输入每行解析为一个 JSON 值；无效 JSON 行会被跳过，不产生响应。`handle_json_rpc()` 返回
    /// `None`（例如没有 `id` 的通知）时也不写响应。到达 EOF 正常结束；读写和 JSON 编码失败向调用者返回 Err。
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

    /// 把一个 JSON 值分派到当前实现的 MCP 方法，并生成 JSON-RPC 响应值。
    ///
    /// 只有带 `id` 的请求会返回响应；支持 initialize、tools/list、tools/call、resources/list 和
    /// prompts/list。未知方法被包装为 JSON-RPC 错误（当前统一使用 -32603）；本方法不校验
    /// `jsonrpc` 版本字段，也不执行协议握手状态机。
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

    /// 验证工具名和参数，把一次 MCP tools/call 映射为 ChromeClient 方法。
    ///
    /// 未知工具/参数错误和浏览器操作失败作为 MCP `isError=true` 的 tool result 返回；Chrome 启动错误
    /// 仍作为 Err 交回 JSON-RPC 层。锁会覆盖 client 懒初始化和浏览器操作，保证共享页面操作串行执行。
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

/// `tools/call` 的已解析内部表示；让 JSON 参数检查和浏览器实际执行分成两步。
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
    /// 按工具名从 JSON arguments 提取所需字段；可选的 full_page 缺省 false，new_tab URL 可省略。
    ///
    /// 此处只做字段类型/是否为空检查，不验证 URL 安全性、CSS selector 语义或脚本内容。
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
    /// 与 `new()` 一样创建惰性 server，不启动外部浏览器。
    fn default() -> Self {
        Self::new()
    }
}

/// 启动行分隔 JSON-RPC/MCP 模式；该输入协议与 Native Host 的二进制长度帧不同。
pub async fn run_stdio() -> anyhow::Result<()> {
    let server = ChromeMcpServer::new();
    let stdin = BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    server.serve_stdio(stdin, stdout).await
}

/// 启动 Chrome Native Messaging Host 模式；与上面的行分隔 MCP stdio server 是不同帧协议。
pub async fn run_native_host_stdio() -> anyhow::Result<()> {
    native_host::run_native_host_stdio().await
}

/// 返回 MCP `tools/list` 宣告的工具名、说明和 JSON Schema。
///
/// 实际 dispatch 在 `ChromeToolCall::parse()` 中另有一份工具名 match；两处需要保持一致，否则
/// 客户端可能看见无法调用的工具，或隐藏仍能被直接调用的名字。
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

/// 从 arguments 中取非空字符串，并借用原字符串切片。
///
/// `trim()` 只用于判断是否全为空白，成功时返回的仍是未 trim 的原值；调用者随后直接把它交给浏览器。
fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

/// 生成 MCP 工具级错误结果；这与 JSON-RPC `error` 字段是两层不同的错误表示。
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
