use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct ReplInput {
    calls: Vec<ReplCall>,
    #[serde(default = "default_stop_on_error")]
    stop_on_error: bool,
}

#[derive(Debug, Deserialize)]
struct ReplCall {
    tool: String,
    #[serde(default)]
    input: Value,
}

pub struct ReplTool;

impl ReplTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReplTool {
    fn name(&self) -> &str {
        "REPL"
    }

    fn description(&self) -> &str {
        "Run a batch of primitive tool calls in order"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("batch read, edit, search, shell, notebook, and agent operations")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Primitive tool name: Read, Write, Edit, Glob, Grep, Bash, NotebookEdit, or Agent"
                            },
                            "input": {
                                "type": "object"
                            }
                        },
                        "required": ["tool"]
                    }
                },
                "stop_on_error": {
                    "type": "boolean",
                    "description": "Stop executing subsequent calls after the first validation, permission, or execution error"
                }
            },
            "required": ["calls"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" },
                "ok": { "type": "boolean" },
                "executed": { "type": "integer" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: ReplInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.calls.is_empty() || input.calls.len() > 50 {
            return ValidationResult::err("calls must contain 1 to 50 items".to_string(), 2);
        }
        for (index, call) in input.calls.iter().enumerate() {
            if call.tool.trim().is_empty() {
                return ValidationResult::err(format!("calls[{index}].tool cannot be empty"), 3);
            }
            if primitive_tool(&call.tool).is_none() {
                return ValidationResult::err(
                    format!(
                        "calls[{index}].tool '{}' is not available in REPL",
                        call.tool
                    ),
                    4,
                );
            }
            if !call.input.is_object() {
                return ValidationResult::err(format!("calls[{index}].input must be an object"), 5);
            }
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ReplInput = serde_json::from_value(input.clone())?;
        if input.calls.is_empty() || input.calls.len() > 50 {
            return Err(ToolError::ValidationError(
                "calls must contain 1 to 50 items".to_string(),
            ));
        }

        let mut results = Vec::new();
        let mut ok = true;

        for (index, call) in input.calls.into_iter().enumerate() {
            let Some(tool) = primitive_tool(&call.tool) else {
                ok = false;
                results.push(repl_error(
                    index,
                    &call.tool,
                    "tool is not available in REPL",
                ));
                if input.stop_on_error {
                    break;
                }
                continue;
            };

            let tool_name = tool.name().to_string();
            let mut registry = crate::ToolRegistry::new();
            registry.register(tool);
            registry.register(Arc::new(crate::mcp_tool::McpTool::new()));
            let result = crate::tool_execution::execute_tool_call(
                &registry,
                None,
                context,
                &tool_name,
                &call.input,
                None,
            )
            .await;
            if result.is_error {
                ok = false;
                results.push(repl_error(
                    index,
                    &tool_name,
                    &result
                        .content
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| result.content.to_string()),
                ));
                if input.stop_on_error {
                    break;
                }
                continue;
            }

            results.push(json!({
                "index": index,
                "tool": tool_name,
                "ok": true,
                "output": result.content
            }));
        }

        let executed = results.len();
        Ok(ToolOutput {
            data: json!({
                "results": results,
                "ok": ok,
                "executed": executed
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_repl_result_for_model(&output.data)
        })
    }
}

fn format_repl_result_for_model(data: &Value) -> String {
    let ok = data.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let executed = data
        .get("executed")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let mut lines = vec![format!(
        "REPL batch {}. Executed {} call(s).",
        if ok { "succeeded" } else { "failed" },
        executed
    )];

    if let Some(results) = data.get("results").and_then(Value::as_array) {
        for result in results {
            let index = result
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let tool = result
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                lines.push(format!("{}. {tool}: ok", index + 1));
                if let Some(output) = result.get("output") {
                    let rendered = render_repl_output(output);
                    if !rendered.trim().is_empty() {
                        lines.push(indent_lines(&rendered, "   "));
                    }
                }
            } else {
                let error = result
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                lines.push(format!("{}. {tool}: error: {error}", index + 1));
            }
        }
    }

    lines.join("\n")
}

fn render_repl_output(output: &Value) -> String {
    if let Some(text) = output.as_str() {
        return text.to_string();
    }
    if let Some(stdout) = output
        .get("stdout")
        .and_then(Value::as_str)
        .filter(|stdout| !stdout.trim().is_empty())
    {
        return stdout.trim_end().to_string();
    }
    if let Some(content) = output
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
    {
        return content.to_string();
    }
    serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string())
}

fn indent_lines(value: &str, indent: &str) -> String {
    value
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn default_stop_on_error() -> bool {
    true
}

fn primitive_tool(name: &str) -> Option<Arc<dyn Tool>> {
    let normalized = normalize_tool_name(name);
    match normalized.as_str() {
        "read" | "fileread" => Some(Arc::new(crate::file_read::FileReadTool::new())),
        "write" | "filewrite" => Some(Arc::new(crate::file_write::FileWriteTool::new())),
        "edit" | "fileedit" => Some(Arc::new(crate::file_edit::FileEditTool::new())),
        "delete" | "filedelete" => Some(Arc::new(crate::file_delete::FileDeleteTool::new())),
        "glob" => Some(Arc::new(crate::glob::GlobTool::new())),
        "grep" => Some(Arc::new(crate::grep::GrepTool::new())),
        "bash" => Some(Arc::new(crate::bash_tool::BashTool::new())),
        "notebookedit" => Some(Arc::new(crate::notebook_edit::NotebookEditTool::new())),
        "agent" | "task" => Some(Arc::new(crate::agent::AgentTool::new())),
        _ => None,
    }
}

fn normalize_tool_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn repl_error(index: usize, tool: &str, message: &str) -> Value {
    json!({
        "index": index,
        "tool": tool,
        "ok": false,
        "error": message
    })
}

#[cfg(test)]
mod tests {
    use super::ReplTool;
    use crate::mcp_tool::MCP_SERVERS_APP_STATE_KEY;
    use crate::tool_execution::PERMISSION_PROMPT_TOOL_APP_STATE_KEY;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    fn test_context(cwd: String) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd,
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        }
    }

    #[tokio::test]
    async fn runs_primitive_calls_in_order() {
        let root = std::env::temp_dir().join(format!("kiana-repl-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("note.txt");
        fs::write(&file, "hello world\n").unwrap();
        let mut context = test_context(root.to_string_lossy().to_string());

        let output = ReplTool::new()
            .call(
                &json!({
                    "calls": [
                        {
                            "tool": "Read",
                            "input": {
                                "file_path": file.to_string_lossy()
                            }
                        },
                        {
                            "tool": "Edit",
                            "input": {
                                "file_path": file.to_string_lossy(),
                                "old_string": "hello",
                                "new_string": "goodbye"
                            }
                        },
                        {
                            "tool": "Bash",
                            "input": {
                                "command": "cat note.txt"
                            }
                        }
                    ]
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["ok"], true);
        assert_eq!(output.data["executed"], 3);
        assert!(output.data["results"][2]["output"]["stdout"]
            .as_str()
            .unwrap()
            .contains("goodbye world"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "goodbye world\n");

        let api_result = ReplTool::new().map_to_api_result(&output, "toolu_repl");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_repl");
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("REPL batch succeeded. Executed 3 call(s)."));
        assert!(content.contains("1. Read: ok"));
        assert!(content.contains("2. Edit: ok"));
        assert!(content.contains("3. Bash: ok"));
        assert!(content.contains("goodbye world"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn uses_mcp_permission_prompt_tool_for_primitive_calls() {
        let _guard = crate::test_support::lock_env();
        let (url, state, handle) = start_mock_permission_mcp_server(json!({"decision":"allow"}));
        let root = std::env::temp_dir().join(format!("kiana-repl-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        clear_permission_env_for_test();
        std::env::set_var("KIANA_PERMISSIONS_FILE", root.join("permissions.json"));
        let file = root.join("approved.txt");
        let mut context = test_context(root.to_string_lossy().to_string());
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

        let input = json!({
            "file_path": file.to_string_lossy(),
            "content": "approved\n"
        });
        let output = ReplTool::new()
            .call(
                &json!({
                    "calls": [{
                        "tool": "Write",
                        "input": input
                    }]
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["ok"], true, "{}", output.data);
        assert_eq!(fs::read_to_string(&file).unwrap(), "approved\n");
        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool_name"], "Write");
        assert_eq!(calls[0]["input"]["content"], "approved\n");
        handle.join().unwrap();
        clear_permission_env_for_test();
        let _ = fs::remove_dir_all(root);
    }

    fn clear_permission_env_for_test() {
        for key in [
            "KIANA_PERMISSIONS_FILE",
            "KIANA_PERMISSION_MODE",
            "KIANA_ALLOWED_TOOLS",
            "KIANA_DISALLOWED_TOOLS",
            "KIANA_ASK_TOOLS",
            "KIANA_PERMISSION_PROMPT_TOOL",
        ] {
            std::env::remove_var(key);
        }
    }

    #[tokio::test]
    async fn stops_on_validation_error_by_default() {
        let mut context = test_context(".".to_string());

        let output = ReplTool::new()
            .call(
                &json!({
                    "calls": [
                        { "tool": "Bash", "input": { "command": "" } },
                        { "tool": "Bash", "input": { "command": "echo should-not-run" } }
                    ]
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["ok"], false);
        assert_eq!(output.data["executed"], 1);
        assert!(output.data["results"][0]["error"]
            .as_str()
            .unwrap()
            .contains("command cannot be empty"));
    }

    #[derive(Debug)]
    struct MockPermissionMcpState {
        decision: Value,
        calls: Vec<Value>,
    }

    fn start_mock_permission_mcp_server(
        decision: Value,
    ) -> (
        String,
        Arc<Mutex<MockPermissionMcpState>>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(MockPermissionMcpState {
            decision,
            calls: Vec::new(),
        }));
        let server_state = state.clone();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut last_request = Instant::now();
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        last_request = Instant::now();
                        handle_mock_permission_mcp_request(&mut stream, &server_state);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if !server_state.lock().unwrap().calls.is_empty()
                            && last_request.elapsed() > Duration::from_millis(100)
                        {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        (format!("http://{}", addr), state, handle)
    }

    fn handle_mock_permission_mcp_request(
        stream: &mut TcpStream,
        state: &Arc<Mutex<MockPermissionMcpState>>,
    ) {
        let body = read_http_body(stream);
        let message: Value = serde_json::from_slice(&body).unwrap();
        let response = mock_permission_mcp_response(&message, state);
        let body = response.map(|value| value.to_string()).unwrap_or_default();
        let status = if body.is_empty() {
            "HTTP/1.1 204 No Content"
        } else {
            "HTTP/1.1 200 OK"
        };
        let response = format!(
            "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn read_http_body(stream: &mut TcpStream) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        let header_end;
        loop {
            let read = stream.read(&mut chunk).unwrap();
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = position;
                break;
            }
        }

        let headers = String::from_utf8_lossy(&buffer[..header_end]);
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
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        buffer[body_start..body_start + content_length].to_vec()
    }

    fn mock_permission_mcp_response(
        message: &Value,
        state: &Arc<Mutex<MockPermissionMcpState>>,
    ) -> Option<Value> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        match message.get("method").and_then(Value::as_str) {
            Some("notifications/initialized") => None,
            Some("initialize") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": "permission-mock", "version": "1.0.0"}
                }
            })),
            Some("tools/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "approve",
                        "description": "Approve a Kiana tool call",
                        "inputSchema": {"type": "object"}
                    }]
                }
            })),
            Some("resources/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resources": []}
            })),
            Some("prompts/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"prompts": []}
            })),
            Some("tools/call") => {
                let args = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let decision = {
                    let mut state = state.lock().unwrap();
                    state.calls.push(args);
                    state.decision.clone()
                };
                Some(json!({
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
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"}
            })),
        }
    }
}
