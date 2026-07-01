// Task management tools

use crate::team_lifecycle::finalize_background_team_agent;
use crate::tool::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const TASKS_STATE_KEY: &str = "tasks";
const TASKS_ROOT_KEY: &str = "tasks_root";
const TASKS_ROOT_ENV: &str = "KIANA_TASKS_ROOT";
const TEAMS_ROOT_KEY: &str = "teams_root";
const TEAMS_ROOT_ENV: &str = "KIANA_TEAMS_ROOT";
const HIGH_WATER_MARK_FILE: &str = ".highwatermark";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub description: Option<String>,
    #[serde(default, rename = "activeForm", alias = "active_form")]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default, rename = "blockedBy", alias = "blocked_by")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "killed")]
    Killed,
}

#[derive(Debug, Deserialize)]
struct TaskCreateInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "activeForm", alias = "active_form")]
    active_form: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default)]
    status: Option<TaskStatus>,
}

#[derive(Debug, Deserialize)]
struct TaskGetInput {
    #[serde(alias = "taskId")]
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct TaskListInput {
    #[serde(default)]
    status: Option<TaskStatus>,
}

#[derive(Debug, Deserialize)]
struct TaskStopInput {
    #[serde(alias = "taskId")]
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct TaskUpdateInput {
    #[serde(alias = "taskId")]
    task_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "activeForm", alias = "active_form")]
    active_form: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default, alias = "claimTask", alias = "claim_task")]
    claim: bool,
    #[serde(default, rename = "checkAgentBusy", alias = "check_agent_busy")]
    check_agent_busy: bool,
    #[serde(default, rename = "addBlocks", alias = "add_blocks")]
    add_blocks: Option<Vec<String>>,
    #[serde(default, rename = "addBlockedBy", alias = "add_blocked_by")]
    add_blocked_by: Option<Vec<String>>,
    #[serde(default)]
    metadata: Option<Value>,
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn trim_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn task_subject(input: &TaskCreateInput) -> Option<String> {
    trim_string(input.subject.clone()).or_else(|| trim_string(input.title.clone()))
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn active_team_name(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get("team_context")
        .or_else(|| context.app_state.get("teamContext"))
        .and_then(Value::as_object)
        .and_then(|team| {
            team.get("team_name")
                .or_else(|| team.get("teamName"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn tasks_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get(TASKS_ROOT_KEY)
        .or_else(|| context.app_state.get("tasksRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return context.resolve_path(path);
    }
    if let Some(path) = std::env::var_os(TASKS_ROOT_ENV) {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            PathBuf::from(&context.cwd).join(path)
        };
    }
    PathBuf::from(&context.cwd).join(".kiana").join("tasks")
}

fn task_list_id(context: &ToolContext) -> String {
    context
        .app_state
        .get("task_list_id")
        .or_else(|| context.app_state.get("taskListId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| active_team_name(context))
        .or_else(|| {
            context
                .app_state
                .get("session_id")
                .or_else(|| context.app_state.get("sessionId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default".to_string())
}

fn task_list_dir(context: &ToolContext) -> PathBuf {
    tasks_root(context).join(sanitize_path_component(&task_list_id(context)))
}

fn task_path(context: &ToolContext, task_id: &str) -> PathBuf {
    task_list_dir(context).join(format!("{}.json", sanitize_path_component(task_id)))
}

fn high_watermark_path(context: &ToolContext) -> PathBuf {
    task_list_dir(context).join(HIGH_WATER_MARK_FILE)
}

fn ensure_task_list_dir(context: &ToolContext) -> ToolResult<()> {
    fs::create_dir_all(task_list_dir(context))?;
    Ok(())
}

fn read_high_watermark(context: &ToolContext) -> i64 {
    fs::read_to_string(high_watermark_path(context))
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or_default()
}

fn write_high_watermark(context: &ToolContext, value: i64) -> ToolResult<()> {
    ensure_task_list_dir(context)?;
    fs::write(high_watermark_path(context), value.to_string())?;
    Ok(())
}

fn highest_task_id_from_files(context: &ToolContext) -> ToolResult<i64> {
    let dir = task_list_dir(context);
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut highest = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if let Some(id) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<i64>().ok())
        {
            highest = highest.max(id);
        }
    }
    Ok(highest)
}

fn next_task_id(context: &ToolContext) -> ToolResult<String> {
    ensure_task_list_dir(context)?;
    let next = highest_task_id_from_files(context)?.max(read_high_watermark(context)) + 1;
    write_high_watermark(context, next)?;
    Ok(next.to_string())
}

fn write_task_file(context: &ToolContext, task: &Task) -> ToolResult<()> {
    ensure_task_list_dir(context)?;
    fs::write(
        task_path(context, &task.id),
        serde_json::to_string_pretty(task)?,
    )?;
    Ok(())
}

fn delete_task_file(context: &ToolContext, task_id: &str) -> ToolResult<bool> {
    let path = task_path(context, task_id);
    if !path.exists() {
        return Ok(false);
    }
    if let Ok(id) = task_id.parse::<i64>() {
        write_high_watermark(context, read_high_watermark(context).max(id))?;
    }
    fs::remove_file(path)?;
    Ok(true)
}

fn prune_task_references(tasks: &mut [Task], task_id: &str) -> bool {
    let mut changed = false;
    for task in tasks {
        let original_blocks = task.blocks.len();
        task.blocks.retain(|block| block != task_id);
        if task.blocks.len() != original_blocks {
            changed = true;
        }

        let original_blocked_by = task.blocked_by.len();
        task.blocked_by.retain(|blocker| blocker != task_id);
        if task.blocked_by.len() != original_blocked_by {
            changed = true;
        }
    }
    changed
}

fn add_task_block_edge(tasks: &mut [Task], from_task_id: &str, to_task_id: &str) -> bool {
    let mut changed = false;
    if let Some(from) = tasks.iter_mut().find(|task| task.id == from_task_id) {
        if !from.blocks.iter().any(|block| block == to_task_id) {
            from.blocks.push(to_task_id.to_string());
            changed = true;
        }
    }
    if let Some(to) = tasks.iter_mut().find(|task| task.id == to_task_id) {
        if !to.blocked_by.iter().any(|blocker| blocker == from_task_id) {
            to.blocked_by.push(from_task_id.to_string());
            changed = true;
        }
    }
    changed
}

fn read_file_tasks(context: &ToolContext) -> ToolResult<Vec<Task>> {
    let dir = task_list_dir(context);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut tasks = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let mut task: Task = serde_json::from_str(&fs::read_to_string(path)?)?;
        if task.subject.is_none() {
            task.subject = Some(task.title.clone());
        }
        tasks.push(task);
    }
    tasks.sort_by(|a, b| {
        let a_id = a.id.parse::<i64>().ok();
        let b_id = b.id.parse::<i64>().ok();
        a_id.cmp(&b_id).then_with(|| a.id.cmp(&b.id))
    });
    Ok(tasks)
}

fn load_tasks(context: &ToolContext) -> ToolResult<Vec<Task>> {
    let mut tasks = read_file_tasks(context)?;
    let value = context
        .app_state
        .get(TASKS_STATE_KEY)
        .cloned()
        .unwrap_or_else(|| json!([]));

    let state_tasks: Vec<Task> = serde_json::from_value(value).map_err(ToolError::from)?;
    for mut task in state_tasks {
        if task.subject.is_none() {
            task.subject = Some(task.title.clone());
        }
        if !tasks.iter().any(|existing| existing.id == task.id) {
            tasks.push(task);
        }
    }
    Ok(tasks)
}

fn save_tasks(context: &mut ToolContext, tasks: &[Task]) -> ToolResult<()> {
    context
        .app_state
        .insert(TASKS_STATE_KEY.to_string(), serde_json::to_value(tasks)?);
    for task in tasks {
        write_task_file(context, task)?;
    }
    Ok(())
}

fn validate_task_id(task_id: &str) -> ValidationResult {
    if task_id.trim().is_empty()
        || task_id.contains('/')
        || task_id.contains('\\')
        || task_id.contains("..")
    {
        return ValidationResult::err("task_id is invalid".to_string(), 1);
    }
    ValidationResult::ok()
}

fn validate_task_title(title: &str) -> ValidationResult {
    if title.trim().is_empty() {
        return ValidationResult::err("title cannot be empty".to_string(), 2);
    }
    ValidationResult::ok()
}

fn parse_task_status(status: &str) -> Result<TaskStatus, String> {
    match status.trim() {
        "pending" => Ok(TaskStatus::Pending),
        "in_progress" => Ok(TaskStatus::InProgress),
        "running" => Ok(TaskStatus::Running),
        "completed" => Ok(TaskStatus::Completed),
        "failed" => Ok(TaskStatus::Failed),
        "cancelled" | "canceled" => Ok(TaskStatus::Cancelled),
        "killed" => Ok(TaskStatus::Killed),
        other => Err(format!("invalid task status: {other}")),
    }
}

fn is_terminal_task_status(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Killed
    )
}

fn active_agent_name(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get("agent_name")
        .or_else(|| context.app_state.get("agentName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_AGENT_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
}

fn unresolved_blockers(tasks: &[Task], task: &Task) -> Vec<String> {
    let unresolved_task_ids = tasks
        .iter()
        .filter(|candidate| !is_terminal_task_status(&candidate.status))
        .map(|candidate| candidate.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    task.blocked_by
        .iter()
        .filter(|blocker| unresolved_task_ids.contains(blocker.as_str()))
        .cloned()
        .collect()
}

fn busy_tasks_for_owner(tasks: &[Task], owner: &str, task_id: &str) -> Vec<String> {
    tasks
        .iter()
        .filter(|task| {
            task.id != task_id
                && !is_terminal_task_status(&task.status)
                && task.owner.as_deref() == Some(owner)
        })
        .map(|task| task.id.clone())
        .collect()
}

fn teams_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get(TEAMS_ROOT_KEY)
        .or_else(|| context.app_state.get("teamsRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return context.resolve_path(path);
    }
    if let Some(path) = std::env::var_os(TEAMS_ROOT_ENV) {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            PathBuf::from(&context.cwd).join(path)
        };
    }
    PathBuf::from(&context.cwd).join(".kiana").join("teams")
}

fn team_inbox_path(context: &ToolContext, team_name: &str, owner: &str) -> PathBuf {
    teams_root(context)
        .join(sanitize_path_component(team_name))
        .join("inboxes")
        .join(format!("{}.json", sanitize_path_component(owner)))
}

fn sender_name(context: &ToolContext) -> String {
    context
        .app_state
        .get("agent_name")
        .or_else(|| context.app_state.get("agentName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("KIANA_AGENT_NAME").ok())
        .or_else(|| std::env::var("CLAUDE_CODE_AGENT_NAME").ok())
        .unwrap_or_else(|| "team-lead".to_string())
}

fn write_task_assignment_mailbox(
    context: &ToolContext,
    owner: &str,
    task: &Task,
) -> ToolResult<Option<PathBuf>> {
    let Some(team_name) = active_team_name(context) else {
        return Ok(None);
    };
    let path = team_inbox_path(context, &team_name, owner);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut inbox = if path.is_file() {
        serde_json::from_str::<Value>(&fs::read_to_string(&path)?)?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let body = json!({
        "type": "task_assignment",
        "taskId": task.id,
        "task_id": task.id,
        "subject": task.subject.as_deref().unwrap_or(&task.title),
        "description": task.description.clone().unwrap_or_default(),
        "assignedBy": sender_name(context),
        "assigned_by": sender_name(context),
        "timestamp": now_unix_seconds().to_string()
    });
    inbox.push(json!({
        "from": sender_name(context),
        "text": serde_json::to_string(&body)?,
        "timestamp": now_unix_seconds().to_string(),
        "read": false
    }));
    fs::write(&path, serde_json::to_string_pretty(&Value::Array(inbox))?)?;
    Ok(Some(path))
}

pub struct TaskCreateTool;
impl TaskCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn description(&self) -> &str {
        "Create a new task"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "subject": { "type": "string" },
                "description": { "type": "string" },
                "activeForm": { "type": "string" },
                "owner": { "type": "string" },
                "metadata": { "type": "object" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "running", "completed", "failed", "cancelled", "killed"] }
            },
            "required": ["title"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: TaskCreateInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 3),
        };
        match task_subject(&input) {
            Some(subject) => validate_task_title(&subject),
            None => ValidationResult::err("title or subject cannot be empty".to_string(), 2),
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TaskCreateInput = serde_json::from_value(input.clone())?;
        let subject = task_subject(&input).ok_or_else(|| {
            ToolError::ValidationError("title or subject is required".to_string())
        })?;
        let mut tasks = load_tasks(context)?;
        let task_id = next_task_id(context)?;
        let now = now_unix_seconds();
        let task = Task {
            id: task_id.clone(),
            title: subject.clone(),
            subject: Some(subject.clone()),
            description: input.description,
            active_form: input.active_form,
            owner: trim_string(input.owner),
            status: input.status.unwrap_or(TaskStatus::Pending),
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            metadata: input.metadata,
            created_at: now,
            updated_at: now,
        };

        tasks.push(task.clone());
        save_tasks(context, &tasks)?;

        Ok(ToolOutput {
            data: json!({
                "task_id": task_id,
                "taskId": task_id,
                "task_list_id": task_list_id(context),
                "taskListId": task_list_id(context),
                "task": task
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_task_create_result(&output.data)
        })
    }
}

pub struct TaskGetTool;
impl TaskGetTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        "TaskGet"
    }
    fn description(&self) -> &str {
        "Get a task by ID"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "taskId": { "type": "string" }
            },
            "required": ["task_id"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": { "type": "object" }
            }
        })
    }
    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: TaskGetInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 3),
        };
        validate_task_id(&input.task_id)
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TaskGetInput = serde_json::from_value(input.clone())?;
        let tasks = load_tasks(context)?;
        let task = if let Some(task) = tasks.into_iter().find(|task| task.id == input.task_id) {
            json!(task)
        } else {
            match read_background_task(context, &input.task_id)? {
                Some(task) => task,
                None => Value::Null,
            }
        };

        Ok(ToolOutput {
            data: json!({ "task": task }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_task_get_result(&output.data)
        })
    }
}

pub struct TaskListTool;
impl TaskListTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "TaskList"
    }
    fn description(&self) -> &str {
        "List all tasks"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": { "type": "string", "enum": ["pending", "in_progress", "running", "completed", "failed", "cancelled", "killed"] }
            }
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": { "type": "array" }
            }
        })
    }
    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match serde_json::from_value::<TaskListInput>(input.clone()) {
            Ok(_) => ValidationResult::ok(),
            Err(e) => ValidationResult::err(format!("Invalid input: {}", e), 3),
        }
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TaskListInput = serde_json::from_value(input.clone())?;
        let loaded_tasks = load_tasks(context)?;
        let resolved_task_ids = loaded_tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Completed)
            .map(|task| task.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let mut tasks = loaded_tasks
            .into_iter()
            .map(|mut task| {
                task.blocked_by
                    .retain(|blocker| !resolved_task_ids.contains(blocker));
                task
            })
            .filter(|task| {
                !task
                    .metadata
                    .as_ref()
                    .and_then(Value::as_object)
                    .and_then(|metadata| metadata.get("_internal"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .map(|task| json!(task))
            .collect::<Vec<_>>();
        tasks.extend(read_background_tasks(context)?);

        if let Some(status) = input.status {
            tasks.retain(|task| task_status_matches(task, &status));
        }

        Ok(ToolOutput {
            data: json!({ "tasks": tasks }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_task_list_result(&output.data)
        })
    }
}

pub struct TaskStopTool;
impl TaskStopTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "TaskStop"
    }
    fn description(&self) -> &str {
        "Stop a running task"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "taskId": { "type": "string" }
            },
            "required": ["task_id"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": { "type": "object" }
            }
        })
    }
    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: TaskStopInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 3),
        };
        validate_task_id(&input.task_id)
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TaskStopInput = serde_json::from_value(input.clone())?;
        let mut tasks = load_tasks(context)?;
        let Some(task) = tasks.iter_mut().find(|task| task.id == input.task_id) else {
            let task = stop_background_task(context, &input.task_id)?;
            return Ok(ToolOutput {
                data: json!({ "task": task }),
                metadata: None,
            });
        };

        task.status = TaskStatus::Cancelled;
        task.updated_at = now_unix_seconds();
        let task = task.clone();
        save_tasks(context, &tasks)?;

        Ok(ToolOutput {
            data: json!({ "task": task }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_task_stop_result(&output.data)
        })
    }
}

pub struct TaskUpdateTool;
impl TaskUpdateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "TaskUpdate"
    }
    fn description(&self) -> &str {
        "Update a task"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "taskId": { "type": "string" },
                "status": { "type": "string" },
                "title": { "type": "string" },
                "subject": { "type": "string" },
                "description": { "type": "string" },
                "activeForm": { "type": "string" },
                "owner": { "type": "string" },
                "claim": { "type": "boolean" },
                "claimTask": { "type": "boolean" },
                "checkAgentBusy": { "type": "boolean" },
                "addBlocks": { "type": "array", "items": { "type": "string" } },
                "addBlockedBy": { "type": "array", "items": { "type": "string" } },
                "metadata": { "type": "object" }
            },
            "required": ["task_id"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "task": { "type": ["object", "null"] },
                "reason": { "type": "string" },
                "busyWithTasks": { "type": "array", "items": { "type": "string" } },
                "blockedByTasks": { "type": "array", "items": { "type": "string" } }
            }
        })
    }
    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: TaskUpdateInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 3),
        };
        let id_validation = validate_task_id(&input.task_id);
        if !id_validation.result {
            return id_validation;
        }
        if let Some(title) = &input.title {
            let title_validation = validate_task_title(title);
            if !title_validation.result {
                return title_validation;
            }
        }
        if let Some(subject) = &input.subject {
            let subject_validation = validate_task_title(subject);
            if !subject_validation.result {
                return subject_validation;
            }
        }
        if let Some(status) = &input.status {
            if status != "deleted" {
                if let Err(error) = parse_task_status(status) {
                    return ValidationResult::err(error, 4);
                }
            }
        }
        ValidationResult::ok()
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TaskUpdateInput = serde_json::from_value(input.clone())?;
        let mut tasks = load_tasks(context)?;
        let Some(existing_index) = tasks.iter().position(|task| task.id == input.task_id) else {
            return Ok(ToolOutput {
                data: json!({
                    "success": false,
                    "taskId": input.task_id,
                    "task_id": input.task_id,
                    "updatedFields": [],
                    "updated_fields": [],
                    "error": "Task not found"
                }),
                metadata: None,
            });
        };
        let existing = tasks[existing_index].clone();
        let mut updated_fields = Vec::<String>::new();

        if input.status.as_deref() == Some("deleted") {
            let deleted = delete_task_file(context, &input.task_id)?;
            tasks.remove(existing_index);
            prune_task_references(&mut tasks, &input.task_id);
            for task in &mut tasks {
                task.updated_at = now_unix_seconds();
            }
            context
                .app_state
                .insert(TASKS_STATE_KEY.to_string(), serde_json::to_value(&tasks)?);
            for task in &tasks {
                write_task_file(context, task)?;
            }
            return Ok(ToolOutput {
                data: json!({
                    "success": deleted,
                    "taskId": input.task_id,
                    "task_id": input.task_id,
                    "updatedFields": if deleted { vec!["deleted"] } else { Vec::<&str>::new() },
                    "updated_fields": if deleted { vec!["deleted"] } else { Vec::<&str>::new() },
                    "statusChange": if deleted {
                        json!({ "from": existing.status, "to": "deleted" })
                    } else {
                        Value::Null
                    },
                    "status_change": if deleted {
                        json!({ "from": existing.status, "to": "deleted" })
                    } else {
                        Value::Null
                    },
                    "task": Value::Null
                }),
                metadata: None,
            });
        }

        let mut assignment_mailbox = None;
        let requested_owner = trim_string(input.owner.clone());
        if input.claim || input.check_agent_busy {
            let Some(owner) = requested_owner.clone() else {
                return Ok(ToolOutput {
                    data: json!({
                        "success": false,
                        "taskId": input.task_id,
                        "task_id": input.task_id,
                        "updatedFields": [],
                        "updated_fields": [],
                        "reason": "missing_owner",
                        "error": "owner is required when claiming a task",
                        "task": existing
                    }),
                    metadata: None,
                });
            };
            let blocked_by_tasks = unresolved_blockers(&tasks, &existing);
            if !blocked_by_tasks.is_empty() {
                return Ok(ToolOutput {
                    data: json!({
                        "success": false,
                        "taskId": input.task_id,
                        "task_id": input.task_id,
                        "updatedFields": [],
                        "updated_fields": [],
                        "reason": "blocked",
                        "blockedByTasks": blocked_by_tasks,
                        "blocked_by_tasks": blocked_by_tasks,
                        "task": existing
                    }),
                    metadata: None,
                });
            }
            if existing
                .owner
                .as_deref()
                .is_some_and(|current| current != owner)
            {
                return Ok(ToolOutput {
                    data: json!({
                        "success": false,
                        "taskId": input.task_id,
                        "task_id": input.task_id,
                        "updatedFields": [],
                        "updated_fields": [],
                        "reason": "already_claimed",
                        "task": existing
                    }),
                    metadata: None,
                });
            }
            if is_terminal_task_status(&existing.status) {
                return Ok(ToolOutput {
                    data: json!({
                        "success": false,
                        "taskId": input.task_id,
                        "task_id": input.task_id,
                        "updatedFields": [],
                        "updated_fields": [],
                        "reason": "already_resolved",
                        "task": existing
                    }),
                    metadata: None,
                });
            }
            if input.check_agent_busy {
                let busy_with_tasks = busy_tasks_for_owner(&tasks, &owner, &input.task_id);
                if !busy_with_tasks.is_empty() {
                    return Ok(ToolOutput {
                        data: json!({
                            "success": false,
                            "taskId": input.task_id,
                            "task_id": input.task_id,
                            "updatedFields": [],
                            "updated_fields": [],
                            "reason": "agent_busy",
                            "busyWithTasks": busy_with_tasks,
                            "busy_with_tasks": busy_with_tasks,
                            "task": existing
                        }),
                        metadata: None,
                    });
                }
            }
        }
        {
            let task = &mut tasks[existing_index];

            if let Some(subject) = trim_string(input.subject).or_else(|| trim_string(input.title)) {
                if task.title != subject {
                    task.title = subject.clone();
                    task.subject = Some(subject);
                    updated_fields.push("subject".to_string());
                }
            }
            if let Some(description) = input.description {
                if task.description.as_deref() != Some(description.as_str()) {
                    task.description = Some(description);
                    updated_fields.push("description".to_string());
                }
            }
            if let Some(active_form) = trim_string(input.active_form) {
                if task.active_form.as_deref() != Some(active_form.as_str()) {
                    task.active_form = Some(active_form);
                    updated_fields.push("activeForm".to_string());
                }
            }
            if let Some(owner) = requested_owner {
                if task.owner.as_deref() != Some(owner.as_str()) {
                    task.owner = Some(owner.clone());
                    updated_fields.push("owner".to_string());
                    assignment_mailbox = Some(owner);
                }
            }
            if let Some(status) = input.status {
                let parsed = parse_task_status(&status).map_err(ToolError::ValidationError)?;
                if task.status != parsed {
                    task.status = parsed;
                    updated_fields.push("status".to_string());
                }
                if status == "in_progress"
                    && task.owner.is_none()
                    && active_team_name(context).is_some()
                {
                    if let Some(agent_name) = active_agent_name(context) {
                        task.owner = Some(agent_name.clone());
                        updated_fields.push("owner".to_string());
                        assignment_mailbox = Some(agent_name);
                    }
                }
            }
            if let Some(metadata) = input.metadata {
                let mut merged = task
                    .metadata
                    .take()
                    .and_then(|value| value.as_object().cloned())
                    .unwrap_or_default();
                if let Some(updates) = metadata.as_object() {
                    for (key, value) in updates {
                        if value.is_null() {
                            merged.remove(key);
                        } else {
                            merged.insert(key.clone(), value.clone());
                        }
                    }
                }
                task.metadata = Some(Value::Object(merged));
                updated_fields.push("metadata".to_string());
            }
        }

        let task_id = input.task_id.clone();
        if let Some(blocks) = input.add_blocks {
            for block in blocks
                .into_iter()
                .filter_map(|block| trim_string(Some(block)))
            {
                if block == task_id {
                    continue;
                }
                if add_task_block_edge(&mut tasks, &task_id, &block) {
                    if !updated_fields.iter().any(|field| field == "blocks") {
                        updated_fields.push("blocks".to_string());
                    }
                }
            }
        }
        if let Some(blocked_by) = input.add_blocked_by {
            for blocker in blocked_by
                .into_iter()
                .filter_map(|blocker| trim_string(Some(blocker)))
            {
                if blocker == task_id {
                    continue;
                }
                if add_task_block_edge(&mut tasks, &blocker, &task_id) {
                    if !updated_fields.iter().any(|field| field == "blockedBy") {
                        updated_fields.push("blockedBy".to_string());
                    }
                }
            }
        }

        tasks[existing_index].updated_at = now_unix_seconds();
        let task = tasks[existing_index].clone();
        save_tasks(context, &tasks)?;
        let assignment_mailbox = if let Some(owner) = assignment_mailbox {
            write_task_assignment_mailbox(context, &owner, &task)?
                .map(|path| path.to_string_lossy().to_string())
        } else {
            None
        };

        Ok(ToolOutput {
            data: json!({
                "success": true,
                "taskId": task.id,
                "task_id": task.id,
                "updatedFields": updated_fields,
                "updated_fields": updated_fields,
                "statusChange": if task.status != existing.status {
                    json!({ "from": existing.status, "to": task.status })
                } else {
                    Value::Null
                },
                "status_change": if task.status != existing.status {
                    json!({ "from": existing.status, "to": task.status })
                } else {
                    Value::Null
                },
                "assignment_mailbox": assignment_mailbox,
                "task": task
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format_task_update_result(&output.data)
        })
    }
}

fn format_task_create_result(data: &Value) -> String {
    let mut lines = Vec::new();
    lines.push("Task created.".to_string());
    if let Some(task) = data.get("task") {
        push_task_summary(&mut lines, task);
    } else if let Some(task_id) = data
        .get("task_id")
        .or_else(|| data.get("taskId"))
        .and_then(Value::as_str)
    {
        lines.push(format!("Task ID: {task_id}"));
    }
    push_string_field(
        &mut lines,
        "Task list",
        data,
        &["task_list_id", "taskListId"],
    );
    lines.join("\n")
}

fn format_task_get_result(data: &Value) -> String {
    let Some(task) = data.get("task").filter(|task| !task.is_null()) else {
        return "Task not found.".to_string();
    };
    let mut lines = vec!["Task details.".to_string()];
    push_task_summary(&mut lines, task);
    lines.join("\n")
}

fn format_task_list_result(data: &Value) -> String {
    let tasks = data
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if tasks.is_empty() {
        return "No tasks found.".to_string();
    }

    let mut lines = vec![format!("{} task(s) found:", tasks.len())];
    lines.extend(tasks.iter().map(task_summary_line));
    lines.join("\n")
}

fn format_task_stop_result(data: &Value) -> String {
    let mut lines = vec!["Task stopped.".to_string()];
    if let Some(task) = data.get("task").filter(|task| !task.is_null()) {
        push_task_summary(&mut lines, task);
    }
    lines.join("\n")
}

fn format_task_update_result(data: &Value) -> String {
    let success = data.get("success").and_then(Value::as_bool).unwrap_or(true);
    let mut lines = vec![if success {
        "Task updated.".to_string()
    } else {
        "Task update failed.".to_string()
    }];

    push_string_field(&mut lines, "Task ID", data, &["task_id", "taskId"]);
    push_string_field(&mut lines, "Reason", data, &["reason", "error"]);
    push_array_field(
        &mut lines,
        "Updated fields",
        data,
        &["updated_fields", "updatedFields"],
    );
    push_array_field(
        &mut lines,
        "Busy with tasks",
        data,
        &["busy_with_tasks", "busyWithTasks"],
    );
    push_array_field(
        &mut lines,
        "Blocked by tasks",
        data,
        &["blocked_by_tasks", "blockedByTasks"],
    );
    if let Some(change) = data
        .get("status_change")
        .or_else(|| data.get("statusChange"))
        .filter(|change| !change.is_null())
    {
        let from = change
            .get("from")
            .and_then(value_to_short_string)
            .unwrap_or_else(|| "unknown".to_string());
        let to = change
            .get("to")
            .and_then(value_to_short_string)
            .unwrap_or_else(|| "unknown".to_string());
        lines.push(format!("Status change: {from} -> {to}"));
    }
    if let Some(task) = data.get("task").filter(|task| !task.is_null()) {
        push_task_summary(&mut lines, task);
    }
    lines.join("\n")
}

fn push_task_summary(lines: &mut Vec<String>, task: &Value) {
    lines.push(format!("Task: {}", task_summary_line(task)));
    push_string_field(lines, "Description", task, &["description"]);
    push_string_field(lines, "Active form", task, &["activeForm", "active_form"]);
    push_array_field(lines, "Blocks", task, &["blocks"]);
    push_array_field(lines, "Blocked by", task, &["blockedBy", "blocked_by"]);
}

fn task_summary_line(task: &Value) -> String {
    let id = task
        .get("id")
        .or_else(|| task.get("task_id"))
        .or_else(|| task.get("taskId"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let status = task
        .get("status")
        .and_then(value_to_short_string)
        .unwrap_or_else(|| "unknown".to_string());
    let title = task
        .get("subject")
        .or_else(|| task.get("title"))
        .or_else(|| task.get("description"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("untitled");
    let mut line = format!("#{id} [{status}] {title}");
    if let Some(owner) = task
        .get("owner")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        line.push_str(&format!(" (owner: {owner})"));
    }
    line
}

fn push_string_field(lines: &mut Vec<String>, label: &str, object: &Value, keys: &[&str]) {
    if let Some(value) = keys
        .iter()
        .find_map(|key| object.get(*key).and_then(value_to_short_string))
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("{label}: {value}"));
    }
}

fn push_array_field(lines: &mut Vec<String>, label: &str, object: &Value, keys: &[&str]) {
    if let Some(values) = keys
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_array))
    {
        let values = values
            .iter()
            .filter_map(value_to_short_string)
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>();
        if !values.is_empty() {
            lines.push(format!("{label}: {}", values.join(", ")));
        }
    }
}

fn value_to_short_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn task_status_matches(task: &Value, status: &TaskStatus) -> bool {
    let Some(task_status) = task.get("status").and_then(Value::as_str) else {
        return false;
    };
    match status {
        TaskStatus::Pending => task_status == "pending",
        TaskStatus::InProgress => matches!(task_status, "in_progress" | "running"),
        TaskStatus::Running => task_status == "running",
        TaskStatus::Completed => matches!(task_status, "completed" | "complete" | "success"),
        TaskStatus::Failed => task_status == "failed",
        TaskStatus::Cancelled => matches!(task_status, "cancelled" | "canceled"),
        TaskStatus::Killed => task_status == "killed",
    }
}

fn read_background_tasks(context: &ToolContext) -> ToolResult<Vec<Value>> {
    let root = background_root(context);
    let tasks_dir = root.join("tasks");
    if !tasks_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(tasks_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Some(task_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if let Some(task) = read_background_task_at(context, &root, task_id)? {
            tasks.push(task);
        }
    }
    tasks.sort_by(|a, b| {
        b.get("updated_at")
            .and_then(Value::as_i64)
            .cmp(&a.get("updated_at").and_then(Value::as_i64))
    });
    Ok(tasks)
}

fn read_background_task(context: &ToolContext, task_id: &str) -> ToolResult<Option<Value>> {
    read_background_task_at(context, &background_root(context), task_id)
}

fn read_background_task_at(
    context: &ToolContext,
    root: &Path,
    task_id: &str,
) -> ToolResult<Option<Value>> {
    let path = root.join("tasks").join(format!("{task_id}.json"));
    if !path.is_file() {
        return Ok(None);
    }
    let mut task: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    apply_exit_marker(context, root, task_id, &mut task)?;
    if let Some(task_object) = task.as_object_mut() {
        task_object.insert("task_id".to_string(), json!(task_id));
        task_object.insert("task_type".to_string(), json!("background"));
    }
    Ok(Some(task))
}

fn stop_background_task(context: &ToolContext, task_id: &str) -> ToolResult<Value> {
    let root = background_root(context);
    let Some(mut task) = read_background_task_at(context, &root, task_id)? else {
        return Err(ToolError::Other(format!("Task '{}' not found", task_id)));
    };
    if let Some(pid) = task.get("pid").and_then(Value::as_u64) {
        terminate_process(pid as u32)?;
        append_background_log(
            &root,
            task_id,
            &format!("sent termination signal to pid={pid}\n"),
        )?;
    } else {
        append_background_log(&root, task_id, "task had no active pid\n")?;
    }

    let now = now_unix_seconds();
    if let Some(task_object) = task.as_object_mut() {
        task_object.insert("status".to_string(), json!("killed"));
        task_object.insert("pid".to_string(), Value::Null);
        task_object.insert("updated_at".to_string(), json!(now));
        task_object.insert("exit_code".to_string(), json!(-1));
    }
    finalize_background_team_agent(context, &mut task, "terminated")?;
    std::fs::write(
        root.join("tasks").join(format!("{task_id}.json")),
        serde_json::to_string_pretty(&task)?,
    )?;
    Ok(task)
}

fn apply_exit_marker(
    context: &ToolContext,
    root: &Path,
    task_id: &str,
    task: &mut Value,
) -> ToolResult<()> {
    let exit_path = root.join("exits").join(format!("{task_id}.exit"));
    let mut changed = false;
    if !exit_path.is_file() || is_terminal_status(task.get("status").and_then(Value::as_str)) {
        if is_terminal_status(task.get("status").and_then(Value::as_str)) {
            let reason = task
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed")
                .to_string();
            changed |= finalize_background_team_agent(context, task, &reason)?;
        }
        if changed {
            std::fs::write(
                root.join("tasks").join(format!("{task_id}.json")),
                serde_json::to_string_pretty(task)?,
            )?;
        }
        return Ok(());
    }

    let exit_code = std::fs::read_to_string(exit_path)?
        .trim()
        .parse::<i64>()
        .unwrap_or(-1);
    if let Some(task_object) = task.as_object_mut() {
        task_object.insert(
            "status".to_string(),
            json!(if exit_code == 0 {
                "completed"
            } else {
                "failed"
            }),
        );
        task_object.insert("exit_code".to_string(), json!(exit_code));
        task_object.insert("pid".to_string(), Value::Null);
        task_object.insert("updated_at".to_string(), json!(now_unix_seconds()));
    }
    changed = true;
    let reason = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
        .to_string();
    changed |= finalize_background_team_agent(context, task, &reason)?;
    if changed {
        std::fs::write(
            root.join("tasks").join(format!("{task_id}.json")),
            serde_json::to_string_pretty(task)?,
        )?;
    }
    Ok(())
}

fn is_terminal_status(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("completed" | "complete" | "success" | "failed" | "killed" | "cancelled" | "canceled")
    )
}

fn background_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get("background_root")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_BG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path).join("bg-tasks");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".kiana").join("bg-tasks");
    }
    PathBuf::from(".kiana").join("bg-tasks")
}

fn append_background_log(root: &Path, task_id: &str, line: &str) -> ToolResult<()> {
    std::fs::create_dir_all(root.join("logs"))?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("logs").join(format!("{task_id}.log")))?;
    file.write_all(format!("[{}] {}", now_unix_seconds(), line).as_bytes())?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessTerminationCommand {
    program: &'static str,
    args: Vec<String>,
}

impl ProcessTerminationCommand {
    fn display(&self) -> String {
        std::iter::once(self.program.to_string())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(any(unix, test))]
fn unix_process_group_termination_command(pid: u32) -> ProcessTerminationCommand {
    ProcessTerminationCommand {
        program: "kill",
        args: vec!["-TERM".to_string(), "--".to_string(), format!("-{pid}")],
    }
}

#[cfg(any(unix, test))]
fn unix_process_termination_command(pid: u32) -> ProcessTerminationCommand {
    ProcessTerminationCommand {
        program: "kill",
        args: vec!["-TERM".to_string(), pid.to_string()],
    }
}

#[cfg(any(windows, test))]
fn windows_tree_termination_command(pid: u32) -> ProcessTerminationCommand {
    ProcessTerminationCommand {
        program: "taskkill",
        args: vec![
            "/PID".to_string(),
            pid.to_string(),
            "/T".to_string(),
            "/F".to_string(),
        ],
    }
}

fn run_termination_command(
    command: &ProcessTerminationCommand,
) -> ToolResult<std::process::ExitStatus> {
    Ok(Command::new(command.program).args(&command.args).status()?)
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> ToolResult<()> {
    let primary = unix_process_group_termination_command(pid);
    let status = run_termination_command(&primary)?;
    if status.success() {
        Ok(())
    } else {
        let fallback_command = unix_process_termination_command(pid);
        let fallback = run_termination_command(&fallback_command)?;
        if fallback.success() {
            Ok(())
        } else {
            Err(ToolError::Other(format!(
                "{} returned status {status}; {} returned status {fallback}",
                primary.display(),
                fallback_command.display()
            )))
        }
    }
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> ToolResult<()> {
    let command = windows_tree_termination_command(pid);
    let status = run_termination_command(&command)?;
    if status.success() {
        Ok(())
    } else {
        Err(ToolError::Other(format!(
            "{} returned status {status}",
            command.display()
        )))
    }
}

#[cfg(not(any(unix, windows)))]
fn terminate_process(_pid: u32) -> ToolResult<()> {
    Err(ToolError::Other(
        "background task termination is not supported on this platform yet".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{TaskCreateTool, TaskGetTool, TaskListTool, TaskStopTool, TaskUpdateTool};
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use uuid::Uuid;

    fn test_context() -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let root = std::env::temp_dir().join(format!("kiana-task-test-{}", Uuid::new_v4()));
        ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                (
                    "tasks_root".to_string(),
                    json!(root.join("tasks").to_string_lossy().to_string()),
                ),
                (
                    "teams_root".to_string(),
                    json!(root.join("teams").to_string_lossy().to_string()),
                ),
            ]),
            abort_signal: abort_rx,
        }
    }

    fn test_context_with_background_root(root: &std::path::Path) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                (
                    "background_root".to_string(),
                    json!(root.to_string_lossy().to_string()),
                ),
                (
                    "tasks_root".to_string(),
                    json!(root.join("tasks").to_string_lossy().to_string()),
                ),
                (
                    "teams_root".to_string(),
                    json!(root.join("teams").to_string_lossy().to_string()),
                ),
            ]),
            abort_signal: abort_rx,
        }
    }

    fn read_json(path: impl AsRef<Path>) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn process_termination_commands_cover_unix_and_windows() {
        let unix_group = super::unix_process_group_termination_command(12345);
        assert_eq!(unix_group.program, "kill");
        assert_eq!(unix_group.args, vec!["-TERM", "--", "-12345"]);

        let unix_process = super::unix_process_termination_command(12345);
        assert_eq!(unix_process.program, "kill");
        assert_eq!(unix_process.args, vec!["-TERM", "12345"]);

        let windows = super::windows_tree_termination_command(12345);
        assert_eq!(windows.program, "taskkill");
        assert_eq!(windows.args, vec!["/PID", "12345", "/T", "/F"]);
    }

    #[tokio::test]
    async fn task_tools_share_session_task_state() {
        let mut context = test_context();
        let create = TaskCreateTool::new();
        let list = TaskListTool::new();
        let get = TaskGetTool::new();
        let update = TaskUpdateTool::new();
        let stop = TaskStopTool::new();

        let create_input = json!({
            "title": "Wire command registry",
            "description": "Make local slash commands executable",
            "status": "in_progress"
        });
        assert!(create.validate_input(&create_input, &context).await.result);
        let created = create.call(&create_input, &mut context).await.unwrap();
        let task_id = created.data["task_id"].as_str().unwrap().to_string();

        let listed = list.call(&json!({}), &mut context).await.unwrap();
        assert_eq!(listed.data["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed.data["tasks"][0]["title"], "Wire command registry");

        let got = get
            .call(&json!({ "task_id": task_id }), &mut context)
            .await
            .unwrap();
        assert_eq!(got.data["task"]["status"], "in_progress");
        let task_id = got.data["task"]["id"].as_str().unwrap().to_string();

        let updated = update
            .call(
                &json!({
                    "task_id": task_id,
                    "title": "Wire real task tools",
                    "status": "completed"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(updated.data["task"]["title"], "Wire real task tools");
        assert_eq!(updated.data["task"]["status"], "completed");
        let task_id = updated.data["task"]["id"].as_str().unwrap().to_string();

        let stopped = stop
            .call(&json!({ "task_id": task_id }), &mut context)
            .await
            .unwrap();
        assert_eq!(stopped.data["task"]["status"], "cancelled");
    }

    #[tokio::test]
    async fn task_tools_map_to_model_facing_text_results() {
        let mut context = test_context();
        let create = TaskCreateTool::new();
        let list = TaskListTool::new();
        let get = TaskGetTool::new();
        let update = TaskUpdateTool::new();
        let stop = TaskStopTool::new();

        let created = create
            .call(
                &json!({
                    "title": "Map task tool output",
                    "description": "Return readable model-facing text"
                }),
                &mut context,
            )
            .await
            .unwrap();
        let created_result = create.map_to_api_result(&created, "toolu_create");
        assert_eq!(created_result["type"], "tool_result");
        assert_eq!(created_result["tool_use_id"], "toolu_create");
        let created_text = created_result["content"].as_str().unwrap();
        assert!(created_text.contains("Task created."));
        assert!(created_text.contains("#1 [pending] Map task tool output"));
        assert!(created_text.contains("Description: Return readable model-facing text"));

        let listed = list.call(&json!({}), &mut context).await.unwrap();
        let listed_result = list.map_to_api_result(&listed, "toolu_list");
        let listed_text = listed_result["content"].as_str().unwrap();
        assert!(listed_text.contains("1 task(s) found"));
        assert!(listed_text.contains("#1 [pending] Map task tool output"));

        let got = get
            .call(&json!({ "task_id": "1" }), &mut context)
            .await
            .unwrap();
        let got_result = get.map_to_api_result(&got, "toolu_get");
        assert!(got_result["content"]
            .as_str()
            .unwrap()
            .contains("Task details."));

        let updated = update
            .call(
                &json!({
                    "task_id": "1",
                    "status": "in_progress",
                    "owner": "alice"
                }),
                &mut context,
            )
            .await
            .unwrap();
        let updated_result = update.map_to_api_result(&updated, "toolu_update");
        let updated_text = updated_result["content"].as_str().unwrap();
        assert!(updated_text.contains("Task updated."));
        assert!(updated_text.contains("Updated fields: owner, status"));
        assert!(updated_text.contains("Status change: pending -> in_progress"));
        assert!(updated_text.contains("(owner: alice)"));

        let stopped = stop
            .call(&json!({ "task_id": "1" }), &mut context)
            .await
            .unwrap();
        let stopped_result = stop.map_to_api_result(&stopped, "toolu_stop");
        let stopped_text = stopped_result["content"].as_str().unwrap();
        assert!(stopped_text.contains("Task stopped."));
        assert!(stopped_text.contains("#1 [cancelled] Map task tool output"));

        let missing = get
            .call(&json!({ "task_id": "missing" }), &mut context)
            .await
            .unwrap();
        let missing_result = get.map_to_api_result(&missing, "toolu_missing");
        assert_eq!(missing_result["content"], "Task not found.");
    }

    #[tokio::test]
    async fn task_tools_persist_reference_shape_to_task_list_files() {
        let mut context = test_context();
        context.app_state.insert(
            "team_context".to_string(),
            json!({
                "team_name": "review",
                "teamName": "review"
            }),
        );

        let created = TaskCreateTool::new()
            .call(
                &json!({
                    "subject": "Audit task storage",
                    "description": "Make tasks shared through files",
                    "activeForm": "Auditing task storage",
                    "metadata": { "slice": "tasks" }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(created.data["task"]["id"], "1");
        assert_eq!(created.data["task"]["subject"], "Audit task storage");
        assert_eq!(created.data["task_list_id"], "review");

        let tasks_root = context.app_state["tasks_root"]
            .as_str()
            .unwrap()
            .to_string();
        let task_file = Path::new(&tasks_root).join("review/1.json");
        let persisted = read_json(&task_file);
        assert_eq!(persisted["subject"], "Audit task storage");
        assert_eq!(persisted["activeForm"], "Auditing task storage");
        assert_eq!(persisted["metadata"]["slice"], "tasks");

        let updated = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "1",
                    "owner": "alice",
                    "addBlocks": ["2"],
                    "addBlockedBy": ["0"],
                    "metadata": {
                        "slice": null,
                        "verified": true
                    },
                    "status": "in_progress"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(updated.data["success"], true);
        assert_eq!(updated.data["task"]["owner"], "alice");
        assert_eq!(updated.data["task"]["blocks"], json!(["2"]));
        assert_eq!(updated.data["task"]["blockedBy"], json!(["0"]));
        assert_eq!(updated.data["task"]["metadata"]["verified"], true);
        assert!(updated.data["task"]["metadata"].get("slice").is_none());

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let inbox = read_json(Path::new(teams_root).join("review/inboxes/alice.json"));
        let assignment: Value = serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(assignment["type"], "task_assignment");
        assert_eq!(assignment["taskId"], "1");
        assert_eq!(assignment["subject"], "Audit task storage");

        let got = TaskGetTool::new()
            .call(&json!({ "taskId": "1" }), &mut context)
            .await
            .unwrap();
        assert_eq!(got.data["task"]["owner"], "alice");

        let deleted = TaskUpdateTool::new()
            .call(&json!({ "taskId": "1", "status": "deleted" }), &mut context)
            .await
            .unwrap();
        assert_eq!(deleted.data["success"], true);
        assert!(!task_file.exists());

        let missing = TaskGetTool::new()
            .call(&json!({ "taskId": "1" }), &mut context)
            .await
            .unwrap();
        assert!(missing.data["task"].is_null());
    }

    #[tokio::test]
    async fn task_update_keeps_block_edges_consistent_on_disk() {
        let mut context = test_context();

        TaskCreateTool::new()
            .call(
                &json!({
                    "subject": "Prepare API",
                    "description": "Create API contract"
                }),
                &mut context,
            )
            .await
            .unwrap();
        TaskCreateTool::new()
            .call(
                &json!({
                    "subject": "Build UI",
                    "description": "Use API contract"
                }),
                &mut context,
            )
            .await
            .unwrap();

        TaskUpdateTool::new()
            .call(&json!({ "taskId": "1", "addBlocks": ["2"] }), &mut context)
            .await
            .unwrap();
        let tasks_root = context.app_state["tasks_root"]
            .as_str()
            .unwrap()
            .to_string();
        let task_one = read_json(Path::new(&tasks_root).join("default/1.json"));
        let task_two = read_json(Path::new(&tasks_root).join("default/2.json"));
        assert_eq!(task_one["blocks"], json!(["2"]));
        assert_eq!(task_two["blockedBy"], json!(["1"]));

        let listed = TaskListTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        assert_eq!(listed.data["tasks"][1]["blockedBy"], json!(["1"]));

        TaskUpdateTool::new()
            .call(
                &json!({ "taskId": "1", "status": "completed" }),
                &mut context,
            )
            .await
            .unwrap();
        let listed = TaskListTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        assert_eq!(listed.data["tasks"][1]["blockedBy"], json!([]));

        TaskUpdateTool::new()
            .call(&json!({ "taskId": "1", "status": "deleted" }), &mut context)
            .await
            .unwrap();
        let task_two = read_json(Path::new(&tasks_root).join("default/2.json"));
        assert_eq!(task_two["blockedBy"], json!([]));
    }

    #[tokio::test]
    async fn task_update_claims_reference_task_ownership_semantics() {
        let mut context = test_context();

        TaskCreateTool::new()
            .call(&json!({ "subject": "Prepare API" }), &mut context)
            .await
            .unwrap();
        TaskCreateTool::new()
            .call(&json!({ "subject": "Build UI" }), &mut context)
            .await
            .unwrap();
        TaskUpdateTool::new()
            .call(
                &json!({ "taskId": "2", "addBlockedBy": ["1"] }),
                &mut context,
            )
            .await
            .unwrap();

        let blocked = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "2",
                    "owner": "alice",
                    "claim": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(blocked.data["success"], false);
        assert_eq!(blocked.data["reason"], "blocked");
        assert_eq!(blocked.data["blockedByTasks"], json!(["1"]));

        TaskUpdateTool::new()
            .call(
                &json!({ "taskId": "1", "status": "completed" }),
                &mut context,
            )
            .await
            .unwrap();
        TaskCreateTool::new()
            .call(
                &json!({
                    "subject": "Review copy",
                    "owner": "alice"
                }),
                &mut context,
            )
            .await
            .unwrap();

        let busy = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "2",
                    "owner": "alice",
                    "claim": true,
                    "checkAgentBusy": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(busy.data["success"], false);
        assert_eq!(busy.data["reason"], "agent_busy");
        assert_eq!(busy.data["busyWithTasks"], json!(["3"]));

        let claimed = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "2",
                    "owner": "bob",
                    "claim": true,
                    "status": "in_progress"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(claimed.data["success"], true);
        assert_eq!(claimed.data["task"]["owner"], "bob");
        assert_eq!(claimed.data["task"]["status"], "in_progress");

        let already_claimed = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "2",
                    "owner": "alice",
                    "claim": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(already_claimed.data["success"], false);
        assert_eq!(already_claimed.data["reason"], "already_claimed");

        TaskUpdateTool::new()
            .call(
                &json!({ "taskId": "2", "status": "completed" }),
                &mut context,
            )
            .await
            .unwrap();
        let already_resolved = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "2",
                    "owner": "bob",
                    "claim": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(already_resolved.data["success"], false);
        assert_eq!(already_resolved.data["reason"], "already_resolved");
    }

    #[tokio::test]
    async fn task_update_auto_assigns_in_progress_teammate() {
        let mut context = test_context();
        context.app_state.insert(
            "team_context".to_string(),
            json!({
                "team_name": "review",
                "teamName": "review"
            }),
        );
        context
            .app_state
            .insert("agent_name".to_string(), json!("researcher"));

        TaskCreateTool::new()
            .call(&json!({ "subject": "Check findings" }), &mut context)
            .await
            .unwrap();
        let updated = TaskUpdateTool::new()
            .call(
                &json!({
                    "taskId": "1",
                    "status": "in_progress"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(updated.data["success"], true);
        assert_eq!(updated.data["task"]["owner"], "researcher");
        assert_eq!(updated.data["updatedFields"], json!(["status", "owner"]));

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let inbox = read_json(Path::new(teams_root).join("review/inboxes/researcher.json"));
        let assignment: Value = serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(assignment["type"], "task_assignment");
        assert_eq!(assignment["taskId"], "1");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn task_tools_manage_background_bash_tasks() {
        let root = std::env::temp_dir().join(format!("kiana-task-bg-{}", Uuid::new_v4()));
        let mut context = test_context_with_background_root(&root);

        let started = BashTool::new()
            .call(
                &json!({
                    "command": "while true; do sleep 1; done",
                    "run_in_background": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        let task_id = started.data["backgroundTaskId"]
            .as_str()
            .unwrap()
            .to_string();

        let listed = TaskListTool::new()
            .call(&json!({"status": "in_progress"}), &mut context)
            .await
            .unwrap();
        assert_eq!(listed.data["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(listed.data["tasks"][0]["task_id"], task_id);
        assert_eq!(listed.data["tasks"][0]["status"], "running");

        let got = TaskGetTool::new()
            .call(&json!({"task_id": task_id}), &mut context)
            .await
            .unwrap();
        assert_eq!(got.data["task"]["task_type"], "background");

        let stopped = TaskStopTool::new()
            .call(&json!({"task_id": task_id}), &mut context)
            .await
            .unwrap();
        assert_eq!(stopped.data["task"]["status"], "killed");
        assert!(stopped.data["task"]["pid"].is_null());

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn task_stop_finalizes_background_teammate_and_releases_tasks() {
        let root = std::env::temp_dir().join(format!("kiana-task-stop-team-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("tasks/review")).unwrap();
        fs::create_dir_all(root.join("teams/review")).unwrap();
        let team_file_path = root.join("teams/review/config.json");
        fs::write(
            &team_file_path,
            serde_json::to_string_pretty(&json!({
                "name": "review",
                "members": [
                    {
                        "agentId": "team-lead@review",
                        "name": "team-lead",
                        "isActive": false
                    },
                    {
                        "agentId": "runner@review",
                        "name": "runner",
                        "isActive": true
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("tasks/review/1.json"),
            serde_json::to_string_pretty(&json!({
                "id": "1",
                "title": "Run tests",
                "subject": "Run tests",
                "owner": "runner@review",
                "status": "running",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("tasks/bg-1.json"),
            serde_json::to_string_pretty(&json!({
                "id": "bg-1",
                "prompt": "Run teammate",
                "status": "running",
                "team_name": "review",
                "agent_name": "runner",
                "teammate_id": "runner@review",
                "task_list_id": "review",
                "team_file_path": team_file_path.to_string_lossy().to_string()
            }))
            .unwrap(),
        )
        .unwrap();

        let mut context = test_context_with_background_root(&root);
        let stopped = TaskStopTool::new()
            .call(&json!({ "task_id": "bg-1" }), &mut context)
            .await
            .unwrap();

        assert_eq!(stopped.data["task"]["status"], "killed");
        assert_eq!(stopped.data["task"]["team_lifecycle_finalized"], true);
        assert_eq!(
            stopped.data["task"]["team_lifecycle"]["unassignedTasks"][0]["id"],
            "1"
        );

        let team_file = read_json(&team_file_path);
        let runner = team_file["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["name"] == "runner")
            .unwrap();
        assert_eq!(runner["isActive"], false);
        assert_eq!(runner["lastExitReason"], "terminated");

        let task = read_json(root.join("tasks/review/1.json"));
        assert!(task.get("owner").is_none());
        assert_eq!(task["status"], "pending");

        let inbox = read_json(root.join("teams/review/inboxes/team-lead.json"));
        let body: Value = serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["type"], "teammate_terminated");
        assert!(body["message"].as_str().unwrap().contains("was terminated"));

        let _ = fs::remove_dir_all(root);
    }
}
