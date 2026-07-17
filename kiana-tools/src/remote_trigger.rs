use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const TRIGGERS_BETA: &str = "ccr-triggers-2026-01-30";

#[derive(Debug, Deserialize)]
struct RemoteTriggerInput {
    action: String,
    #[serde(default)]
    trigger_id: Option<String>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

pub struct RemoteTriggerTool;

impl RemoteTriggerTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for RemoteTriggerTool {
    fn name(&self) -> &str {
        "RemoteTrigger"
    }

    fn description(&self) -> &str {
        "Manage scheduled remote Claude Code agents via the remote trigger API"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("manage scheduled remote agent triggers")
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "create", "update", "run"]
                },
                "trigger_id": {
                    "type": "string",
                    "pattern": "^[A-Za-z0-9_-]+$",
                    "description": "Required for get, update, and run"
                },
                "body": {
                    "type": "object",
                    "description": "JSON body for create and update"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 120
                }
            },
            "required": ["action"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": { "type": "integer" },
                "json": { "type": "string" }
            },
            "required": ["status", "json"]
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: RemoteTriggerInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        validate_remote_trigger_input(&input)
            .map(|_| ValidationResult::ok())
            .unwrap_or_else(|message| ValidationResult::err(message, 2))
    }

    async fn check_permissions(&self, input: &Value, context: &ToolContext) -> PermissionDecision {
        let read_only = input
            .get("action")
            .and_then(Value::as_str)
            .map(|action| matches!(action, "list" | "get"))
            .unwrap_or(false);
        match crate::permissions::permission_check_for_tool(
            self.name(),
            read_only,
            input,
            &context.app_state,
        ) {
            crate::permissions::ToolPermissionCheck::Allow => PermissionDecision::allow(),
            crate::permissions::ToolPermissionCheck::Deny(reason) => {
                PermissionDecision::deny(reason)
            }
            crate::permissions::ToolPermissionCheck::Ask(_) => {
                match crate::permissions::prompt_for_tool_permission(self.name(), input) {
                    Ok(true) => PermissionDecision::allow(),
                    Ok(false) => PermissionDecision::deny(format!(
                        "Tool {} was denied by the user in ask mode.",
                        self.name()
                    )),
                    Err(reason) => PermissionDecision::deny(reason),
                }
            }
        }
    }

    async fn call(&self, input: &Value, _context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: RemoteTriggerInput = serde_json::from_value(input.clone())?;
        validate_remote_trigger_input(&input).map_err(ToolError::ValidationError)?;

        let access_token = remote_trigger_access_token().ok_or_else(|| {
            ToolError::Other(
                "RemoteTrigger requires KIANA_REMOTE_TRIGGER_ACCESS_TOKEN, CLAUDE_ACCESS_TOKEN, or ANTHROPIC_AUTH_TOKEN.".to_string(),
            )
        })?;
        let organization_uuid = remote_trigger_organization_uuid().ok_or_else(|| {
            ToolError::Other(
                "RemoteTrigger requires KIANA_REMOTE_TRIGGER_ORG_UUID, CLAUDE_ORG_UUID, or ANTHROPIC_ORGANIZATION_ID.".to_string(),
            )
        })?;

        let timeout = Duration::from_secs(input.timeout_seconds.unwrap_or(20).clamp(1, 120));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("kiana-code/0.1")
            .build()
            .map_err(|error| ToolError::Other(error.to_string()))?;

        let (method, url, body) = remote_trigger_request(&input)?;
        let mut request = match method {
            RemoteTriggerMethod::Get => client.get(url),
            RemoteTriggerMethod::Post => client.post(url),
        }
        .bearer_auth(access_token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", TRIGGERS_BETA)
        .header("x-organization-uuid", organization_uuid);

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|error| ToolError::Other(error.to_string()))?;

        Ok(ToolOutput {
            data: json!({
                "status": status,
                "json": text,
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let status = output
            .data
            .get("status")
            .and_then(Value::as_u64)
            .map(|status| status.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let body = output
            .data
            .get("json")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|json| serde_json::to_string_pretty(&json).ok())
            .unwrap_or_else(|| body.to_string());

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format!("RemoteTrigger response status: {status}\n{body}")
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteTriggerMethod {
    Get,
    Post,
}

fn validate_remote_trigger_input(input: &RemoteTriggerInput) -> Result<(), String> {
    match input.action.as_str() {
        "list" => Ok(()),
        "get" | "run" => require_trigger_id(input),
        "create" => require_body(input),
        "update" => {
            require_trigger_id(input)?;
            require_body(input)
        }
        _ => Err("action must be one of list, get, create, update, or run".to_string()),
    }
}

fn require_trigger_id(input: &RemoteTriggerInput) -> Result<(), String> {
    let Some(trigger_id) = input.trigger_id.as_deref() else {
        return Err(format!("{} requires trigger_id", input.action));
    };
    if !valid_trigger_id(trigger_id) {
        return Err(
            "trigger_id may only contain letters, numbers, underscores, and hyphens".into(),
        );
    }
    Ok(())
}

fn require_body(input: &RemoteTriggerInput) -> Result<(), String> {
    match input.body.as_ref().and_then(Value::as_object) {
        Some(_) => Ok(()),
        None => Err(format!("{} requires body", input.action)),
    }
}

fn valid_trigger_id(trigger_id: &str) -> bool {
    !trigger_id.is_empty()
        && trigger_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn remote_trigger_request(
    input: &RemoteTriggerInput,
) -> ToolResult<(RemoteTriggerMethod, String, Option<Value>)> {
    let base = remote_trigger_base_url();
    match input.action.as_str() {
        "list" => Ok((RemoteTriggerMethod::Get, base, None)),
        "get" => Ok((
            RemoteTriggerMethod::Get,
            format!(
                "{}/{}",
                base,
                input.trigger_id.as_deref().unwrap_or_default()
            ),
            None,
        )),
        "create" => Ok((RemoteTriggerMethod::Post, base, input.body.clone())),
        "update" => Ok((
            RemoteTriggerMethod::Post,
            format!(
                "{}/{}",
                base,
                input.trigger_id.as_deref().unwrap_or_default()
            ),
            input.body.clone(),
        )),
        "run" => Ok((
            RemoteTriggerMethod::Post,
            format!(
                "{}/{}/run",
                base,
                input.trigger_id.as_deref().unwrap_or_default()
            ),
            Some(json!({})),
        )),
        _ => Err(ToolError::ValidationError(
            "action must be one of list, get, create, update, or run".into(),
        )),
    }
}

fn remote_trigger_base_url() -> String {
    if let Some(value) = env_nonempty("KIANA_REMOTE_TRIGGER_BASE_URL") {
        return value.trim_end_matches('/').to_string();
    }
    let api_base = env_nonempty("KIANA_REMOTE_TRIGGER_API_BASE_URL")
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    format!("{}/v1/code/triggers", api_base.trim_end_matches('/'))
}

fn remote_trigger_access_token() -> Option<String> {
    env_nonempty("KIANA_REMOTE_TRIGGER_ACCESS_TOKEN")
        .or_else(|| env_nonempty("CLAUDE_ACCESS_TOKEN"))
        .or_else(|| env_nonempty("ANTHROPIC_AUTH_TOKEN"))
}

fn remote_trigger_organization_uuid() -> Option<String> {
    env_nonempty("KIANA_REMOTE_TRIGGER_ORG_UUID")
        .or_else(|| env_nonempty("CLAUDE_ORG_UUID"))
        .or_else(|| env_nonempty("ANTHROPIC_ORGANIZATION_ID"))
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvRestore {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvRestore {
        fn set(values: &[(&'static str, &str)]) -> Self {
            let keys = [
                "KIANA_REMOTE_TRIGGER_BASE_URL",
                "KIANA_REMOTE_TRIGGER_API_BASE_URL",
                "KIANA_REMOTE_TRIGGER_ACCESS_TOKEN",
                "CLAUDE_ACCESS_TOKEN",
                "ANTHROPIC_AUTH_TOKEN",
                "KIANA_REMOTE_TRIGGER_ORG_UUID",
                "CLAUDE_ORG_UUID",
                "ANTHROPIC_ORGANIZATION_ID",
            ];
            let previous = keys
                .into_iter()
                .map(|key| (key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for key in keys {
                std::env::remove_var(key);
            }
            for (key, value) in values {
                std::env::set_var(key, value);
            }
            Self { values: previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    fn tool_context() -> ToolContext {
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal,
        }
    }

    #[tokio::test]
    async fn validates_action_specific_requirements() {
        let context = tool_context();
        let tool = RemoteTriggerTool::new();

        let missing_id = tool
            .validate_input(&json!({ "action": "get" }), &context)
            .await;
        assert!(!missing_id.result);
        assert!(missing_id.message.unwrap().contains("requires trigger_id"));

        let missing_body = tool
            .validate_input(&json!({ "action": "create" }), &context)
            .await;
        assert!(!missing_body.result);
        assert!(missing_body.message.unwrap().contains("requires body"));

        let valid = tool
            .validate_input(
                &json!({ "action": "run", "trigger_id": "nightly_1" }),
                &context,
            )
            .await;
        assert!(valid.result);
    }

    #[tokio::test]
    async fn plan_mode_allows_reads_and_denies_mutations() {
        let mut context = tool_context();
        context
            .app_state
            .insert("permission_mode".to_string(), json!("plan"));
        context
            .app_state
            .insert("project_trusted".to_string(), json!(true));
        let tool = RemoteTriggerTool::new();

        let read = tool
            .check_permissions(&json!({ "action": "list" }), &context)
            .await;
        assert!(read.granted);

        let write = tool
            .check_permissions(&json!({ "action": "create", "body": {} }), &context)
            .await;
        assert!(!write.granted);
        assert!(write.reason.unwrap().contains("plan permission mode"));
    }

    #[tokio::test]
    async fn list_calls_authenticated_remote_trigger_endpoint() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (base_url, request_rx) = one_shot_server(r#"{"triggers":[]}"#).await;
        let _env = EnvRestore::set(&[
            ("KIANA_REMOTE_TRIGGER_BASE_URL", &base_url),
            ("KIANA_REMOTE_TRIGGER_ACCESS_TOKEN", "token-1"),
            ("KIANA_REMOTE_TRIGGER_ORG_UUID", "org-1"),
        ]);

        let mut context = tool_context();
        let output = RemoteTriggerTool::new()
            .call(&json!({ "action": "list" }), &mut context)
            .await
            .unwrap();
        let request = request_rx.await.unwrap();

        assert_eq!(output.data["status"], 200);
        assert_eq!(output.data["json"], r#"{"triggers":[]}"#);
        let api_result = RemoteTriggerTool::new().map_to_api_result(&output, "toolu_remote");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_remote");
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("RemoteTrigger response status: 200"));
        assert!(content.contains("\"triggers\": []"));
        assert!(request.starts_with("get /v1/code/triggers http/1.1"));
        assert!(request.contains("authorization: bearer token-1"));
        assert!(request.contains("anthropic-beta: ccr-triggers-2026-01-30"));
        assert!(request.contains("x-organization-uuid: org-1"));
    }

    #[tokio::test]
    async fn create_posts_json_body_to_remote_trigger_endpoint() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (base_url, request_rx) = one_shot_server(r#"{"id":"trigger-1"}"#).await;
        let _env = EnvRestore::set(&[
            ("KIANA_REMOTE_TRIGGER_BASE_URL", &base_url),
            ("KIANA_REMOTE_TRIGGER_ACCESS_TOKEN", "token-1"),
            ("KIANA_REMOTE_TRIGGER_ORG_UUID", "org-1"),
        ]);

        let mut context = tool_context();
        let output = RemoteTriggerTool::new()
            .call(
                &json!({
                    "action": "create",
                    "body": {
                        "name": "nightly",
                        "prompt": "check status"
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        let request = request_rx.await.unwrap();

        assert_eq!(output.data["status"], 200);
        assert!(request.starts_with("post /v1/code/triggers http/1.1"));
        assert!(request.contains(r#""name":"nightly""#));
        assert!(request.contains(r#""prompt":"check status""#));
    }

    #[tokio::test]
    async fn missing_auth_returns_clear_error() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _env = EnvRestore::set(&[]);
        let mut context = tool_context();

        let error = RemoteTriggerTool::new()
            .call(&json!({ "action": "list" }), &mut context)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("RemoteTrigger requires KIANA_REMOTE_TRIGGER_ACCESS_TOKEN"));
    }

    async fn one_shot_server(response_body: &'static str) -> (String, oneshot::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = request_tx.send(request);
        });

        (format!("http://{addr}/v1/code/triggers"), request_rx)
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if request_complete(&bytes) {
                break;
            }
        }
        String::from_utf8_lossy(&bytes).to_ascii_lowercase()
    }

    fn request_complete(bytes: &[u8]) -> bool {
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let header = String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase();
        let content_length = header
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        bytes.len() >= header_end + 4 + content_length
    }
}
