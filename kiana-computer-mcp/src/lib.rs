//! # 文件职责
//!
//! 本文件实现一个独立的计算机使用（computer use）MCP 服务：默认构建只报告 native backend 状态；
//! 编译 `native` feature 后，工具可以截屏、移动鼠标、点击、输入按键/文字和读写剪贴板。
//! MCP 指模型上下文协议（Model Context Protocol），本 crate 用 JSON-RPC 请求/响应包装这些工具。
//!
//! # 调用链与产品边界
//!
//! ```text
//! kiana-entrypoints CLI 的 computer-mcp aliases
//!   -> run_stdio() -> ComputerMcpServer -> handle_json_rpc()
//!   -> tools/list 或 tools/call -> call_tool()
//!   -> native feature 的 xcap / enigo / arboard -> 当前桌面
//!   -> CuCallToolResult -> MCP content -> stdio
//! ```
//!
//! CLI 入口直接运行这个 MCP server，没有进入 DaemonHost、ControlPlane、Policy/Gate 或 CapabilityBroker。
//! `docs/coding-pack-matrix.md` 将 computer use 标为 feature-gated、非产品路径和冻结面；本注释说明
//! 当前代码如何工作，不表示它已纳入主 Harness 授权链。
//!
//! Tokio 提供异步 stdio 循环；Serde derive 负责结构体与 JSON 间转换，不负责权限判断。`native` feature
//! 会引入实际桌面控制库，因此成功调用可以影响当前登录用户的鼠标、键盘、屏幕或剪贴板。

#[cfg(feature = "native")]
use anyhow::anyhow;
use anyhow::Result;
#[cfg(feature = "native")]
use base64::{engine::general_purpose::STANDARD, Engine as _};
#[cfg(feature = "native")]
use enigo::{Axis, Button, Coordinate, Enigo, Key, Keyboard, Mouse, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
#[cfg(feature = "native")]
use xcap::Monitor;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// MCP 工具目录的一条声明；`input_schema=None` 会在外层转换时变成空 object schema。
pub struct ToolDef {
    /// 工具名；必须与 `call_tool()` 的 match 分支一致。
    pub name: String,
    /// 供 MCP 客户端显示的工具说明。
    pub description: String,
    /// 可选参数 JSON Schema；当前目录项都未提供具体字段 schema。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 截图尺寸 DTO；当前 `screenshot_result()` 忽略传入的宽高，MCP 输出不包含这些字段。
pub struct ScreenshotDims {
    /// 截图图片宽度。
    pub width: u32,
    /// 截图图片高度。
    pub height: u32,
    /// 可选桌面显示器宽度。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_width: Option<u32>,
    /// 可选桌面显示器高度。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 请求计算机/应用权限的协议 DTO；当前工具实现没有用它创建真实权限请求。
pub struct CuPermissionRequest {
    /// 可选的应用身份列表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<AppInfo>>,
    /// 可选权限名称到 bool 的请求标记。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 权限请求里的应用描述；当前 crate 中没有真实授权记录消费者。
pub struct AppInfo {
    /// 应用 bundle ID 或平台应用标识。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    /// 面向用户展示的应用名称。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 权限 API 的结果 DTO；字段表示 granted/denied 分类，但不等同于操作系统授权回执。
pub struct CuPermissionResponse {
    /// 被报告为允许的应用。
    pub granted: Vec<AppInfo>,
    /// 被报告为拒绝的应用。
    pub denied: Vec<AppInfo>,
    /// 权限名称到结果 bool 的映射。
    pub flags: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// MCP 返回内容的一项；`content_type` 会序列化成协议字段 `type`。
pub struct ToolContent {
    /// 内容种类，例如 `text` 或 `image`。
    #[serde(rename = "type")]
    pub content_type: String,
    /// 可选人类可读文本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 可选 Base64 编码的二进制数据。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// 数据的 MIME 类型，例如截图的 `image/png`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 桌面工具内部结果 DTO，随后由 adapter 转成 MCP `content` 和 `isError`。
pub struct CuCallToolResult {
    /// 可选错误标记；缺省时 MCP adapter 按 false 处理。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// 可选内容数组；缺省时 MCP adapter 返回空数组。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolContent>>,
}

/// 根据当前编译 feature 返回可见的 computer-use 工具目录。
///
/// 未启用 `native` 时仅返回状态工具，避免宣告实际上无法调用的桌面操作；启用后列出完整工具名。
/// 这些 ToolDef 的 `input_schema` 当前仍为 None，`computer_tools_for_mcp()` 会补成无属性的空对象 schema，
/// 所以 schema 并未描述 mouse/type/key 所需参数。该方法只生成目录，不检查 OS 权限或执行工具。
pub fn build_computer_use_tools() -> Vec<ToolDef> {
    #[cfg(not(feature = "native"))]
    {
        return vec![ToolDef {
            name: "computer_use_status".into(),
            description:
                "Report whether native computer-use support was compiled into this binary.".into(),
            input_schema: None,
        }];
    }

    #[cfg(feature = "native")]
    vec![
        ToolDef {
            name: "computer_use_status".into(),
            description:
                "Report whether native computer-use support was compiled into this binary.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "request_access".into(),
            description:
                "Request access to applications and computer-use permissions for this session."
                    .into(),
            input_schema: None,
        },
        ToolDef {
            name: "list_granted_applications".into(),
            description: "List applications currently granted for computer use.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "screenshot".into(),
            description: "Capture a screenshot.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "cursor_position".into(),
            description: "Read the current cursor position.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "mouse_move".into(),
            description: "Move the mouse cursor.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "left_click".into(),
            description: "Left click at a coordinate.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "right_click".into(),
            description: "Right click at a coordinate.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "middle_click".into(),
            description: "Middle click at a coordinate.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "double_click".into(),
            description: "Double click at a coordinate.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "left_mouse_down".into(),
            description: "Press the left mouse button.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "left_mouse_up".into(),
            description: "Release the left mouse button.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "scroll".into(),
            description: "Scroll at a coordinate or direction.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "type".into(),
            description: "Type text through the active application.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "key".into(),
            description: "Press a key or key chord.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "read_clipboard".into(),
            description: "Read clipboard text.".into(),
            input_schema: None,
        },
        ToolDef {
            name: "write_clipboard".into(),
            description: "Write clipboard text.".into(),
            input_schema: None,
        },
    ]
}

/// 将文本包装为没有设置错误标记的桌面工具结果。
fn success_text(text: String) -> CuCallToolResult {
    CuCallToolResult {
        is_error: None,
        content: Some(vec![ToolContent {
            content_type: "text".into(),
            text: Some(text),
            data: None,
            mime_type: None,
        }]),
    }
}

/// 将文本包装为显式 `is_error=true` 的桌面工具结果。
fn error_text(text: String) -> CuCallToolResult {
    CuCallToolResult {
        is_error: Some(true),
        content: Some(vec![ToolContent {
            content_type: "text".into(),
            text: Some(text),
            data: None,
            mime_type: None,
        }]),
    }
}

/// 默认构建中向调用者解释 native 桌面依赖未启用的固定提示。
fn native_backend_disabled_message() -> String {
    "Native computer-use backend is not enabled in this build. Rebuild kiana-computer-mcp with `--features native` and install the required Linux desktop development libraries before using screenshot, input, or clipboard tools.".to_string()
}

/// 判断名字是否属于需要 native feature 的桌面工具集合。
///
/// 默认 build 用它把“已知但不可用”的工具与真正未知工具区分开。
fn is_native_tool(name: &str) -> bool {
    matches!(
        name,
        "request_access"
            | "list_granted_applications"
            | "screenshot"
            | "cursor_position"
            | "mouse_move"
            | "left_click"
            | "right_click"
            | "middle_click"
            | "double_click"
            | "left_mouse_down"
            | "left_mouse_up"
            | "scroll"
            | "type"
            | "key"
            | "read_clipboard"
            | "write_clipboard"
    )
}

/// 无持久状态的 MCP stdio server；每个调用直接进入本模块的工具函数。
///
/// 它没有保存授权/session/当前鼠标状态，也不会把工具请求委托给 ControlPlane。
pub struct ComputerMcpServer;

impl ComputerMcpServer {
    /// 构造零状态的 server，不会探测桌面设备或申请系统权限。
    pub fn new() -> Self {
        Self
    }

    /// 按行读取 JSON-RPC 请求，把有响应的结果写回 stdout。
    ///
    /// EOF 正常结束；无效 JSON 行会跳过；读写或 JSON 编码错误通过 Err 返回。没有 `id` 的通知由
    /// `handle_json_rpc()` 返回 None，因此不写响应。stdout 是 MCP 传输通道，不能混入调试文本。
    pub async fn serve_stdio<R, W>(&self, reader: R, mut writer: W) -> Result<()>
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

    /// 处理单个 JSON-RPC 值，支持初始化、工具列表/调用和空 resources/prompts 列表。
    ///
    /// 只有存在 `id` 的请求返回 envelope；未知方法变成 JSON-RPC error，工具执行结果则是 MCP tool result。
    /// 当前代码没有校验 `jsonrpc` 版本，也没有在 initialize 后维护协议状态。
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
                    "name": "kiana-computer-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "tools/list" => Ok(json!({ "tools": computer_tools_for_mcp() })),
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
                Ok(self.handle_tool_call(name, args).await)
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

    /// 将 `call_tool()` 的结构化结果转为 MCP tool result；Rust Err 映射为 `isError=true` 文本。
    async fn handle_tool_call(&self, name: &str, args: Value) -> Value {
        match call_tool(name, args).await {
            Ok(result) => cu_call_tool_result_for_mcp(result),
            Err(error) => mcp_tool_error(error.to_string()),
        }
    }
}

impl Default for ComputerMcpServer {
    /// 与 `new()` 一样构造无状态 server。
    fn default() -> Self {
        Self::new()
    }
}

/// 从进程 stdin/stdout 启动 ComputerMcpServer；由 CLI 的 computer-mcp aliases 调用。
pub async fn run_stdio() -> Result<()> {
    let server = ComputerMcpServer::new();
    let stdin = BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    server.serve_stdio(stdin, stdout).await
}

/// 把内部 ToolDef 转成 MCP `tools/list` 要求的 JSON 对象。
///
/// `input_schema=None` 会被替换为 `empty_input_schema()`；该兜底保证 schema 字段存在，但不会补出真实参数定义。
fn computer_tools_for_mcp() -> Vec<Value> {
    build_computer_use_tools()
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema.unwrap_or_else(empty_input_schema),
            })
        })
        .collect()
}

/// 构造只声明 object、没有任何 properties 的空参数 schema。
fn empty_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {}
    })
}

/// 从内部可选字段结果构造 MCP 的 `content` 与布尔 `isError`。
///
/// 两个 Option 缺省分别变成空内容与 false，因此这一步会把“未提供”和“明确为 false”折叠为同一结果。
fn cu_call_tool_result_for_mcp(result: CuCallToolResult) -> Value {
    let is_error = result.is_error.unwrap_or(false);
    let content = result
        .content
        .unwrap_or_default()
        .into_iter()
        .map(tool_content_for_mcp)
        .collect::<Vec<_>>();

    json!({
        "content": content,
        "isError": is_error
    })
}

/// 把 Rust ToolContent DTO 映射为协议 JSON，并只输出实际存在的可选字段。
fn tool_content_for_mcp(content: ToolContent) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("type".to_string(), Value::String(content.content_type));
    if let Some(text) = content.text {
        value.insert("text".to_string(), Value::String(text));
    }
    if let Some(data) = content.data {
        value.insert("data".to_string(), Value::String(data));
    }
    if let Some(mime_type) = content.mime_type {
        value.insert("mimeType".to_string(), Value::String(mime_type));
    }
    Value::Object(value)
}

/// 生成 MCP 工具级错误；与 JSON-RPC envelope 中的 `error` 字段不是同一层。
fn mcp_tool_error(message: String) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": message
        }],
        "isError": true
    })
}

#[cfg(feature = "native")]
/// 将 Base64 PNG 包装为 image 内容；宽高参数当前未被写入结果。
fn screenshot_result(data: String, _width: u32, _height: u32) -> CuCallToolResult {
    CuCallToolResult {
        is_error: None,
        content: Some(vec![ToolContent {
            content_type: "image".into(),
            text: None,
            data: Some(data),
            mime_type: Some("image/png".into()),
        }]),
    }
}

#[cfg(not(feature = "native"))]
/// 默认编译配置下的工具分派：状态工具报告 disabled，其余 native 工具返回明确错误结果。
///
/// `_args` 被忽略，因为该分支不会触达 OS 桌面 API；未知名称也以 tool error 返回，而非 Rust Err。
pub async fn call_tool(name: &str, _args: serde_json::Value) -> Result<CuCallToolResult> {
    match name {
        "computer_use_status" => Ok(success_text(native_backend_disabled_message())),
        name if is_native_tool(name) => Ok(error_text(native_backend_disabled_message())),
        _ => Ok(error_text(format!("Unknown tool: {}", name))),
    }
}

#[cfg(feature = "native")]
/// native feature 下将工具名映射成实际截屏、输入或剪贴板操作。
///
/// 输入 JSON 缺字段、桌面库初始化失败或 OS API 返回错误时通过 anyhow::Result 返回；server wrapper
/// 会把这些错误转成 MCP `isError=true`。函数不做应用 allowlist、屏幕坐标范围验证或真实权限申请；
/// 当前 `request_access`/`list_granted_applications` 分支只是固定文本 stub。
pub async fn call_tool(name: &str, args: serde_json::Value) -> Result<CuCallToolResult> {
    match name {
        "computer_use_status" => Ok(success_text(
            "Native computer-use backend is enabled in this build.".into(),
        )),
        "screenshot" => {
            // 仅截取 Monitor::all() 返回列表中的第一个显示器；图片先编码为 PNG，再 Base64 放入 MCP image content。
            let monitors = Monitor::all()?;
            let monitor = monitors
                .first()
                .ok_or_else(|| anyhow!("No monitors found"))?;
            let image = monitor.capture_image()?;
            let mut png_data = Vec::new();
            image.write_to(
                &mut std::io::Cursor::new(&mut png_data),
                image::ImageFormat::Png,
            )?;
            let base64_data = STANDARD.encode(&png_data);
            Ok(screenshot_result(
                base64_data,
                image.width(),
                image.height(),
            ))
        }
        "cursor_position" => {
            let enigo = Enigo::new(&Settings::default())?;
            let (x, y) = enigo.location()?;
            Ok(success_text(format!("Cursor position: ({}, {})", x, y)))
        }
        "mouse_move" => {
            // 该工具要求 x/y 同时是可表示为 i64 的 JSON 整数，之后直接窄化为 i32 并按绝对屏幕坐标移动；
            // 当前分支没有检查溢出后的坐标或显示器范围。
            let x = args["x"]
                .as_i64()
                .ok_or_else(|| anyhow!("Missing x coordinate"))?;
            let y = args["y"]
                .as_i64()
                .ok_or_else(|| anyhow!("Missing y coordinate"))?;
            let mut enigo = Enigo::new(&Settings::default())?;
            enigo.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
            Ok(success_text("Mouse moved".into()))
        }
        // 这四类 click 仅在 x/y 同时存在且可读为整数时先移动鼠标；只缺一个坐标时不会报错，而是在当前位置点击。
        "left_click" => {
            let mut enigo = Enigo::new(&Settings::default())?;
            if let (Some(x), Some(y)) = (args["x"].as_i64(), args["y"].as_i64()) {
                enigo.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
            }
            enigo.button(Button::Left, enigo::Direction::Click)?;
            Ok(success_text("Left click".into()))
        }
        "right_click" => {
            let mut enigo = Enigo::new(&Settings::default())?;
            if let (Some(x), Some(y)) = (args["x"].as_i64(), args["y"].as_i64()) {
                enigo.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
            }
            enigo.button(Button::Right, enigo::Direction::Click)?;
            Ok(success_text("Right click".into()))
        }
        "middle_click" => {
            let mut enigo = Enigo::new(&Settings::default())?;
            if let (Some(x), Some(y)) = (args["x"].as_i64(), args["y"].as_i64()) {
                enigo.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
            }
            enigo.button(Button::Middle, enigo::Direction::Click)?;
            Ok(success_text("Middle click".into()))
        }
        "double_click" => {
            let mut enigo = Enigo::new(&Settings::default())?;
            if let (Some(x), Some(y)) = (args["x"].as_i64(), args["y"].as_i64()) {
                enigo.move_mouse(x as i32, y as i32, Coordinate::Abs)?;
            }
            enigo.button(Button::Left, enigo::Direction::Click)?;
            enigo.button(Button::Left, enigo::Direction::Click)?;
            Ok(success_text("Double click".into()))
        }
        // 按下和释放是两个独立调用；本函数没有跨调用的 guard 自动配对，调用端需要明确安排 release。
        "left_mouse_down" => {
            let mut enigo = Enigo::new(&Settings::default())?;
            enigo.button(Button::Left, enigo::Direction::Press)?;
            Ok(success_text("Left mouse down".into()))
        }
        "left_mouse_up" => {
            let mut enigo = Enigo::new(&Settings::default())?;
            enigo.button(Button::Left, enigo::Direction::Release)?;
            Ok(success_text("Left mouse up".into()))
        }
        "scroll" => {
            // 未提供合法整数 amount 时默认向垂直方向滚动 1；值直接转为 i32，没有统一范围限制。
            let amount = args["amount"].as_i64().unwrap_or(1);
            let mut enigo = Enigo::new(&Settings::default())?;
            enigo.scroll(amount as i32, Axis::Vertical)?;
            Ok(success_text("Scrolled".into()))
        }
        "type" => {
            let text = args["text"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing text"))?;
            let mut enigo = Enigo::new(&Settings::default())?;
            enigo.text(text)?;
            Ok(success_text("Text typed".into()))
        }
        "key" => {
            // 常见按键名映射到 Enigo Key；其他输入只取第一个 Unicode 字符，空字符串会退回空格键。
            let key_str = args["key"].as_str().ok_or_else(|| anyhow!("Missing key"))?;
            let mut enigo = Enigo::new(&Settings::default())?;
            let key = match key_str {
                "return" | "enter" => Key::Return,
                "escape" => Key::Escape,
                "backspace" => Key::Backspace,
                "delete" => Key::Delete,
                "tab" => Key::Tab,
                "space" => Key::Space,
                _ => Key::Unicode(key_str.chars().next().unwrap_or(' ')),
            };
            enigo.key(key, enigo::Direction::Click)?;
            Ok(success_text("Key pressed".into()))
        }
        "read_clipboard" => {
            // 读取当前系统剪贴板文本；这不是隔离的应用内剪贴板。
            let mut clipboard = arboard::Clipboard::new()?;
            let text = clipboard.get_text()?;
            Ok(success_text(text))
        }
        "write_clipboard" => {
            // 覆盖当前系统剪贴板文本，属于会影响用户桌面状态的写操作。
            let text = args["text"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing text"))?;
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(text)?;
            Ok(success_text("Clipboard written".into()))
        }
        // ⚠ 这两个响应是固定 stub：没有系统权限弹窗、应用列表或授权状态存储。
        "request_access" => Ok(success_text("Computer-use access granted".into())),
        "list_granted_applications" => Ok(success_text("No restrictions".into())),
        _ => Ok(error_text(format!("Unknown tool: {}", name))),
    }
}

#[cfg(test)]
/// 协议 DTO 和默认配置行为的本地测试；不会证明 native 桌面操作经过 ControlPlane 授权。
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_tools() {
        let tools = build_computer_use_tools();
        assert!(!tools.is_empty());
        #[cfg(feature = "native")]
        assert!(tools.iter().any(|t| t.name == "screenshot"));
        #[cfg(not(feature = "native"))]
        assert_eq!(tools[0].name, "computer_use_status");
    }

    #[tokio::test]
    async fn test_permission_tools() {
        let result = call_tool("request_access", serde_json::json!({}))
            .await
            .unwrap();
        #[cfg(feature = "native")]
        assert!(result.is_error.is_none() || !result.is_error.unwrap());
        #[cfg(not(feature = "native"))]
        {
            assert_eq!(result.is_error, Some(true));
            let text = result.content.unwrap()[0].text.clone().unwrap();
            assert!(text.contains("Native computer-use backend is not enabled"));
        }
    }

    #[tokio::test]
    async fn initialize_returns_mcp_server_info() {
        let server = ComputerMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            }))
            .await
            .unwrap();

        assert_eq!(
            response["result"]["serverInfo"]["name"],
            "kiana-computer-mcp"
        );
        assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
    }

    #[tokio::test]
    async fn tools_list_exposes_mcp_input_schema() {
        let server = ComputerMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            }))
            .await
            .unwrap();

        let tools = response["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
        #[cfg(not(feature = "native"))]
        assert_eq!(tools[0]["name"], "computer_use_status");
    }

    #[tokio::test]
    async fn status_tool_returns_mcp_tool_result() {
        let server = ComputerMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "computer_use_status",
                    "arguments": {}
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Native computer-use backend"));
    }

    #[cfg(not(feature = "native"))]
    #[tokio::test]
    async fn native_tool_call_returns_actionable_error_in_default_build() {
        let server = ComputerMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "screenshot",
                    "arguments": {}
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Rebuild kiana-computer-mcp with `--features native`"));
    }

    #[tokio::test]
    async fn notifications_do_not_emit_responses() {
        let server = ComputerMcpServer::new();
        let response = server
            .handle_json_rpc(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }))
            .await;

        assert!(response.is_none());
    }

    #[tokio::test]
    async fn serve_stdio_writes_json_rpc_responses() {
        let input = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"computer_use_status","arguments":{}}}
"#;
        let server = ComputerMcpServer::new();
        let reader = BufReader::new(input.as_slice());
        let mut output = Vec::new();

        server.serve_stdio(reader, &mut output).await.unwrap();

        let output = String::from_utf8(output).unwrap();
        let responses = output
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(responses.len(), 3);
        assert_eq!(
            responses[0]["result"]["serverInfo"]["name"],
            "kiana-computer-mcp"
        );
        assert_eq!(
            responses[1]["result"]["tools"][0]["name"],
            "computer_use_status"
        );
        assert_eq!(responses[2]["result"]["isError"], false);
    }
}
