use crate::types::*;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

pub fn create_synthetic_assistant_message(
    request: &SDKControlPermissionRequest,
    request_id: &str,
) -> serde_json::Value {
    json!({
        "type": "assistant",
        "uuid": Uuid::new_v4().to_string(),
        "message": {
            "id": format!("remote-{}", request_id),
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": request.tool_use_id,
                "name": request.tool_name,
                "input": request.input
            }],
            "model": "",
            "stop_reason": null,
            "stop_sequence": null,
            "container": null,
            "context_management": null,
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        },
        "requestId": null,
        "timestamp": chrono::Utc::now().to_rfc3339()
    })
}

#[derive(Debug, Clone)]
pub struct ToolStub {
    pub name: String,
}

impl ToolStub {
    pub fn new(tool_name: String) -> Self {
        Self { name: tool_name }
    }

    pub fn render_tool_use_message(&self, input: &HashMap<String, serde_json::Value>) -> String {
        if input.is_empty() {
            return String::new();
        }

        input
            .iter()
            .take(3)
            .map(|(key, value)| {
                let value_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    _ => serde_json::to_string(value).unwrap_or_default(),
                };
                format!("{}: {}", key, value_str)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn is_enabled(&self) -> bool {
        true
    }

    pub fn user_facing_name(&self) -> &str {
        &self.name
    }

    pub fn needs_permissions(&self) -> bool {
        true
    }
}
