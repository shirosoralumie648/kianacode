//! Codex App-shaped Thread / Turn / Item view over Kiana receipts.
//!
//! Mapping (clean-room, not app-server):
//! thread  = Kiana session
//! turn    = one DaemonHost run / continue
//! item    = userMessage | commandExecution | fileChange | agentMessage | error
//!
//! Token streaming is not claimed. Approvals stay fail-closed trust/sandbox.

use kiana_protocol::ResponseEnvelope;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct ThreadView {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub turns: Vec<TurnView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TurnView {
    pub id: String,
    pub status: String,
    pub items: Vec<ItemView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemView {
    pub kind: String,
    pub status: String,
    pub title: String,
    pub body: String,
}

pub fn thread_name(prompt: &str) -> String {
    let trimmed = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return "New thread".to_owned();
    }
    let mut name = trimmed.chars().take(48).collect::<String>();
    if trimmed.chars().count() > 48 {
        name.push('…');
    }
    name
}

pub fn items_from_turn(prompt: &str, response: &ResponseEnvelope) -> Vec<ItemView> {
    let mut items = vec![ItemView {
        kind: "userMessage".to_owned(),
        status: "completed".to_owned(),
        title: "You".to_owned(),
        body: prompt.to_owned(),
    }];
    for cap in capabilities(response) {
        items.push(ItemView {
            kind: "commandExecution".to_owned(),
            status: status_of(response),
            title: format!("tool · {cap}"),
            body: cap,
        });
    }
    let files = files_changed(response);
    if !files.is_empty() {
        items.push(ItemView {
            kind: "fileChange".to_owned(),
            status: status_of(response),
            title: format!(
                "{} file{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ),
            body: files.join("\n"),
        });
    }
    let text = assistant_text(response);
    if !text.trim().is_empty() {
        items.push(ItemView {
            kind: "agentMessage".to_owned(),
            status: status_of(response),
            title: "Builder".to_owned(),
            body: text,
        });
    }
    if let Some(error) = response.error.as_deref().filter(|value| !value.is_empty()) {
        items.push(ItemView {
            kind: "error".to_owned(),
            status: status_of(response),
            title: "blocked".to_owned(),
            body: error.to_owned(),
        });
    }
    items
}

pub fn status_of(response: &ResponseEnvelope) -> String {
    serde_json::to_value(response.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{:?}", response.status).to_ascii_lowercase())
}

pub fn assistant_text(response: &ResponseEnvelope) -> String {
    response.output["output"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

pub fn files_changed(response: &ResponseEnvelope) -> Vec<String> {
    response.output["files_changed"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn capabilities(response: &ResponseEnvelope) -> Vec<String> {
    response.output["capabilities"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("operation").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_protocol::{ExecutionStatus, RequestId, PROTOCOL_SCHEMA};
    use serde_json::json;

    #[test]
    fn maps_receipt_to_codex_shaped_items() {
        let response = ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: RequestId::new(),
            status: ExecutionStatus::Completed,
            output: json!({
                "files_changed": ["GOLDEN_PATH.txt"],
                "capabilities": [{"operation": "apply_patch"}],
                "output": {"text": "created GOLDEN_PATH.txt"}
            }),
            error: None,
        };
        let items = items_from_turn("create GOLDEN_PATH.txt containing hello", &response);
        assert_eq!(items[0].kind, "userMessage");
        assert!(items.iter().any(|item| item.kind == "commandExecution"));
        assert!(items.iter().any(|item| item.kind == "fileChange"));
        assert!(items.iter().any(|item| item.kind == "agentMessage"));
        assert_eq!(
            thread_name("create GOLDEN_PATH.txt containing hello"),
            "create GOLDEN_PATH.txt containing hello"
        );
    }
}
