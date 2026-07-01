use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    #[serde(rename = "progress")]
    Progress(ProgressMessage),
    #[serde(rename = "system")]
    System(SystemMessage),
    #[serde(rename = "attachment")]
    Attachment(AttachmentMessage),
    #[serde(rename = "hook_result")]
    HookResult(HookResultMessage),
    #[serde(rename = "tool_use_summary")]
    ToolUseSummary(ToolUseSummaryMessage),
    #[serde(rename = "tombstone")]
    Tombstone(TombstoneMessage),
    #[serde(rename = "grouped_tool_use")]
    GroupedToolUse(GroupedToolUseMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBase {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_meta: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_virtual: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_compact_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<MessageOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageOrigin {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    #[serde(flatten)]
    pub base: MessageBase,
    pub message: UserMessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessageContent {
    #[serde(flatten)]
    pub content: UserMessageContentInner,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserMessageContentInner {
    String(String),
    Array(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<AssistantMessageContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessageContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressMessage {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessage {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMessage {
    #[serde(flatten)]
    pub base: MessageBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResultMessage {
    #[serde(flatten)]
    pub base: MessageBase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseSummaryMessage {
    #[serde(flatten)]
    pub base: MessageBase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstoneMessage {
    #[serde(flatten)]
    pub base: MessageBase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedToolUseMessage {
    #[serde(flatten)]
    pub base: MessageBase,
}
