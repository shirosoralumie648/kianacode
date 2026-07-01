use crate::tool::*;
use async_trait::async_trait;
use kiana_services::network_policy::{
    validate_http_redirect, validate_http_url, HttpNetworkSurface,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct WebFetchInput {
    url: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    max_bytes: Option<usize>,
}

pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetch HTTP or HTTPS content"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "timeout_seconds": { "type": "integer", "minimum": 1 },
                "max_bytes": { "type": "integer", "minimum": 1 }
            },
            "required": ["url"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "status": { "type": "integer" },
                "content_type": { "type": "string" },
                "body": { "type": "string" },
                "truncated": { "type": "boolean" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: WebFetchInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };
        if let Err(reason) = validate_http_url(HttpNetworkSurface::WebFetch, &input.url) {
            return ValidationResult::err(reason, 2);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, _context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: WebFetchInput = serde_json::from_value(input.clone())?;
        let target_url = validate_http_url(HttpNetworkSurface::WebFetch, &input.url)
            .map_err(ToolError::ValidationError)?;

        let timeout = Duration::from_secs(input.timeout_seconds.unwrap_or(30).clamp(1, 300));
        let max_bytes = input.max_bytes.unwrap_or(64 * 1024).clamp(1, 1024 * 1024);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("kiana-code/0.1")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let Some(previous_url) = attempt.previous().last() else {
                    return attempt.follow();
                };
                if let Err(reason) = validate_http_redirect(
                    HttpNetworkSurface::WebFetch,
                    previous_url,
                    attempt.url(),
                ) {
                    return attempt.error(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        reason,
                    ));
                }
                attempt.follow()
            }))
            .build()
            .map_err(|e| ToolError::Other(e.to_string()))?;

        let response = client
            .get(target_url)
            .send()
            .await
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let final_url = response.url().to_string();
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let truncated = bytes.len() > max_bytes;
        let body = String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes)]).to_string();

        Ok(ToolOutput {
            data: json!({
                "url": input.url,
                "final_url": final_url,
                "status": status,
                "content_type": content_type,
                "body": body,
                "truncated": truncated
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let final_url = output
            .data
            .get("final_url")
            .or_else(|| output.data.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let status = output.data.get("status").and_then(Value::as_u64);
        let content_type = output
            .data
            .get("content_type")
            .and_then(Value::as_str)
            .filter(|content_type| !content_type.is_empty());
        let body = output
            .data
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("");
        let truncated = output
            .data
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut content = String::new();
        if !final_url.is_empty() {
            content.push_str(&format!("URL: {final_url}\n"));
        }
        if let Some(status) = status {
            content.push_str(&format!("Status: {status}\n"));
        }
        if let Some(content_type) = content_type {
            content.push_str(&format!("Content-Type: {content_type}\n"));
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(body);
        if truncated {
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str("\n[Content truncated]");
        }

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content.trim().to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::WebFetchTool;
    use crate::{Tool, ToolContext, ToolOutput};
    use serde_json::json;
    use std::collections::HashMap;

    fn test_context() -> ToolContext {
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .to_string_lossy()
                .to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal,
        }
    }

    #[tokio::test]
    async fn validation_blocks_local_private_metadata_and_non_http_targets() {
        let tool = WebFetchTool::new();
        let context = test_context();

        for url in [
            "file:///etc/passwd",
            "http://localhost/",
            "http://127.0.0.1/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://192.168.1.1/",
            "http://",
        ] {
            let result = tool.validate_input(&json!({ "url": url }), &context).await;

            assert!(!result.result, "{url} should be blocked");
            let message = result.message.unwrap_or_default();
            assert!(
                message.contains("network policy") || message.contains("http:// or https://"),
                "{url} should report a URL/network-policy error, got {message:?}"
            );
        }
    }

    #[tokio::test]
    async fn validation_allows_public_http_and_https_targets() {
        let tool = WebFetchTool::new();
        let context = test_context();

        for url in ["https://example.com/", "http://93.184.216.34/"] {
            let result = tool.validate_input(&json!({ "url": url }), &context).await;

            assert!(
                result.result,
                "{url} should be allowed: {:?}",
                result.message
            );
        }
    }

    #[test]
    fn maps_fetch_output_to_model_facing_text() {
        let output = ToolOutput {
            data: json!({
                "url": "https://example.com",
                "final_url": "https://example.com/final",
                "status": 200,
                "content_type": "text/plain",
                "body": "hello web",
                "truncated": true
            }),
            metadata: None,
        };

        let result = WebFetchTool::new().map_to_api_result(&output, "toolu_fetch");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_fetch");
        assert_eq!(
            result["content"],
            "URL: https://example.com/final\nStatus: 200\nContent-Type: text/plain\n\nhello web\n\n[Content truncated]"
        );
        assert!(result["content"]["body"].is_null());
    }
}
