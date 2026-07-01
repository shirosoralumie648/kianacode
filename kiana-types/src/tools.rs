use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

pub type ShellProgress = ToolProgressData;
pub type BashProgress = ToolProgressData;
pub type PowerShellProgress = ToolProgressData;
pub type McpProgress = ToolProgressData;
pub type SkillToolProgress = ToolProgressData;
pub type TaskOutputProgress = ToolProgressData;
pub type WebSearchProgress = ToolProgressData;
pub type AgentToolProgress = ToolProgressData;
pub type ReplToolProgress = ToolProgressData;
pub type SdkWorkflowProgress = ToolProgressData;
