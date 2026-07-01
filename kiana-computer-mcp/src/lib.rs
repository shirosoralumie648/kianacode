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
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotDims {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuPermissionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<AppInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuPermissionResponse {
    pub granted: Vec<AppInfo>,
    pub denied: Vec<AppInfo>,
    pub flags: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuCallToolResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolContent>>,
}

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

fn native_backend_disabled_message() -> String {
    "Native computer-use backend is not enabled in this build. Rebuild kiana-computer-mcp with `--features native` and install the required Linux desktop development libraries before using screenshot, input, or clipboard tools.".to_string()
}

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

pub struct ComputerMcpServer;

impl ComputerMcpServer {
    pub fn new() -> Self {
        Self
    }

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

    async fn handle_tool_call(&self, name: &str, args: Value) -> Value {
        match call_tool(name, args).await {
            Ok(result) => cu_call_tool_result_for_mcp(result),
            Err(error) => mcp_tool_error(error.to_string()),
        }
    }
}

impl Default for ComputerMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run_stdio() -> Result<()> {
    let server = ComputerMcpServer::new();
    let stdin = BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    server.serve_stdio(stdin, stdout).await
}

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

fn empty_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {}
    })
}

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
pub async fn call_tool(name: &str, _args: serde_json::Value) -> Result<CuCallToolResult> {
    match name {
        "computer_use_status" => Ok(success_text(native_backend_disabled_message())),
        name if is_native_tool(name) => Ok(error_text(native_backend_disabled_message())),
        _ => Ok(error_text(format!("Unknown tool: {}", name))),
    }
}

#[cfg(feature = "native")]
pub async fn call_tool(name: &str, args: serde_json::Value) -> Result<CuCallToolResult> {
    match name {
        "computer_use_status" => Ok(success_text(
            "Native computer-use backend is enabled in this build.".into(),
        )),
        "screenshot" => {
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
            let mut clipboard = arboard::Clipboard::new()?;
            let text = clipboard.get_text()?;
            Ok(success_text(text))
        }
        "write_clipboard" => {
            let text = args["text"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing text"))?;
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(text)?;
            Ok(success_text("Clipboard written".into()))
        }
        "request_access" => Ok(success_text("Computer-use access granted".into())),
        "list_granted_applications" => Ok(success_text("No restrictions".into())),
        _ => Ok(error_text(format!("Unknown tool: {}", name))),
    }
}

#[cfg(test)]
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
