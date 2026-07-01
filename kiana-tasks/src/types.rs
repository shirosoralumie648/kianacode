use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStateBase {
    pub id: String,
    #[serde(rename = "type")]
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub start_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_paused_ms: Option<u64>,
    pub output_file: String,
    pub output_offset: u64,
    pub notified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BashTaskKind {
    Bash,
    Monitor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalShellTaskState {
    #[serde(flatten)]
    pub base: TaskStateBase,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ShellResult>,
    pub completion_status_sent_in_attachment: bool,
    pub last_reported_total_lines: usize,
    pub is_backgrounded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<BashTaskKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellResult {
    pub code: i32,
    pub interrupted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamPhase {
    Starting,
    Updating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamTurn {
    pub text: String,
    pub tool_use_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamTaskState {
    #[serde(flatten)]
    pub base: TaskStateBase,
    pub phase: DreamPhase,
    pub sessions_reviewing: usize,
    pub files_touched: Vec<String>,
    pub turns: Vec<DreamTurn>,
    pub prior_mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskState {
    LocalBash(LocalShellTaskState),
    LocalAgent(serde_json::Value),
    RemoteAgent(serde_json::Value),
    InProcessTeammate(serde_json::Value),
    LocalWorkflow(serde_json::Value),
    MonitorMcp(serde_json::Value),
    Dream(DreamTaskState),
}

impl TaskState {
    pub fn base(&self) -> Cow<'_, TaskStateBase> {
        match self {
            TaskState::LocalBash(s) => Cow::Borrowed(&s.base),
            TaskState::Dream(s) => Cow::Borrowed(&s.base),
            TaskState::LocalAgent(value) => Cow::Owned(base_from_legacy_json(
                value,
                TaskType::LocalAgent,
                "local-agent",
            )),
            TaskState::RemoteAgent(value) => Cow::Owned(base_from_legacy_json(
                value,
                TaskType::RemoteAgent,
                "remote-agent",
            )),
            TaskState::InProcessTeammate(value) => Cow::Owned(base_from_legacy_json(
                value,
                TaskType::InProcessTeammate,
                "teammate",
            )),
            TaskState::LocalWorkflow(value) => Cow::Owned(base_from_legacy_json(
                value,
                TaskType::LocalWorkflow,
                "workflow",
            )),
            TaskState::MonitorMcp(value) => Cow::Owned(base_from_legacy_json(
                value,
                TaskType::MonitorMcp,
                "monitor",
            )),
        }
    }

    pub fn is_background(&self) -> bool {
        let base = self.base();
        if !matches!(base.status, TaskStatus::Running | TaskStatus::Pending) {
            return false;
        }
        match self {
            TaskState::LocalBash(s) => s.is_backgrounded,
            _ => true,
        }
    }
}

fn base_from_legacy_json(value: &Value, task_type: TaskType, fallback_id: &str) -> TaskStateBase {
    TaskStateBase {
        id: string_field(value, &["id", "task_id", "taskId"])
            .unwrap_or_else(|| format!("legacy-{}", fallback_id)),
        task_type,
        status: task_status_field(value).unwrap_or(TaskStatus::Pending),
        description: string_field(value, &["description", "title", "prompt", "command"])
            .unwrap_or_else(|| fallback_id.to_string()),
        tool_use_id: string_field(value, &["tool_use_id", "toolUseId"]),
        start_time: u64_field(
            value,
            &["start_time", "startTime", "created_at", "createdAt"],
        )
        .unwrap_or_default(),
        end_time: u64_field(value, &["end_time", "endTime", "updated_at", "updatedAt"]),
        total_paused_ms: u64_field(value, &["total_paused_ms", "totalPausedMs"]),
        output_file: string_field(value, &["output_file", "outputFile"]).unwrap_or_default(),
        output_offset: u64_field(value, &["output_offset", "outputOffset"]).unwrap_or_default(),
        notified: bool_field(value, &["notified"]).unwrap_or(false),
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

fn bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
}

fn task_status_field(value: &Value) -> Option<TaskStatus> {
    match string_field(value, &["status"])?.as_str() {
        "pending" => Some(TaskStatus::Pending),
        "running" | "in_progress" | "inProgress" => Some(TaskStatus::Running),
        "completed" | "complete" | "success" => Some(TaskStatus::Completed),
        "failed" | "error" => Some(TaskStatus::Failed),
        "killed" | "cancelled" | "canceled" => Some(TaskStatus::Killed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base_reads_legacy_json_task_variants() {
        let task = TaskState::LocalAgent(json!({
            "id": "a123",
            "title": "Inspect workspace",
            "status": "running",
            "created_at": 42
        }));

        let base = task.base();
        assert_eq!(base.id, "a123");
        assert_eq!(base.task_type, TaskType::LocalAgent);
        assert_eq!(base.status, TaskStatus::Running);
        assert_eq!(base.description, "Inspect workspace");
        assert_eq!(base.start_time, 42);
        assert!(task.is_background());
    }

    #[test]
    fn base_uses_stable_fallback_for_sparse_legacy_tasks() {
        let task = TaskState::MonitorMcp(json!({}));

        let base = task.base();
        assert_eq!(base.id, "legacy-monitor");
        assert_eq!(base.task_type, TaskType::MonitorMcp);
        assert_eq!(base.status, TaskStatus::Pending);
        assert_eq!(base.description, "monitor");
    }
}
