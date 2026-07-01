use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub dir: String,
    pub machine_name: String,
    pub branch: String,
    pub git_repo_url: Option<String>,
    pub max_sessions: usize,
    pub spawn_mode: SpawnMode,
    pub bridge_id: String,
    pub worker_type: String,
    pub environment_id: String,
    pub api_base_url: String,
    pub session_ingress_url: String,
    pub heartbeat_interval_ms: u64,
    pub session_timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ccr_v2_sse_reconnect_give_up_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ccr_v2_sse_liveness_timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnMode {
    SingleSession,
    SameDir,
    Worktree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub work_type: String,
    pub environment_id: String,
    pub state: String,
    pub data: WorkData,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkData {
    #[serde(rename = "type")]
    pub data_type: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkSecret {
    pub version: u32,
    pub session_ingress_token: String,
    pub api_base_url: String,
    pub use_code_sessions: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub lease_extended: bool,
    pub state: String,
    #[serde(default)]
    pub last_heartbeat: Option<String>,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    pub session_id: String,
    pub access_token: String,
    pub sdk_url: Option<String>,
    pub work_dir: Option<PathBuf>,
    pub use_ccr_v2: bool,
    pub worker_epoch: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionActivity {
    #[serde(rename = "type")]
    pub activity_type: String,
    pub summary: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SDKMessage {
    #[serde(rename = "user")]
    User {
        uuid: String,
        message: MessageContent,
    },
    #[serde(rename = "assistant")]
    Assistant {
        uuid: String,
        message: MessageContent,
    },
    #[serde(rename = "control_request")]
    ControlRequest {
        request_id: String,
        request: ControlRequestType,
    },
    #[serde(rename = "control_response")]
    ControlResponse { response: ControlResponseType },
    #[serde(rename = "control_cancel_request")]
    ControlCancelRequest {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
    },
    #[serde(rename = "update_environment_variables")]
    UpdateEnvironmentVariables { variables: HashMap<String, String> },
    #[serde(rename = "result")]
    Result {
        #[serde(flatten)]
        data: HashMap<String, serde_json::Value>,
    },
    #[serde(rename = "system")]
    System {
        #[serde(flatten)]
        data: HashMap<String, serde_json::Value>,
    },
    #[serde(rename = "stream_event")]
    StreamEvent {
        #[serde(flatten)]
        data: HashMap<String, serde_json::Value>,
    },
    #[serde(rename = "tool_progress")]
    ToolProgress {
        #[serde(flatten)]
        data: HashMap<String, serde_json::Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub content: ContentBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentBlock {
    Text(String),
    Blocks(Vec<HashMap<String, serde_json::Value>>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype")]
pub enum ControlRequestType {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "interrupt")]
    Interrupt,
    #[serde(rename = "set_model")]
    SetModel { model: Option<String> },
    #[serde(rename = "set_max_thinking_tokens")]
    SetMaxThinkingTokens { max_thinking_tokens: Option<u64> },
    #[serde(rename = "set_permission_mode")]
    SetPermissionMode { mode: Option<String> },
    #[serde(rename = "mcp_status")]
    McpStatus,
    #[serde(rename = "can_use_tool")]
    CanUseTool {
        tool_name: String,
        input: HashMap<String, serde_json::Value>,
        tool_use_id: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype")]
pub enum ControlResponseType {
    #[serde(rename = "success")]
    Success {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<HashMap<String, serde_json::Value>>,
    },
    #[serde(rename = "error")]
    Error { request_id: String, error: String },
}
