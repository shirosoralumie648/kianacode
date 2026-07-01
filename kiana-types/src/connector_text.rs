use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorTextBlock {
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub block_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

impl ConnectorTextBlock {
    pub fn is_text_block(value: &Value) -> bool {
        value
            .as_object()
            .map(|o| o.contains_key("text"))
            .unwrap_or(false)
    }
}
