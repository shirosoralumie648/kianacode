use crate::ids::AgentId;
use crate::message::Message;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedMessage {
    #[serde(flatten)]
    pub message: Message,
    pub cwd: String,
    pub user_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogOption {
    pub date: String,
    pub messages: Vec<SerializedMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_path: Option<String>,
    pub value: i32,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub first_prompt: String,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    pub is_sidechain: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_lite: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_setting: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_teammate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leaf_uuid: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_history_snapshots: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution_snapshots: Option<Vec<AttributionSnapshotMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_collapse_commits: Option<Vec<ContextCollapseCommitEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_collapse_snapshot: Option<ContextCollapseSnapshotEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<SessionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_session: Option<Option<PersistedWorktreeSession>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_replacements: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    Coordinator,
    Normal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorktreeSession {
    pub original_cwd: String,
    pub worktree_path: String,
    pub worktree_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_head_commit: Option<String>,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmux_session_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_based: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttributionState {
    pub content_hash: String,
    pub claude_contribution: u64,
    pub mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename = "attribution-snapshot")]
pub struct AttributionSnapshotMessage {
    pub message_id: Uuid,
    pub surface: String,
    pub file_states: std::collections::HashMap<String, FileAttributionState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_count_at_last_commit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_prompt_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_prompt_count_at_last_commit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escape_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escape_count_at_last_commit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename = "marble-origami-commit")]
pub struct ContextCollapseCommitEntry {
    pub session_id: Uuid,
    pub collapse_id: String,
    pub summary_uuid: String,
    pub summary_content: String,
    pub summary: String,
    pub first_archived_uuid: String,
    pub last_archived_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedCollapse {
    pub start_uuid: String,
    pub end_uuid: String,
    pub summary: String,
    pub risk: i32,
    pub staged_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename = "marble-origami-snapshot")]
pub struct ContextCollapseSnapshotEntry {
    pub session_id: Uuid,
    pub staged: Vec<StagedCollapse>,
    pub armed: bool,
    pub last_spawn_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    #[serde(flatten)]
    pub message: SerializedMessage,
    pub parent_uuid: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_parent_uuid: Option<Uuid>,
    pub is_sidechain: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<AgentId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Entry {
    #[serde(rename = "transcript")]
    Transcript(TranscriptMessage),
    #[serde(rename = "summary")]
    Summary { leaf_uuid: Uuid, summary: String },
    #[serde(rename = "custom-title")]
    CustomTitle {
        session_id: Uuid,
        custom_title: String,
    },
    #[serde(rename = "ai-title")]
    AiTitle { session_id: Uuid, ai_title: String },
    #[serde(rename = "last-prompt")]
    LastPrompt {
        session_id: Uuid,
        last_prompt: String,
    },
    #[serde(rename = "task-summary")]
    TaskSummary {
        session_id: Uuid,
        summary: String,
        timestamp: String,
    },
    #[serde(rename = "tag")]
    Tag { session_id: Uuid, tag: String },
    #[serde(rename = "agent-name")]
    AgentName {
        session_id: Uuid,
        agent_name: String,
    },
    #[serde(rename = "agent-color")]
    AgentColor {
        session_id: Uuid,
        agent_color: String,
    },
    #[serde(rename = "agent-setting")]
    AgentSetting {
        session_id: Uuid,
        agent_setting: String,
    },
    #[serde(rename = "pr-link")]
    PrLink {
        session_id: Uuid,
        pr_number: i32,
        pr_url: String,
        pr_repository: String,
        timestamp: String,
    },
    #[serde(rename = "file-history-snapshot")]
    FileHistorySnapshot {
        message_id: Uuid,
        snapshot: Value,
        is_snapshot_update: bool,
    },
    #[serde(rename = "attribution-snapshot")]
    AttributionSnapshot(AttributionSnapshotMessage),
    #[serde(rename = "speculation-accept")]
    SpeculationAccept {
        timestamp: String,
        time_saved_ms: i64,
    },
    #[serde(rename = "mode")]
    Mode { session_id: Uuid, mode: SessionMode },
    #[serde(rename = "worktree-state")]
    WorktreeState {
        session_id: Uuid,
        worktree_session: Option<PersistedWorktreeSession>,
    },
    #[serde(rename = "content-replacement")]
    ContentReplacement {
        session_id: Uuid,
        agent_id: Option<AgentId>,
        replacements: Vec<Value>,
    },
    #[serde(rename = "marble-origami-commit")]
    ContextCollapseCommit(ContextCollapseCommitEntry),
    #[serde(rename = "marble-origami-snapshot")]
    ContextCollapseSnapshot(ContextCollapseSnapshotEntry),
}

pub fn sort_logs(mut logs: Vec<LogOption>) -> Vec<LogOption> {
    logs.sort_by(|a, b| {
        let modified_diff = b.modified.cmp(&a.modified);
        if modified_diff != std::cmp::Ordering::Equal {
            return modified_diff;
        }
        b.created.cmp(&a.created)
    });
    logs
}
