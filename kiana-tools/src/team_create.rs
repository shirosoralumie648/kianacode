use crate::tool::*;
use async_trait::async_trait;
use kiana_skills::Command as SkillCommand;
use kiana_types::project_trust_from_app_state;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use uuid::Uuid;

pub use crate::todo_write::TodoWriteTool;

const TEAMS_KEY: &str = "teams";
const MESSAGES_KEY: &str = "messages";
const TEAM_CONTEXT_KEY: &str = "team_context";
const TEAM_MAILBOXES_KEY: &str = "team_mailboxes";
const TEAMS_ROOT_KEY: &str = "teams_root";
const TEAMS_ROOT_ENV: &str = "KIANA_TEAMS_ROOT";
const TASKS_ROOT_KEY: &str = "tasks_root";
const TASKS_ROOT_ENV: &str = "KIANA_TASKS_ROOT";
const TASKS_STATE_KEY: &str = "tasks";
const HIGH_WATER_MARK_FILE: &str = ".highwatermark";
const CONFIG_KEY: &str = "config";
const WORKFLOW_RUNS_KEY: &str = "workflow_runs";
const SKILL_INVOCATIONS_KEY: &str = "skill_invocations";
const INVOKED_SKILLS_KEY: &str = "invoked_skills";
const CRON_JOBS_KEY: &str = "cron_jobs";

#[derive(Debug, Deserialize)]
struct TeamCreateInput {
    #[serde(alias = "team_name")]
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    agent_type: Option<String>,
    #[serde(default)]
    members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TeamDeleteInput {
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageInput {
    message: Value,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfigInput {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WorkflowInput {
    name: String,
    #[serde(default)]
    steps: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct WorktreeInput {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExitPlanModeInput {
    #[serde(default)]
    plan: Option<String>,
    #[serde(
        default,
        rename = "planFilePath",
        alias = "plan_file_path",
        alias = "filePath"
    )]
    plan_file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillInput {
    skill: String,
    #[serde(default)]
    args: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ToolSearchInput {
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    include_schema: bool,
}

#[derive(Debug, Deserialize)]
struct CronCreateInput {
    cron: String,
    prompt: String,
    #[serde(default)]
    recurring: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CronDeleteInput {
    id: String,
}

#[derive(Clone, Debug, Eq)]
struct SessionTeamCleanupTarget {
    team_name: String,
    teams_root: PathBuf,
    tasks_root: PathBuf,
}

impl PartialEq for SessionTeamCleanupTarget {
    fn eq(&self, other: &Self) -> bool {
        self.team_name.eq_ignore_ascii_case(&other.team_name)
            && self.teams_root == other.teams_root
            && self.tasks_root == other.tasks_root
    }
}

impl Hash for SessionTeamCleanupTarget {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.team_name.to_ascii_lowercase().hash(state);
        self.teams_root.hash(state);
        self.tasks_root.hash(state);
    }
}

#[derive(Debug, Deserialize)]
struct MonitorInput {
    #[serde(default)]
    limit: Option<usize>,
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn load_array(context: &ToolContext, key: &str) -> ToolResult<Vec<Value>> {
    let value = context
        .app_state
        .get(key)
        .cloned()
        .unwrap_or_else(|| json!([]));
    serde_json::from_value(value).map_err(ToolError::from)
}

fn save_array(context: &mut ToolContext, key: &str, value: Vec<Value>) {
    context
        .app_state
        .insert(key.to_string(), Value::Array(value));
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn require_non_empty(value: &str, field: &str) -> ToolResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ToolError::ValidationError(format!(
            "{} cannot be empty",
            field
        )));
    }
    Ok(value.to_string())
}

fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
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

fn sanitize_agent_id_part(value: &str) -> String {
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
        .to_ascii_lowercase();
    if sanitized.is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
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

fn team_dir_path(context: &ToolContext, team_name: &str) -> PathBuf {
    teams_root(context).join(sanitize_name(team_name))
}

fn task_list_dir(context: &ToolContext, team_name: &str) -> PathBuf {
    tasks_root(context).join(sanitize_name(team_name))
}

fn high_watermark_path_for_task_list(context: &ToolContext, team_name: &str) -> PathBuf {
    task_list_dir(context, team_name).join(HIGH_WATER_MARK_FILE)
}

fn team_file_path(context: &ToolContext, team_name: &str) -> PathBuf {
    team_dir_path(context, team_name).join("config.json")
}

fn inbox_path(context: &ToolContext, team_name: &str, agent_name: &str) -> PathBuf {
    team_dir_path(context, team_name)
        .join("inboxes")
        .join(format!("{}.json", sanitize_name(agent_name)))
}

fn write_json_file(path: &Path, value: &Value) -> ToolResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn read_high_watermark(path: &Path) -> i64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or_default()
}

fn highest_task_id_from_task_list_dir(dir: &Path) -> ToolResult<i64> {
    if !dir.is_dir() {
        return Ok(0);
    }

    let mut highest = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
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

fn reset_task_list_for_team(context: &ToolContext, team_name: &str) -> ToolResult<()> {
    let dir = task_list_dir(context, team_name);
    fs::create_dir_all(&dir)?;

    let high_watermark_path = high_watermark_path_for_task_list(context, team_name);
    let highest_task_id = highest_task_id_from_task_list_dir(&dir)?;
    let existing_high_watermark = read_high_watermark(&high_watermark_path);
    if highest_task_id > existing_high_watermark {
        fs::write(&high_watermark_path, highest_task_id.to_string())?;
    }

    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

fn cleanup_registry() -> &'static Mutex<HashSet<SessionTeamCleanupTarget>> {
    static REGISTRY: OnceLock<Mutex<HashSet<SessionTeamCleanupTarget>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn cleanup_registry_lock() -> std::sync::MutexGuard<'static, HashSet<SessionTeamCleanupTarget>> {
    cleanup_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn session_team_cleanup_target(context: &ToolContext, team_name: &str) -> SessionTeamCleanupTarget {
    SessionTeamCleanupTarget {
        team_name: sanitize_name(team_name),
        teams_root: teams_root(context),
        tasks_root: tasks_root(context),
    }
}

fn register_team_for_session_cleanup(context: &ToolContext, team_name: &str) {
    cleanup_registry_lock().insert(session_team_cleanup_target(context, team_name));
}

fn unregister_team_for_session_cleanup(context: &ToolContext, team_name: &str) {
    let target = session_team_cleanup_target(context, team_name);
    cleanup_registry_lock().retain(|registered| registered != &target);
}

fn cleanup_team_directories_for_target(target: &SessionTeamCleanupTarget) -> ToolResult<()> {
    let sanitized_name = sanitize_name(&target.team_name);
    let team_dir = target.teams_root.join(&sanitized_name);
    if team_dir.exists() {
        fs::remove_dir_all(team_dir)?;
    }
    let task_dir = target.tasks_root.join(sanitized_name);
    if task_dir.exists() {
        fs::remove_dir_all(task_dir)?;
    }
    Ok(())
}

fn cleanup_session_teams_matching<F>(mut predicate: F) -> ToolResult<usize>
where
    F: FnMut(&SessionTeamCleanupTarget) -> bool,
{
    let targets = {
        let mut registry = cleanup_registry_lock();
        let targets = registry
            .iter()
            .filter(|target| predicate(target))
            .cloned()
            .collect::<Vec<_>>();
        for target in &targets {
            registry.remove(target);
        }
        targets
    };

    for target in &targets {
        cleanup_team_directories_for_target(target)?;
    }
    Ok(targets.len())
}

pub fn cleanup_session_teams() -> ToolResult<usize> {
    cleanup_session_teams_matching(|_| true)
}

#[cfg(test)]
fn cleanup_session_teams_for_roots(teams_root: &Path, tasks_root: &Path) -> ToolResult<usize> {
    cleanup_session_teams_matching(|target| {
        target.teams_root == teams_root && target.tasks_root == tasks_root
    })
}

fn clear_task_list_state(context: &mut ToolContext) {
    context.app_state.remove(TASKS_STATE_KEY);
    context.app_state.remove("task_list_id");
    context.app_state.remove("taskListId");
}

fn read_json_file(path: &Path) -> ToolResult<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

fn read_team_file(context: &ToolContext, team_name: &str) -> ToolResult<Option<Value>> {
    read_json_file(&team_file_path(context, team_name))
}

fn write_team_file(context: &ToolContext, team_name: &str, team_file: &Value) -> ToolResult<()> {
    write_json_file(&team_file_path(context, team_name), team_file)
}

fn team_exists(context: &ToolContext, team_name: &str) -> bool {
    if team_file_path(context, team_name).is_file() {
        return true;
    }
    context
        .app_state
        .get(TEAMS_KEY)
        .and_then(Value::as_array)
        .is_some_and(|teams| {
            teams.iter().any(|team| {
                team.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case(team_name))
            })
        })
}

fn unique_team_name(context: &ToolContext, requested: &str) -> String {
    if !team_exists(context, requested) {
        return requested.to_string();
    }
    for index in 2..1000 {
        let candidate = format!("{requested}-{index}");
        if !team_exists(context, &candidate) {
            return candidate;
        }
    }
    format!("{requested}-{}", Uuid::new_v4().simple())
}

fn team_agent_id(member: &str, team_name: &str) -> String {
    format!(
        "{}@{}",
        sanitize_agent_id_part(member),
        sanitize_agent_id_part(team_name)
    )
}

fn normalized_team_members(members: Vec<String>) -> Vec<String> {
    let mut normalized = vec!["team-lead".to_string()];
    for member in members {
        let Some(member) = trim_optional(Some(member)) else {
            continue;
        };
        if !normalized
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&member))
        {
            normalized.push(member);
        }
    }
    normalized
}

fn active_team_context(context: &ToolContext) -> Option<&serde_json::Map<String, Value>> {
    context
        .app_state
        .get(TEAM_CONTEXT_KEY)
        .or_else(|| context.app_state.get("teamContext"))
        .and_then(Value::as_object)
}

fn active_team_name(context: &ToolContext) -> Option<String> {
    active_team_context(context)?
        .get("team_name")
        .or_else(|| active_team_context(context)?.get("teamName"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn teammate_names(context: &ToolContext) -> Vec<String> {
    let Some(team_context) = active_team_context(context) else {
        return Vec::new();
    };
    let Some(teammates) = team_context.get("teammates").and_then(Value::as_object) else {
        return Vec::new();
    };
    teammates
        .values()
        .filter_map(|teammate| teammate.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn team_member_names(context: &ToolContext, team_name: &str) -> ToolResult<Vec<String>> {
    if let Some(team_file) = read_team_file(context, team_name)? {
        if let Some(members) = team_file.get("members").and_then(Value::as_array) {
            let names = members
                .iter()
                .filter_map(|member| member.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !names.is_empty() {
                return Ok(names);
            }
        }
    }
    Ok(teammate_names(context))
}

fn resolve_message_recipients(to: Option<&str>, context: &ToolContext) -> ToolResult<Vec<String>> {
    let Some(to) = to.map(str::trim).filter(|to| !to.is_empty()) else {
        return Ok(Vec::new());
    };

    let team_name = active_team_name(context);
    let names = if let Some(team_name) = team_name.as_deref() {
        team_member_names(context, team_name)?
    } else {
        teammate_names(context)
    };
    if to == "*"
        || team_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(to))
    {
        if team_name.is_none() {
            return Err(ToolError::ValidationError(
                "broadcast requires an active team".to_string(),
            ));
        }
        return Ok(names
            .into_iter()
            .filter(|name| !name.eq_ignore_ascii_case("team-lead"))
            .collect());
    }

    if names.is_empty() || names.iter().any(|name| name.eq_ignore_ascii_case(to)) {
        return Ok(vec![to.to_string()]);
    }

    Err(ToolError::ValidationError(format!(
        "unknown teammate '{to}'"
    )))
}

fn append_mailbox_message(
    context: &mut ToolContext,
    team_name: Option<&str>,
    recipient: &str,
    message: &Value,
) -> ToolResult<()> {
    let mut mailboxes = context
        .app_state
        .get(TEAM_MAILBOXES_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut inbox = mailboxes
        .remove(recipient)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    inbox.push(message.clone());
    mailboxes.insert(recipient.to_string(), Value::Array(inbox));
    context
        .app_state
        .insert(TEAM_MAILBOXES_KEY.to_string(), Value::Object(mailboxes));

    let Some(team_name) = team_name else {
        return Ok(());
    };
    let text = message
        .get("content")
        .or_else(|| message.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mailbox_entry = json!({
        "from": message.get("from").and_then(Value::as_str).unwrap_or("team-lead"),
        "text": text,
        "summary": message.get("summary").cloned().unwrap_or(Value::Null),
        "timestamp": message.get("timestamp").cloned().unwrap_or_else(|| json!(now_unix_seconds().to_string())),
        "read": false
    });
    let path = inbox_path(context, team_name, recipient);
    let mut inbox = read_json_file(&path)?
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    inbox.push(mailbox_entry);
    write_json_file(&path, &Value::Array(inbox))?;
    Ok(())
}

fn recipients_for_delivery(message: &Value) -> Vec<String> {
    message
        .get("recipients")
        .and_then(Value::as_array)
        .map(|recipients| {
            recipients
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
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
        .or_else(|| {
            std::env::var("KIANA_AGENT_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "team-lead".to_string())
}

fn current_agent_id(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get("agent_id")
        .or_else(|| context.app_state.get("agentId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_AGENT_ID")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_ID").ok())
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
        })
}

fn current_teammate_plan_mode_required(context: &ToolContext, agent_name: &str) -> bool {
    if value_bool(context.app_state.get("planModeRequired"))
        .or_else(|| value_bool(context.app_state.get("plan_mode_required")))
        .is_some_and(|required| required)
    {
        return true;
    }

    let Some(team_context) = active_team_context(context) else {
        return false;
    };
    let Some(teammates) = team_context.get("teammates").and_then(Value::as_object) else {
        return false;
    };
    let agent_id = current_agent_id(context);
    teammates.iter().any(|(id, teammate)| {
        let id_matches = agent_id.as_deref().is_some_and(|agent_id| agent_id == id);
        let name_matches = teammate
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name.eq_ignore_ascii_case(agent_name));
        if !id_matches && !name_matches {
            return false;
        }
        value_bool(teammate.get("planModeRequired"))
            .or_else(|| value_bool(teammate.get("plan_mode_required")))
            .unwrap_or(false)
    })
}

fn value_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(value) => Some(*value),
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" => Some(true),
            "false" | "no" | "n" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn message_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn message_object_type(value: &Value) -> Option<&str> {
    value
        .as_object()?
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn object_string<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn object_bool(object: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(|value| match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" | "approve" | "approved" => Some(true),
            "false" | "no" | "n" | "0" | "reject" | "rejected" => Some(false),
            _ => None,
        },
        _ => None,
    })
}

fn object_value<'a>(
    object: &'a serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<&'a Value> {
    keys.iter().find_map(|key| object.get(*key))
}

fn require_team_lead_sender(sender: &str, action: &str) -> ToolResult<()> {
    if sender.eq_ignore_ascii_case("team-lead") {
        Ok(())
    } else {
        Err(ToolError::ValidationError(format!(
            "Only the team lead can {action}"
        )))
    }
}

fn permission_update_behavior(object: &serde_json::Map<String, Value>) -> ToolResult<String> {
    let permission_update = object_value(object, &["permissionUpdate", "permission_update"]);
    let behavior = permission_update
        .and_then(|value| value.get("behavior"))
        .and_then(Value::as_str)
        .or_else(|| object_string(object, "behavior"))
        .map(str::trim)
        .filter(|behavior| !behavior.is_empty())
        .ok_or_else(|| {
            ToolError::ValidationError(
                "behavior is required for team_permission_update".to_string(),
            )
        })?;
    match behavior {
        "allow" | "deny" | "ask" => Ok(behavior.to_string()),
        _ => Err(ToolError::ValidationError(
            "behavior must be allow, deny, or ask".to_string(),
        )),
    }
}

fn permission_update_rules(object: &serde_json::Map<String, Value>) -> ToolResult<Vec<Value>> {
    let permission_update = object_value(object, &["permissionUpdate", "permission_update"]);
    if let Some(rules) = permission_update
        .and_then(|value| value.get("rules"))
        .and_then(Value::as_array)
        .filter(|rules| !rules.is_empty())
    {
        return Ok(rules.clone());
    }
    if let Some(rules) = object
        .get("rules")
        .and_then(Value::as_array)
        .filter(|rules| !rules.is_empty())
    {
        return Ok(rules.clone());
    }

    let tool_name = object_string(object, "tool_name")
        .or_else(|| object_string(object, "toolName"))
        .ok_or_else(|| {
            ToolError::ValidationError(
                "rules or tool_name is required for team_permission_update".to_string(),
            )
        })?;
    let rule_content =
        object_string(object, "rule_content").or_else(|| object_string(object, "ruleContent"));
    let rule = json!({
        "toolName": tool_name,
        "tool_name": tool_name,
        "ruleContent": rule_content,
        "rule_content": rule_content
    });
    Ok(vec![rule])
}

fn request_id(prefix: &str, target: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        sanitize_name(target),
        Uuid::new_v4().simple()
    )
}

fn active_non_lead_members(team_file: &Value) -> Vec<String> {
    team_file
        .get("members")
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| {
                    let name = member.get("name").and_then(Value::as_str)?;
                    if name.eq_ignore_ascii_case("team-lead") {
                        return None;
                    }
                    let inactive = member
                        .get("isActive")
                        .or_else(|| member.get("is_active"))
                        .and_then(Value::as_bool)
                        == Some(false);
                    if inactive {
                        None
                    } else {
                        Some(name.to_string())
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn record_team_message(context: &mut ToolContext, message: Value) -> ToolResult<Value> {
    let team_name = active_team_name(context);
    for recipient in recipients_for_delivery(&message) {
        append_mailbox_message(context, team_name.as_deref(), &recipient, &message)?;
    }
    let mut messages = load_array(context, MESSAGES_KEY)?;
    messages.push(message.clone());
    save_array(context, MESSAGES_KEY, messages);
    Ok(message)
}

fn mailboxes_json(context: &ToolContext) -> Value {
    context
        .app_state
        .get(TEAM_MAILBOXES_KEY)
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn read_team_tasks(context: &ToolContext, team_name: &str) -> ToolResult<Vec<Value>> {
    let dir = task_list_dir(context, team_name);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut tasks = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        tasks.push(serde_json::from_str(&fs::read_to_string(path)?)?);
    }
    Ok(tasks)
}

fn task_status_is_terminal(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("completed" | "complete" | "success" | "failed" | "cancelled" | "canceled" | "killed")
    )
}

fn team_agent_statuses(context: &ToolContext) -> ToolResult<Option<Value>> {
    let Some(team_name) = active_team_name(context) else {
        return Ok(None);
    };
    let Some(team_file) = read_team_file(context, &team_name)? else {
        return Ok(None);
    };
    let members = team_file
        .get("members")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tasks = read_team_tasks(context, &team_name)?;

    let statuses = members
        .into_iter()
        .filter_map(|member| {
            let name = member.get("name").and_then(Value::as_str)?.to_string();
            let agent_id = member
                .get("agentId")
                .or_else(|| member.get("agent_id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let agent_type = member
                .get("agentType")
                .or_else(|| member.get("agent_type"))
                .cloned()
                .unwrap_or(Value::Null);
            let is_active = member
                .get("isActive")
                .or_else(|| member.get("is_active"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let lifecycle_status = member
                .get("lifecycleStatus")
                .or_else(|| member.get("lifecycle_status"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut current_tasks = Vec::<String>::new();
            for task in &tasks {
                if task_status_is_terminal(task.get("status").and_then(Value::as_str)) {
                    continue;
                }
                let Some(owner) = task.get("owner").and_then(Value::as_str) else {
                    continue;
                };
                if owner == name || (!agent_id.is_empty() && owner == agent_id) {
                    if let Some(task_id) = task.get("id").and_then(Value::as_str) {
                        if !current_tasks.iter().any(|existing| existing == task_id) {
                            current_tasks.push(task_id.to_string());
                        }
                    }
                }
            }
            let status = if current_tasks.is_empty() {
                if lifecycle_status.as_deref() == Some("running") {
                    "running"
                } else {
                    "idle"
                }
            } else {
                "busy"
            };
            Some(json!({
                "agentId": agent_id,
                "agent_id": agent_id,
                "name": name,
                "agentType": agent_type.clone(),
                "agent_type": agent_type,
                "status": status,
                "isActive": is_active,
                "is_active": is_active,
                "lifecycleStatus": lifecycle_status,
                "lifecycle_status": lifecycle_status,
                "currentTasks": current_tasks.clone(),
                "current_tasks": current_tasks
            }))
        })
        .collect::<Vec<_>>();

    Ok(Some(json!({
        "teamName": team_name,
        "team_name": team_name,
        "agents": statuses
    })))
}

fn protocol_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn protocol_value_field(value: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| value.get(*key).cloned())
        .unwrap_or(Value::Null)
}

fn sandbox_host_from_request(value: &Value) -> Option<String> {
    protocol_string_field(value, &["host"]).or_else(|| {
        value
            .get("hostPattern")
            .or_else(|| value.get("host_pattern"))
            .and_then(|host_pattern| protocol_string_field(host_pattern, &["host"]))
    })
}

fn pending_protocol_request_from_inbox_message(message: &Value) -> Option<(String, Value)> {
    if message
        .get("read")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let text = message
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())?;
    let body = serde_json::from_str::<Value>(text).ok()?;
    let request_type = body.get("type").and_then(Value::as_str)?;
    let from = protocol_string_field(&body, &["from"])
        .or_else(|| protocol_string_field(message, &["from"]))
        .unwrap_or_else(|| "team-lead".to_string());
    let request_id = protocol_string_field(&body, &["requestId", "request_id"]);
    let timestamp = protocol_value_field(&body, &["timestamp"]);
    let message_timestamp = protocol_value_field(message, &["timestamp"]);

    let request = match request_type {
        "permission_request" => {
            let agent_id = protocol_string_field(&body, &["agentId", "agent_id"]);
            let tool_name = protocol_string_field(&body, &["toolName", "tool_name"]);
            let tool_use_id = protocol_string_field(&body, &["toolUseId", "tool_use_id"]);
            let description = protocol_string_field(&body, &["description"]);
            let input = protocol_value_field(&body, &["input"]);
            let permission_suggestions =
                protocol_value_field(&body, &["permissionSuggestions", "permission_suggestions"]);
            json!({
                "type": request_type,
                "requestType": request_type,
                "request_type": request_type,
                "requestId": request_id.clone(),
                "request_id": request_id,
                "from": from,
                "agentId": agent_id.clone(),
                "agent_id": agent_id,
                "toolName": tool_name.clone(),
                "tool_name": tool_name,
                "toolUseId": tool_use_id.clone(),
                "tool_use_id": tool_use_id,
                "description": description,
                "input": input,
                "permissionSuggestions": permission_suggestions.clone(),
                "permission_suggestions": permission_suggestions,
                "timestamp": timestamp,
                "messageTimestamp": message_timestamp.clone(),
                "message_timestamp": message_timestamp
            })
        }
        "sandbox_permission_request" => {
            let worker_id = protocol_string_field(&body, &["workerId", "worker_id"]);
            let worker_name = protocol_string_field(&body, &["workerName", "worker_name"]);
            let worker_color = protocol_string_field(&body, &["workerColor", "worker_color"]);
            let host = sandbox_host_from_request(&body);
            let host_pattern = protocol_value_field(&body, &["hostPattern", "host_pattern"]);
            let created_at = protocol_value_field(&body, &["createdAt", "created_at"]);
            json!({
                "type": request_type,
                "requestType": request_type,
                "request_type": request_type,
                "requestId": request_id.clone(),
                "request_id": request_id,
                "from": from,
                "workerId": worker_id.clone(),
                "worker_id": worker_id,
                "workerName": worker_name.clone(),
                "worker_name": worker_name,
                "workerColor": worker_color.clone(),
                "worker_color": worker_color,
                "host": host,
                "hostPattern": host_pattern.clone(),
                "host_pattern": host_pattern,
                "createdAt": created_at.clone(),
                "created_at": created_at,
                "timestamp": timestamp,
                "messageTimestamp": message_timestamp.clone(),
                "message_timestamp": message_timestamp
            })
        }
        "plan_approval_request" => {
            let plan_file_path = protocol_string_field(
                &body,
                &["planFilePath", "plan_file_path", "filePath", "file_path"],
            );
            let plan_content = protocol_string_field(&body, &["planContent", "plan_content"]);
            json!({
                "type": request_type,
                "requestType": request_type,
                "request_type": request_type,
                "requestId": request_id.clone(),
                "request_id": request_id,
                "from": from,
                "planFilePath": plan_file_path.clone(),
                "plan_file_path": plan_file_path,
                "planContent": plan_content.clone(),
                "plan_content": plan_content,
                "timestamp": timestamp,
                "messageTimestamp": message_timestamp.clone(),
                "message_timestamp": message_timestamp
            })
        }
        "shutdown_request" => {
            let reason = protocol_string_field(&body, &["reason"]);
            json!({
                "type": request_type,
                "requestType": request_type,
                "request_type": request_type,
                "requestId": request_id.clone(),
                "request_id": request_id,
                "from": from,
                "reason": reason,
                "timestamp": timestamp,
                "messageTimestamp": message_timestamp.clone(),
                "message_timestamp": message_timestamp
            })
        }
        _ => return None,
    };

    Some((request_type.to_string(), request))
}

fn pending_protocol_requests(context: &ToolContext) -> ToolResult<Option<Value>> {
    let Some(team_name) = active_team_name(context) else {
        return Ok(None);
    };
    let inbox = read_json_file(&inbox_path(context, &team_name, "team-lead"))?
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut permission_requests = Vec::new();
    let mut sandbox_permission_requests = Vec::new();
    let mut plan_approval_requests = Vec::new();
    let mut shutdown_requests = Vec::new();
    let mut requests = Vec::new();

    for message in inbox {
        let Some((request_type, request)) = pending_protocol_request_from_inbox_message(&message)
        else {
            continue;
        };
        match request_type.as_str() {
            "permission_request" => permission_requests.push(request.clone()),
            "sandbox_permission_request" => sandbox_permission_requests.push(request.clone()),
            "plan_approval_request" => plan_approval_requests.push(request.clone()),
            "shutdown_request" => shutdown_requests.push(request.clone()),
            _ => {}
        }
        requests.push(request);
    }

    Ok(Some(json!({
        "teamName": team_name,
        "team_name": team_name,
        "total": requests.len(),
        "counts": {
            "total": requests.len(),
            "permissionRequests": permission_requests.len(),
            "permission_requests": permission_requests.len(),
            "sandboxPermissionRequests": sandbox_permission_requests.len(),
            "sandbox_permission_requests": sandbox_permission_requests.len(),
            "planApprovalRequests": plan_approval_requests.len(),
            "plan_approval_requests": plan_approval_requests.len(),
            "shutdownRequests": shutdown_requests.len(),
            "shutdown_requests": shutdown_requests.len()
        },
        "requests": requests,
        "permissionRequests": permission_requests.clone(),
        "permission_requests": permission_requests,
        "sandboxPermissionRequests": sandbox_permission_requests.clone(),
        "sandbox_permission_requests": sandbox_permission_requests,
        "planApprovalRequests": plan_approval_requests.clone(),
        "plan_approval_requests": plan_approval_requests,
        "shutdownRequests": shutdown_requests.clone(),
        "shutdown_requests": shutdown_requests
    })))
}

fn mark_protocol_request_read(
    context: &ToolContext,
    recipient: &str,
    request_types: &[&str],
    request_id: &str,
) -> ToolResult<bool> {
    let Some(team_name) = active_team_name(context) else {
        return Ok(false);
    };
    let path = inbox_path(context, &team_name, recipient);
    let Some(Value::Array(mut inbox)) = read_json_file(&path)? else {
        return Ok(false);
    };

    let mut changed = false;
    for message in inbox.iter_mut() {
        if message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(text) = message
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        let Ok(body) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        let request_type = body.get("type").and_then(Value::as_str);
        if !request_type.is_some_and(|request_type| request_types.contains(&request_type)) {
            continue;
        }
        let Some(candidate_id) = protocol_string_field(&body, &["requestId", "request_id"]) else {
            continue;
        };
        if candidate_id != request_id {
            continue;
        }
        if let Some(object) = message.as_object_mut() {
            object.insert("read".to_string(), json!(true));
            changed = true;
        }
    }

    if changed {
        write_json_file(&path, &Value::Array(inbox))?;
    }
    Ok(changed)
}

fn remove_protocol_request_state(context: &mut ToolContext, key: &str, request_id: &str) {
    let Some(mut requests) = context
        .app_state
        .get(key)
        .and_then(Value::as_object)
        .cloned()
    else {
        return;
    };
    if requests.remove(request_id).is_some() {
        context
            .app_state
            .insert(key.to_string(), Value::Object(requests));
    }
}

fn normalize_skill_name(skill: &str) -> Result<String, String> {
    let trimmed = skill.trim();
    if trimmed.is_empty() {
        return Err("skill cannot be empty".to_string());
    }
    let normalized = trimmed
        .strip_prefix('/')
        .unwrap_or(trimmed)
        .trim()
        .to_string();
    if normalized.is_empty() {
        return Err("skill cannot be empty".to_string());
    }
    Ok(normalized)
}

async fn resolve_invocable_skill(
    skill: &str,
    cwd: &str,
    app_state: &HashMap<String, Value>,
) -> Result<SkillCommand, String> {
    let normalized = normalize_skill_name(skill)?;
    let commands =
        kiana_skills::load_all_skills_with_trust(cwd, project_trust_from_app_state(app_state))
            .await;
    let command = kiana_skills::find_command(&normalized, &commands)
        .cloned()
        .ok_or_else(|| format!("Unknown skill: {}", normalized))?;

    if command.disable_model_invocation {
        return Err(format!(
            "Skill {} cannot be used with Skill tool due to disable-model-invocation",
            normalized
        ));
    }

    Ok(command)
}

fn skill_path(command: &SkillCommand) -> String {
    command
        .skill_root
        .as_ref()
        .map(|path| path.join("SKILL.md").to_string_lossy().to_string())
        .unwrap_or_default()
}

fn skill_command_to_json(command: &SkillCommand, path: &str) -> Value {
    json!({
        "name": command.name.clone(),
        "display_name": command.display_name.clone(),
        "description": command.description.clone(),
        "when_to_use": command.when_to_use.clone(),
        "argument_hint": command.argument_hint.clone(),
        "allowed_tools": command.allowed_tools.clone(),
        "model": command.model.clone(),
        "disable_model_invocation": command.disable_model_invocation,
        "user_invocable": command.user_invocable,
        "source": command.source,
        "loaded_from": command.loaded_from,
        "context": command.context.clone(),
        "paths": command.paths.clone(),
        "path": path,
        "content": command.content.clone()
    })
}

pub struct TeamCreateTool;
impl TeamCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "TeamCreate"
    }
    fn description(&self) -> &str {
        "Create a session-local team context"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "team_name": { "type": "string" },
                "description": { "type": "string" },
                "agent_type": { "type": "string" },
                "members": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["name"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "team": { "type": "object" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TeamCreateInput = serde_json::from_value(input.clone())?;
        let requested_name = require_non_empty(&input.name, "name")?;
        if active_team_name(context).is_some() {
            return Err(ToolError::ValidationError(
                "a team is already active in this session".to_string(),
            ));
        }
        let name = unique_team_name(context, &requested_name);
        let members = normalized_team_members(input.members);
        let lead_agent_id = team_agent_id("team-lead", &name);
        let task_list_id = sanitize_name(&name);
        let created_at = now_unix_seconds();
        let member_records = members
            .iter()
            .map(|member| {
                let agent_id = team_agent_id(member, &name);
                json!({
                    "agentId": agent_id,
                    "agent_id": agent_id,
                    "name": member,
                    "agentType": if member == "team-lead" {
                        trim_optional(input.agent_type.clone()).unwrap_or_else(|| "team-lead".to_string())
                    } else {
                        "teammate".to_string()
                    },
                    "agent_type": if member == "team-lead" {
                        trim_optional(input.agent_type.clone()).unwrap_or_else(|| "team-lead".to_string())
                    } else {
                        "teammate".to_string()
                    },
                    "joinedAt": created_at,
                    "joined_at": created_at,
                    "tmuxPaneId": "",
                    "tmux_pane_id": "",
                    "cwd": context.cwd.clone(),
                    "subscriptions": [],
                    "isActive": false
                })
            })
            .collect::<Vec<_>>();
        let teammates = member_records
            .iter()
            .map(|member| {
                let agent_id = member
                    .get("agentId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                (
                    agent_id.clone(),
                    json!({
                        "agent_id": agent_id,
                        "agentId": agent_id,
                        "name": member.get("name").cloned().unwrap_or(Value::Null),
                        "agent_type": member.get("agent_type").cloned().unwrap_or(Value::Null),
                        "agentType": member.get("agentType").cloned().unwrap_or(Value::Null),
                        "cwd": context.cwd.clone(),
                        "spawned_at": created_at
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let mut teams = load_array(context, TEAMS_KEY)?;
        let team = json!({
            "id": Uuid::new_v4().to_string(),
            "name": name,
            "description": trim_optional(input.description),
            "members": members,
            "lead_agent_id": lead_agent_id,
            "leadAgentId": lead_agent_id,
            "team_file_path": team_file_path(context, &name).to_string_lossy().to_string(),
            "created_at": created_at
        });
        let team_file = json!({
            "name": team["name"],
            "description": team["description"],
            "createdAt": created_at,
            "created_at": created_at,
            "leadAgentId": lead_agent_id,
            "lead_agent_id": lead_agent_id,
            "leadSessionId": context.app_state.get("session_id").cloned().unwrap_or(Value::Null),
            "lead_session_id": context.app_state.get("session_id").cloned().unwrap_or(Value::Null),
            "members": member_records
        });
        write_team_file(context, &name, &team_file)?;
        reset_task_list_for_team(context, &name)?;
        register_team_for_session_cleanup(context, &name);
        teams.push(team.clone());
        save_array(context, TEAMS_KEY, teams);
        context.app_state.remove(TASKS_STATE_KEY);
        let team_context = json!({
            "team_name": team["name"],
            "teamName": team["name"],
            "team_file_path": team["team_file_path"],
            "teamFilePath": team["team_file_path"],
            "lead_agent_id": lead_agent_id,
            "leadAgentId": lead_agent_id,
            "task_list_id": task_list_id.clone(),
            "taskListId": task_list_id.clone(),
            "teammates": teammates
        });
        context
            .app_state
            .insert(TEAM_CONTEXT_KEY.to_string(), team_context.clone());
        context
            .app_state
            .insert("teamContext".to_string(), team_context.clone());
        context
            .app_state
            .insert("task_list_id".to_string(), json!(task_list_id.clone()));
        context
            .app_state
            .insert("taskListId".to_string(), json!(task_list_id));
        Ok(ToolOutput {
            data: json!({
                "success": true,
                "team_name": team["name"],
                "team_file_path": team["team_file_path"],
                "lead_agent_id": lead_agent_id,
                "team": team,
                "team_file": team_file,
                "team_context": team_context
            }),
            metadata: None,
        })
    }
}

pub struct TeamDeleteTool;
impl TeamDeleteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "TeamDelete"
    }
    fn description(&self) -> &str {
        "Delete a session-local team by id or name"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": { "type": "string" },
                "name": { "type": "string" }
            }
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "deleted": { "type": "object" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TeamDeleteInput = serde_json::from_value(input.clone())?;
        let team_id = trim_optional(input.team_id);
        let mut name = trim_optional(input.name).or_else(|| active_team_name(context));
        if name.is_none() && team_id.is_none() {
            return Ok(ToolOutput {
                data: json!({
                    "success": true,
                    "message": "No team name found, nothing to clean up",
                    "team_name": Value::Null,
                    "deleted": []
                }),
                metadata: None,
            });
        }

        let mut teams = load_array(context, TEAMS_KEY)?;
        let mut deleted = Vec::new();
        if name.is_none() {
            name = team_id.as_ref().and_then(|id| {
                teams
                    .iter()
                    .find(|team| team.get("id").and_then(Value::as_str) == Some(id.as_str()))
                    .and_then(|team| team.get("name").and_then(Value::as_str))
                    .map(str::to_string)
            });
        }
        let target_name = name.clone();
        if let Some(team_name) = target_name.as_deref() {
            if let Some(team_file) = read_team_file(context, team_name)? {
                let active_members = active_non_lead_members(&team_file);
                if !active_members.is_empty() {
                    return Ok(ToolOutput {
                        data: json!({
                            "success": false,
                            "message": format!(
                                "Cannot cleanup team with {} active member(s): {}. Use requestShutdown to gracefully terminate teammates first.",
                                active_members.len(),
                                active_members.join(", ")
                            ),
                            "team_name": team_name,
                            "deleted": []
                        }),
                        metadata: None,
                    });
                }
            }
        }

        teams.retain(|team| {
            let matches_id = team_id
                .as_ref()
                .is_some_and(|id| team.get("id").and_then(Value::as_str) == Some(id.as_str()));
            let matches_name = target_name.as_ref().is_some_and(|name| {
                team.get("name").and_then(Value::as_str) == Some(name.as_str())
            });
            if matches_id || matches_name {
                deleted.push(team.clone());
                false
            } else {
                true
            }
        });

        if deleted.is_empty() {
            if let Some(team_name) = target_name.as_deref() {
                if team_file_path(context, team_name).is_file() {
                    deleted.push(json!({ "name": team_name }));
                } else {
                    return Err(ToolError::Other("team not found".to_string()));
                }
            }
        }
        if let Some(team_name) = target_name.as_deref() {
            let team_dir = team_dir_path(context, team_name);
            if team_dir.exists() {
                fs::remove_dir_all(team_dir)?;
            }
            let task_dir = task_list_dir(context, team_name);
            if task_dir.exists() {
                fs::remove_dir_all(task_dir)?;
            }
            unregister_team_for_session_cleanup(context, team_name);
        }
        if deleted.iter().any(|team| {
            let deleted_name = team.get("name").and_then(Value::as_str);
            active_team_name(context)
                .as_deref()
                .is_some_and(|active| deleted_name == Some(active))
        }) {
            context.app_state.remove(TEAM_CONTEXT_KEY);
            context.app_state.remove("teamContext");
            context.app_state.remove(TEAM_MAILBOXES_KEY);
            clear_task_list_state(context);
        }
        save_array(context, TEAMS_KEY, teams);
        Ok(ToolOutput {
            data: json!({
                "success": true,
                "message": target_name
                    .as_ref()
                    .map(|name| format!("Cleaned up directories and worktrees for team \"{name}\""))
                    .unwrap_or_else(|| "No team name found, nothing to clean up".to_string()),
                "team_name": target_name,
                "deleted": deleted
            }),
            metadata: None,
        })
    }
}

pub struct SendMessageTool;
impl SendMessageTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "SendMessage"
    }
    fn description(&self) -> &str {
        "Send a message to a teammate mailbox or record it for the current session"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "oneOf": [
                        { "type": "string" },
                        {
                            "type": "object",
                            "properties": {
                                    "type": {
                                        "type": "string",
                                        "enum": ["shutdown_request", "shutdown_response", "plan_approval_response", "permission_response", "sandbox_permission_response", "team_permission_update", "mode_set_request"]
                                    },
                                    "request_id": { "type": "string" },
                                    "approve": { "type": "boolean" },
                                    "allow": { "type": "boolean" },
                                    "mode": { "type": "string" },
                                    "behavior": { "type": "string", "enum": ["allow", "deny", "ask"] },
                                    "rules": { "type": "array" },
                                    "permissionUpdate": { "type": "object" },
                                    "permission_update": { "type": "object" },
                                    "host": { "type": "string" },
                                    "reason": { "type": "string" },
                                    "feedback": { "type": "string" },
                                    "tool_name": { "type": "string" },
                                    "rule_content": { "type": "string" },
                                    "directory_path": { "type": "string" },
                                    "input": { "type": "object" },
                                    "updated_input": { "type": "object" },
                                    "permission_updates": { "type": "array" }
                            },
                            "required": ["type"]
                        }
                    ]
                },
                "to": { "type": "string" },
                "summary": { "type": "string" },
                "role": { "type": "string" }
            },
            "required": ["message"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "message": { "type": "object" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: SendMessageInput = serde_json::from_value(input.clone())?;
        let to = trim_optional(input.to);
        let summary = trim_optional(input.summary);
        let sender = sender_name(context);
        let role = trim_optional(input.role).unwrap_or_else(|| "assistant".to_string());
        let now = now_unix_seconds();
        let timestamp = now.to_string();

        if let Some(content) = message_string(&input.message) {
            let recipients = resolve_message_recipients(to.as_deref(), context)?;
            let message = record_team_message(
                context,
                json!({
                    "id": Uuid::new_v4().to_string(),
                    "role": role,
                    "from": sender,
                    "to": to,
                    "recipients": recipients,
                    "summary": summary,
                    "content": content,
                    "timestamp": timestamp,
                    "created_at": now
                }),
            )?;
            let recipients = recipients_for_delivery(&message);
            let target = if recipients.len() > 1 {
                "@team".to_string()
            } else {
                to.clone().unwrap_or_default()
            };
            return Ok(ToolOutput {
                data: json!({
                    "success": true,
                    "message": if recipients.len() > 1 {
                        format!("Message broadcast to {} teammate(s): {}", recipients.len(), recipients.join(", "))
                    } else if let Some(target) = to.as_deref() {
                        format!("Message sent to {target}'s inbox")
                    } else {
                        "Message recorded".to_string()
                    },
                    "recipients": recipients,
                    "routing": {
                        "sender": message["from"],
                        "target": target,
                        "summary": message["summary"],
                        "content": message["content"]
                    },
                    "message_record": message,
                    "mailboxes": mailboxes_json(context)
                }),
                metadata: None,
            });
        }

        let object = input.message.as_object().ok_or_else(|| {
            ToolError::ValidationError("message must be a string or object".to_string())
        })?;
        let message_type = message_object_type(&input.message)
            .ok_or_else(|| ToolError::ValidationError("message.type is required".to_string()))?;
        let target = to
            .as_deref()
            .ok_or_else(|| ToolError::ValidationError("to is required".to_string()))?;
        if target == "*" && message_type != "team_permission_update" {
            return Err(ToolError::ValidationError(
                "structured messages cannot be broadcast".to_string(),
            ));
        }

        match message_type {
            "shutdown_request" => {
                let id = request_id("shutdown", target);
                let body = json!({
                    "type": "shutdown_request",
                    "requestId": id,
                    "request_id": id,
                    "from": sender,
                    "reason": object_string(object, "reason"),
                    "timestamp": timestamp
                });
                let content = serde_json::to_string(&body)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": sender,
                        "to": target,
                        "recipients": [target],
                        "summary": "shutdown_request",
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                Ok(ToolOutput {
                    data: json!({
                        "success": true,
                        "message": format!("Shutdown request sent to {target}. Request ID: {id}"),
                        "request_id": id,
                        "target": target,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            "shutdown_response" => {
                if !target.eq_ignore_ascii_case("team-lead") {
                    return Err(ToolError::ValidationError(
                        "shutdown_response must be sent to \"team-lead\"".to_string(),
                    ));
                }
                let id = object_string(object, "request_id")
                    .or_else(|| object_string(object, "requestId"))
                    .ok_or_else(|| {
                        ToolError::ValidationError(
                            "request_id is required for shutdown_response".to_string(),
                        )
                    })?;
                let approve = object_bool(object, "approve").ok_or_else(|| {
                    ToolError::ValidationError(
                        "approve is required for shutdown_response".to_string(),
                    )
                })?;
                let body = if approve {
                    json!({
                        "type": "shutdown_approved",
                        "requestId": id,
                        "request_id": id,
                        "from": sender,
                        "timestamp": timestamp
                    })
                } else {
                    let reason = object_string(object, "reason").ok_or_else(|| {
                        ToolError::ValidationError(
                            "reason is required when rejecting a shutdown request".to_string(),
                        )
                    })?;
                    json!({
                        "type": "shutdown_rejected",
                        "requestId": id,
                        "request_id": id,
                        "from": sender,
                        "reason": reason,
                        "timestamp": timestamp
                    })
                };
                let content = serde_json::to_string(&body)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": sender,
                        "to": "team-lead",
                        "recipients": ["team-lead"],
                        "summary": if approve { "shutdown_approved" } else { "shutdown_rejected" },
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                if approve {
                    context
                        .app_state
                        .insert("teammate_shutdown_approved".to_string(), json!(true));
                }
                let resolved_request =
                    mark_protocol_request_read(context, &sender, &["shutdown_request"], id)?;
                Ok(ToolOutput {
                    data: json!({
                        "success": true,
                        "message": if approve {
                            format!("Shutdown approved. Sent confirmation to team-lead. Agent {} is now exiting.", sender)
                        } else {
                            format!("Shutdown rejected. Reason: \"{}\". Continuing to work.", object_string(object, "reason").unwrap_or_default())
                        },
                        "request_id": id,
                        "resolved_request": resolved_request,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            "plan_approval_response" => {
                if !sender.eq_ignore_ascii_case("team-lead") {
                    return Err(ToolError::ValidationError(
                        "Only the team lead can approve or reject plans".to_string(),
                    ));
                }
                let id = object_string(object, "request_id")
                    .or_else(|| object_string(object, "requestId"))
                    .ok_or_else(|| {
                        ToolError::ValidationError(
                            "request_id is required for plan_approval_response".to_string(),
                        )
                    })?;
                let approve = object_bool(object, "approve").ok_or_else(|| {
                    ToolError::ValidationError(
                        "approve is required for plan_approval_response".to_string(),
                    )
                })?;
                let permission_mode = context
                    .app_state
                    .get("mode")
                    .or_else(|| context.app_state.get("permission_mode"))
                    .and_then(Value::as_str)
                    .filter(|mode| *mode != "plan")
                    .unwrap_or("default");
                let body = if approve {
                    json!({
                        "type": "plan_approval_response",
                        "requestId": id,
                        "request_id": id,
                        "approved": true,
                        "permissionMode": permission_mode,
                        "permission_mode": permission_mode,
                        "timestamp": timestamp
                    })
                } else {
                    let feedback =
                        object_string(object, "feedback").unwrap_or("Plan needs revision");
                    json!({
                        "type": "plan_approval_response",
                        "requestId": id,
                        "request_id": id,
                        "approved": false,
                        "feedback": feedback,
                        "timestamp": timestamp
                    })
                };
                let content = serde_json::to_string(&body)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": "team-lead",
                        "to": target,
                        "recipients": [target],
                        "summary": "plan_approval_response",
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                let resolved_request = mark_protocol_request_read(
                    context,
                    "team-lead",
                    &["plan_approval_request"],
                    id,
                )?;
                Ok(ToolOutput {
                    data: json!({
                        "success": true,
                        "message": if approve {
                            format!("Plan approved for {target}. They will receive the approval and can proceed with implementation.")
                        } else {
                            format!("Plan rejected for {target} with feedback: \"{}\"", object_string(object, "feedback").unwrap_or("Plan needs revision"))
                        },
                        "request_id": id,
                        "resolved_request": resolved_request,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            "permission_response" => {
                if !sender.eq_ignore_ascii_case("team-lead") {
                    return Err(ToolError::ValidationError(
                        "Only the team lead can answer permission requests".to_string(),
                    ));
                }
                let id = object_string(object, "request_id")
                    .or_else(|| object_string(object, "requestId"))
                    .ok_or_else(|| {
                        ToolError::ValidationError(
                            "request_id is required for permission_response".to_string(),
                        )
                    })?;
                let approve = object_bool(object, "approve").ok_or_else(|| {
                    ToolError::ValidationError(
                        "approve is required for permission_response".to_string(),
                    )
                })?;
                let original_request = context
                    .app_state
                    .get("mailbox_permission_requests")
                    .and_then(Value::as_object)
                    .and_then(|requests| requests.get(id))
                    .cloned();
                let tool_name = object_string(object, "tool_name")
                    .or_else(|| object_string(object, "toolName"))
                    .map(str::to_string)
                    .or_else(|| {
                        original_request
                            .as_ref()
                            .and_then(|request| {
                                request.get("tool_name").or_else(|| request.get("toolName"))
                            })
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    });
                let request_input = object
                    .get("input")
                    .cloned()
                    .or_else(|| {
                        original_request
                            .as_ref()
                            .and_then(|request| request.get("input"))
                            .cloned()
                    })
                    .unwrap_or_else(|| json!({}));
                let body = if approve {
                    let mut response = serde_json::Map::new();
                    if let Some(updated_input) = object
                        .get("updated_input")
                        .or_else(|| object.get("updatedInput"))
                        .cloned()
                    {
                        response.insert("updated_input".to_string(), updated_input);
                    }
                    if let Some(permission_updates) = object
                        .get("permission_updates")
                        .or_else(|| object.get("permissionUpdates"))
                        .cloned()
                    {
                        response.insert("permission_updates".to_string(), permission_updates);
                    }
                    json!({
                        "type": "permission_response",
                        "request_id": id,
                        "requestId": id,
                        "subtype": "success",
                        "response": Value::Object(response),
                        "tool_name": tool_name,
                        "toolName": tool_name,
                        "input": request_input
                    })
                } else {
                    let reason = object_string(object, "reason")
                        .or_else(|| object_string(object, "feedback"))
                        .unwrap_or("Permission denied");
                    json!({
                        "type": "permission_response",
                        "request_id": id,
                        "requestId": id,
                        "subtype": "error",
                        "error": reason,
                        "tool_name": tool_name,
                        "toolName": tool_name,
                        "input": request_input
                    })
                };
                let content = serde_json::to_string(&body)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": "team-lead",
                        "to": target,
                        "recipients": [target],
                        "summary": "permission_response",
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                let resolved_request =
                    mark_protocol_request_read(context, "team-lead", &["permission_request"], id)?;
                remove_protocol_request_state(context, "mailbox_permission_requests", id);
                Ok(ToolOutput {
                    data: json!({
                        "success": true,
                        "message": if approve {
                            format!("Permission approved for {target}.")
                        } else {
                            format!("Permission rejected for {target}.")
                        },
                        "request_id": id,
                        "resolved_request": resolved_request,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            "sandbox_permission_response" => {
                if !sender.eq_ignore_ascii_case("team-lead") {
                    return Err(ToolError::ValidationError(
                        "Only the team lead can answer sandbox permission requests".to_string(),
                    ));
                }
                let id = object_string(object, "request_id")
                    .or_else(|| object_string(object, "requestId"))
                    .ok_or_else(|| {
                        ToolError::ValidationError(
                            "request_id is required for sandbox_permission_response".to_string(),
                        )
                    })?;
                let allow = object_bool(object, "allow")
                    .or_else(|| object_bool(object, "approve"))
                    .ok_or_else(|| {
                        ToolError::ValidationError(
                            "allow is required for sandbox_permission_response".to_string(),
                        )
                    })?;
                let original_request = context
                    .app_state
                    .get("sandbox_permission_requests")
                    .and_then(Value::as_object)
                    .and_then(|requests| requests.get(id))
                    .cloned();
                let host = object_string(object, "host")
                    .map(str::to_string)
                    .or_else(|| {
                        original_request
                            .as_ref()
                            .and_then(|request| request.get("host"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "*".to_string());
                let body = json!({
                    "type": "sandbox_permission_response",
                    "requestId": id,
                    "request_id": id,
                    "host": host,
                    "allow": allow,
                    "timestamp": timestamp
                });
                let content = serde_json::to_string(&body)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": "team-lead",
                        "to": target,
                        "recipients": [target],
                        "summary": "sandbox_permission_response",
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                let resolved_request = mark_protocol_request_read(
                    context,
                    "team-lead",
                    &["sandbox_permission_request"],
                    id,
                )?;
                remove_protocol_request_state(context, "sandbox_permission_requests", id);
                Ok(ToolOutput {
                    data: json!({
                            "success": true,
                            "message": if allow {
                                format!("Sandbox permission approved for {target}.")
                        } else {
                            format!("Sandbox permission rejected for {target}.")
                        },
                        "request_id": id,
                        "resolved_request": resolved_request,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            "team_permission_update" => {
                require_team_lead_sender(&sender, "send team permission updates")?;
                let behavior = permission_update_behavior(object)?;
                let rules = permission_update_rules(object)?;
                let tool_name = object_string(object, "tool_name")
                    .or_else(|| object_string(object, "toolName"))
                    .map(str::to_string)
                    .or_else(|| {
                        rules
                            .iter()
                            .find_map(|rule| {
                                rule.get("toolName")
                                    .or_else(|| rule.get("tool_name"))
                                    .and_then(Value::as_str)
                            })
                            .map(str::to_string)
                    });
                let directory_path = object_string(object, "directory_path")
                    .or_else(|| object_string(object, "directoryPath"))
                    .map(str::to_string);
                let permission_update = json!({
                    "type": "addRules",
                    "rules": rules,
                    "behavior": behavior,
                    "destination": "session"
                });
                let body = json!({
                    "type": "team_permission_update",
                    "from": "team-lead",
                    "permissionUpdate": permission_update.clone(),
                    "permission_update": permission_update,
                    "directoryPath": directory_path.clone(),
                    "directory_path": directory_path,
                    "toolName": tool_name.clone(),
                    "tool_name": tool_name,
                    "timestamp": timestamp
                });
                let content = serde_json::to_string(&body)?;
                let recipients = resolve_message_recipients(Some(target), context)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": "team-lead",
                        "to": target,
                        "recipients": recipients,
                        "summary": "team_permission_update",
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                let recipients = recipients_for_delivery(&message);
                Ok(ToolOutput {
                    data: json!({
                        "success": true,
                        "message": if target == "*" {
                            format!("Team permission update sent to {} teammate(s): {}", recipients.len(), recipients.join(", "))
                        } else {
                            format!("Team permission update sent to {target}.")
                        },
                        "recipients": recipients,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            "mode_set_request" => {
                require_team_lead_sender(&sender, "send mode set requests")?;
                let mode = object_string(object, "mode").ok_or_else(|| {
                    ToolError::ValidationError("mode is required for mode_set_request".to_string())
                })?;
                let body = json!({
                    "type": "mode_set_request",
                    "mode": mode,
                    "from": "team-lead",
                    "timestamp": timestamp
                });
                let content = serde_json::to_string(&body)?;
                let message = record_team_message(
                    context,
                    json!({
                        "id": Uuid::new_v4().to_string(),
                        "role": role,
                        "from": "team-lead",
                        "to": target,
                        "recipients": [target],
                        "summary": "mode_set_request",
                        "content": content,
                        "structured": body,
                        "timestamp": timestamp,
                        "created_at": now
                    }),
                )?;
                Ok(ToolOutput {
                    data: json!({
                        "success": true,
                        "message": format!("Mode set request sent to {target}."),
                        "target": target,
                        "message_record": message,
                        "mailboxes": mailboxes_json(context)
                    }),
                    metadata: None,
                })
            }
            _ => Err(ToolError::ValidationError(format!(
                "unsupported structured message type: {message_type}"
            ))),
        }
    }
}

pub struct ConfigTool;
impl ConfigTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "Config"
    }
    fn description(&self) -> &str {
        "Read or update session-local tool configuration"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string" },
                "value": {}
            }
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "config": { "type": "object" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ConfigInput = serde_json::from_value(input.clone())?;
        let mut config = context
            .app_state
            .get(CONFIG_KEY)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        if let Some(key) = trim_optional(input.key) {
            if let Some(value) = input.value {
                config.insert(key.clone(), value);
                context
                    .app_state
                    .insert(CONFIG_KEY.to_string(), Value::Object(config.clone()));
            }
            let value = config.get(&key).cloned().unwrap_or(Value::Null);
            return Ok(ToolOutput {
                data: json!({ "key": key, "value": value, "config": config }),
                metadata: None,
            });
        }

        Ok(ToolOutput {
            data: json!({ "config": config }),
            metadata: None,
        })
    }
}

pub struct WorkflowTool;
impl WorkflowTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WorkflowTool {
    fn name(&self) -> &str {
        "Workflow"
    }
    fn description(&self) -> &str {
        "Record a workflow run and its planned steps"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "steps": { "type": "array" }
            },
            "required": ["name"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "workflow": { "type": "object" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: WorkflowInput = serde_json::from_value(input.clone())?;
        let name = require_non_empty(&input.name, "name")?;
        let mut runs = load_array(context, WORKFLOW_RUNS_KEY)?;
        let workflow = json!({
            "id": Uuid::new_v4().to_string(),
            "name": name,
            "steps": input.steps,
            "status": "recorded",
            "created_at": now_unix_seconds()
        });
        runs.push(workflow.clone());
        save_array(context, WORKFLOW_RUNS_KEY, runs);
        Ok(ToolOutput {
            data: json!({ "workflow": workflow }),
            metadata: None,
        })
    }
}

pub struct EnterPlanModeTool;
impl EnterPlanModeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }
    fn description(&self) -> &str {
        "Enter planning mode for the current tool context"
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "mode": { "type": "string" } } })
    }
    async fn call(&self, _input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        context
            .app_state
            .insert("mode".to_string(), Value::String("plan".to_string()));
        Ok(ToolOutput {
            data: json!({ "mode": "plan" }),
            metadata: None,
        })
    }
}

pub struct ExitPlanModeTool;
impl ExitPlanModeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }
    fn description(&self) -> &str {
        "Exit planning mode for the current tool context"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "Plan text to submit when this teammate requires leader approval"
                },
                "planFilePath": {
                    "type": "string",
                    "description": "Optional path to the plan file"
                }
            }
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string" },
                "awaitingLeaderApproval": { "type": "boolean" },
                "requestId": { "type": "string" }
            }
        })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ExitPlanModeInput = serde_json::from_value(input.clone())?;
        let sender = sender_name(context);
        if !sender.eq_ignore_ascii_case("team-lead")
            && current_teammate_plan_mode_required(context, &sender)
        {
            let team_name = active_team_name(context).ok_or_else(|| {
                ToolError::ValidationError(
                    "plan approval requires an active team context".to_string(),
                )
            })?;
            let plan = trim_optional(input.plan).ok_or_else(|| {
                ToolError::ValidationError(
                    "plan is required when this teammate requires leader approval".to_string(),
                )
            })?;
            let request_id = request_id("plan-approval", &sender);
            let now = now_unix_seconds();
            let timestamp = now.to_string();
            let plan_file_path = trim_optional(input.plan_file_path);
            let body = json!({
                "type": "plan_approval_request",
                "from": sender,
                "timestamp": timestamp,
                "planFilePath": plan_file_path,
                "plan_file_path": plan_file_path,
                "planContent": plan,
                "plan_content": plan,
                "requestId": request_id,
                "request_id": request_id
            });
            let content = serde_json::to_string(&body)?;
            let message = record_team_message(
                context,
                json!({
                    "id": Uuid::new_v4().to_string(),
                    "role": "assistant",
                    "from": sender,
                    "to": "team-lead",
                    "recipients": ["team-lead"],
                    "summary": "plan_approval_request",
                    "content": content,
                    "structured": body,
                    "timestamp": timestamp,
                    "created_at": now
                }),
            )?;
            context
                .app_state
                .insert("mode".to_string(), Value::String("plan".to_string()));
            return Ok(ToolOutput {
                data: json!({
                    "mode": "plan",
                    "awaitingLeaderApproval": true,
                    "awaiting_leader_approval": true,
                    "requestId": request_id,
                    "request_id": request_id,
                    "teamName": team_name,
                    "team_name": team_name,
                    "message_record": message
                }),
                metadata: None,
            });
        }

        context
            .app_state
            .insert("mode".to_string(), Value::String("default".to_string()));
        Ok(ToolOutput {
            data: json!({ "mode": "default" }),
            metadata: None,
        })
    }
}

pub struct EnterWorktreeTool;
impl EnterWorktreeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "EnterWorktree"
    }
    fn description(&self) -> &str {
        "Switch the tool context into a worktree directory"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "cwd": { "type": "string" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: WorktreeInput = serde_json::from_value(input.clone())?;
        let target = trim_optional(input.path).unwrap_or_else(|| context.cwd.clone());
        let mut path = PathBuf::from(&target);
        if path.is_relative() {
            path = PathBuf::from(&context.cwd).join(path);
        }
        if !path.is_dir() {
            return Err(ToolError::Other(format!(
                "worktree directory does not exist: {}",
                path.display()
            )));
        }
        context.app_state.insert(
            "previous_cwd".to_string(),
            Value::String(context.cwd.clone()),
        );
        context.cwd = path.to_string_lossy().to_string();
        Ok(ToolOutput {
            data: json!({ "cwd": context.cwd }),
            metadata: None,
        })
    }
}

pub struct ExitWorktreeTool;
impl ExitWorktreeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "ExitWorktree"
    }
    fn description(&self) -> &str {
        "Restore the previous tool context directory"
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "cwd": { "type": "string" } } })
    }
    async fn call(&self, _input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        if let Some(previous) = context
            .app_state
            .remove("previous_cwd")
            .and_then(|value| value.as_str().map(str::to_string))
        {
            context.cwd = previous;
        }
        Ok(ToolOutput {
            data: json!({ "cwd": context.cwd }),
            metadata: None,
        })
    }
}

pub struct SkillTool;
impl SkillTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }
    fn description(&self) -> &str {
        "Load and invoke an available project or user skill"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": { "type": "string" },
                "args": {}
            },
            "required": ["skill"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "commandName": { "type": "string" },
                "skill": { "type": "object" },
                "invocation": { "type": "object" }
            }
        })
    }
    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: SkillInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        match resolve_invocable_skill(&input.skill, &context.cwd, &context.app_state).await {
            Ok(_) => ValidationResult::ok(),
            Err(message) => ValidationResult::err(message, 2),
        }
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: SkillInput = serde_json::from_value(input.clone())?;
        let command = resolve_invocable_skill(&input.skill, &context.cwd, &context.app_state)
            .await
            .map_err(ToolError::ValidationError)?;
        let path = skill_path(&command);
        let args = input.args.unwrap_or(Value::Null);

        let mut invocations = load_array(context, SKILL_INVOCATIONS_KEY)?;
        let invocation = json!({
            "id": Uuid::new_v4().to_string(),
            "skill": command.name.clone(),
            "args": args,
            "created_at": now_unix_seconds()
        });
        invocations.push(invocation.clone());
        save_array(context, SKILL_INVOCATIONS_KEY, invocations);

        let mut invoked_skills = load_array(context, INVOKED_SKILLS_KEY)?;
        invoked_skills.push(json!({
            "name": command.name.clone(),
            "path": path.clone(),
            "content": command.content.clone()
        }));
        save_array(context, INVOKED_SKILLS_KEY, invoked_skills);

        let skill = skill_command_to_json(&command, &path);
        Ok(ToolOutput {
            data: json!({
                "success": true,
                "commandName": command.name,
                "skill": skill,
                "invocation": invocation
            }),
            metadata: None,
        })
    }
}

pub struct ToolSearchTool;
impl ToolSearchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }
    fn description(&self) -> &str {
        "Search known local tools"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use * to list all registered tools."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100
                },
                "include_schema": {
                    "type": "boolean",
                    "description": "Include input_schema for each matched tool"
                }
            },
            "required": ["query"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tools": { "type": "array" },
                "count": { "type": "integer" },
                "total_registered": { "type": "integer" }
            }
        })
    }
    async fn call(&self, input: &Value, _context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: ToolSearchInput = serde_json::from_value(input.clone())?;
        let query = require_non_empty(&input.query, "query")?.to_lowercase();
        let max_results = input.max_results.unwrap_or(20).clamp(1, 100);
        let registry = crate::registry::create_default_registry();
        let registered = registry.list_tools();
        let total_registered = registered.len();
        let mut matches = registered
            .into_iter()
            .filter_map(|tool| {
                tool_search_score(tool.as_ref(), &query).map(|score| {
                    let mut result = json!({
                        "name": tool.name(),
                        "description": tool.description(),
                        "search_hint": tool.search_hint(),
                        "read_only": tool.is_read_only(),
                        "concurrency_safe": tool.is_concurrency_safe(),
                        "score": score
                    });
                    if input.include_schema {
                        result["input_schema"] = tool.input_schema();
                    }
                    result
                })
            })
            .collect::<Vec<_>>();

        matches.sort_by(|left, right| {
            let left_score = left
                .get("score")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let right_score = right
                .get("score")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            right_score.cmp(&left_score).then_with(|| {
                left.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .cmp(
                        right
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
            })
        });
        matches.truncate(max_results);
        let count = matches.len();

        Ok(ToolOutput {
            data: json!({
                "tools": matches,
                "count": count,
                "total_registered": total_registered
            }),
            metadata: None,
        })
    }
}

fn tool_search_score(tool: &dyn Tool, query: &str) -> Option<i64> {
    let query = query.trim();
    if query == "*" || query == "all" {
        return Some(1);
    }

    let name = tool.name().to_ascii_lowercase();
    let description = tool.description().to_ascii_lowercase();
    let search_hint = tool.search_hint().unwrap_or_default().to_ascii_lowercase();
    let haystack = format!("{name}\n{description}\n{search_hint}");
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    if terms.is_empty() || !terms.iter().all(|term| haystack.contains(term)) {
        return None;
    }

    let mut score = 0;
    for term in terms {
        if name == term {
            score += 100;
        } else if name.starts_with(term) {
            score += 80;
        } else if name.contains(term) {
            score += 60;
        } else if search_hint.contains(term) {
            score += 35;
        } else if description.contains(term) {
            score += 25;
        }
    }
    Some(score)
}

pub struct CronCreateTool;
impl CronCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CronCreateTool {
    fn name(&self) -> &str {
        "CronCreate"
    }
    fn description(&self) -> &str {
        "Create a session-local scheduled prompt"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cron": { "type": "string" },
                "prompt": { "type": "string" },
                "recurring": { "type": "boolean" }
            },
            "required": ["cron", "prompt"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "cron": { "type": "object" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: CronCreateInput = serde_json::from_value(input.clone())?;
        let cron = require_non_empty(&input.cron, "cron")?;
        let prompt = require_non_empty(&input.prompt, "prompt")?;
        let mut jobs = load_array(context, CRON_JOBS_KEY)?;
        let job = json!({
            "id": Uuid::new_v4().to_string(),
            "cron": cron,
            "prompt": prompt,
            "recurring": input.recurring.unwrap_or(true),
            "created_at": now_unix_seconds()
        });
        jobs.push(job.clone());
        save_array(context, CRON_JOBS_KEY, jobs);
        Ok(ToolOutput {
            data: json!({ "cron": job }),
            metadata: None,
        })
    }
}

pub struct CronDeleteTool;
impl CronDeleteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CronDeleteTool {
    fn name(&self) -> &str {
        "CronDelete"
    }
    fn description(&self) -> &str {
        "Delete a session-local scheduled prompt"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "deleted": { "type": "array" } } })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: CronDeleteInput = serde_json::from_value(input.clone())?;
        let id = require_non_empty(&input.id, "id")?;
        let mut jobs = load_array(context, CRON_JOBS_KEY)?;
        let mut deleted = Vec::new();
        jobs.retain(|job| {
            if job.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                deleted.push(job.clone());
                false
            } else {
                true
            }
        });
        if deleted.is_empty() {
            return Err(ToolError::Other(format!("cron job '{}' not found", id)));
        }
        save_array(context, CRON_JOBS_KEY, jobs);
        Ok(ToolOutput {
            data: json!({ "deleted": deleted }),
            metadata: None,
        })
    }
}

pub struct CronListTool;
impl CronListTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str {
        "CronList"
    }
    fn description(&self) -> &str {
        "List session-local scheduled prompts"
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn output_schema(&self) -> Value {
        json!({ "type": "object", "properties": { "crons": { "type": "array" } } })
    }
    async fn call(&self, _input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let jobs = load_array(context, CRON_JOBS_KEY)?;
        Ok(ToolOutput {
            data: json!({ "crons": jobs }),
            metadata: None,
        })
    }
}

pub struct MonitorTool;
impl MonitorTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for MonitorTool {
    fn name(&self) -> &str {
        "Monitor"
    }
    fn description(&self) -> &str {
        "Return a snapshot of local processes"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "minimum": 1 }
            }
        })
    }
    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "processes": { "type": "array" },
                "teamAgentStatuses": { "type": ["object", "null"] },
                "team_agent_statuses": { "type": ["object", "null"] },
                "pendingProtocolRequests": { "type": ["object", "null"] },
                "pending_protocol_requests": { "type": ["object", "null"] }
            }
        })
    }
    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: MonitorInput =
            serde_json::from_value(input.clone()).unwrap_or(MonitorInput { limit: Some(20) });
        let limit = input.limit.unwrap_or(20).clamp(1, 100);
        let output = Command::new("ps")
            .args(["-eo", "pid=,comm=,etime="])
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let processes = stdout
            .lines()
            .take(limit)
            .map(|line| {
                let mut parts = line.split_whitespace();
                let pid = parts.next().unwrap_or_default();
                let command = parts.next().unwrap_or_default();
                let elapsed = parts.next().unwrap_or_default();
                json!({
                    "pid": pid,
                    "command": command,
                    "elapsed": elapsed
                })
            })
            .collect::<Vec<_>>();
        let team_agent_statuses = team_agent_statuses(context)?.unwrap_or(Value::Null);
        let pending_protocol_requests = pending_protocol_requests(context)?.unwrap_or(Value::Null);
        Ok(ToolOutput {
            data: json!({
                "processes": processes,
                "teamAgentStatuses": team_agent_statuses.clone(),
                "team_agent_statuses": team_agent_statuses,
                "pendingProtocolRequests": pending_protocol_requests.clone(),
                "pending_protocol_requests": pending_protocol_requests
            }),
            metadata: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cleanup_session_teams_for_roots, ConfigTool, CronCreateTool, CronDeleteTool, CronListTool,
        EnterPlanModeTool, EnterWorktreeTool, ExitPlanModeTool, ExitWorktreeTool, MonitorTool,
        SendMessageTool, SkillTool, TeamCreateTool, TeamDeleteTool, ToolSearchTool,
    };
    use crate::task_create::TaskCreateTool;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::sync::OnceLock;
    use uuid::Uuid;

    fn test_context() -> ToolContext {
        test_context_with_cwd(
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        )
    }

    fn test_context_with_cwd(cwd: String) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let root = std::env::temp_dir().join(format!("kiana-team-test-{}", Uuid::new_v4()));
        let teams_root = root.join("teams");
        let tasks_root = root.join("tasks");
        ToolContext {
            cwd,
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                (
                    "teams_root".to_string(),
                    json!(teams_root.to_string_lossy().to_string()),
                ),
                (
                    "tasks_root".to_string(),
                    json!(tasks_root.to_string_lossy().to_string()),
                ),
            ]),
            abort_signal: abort_rx,
        }
    }

    fn skill_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn read_json(path: impl AsRef<Path>) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn session_tools_mutate_shared_context_state() {
        let mut context = test_context();

        let created = TeamCreateTool::new()
            .call(
                &json!({ "name": "review", "members": ["alice", "bob"] }),
                &mut context,
            )
            .await
            .unwrap();
        let team_id = created.data["team"]["id"].as_str().unwrap().to_string();
        let team_file_path = created.data["team_file_path"].as_str().unwrap();
        assert_eq!(context.app_state["teams"][0]["name"], "review");
        assert_eq!(
            context.app_state["team_context"]["team_name"],
            json!("review")
        );
        assert_eq!(
            context.app_state["team_context"]["teammates"]["alice@review"]["name"],
            json!("alice")
        );
        let team_file = read_json(team_file_path);
        assert_eq!(team_file["name"], "review");
        assert_eq!(team_file["leadAgentId"], "team-lead@review");
        assert_eq!(team_file["members"].as_array().unwrap().len(), 3);

        SendMessageTool::new()
            .call(&json!({ "message": "hello", "to": "review" }), &mut context)
            .await
            .unwrap();
        assert_eq!(context.app_state["messages"][0]["content"], "hello");
        assert_eq!(
            context.app_state["team_mailboxes"]["alice"][0]["content"],
            "hello"
        );
        assert_eq!(
            context.app_state["team_mailboxes"]["bob"][0]["content"],
            "hello"
        );
        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let alice_inbox = read_json(Path::new(teams_root).join("review/inboxes/alice.json"));
        assert_eq!(alice_inbox[0]["text"], "hello");
        assert_eq!(alice_inbox[0]["read"], false);

        ConfigTool::new()
            .call(
                &json!({ "key": "approval_mode", "value": "manual" }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(context.app_state["config"]["approval_mode"], "manual");

        EnterPlanModeTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        assert_eq!(context.app_state["mode"], "plan");
        ExitPlanModeTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        assert_eq!(context.app_state["mode"], "default");

        let deleted = TeamDeleteTool::new()
            .call(&json!({ "team_id": team_id }), &mut context)
            .await
            .unwrap();
        assert_eq!(deleted.data["deleted"].as_array().unwrap().len(), 1);
        assert!(context.app_state.get("team_context").is_none());
        assert!(context.app_state.get("team_mailboxes").is_none());
    }

    #[tokio::test]
    async fn team_lifecycle_resets_and_deletes_team_task_list() {
        let mut context = test_context();
        let tasks_root = context.app_state["tasks_root"]
            .as_str()
            .unwrap()
            .to_string();
        let teams_root = context.app_state["teams_root"]
            .as_str()
            .unwrap()
            .to_string();
        let stale_task_dir = Path::new(&tasks_root).join("review");
        fs::create_dir_all(&stale_task_dir).unwrap();
        fs::write(
            stale_task_dir.join("7.json"),
            serde_json::to_string_pretty(&json!({
                "id": "7",
                "title": "stale task",
                "status": "pending",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1
            }))
            .unwrap(),
        )
        .unwrap();
        context.app_state.insert(
            "tasks".to_string(),
            json!([{
                "id": "99",
                "title": "old session cache",
                "status": "pending",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1
            }]),
        );

        TeamCreateTool::new()
            .call(&json!({ "name": "review" }), &mut context)
            .await
            .unwrap();

        assert_eq!(context.app_state["task_list_id"], "review");
        assert_eq!(
            context.app_state["team_context"]["task_list_id"],
            json!("review")
        );
        assert!(context.app_state.get("tasks").is_none());
        assert!(!stale_task_dir.join("7.json").exists());
        assert_eq!(
            fs::read_to_string(stale_task_dir.join(".highwatermark"))
                .unwrap()
                .trim(),
            "7"
        );

        let created_task = TaskCreateTool::new()
            .call(&json!({ "title": "fresh team task" }), &mut context)
            .await
            .unwrap();
        assert_eq!(created_task.data["task_id"], "8");
        assert_eq!(created_task.data["task"]["title"], "fresh team task");
        let tasks = context.app_state["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], "8");

        let deleted = TeamDeleteTool::new()
            .call(&json!({ "name": "review" }), &mut context)
            .await
            .unwrap();
        assert_eq!(deleted.data["success"], true);
        assert!(!Path::new(&teams_root).join("review").exists());
        assert!(!stale_task_dir.exists());
        assert!(context.app_state.get("team_context").is_none());
        assert!(context.app_state.get("tasks").is_none());
        assert!(context.app_state.get("task_list_id").is_none());
    }

    #[tokio::test]
    async fn session_cleanup_removes_registered_team_task_directories() {
        let mut context = test_context();
        let tasks_root = context.app_state["tasks_root"]
            .as_str()
            .unwrap()
            .to_string();
        let teams_root = context.app_state["teams_root"]
            .as_str()
            .unwrap()
            .to_string();

        TeamCreateTool::new()
            .call(&json!({ "name": "cleanup-review" }), &mut context)
            .await
            .unwrap();
        TaskCreateTool::new()
            .call(&json!({ "title": "left behind" }), &mut context)
            .await
            .unwrap();

        let team_dir = Path::new(&teams_root).join("cleanup-review");
        let task_dir = Path::new(&tasks_root).join("cleanup-review");
        assert!(team_dir.exists());
        assert!(task_dir.exists());

        let cleaned =
            cleanup_session_teams_for_roots(Path::new(&teams_root), Path::new(&tasks_root))
                .unwrap();
        assert_eq!(cleaned, 1);
        assert!(!team_dir.exists());
        assert!(!task_dir.exists());

        let cleaned_again =
            cleanup_session_teams_for_roots(Path::new(&teams_root), Path::new(&tasks_root))
                .unwrap();
        assert_eq!(cleaned_again, 0);
    }

    #[tokio::test]
    async fn exit_plan_mode_requests_leader_approval_for_plan_required_teammate() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "planning", "members": ["researcher"] }),
                &mut context,
            )
            .await
            .unwrap();
        context
            .app_state
            .insert("agent_name".to_string(), json!("researcher"));
        context
            .app_state
            .insert("agent_id".to_string(), json!("researcher@planning"));
        let teammate = context
            .app_state
            .get_mut("team_context")
            .and_then(|context| context.get_mut("teammates"))
            .and_then(|teammates| teammates.get_mut("researcher@planning"))
            .unwrap();
        teammate["planModeRequired"] = json!(true);
        teammate["plan_mode_required"] = json!(true);

        EnterPlanModeTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        let exited = ExitPlanModeTool::new()
            .call(
                &json!({
                    "plan": "1. Inspect parser\n2. Patch edge cases\n3. Add tests",
                    "planFilePath": ".claude/plans/researcher.md"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(exited.data["mode"], "plan");
        assert_eq!(exited.data["awaitingLeaderApproval"], true);
        assert!(exited.data["requestId"]
            .as_str()
            .unwrap()
            .starts_with("plan-approval-researcher-"));
        assert_eq!(context.app_state["mode"], "plan");

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let leader_inbox = read_json(Path::new(teams_root).join("planning/inboxes/team-lead.json"));
        assert_eq!(leader_inbox.as_array().unwrap().len(), 1);
        let body: Value = serde_json::from_str(leader_inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["type"], "plan_approval_request");
        assert_eq!(body["from"], "researcher");
        assert_eq!(body["planFilePath"], ".claude/plans/researcher.md");
        assert_eq!(
            body["planContent"],
            "1. Inspect parser\n2. Patch edge cases\n3. Add tests"
        );
        assert_eq!(leader_inbox[0]["summary"], "plan_approval_request");
        assert_eq!(
            context.app_state["team_mailboxes"]["team-lead"][0]["summary"],
            "plan_approval_request"
        );
    }

    #[tokio::test]
    async fn send_message_structured_messages_are_written_to_file_inboxes() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "planning", "members": ["researcher"] }),
                &mut context,
            )
            .await
            .unwrap();

        let shutdown = SendMessageTool::new()
            .call(
                &json!({
                    "to": "researcher",
                    "message": {
                        "type": "shutdown_request",
                        "reason": "slice complete"
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        let request_id = shutdown.data["request_id"].as_str().unwrap();
        assert!(request_id.starts_with("shutdown-researcher-"));

        SendMessageTool::new()
            .call(
                &json!({
                    "to": "researcher",
                    "message": {
                        "type": "plan_approval_response",
                        "request_id": "plan-1",
                        "approve": false,
                        "feedback": "add error handling"
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();

        context.app_state.insert(
            "mailbox_permission_requests".to_string(),
            json!({
                "perm-1": {
                    "request_id": "perm-1",
                    "from": "researcher",
                    "tool_name": "Bash",
                    "input": { "command": "git status" }
                }
            }),
        );
        SendMessageTool::new()
            .call(
                &json!({
                    "to": "researcher",
                    "message": {
                        "type": "permission_response",
                        "request_id": "perm-1",
                        "approve": true
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();

        context.app_state.insert(
            "sandbox_permission_requests".to_string(),
            json!({
                "sandbox-1": {
                    "requestId": "sandbox-1",
                    "host": "*",
                    "workerName": "researcher"
                }
            }),
        );
        SendMessageTool::new()
            .call(
                &json!({
                    "to": "researcher",
                    "message": {
                        "type": "sandbox_permission_response",
                        "request_id": "sandbox-1",
                        "allow": true
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let inbox = read_json(Path::new(teams_root).join("planning/inboxes/researcher.json"));
        assert_eq!(inbox.as_array().unwrap().len(), 4);
        let shutdown_body: Value =
            serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(shutdown_body["type"], "shutdown_request");
        assert_eq!(shutdown_body["reason"], "slice complete");
        let plan_body: Value = serde_json::from_str(inbox[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(plan_body["type"], "plan_approval_response");
        assert_eq!(plan_body["approved"], false);
        assert_eq!(plan_body["feedback"], "add error handling");
        let permission_body: Value =
            serde_json::from_str(inbox[2]["text"].as_str().unwrap()).unwrap();
        assert_eq!(permission_body["type"], "permission_response");
        assert_eq!(permission_body["subtype"], "success");
        assert_eq!(permission_body["request_id"], "perm-1");
        assert_eq!(permission_body["tool_name"], "Bash");
        assert_eq!(permission_body["input"]["command"], "git status");
        let sandbox_body: Value = serde_json::from_str(inbox[3]["text"].as_str().unwrap()).unwrap();
        assert_eq!(sandbox_body["type"], "sandbox_permission_response");
        assert_eq!(sandbox_body["requestId"], "sandbox-1");
        assert_eq!(sandbox_body["host"], "*");
        assert_eq!(sandbox_body["allow"], true);
    }

    #[tokio::test]
    async fn send_message_sends_control_plane_state_updates_to_teammates() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "review", "members": ["alice", "bob"] }),
                &mut context,
            )
            .await
            .unwrap();

        let mode = SendMessageTool::new()
            .call(
                &json!({
                    "to": "alice",
                    "message": {
                        "type": "mode_set_request",
                        "mode": "ask"
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(mode.data["success"], true);

        let permission_update = SendMessageTool::new()
            .call(
                &json!({
                    "to": "*",
                    "message": {
                        "type": "team_permission_update",
                        "behavior": "allow",
                        "tool_name": "Bash",
                        "rule_content": "git:*",
                        "directory_path": "/repo"
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(permission_update.data["success"], true);
        assert_eq!(
            permission_update.data["recipients"],
            json!(["alice", "bob"])
        );

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let alice_inbox = read_json(Path::new(teams_root).join("review/inboxes/alice.json"));
        let bob_inbox = read_json(Path::new(teams_root).join("review/inboxes/bob.json"));
        assert_eq!(alice_inbox.as_array().unwrap().len(), 2);
        assert_eq!(bob_inbox.as_array().unwrap().len(), 1);

        let mode_body: Value =
            serde_json::from_str(alice_inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(mode_body["type"], "mode_set_request");
        assert_eq!(mode_body["from"], "team-lead");
        assert_eq!(mode_body["mode"], "ask");
        assert_eq!(alice_inbox[0]["summary"], "mode_set_request");

        let alice_update: Value =
            serde_json::from_str(alice_inbox[1]["text"].as_str().unwrap()).unwrap();
        let bob_update: Value =
            serde_json::from_str(bob_inbox[0]["text"].as_str().unwrap()).unwrap();
        for update in [&alice_update, &bob_update] {
            assert_eq!(update["type"], "team_permission_update");
            assert_eq!(update["from"], "team-lead");
            assert_eq!(update["permissionUpdate"]["behavior"], "allow");
            assert_eq!(update["permissionUpdate"]["destination"], "session");
            assert_eq!(update["permissionUpdate"]["rules"][0]["toolName"], "Bash");
            assert_eq!(
                update["permissionUpdate"]["rules"][0]["ruleContent"],
                "git:*"
            );
            assert_eq!(update["directoryPath"], "/repo");
            assert_eq!(update["toolName"], "Bash");
        }

        context
            .app_state
            .insert("agent_name".to_string(), json!("alice"));
        let denied = SendMessageTool::new()
            .call(
                &json!({
                    "to": "bob",
                    "message": {
                        "type": "mode_set_request",
                        "mode": "default"
                    }
                }),
                &mut context,
            )
            .await;
        assert!(denied
            .unwrap_err()
            .to_string()
            .contains("Only the team lead"));
    }

    #[tokio::test]
    async fn shutdown_response_approval_marks_current_teammate_for_exit() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "planning", "members": ["researcher"] }),
                &mut context,
            )
            .await
            .unwrap();
        context
            .app_state
            .insert("agent_name".to_string(), json!("researcher"));

        let researcher_inbox_path = super::inbox_path(&context, "planning", "researcher");
        fs::create_dir_all(researcher_inbox_path.parent().unwrap()).unwrap();
        fs::write(
            &researcher_inbox_path,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"shutdown_request\",\"requestId\":\"shutdown-1\",\"from\":\"team-lead\",\"reason\":\"done\"}",
                    "summary": "shutdown_request",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();

        let response = SendMessageTool::new()
            .call(
                &json!({
                    "to": "team-lead",
                    "message": {
                        "type": "shutdown_response",
                        "request_id": "shutdown-1",
                        "approve": true
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(response.data["resolved_request"], true);
        assert_eq!(context.app_state["teammate_shutdown_approved"], true);
        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let inbox = read_json(Path::new(teams_root).join("planning/inboxes/team-lead.json"));
        let body: Value = serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["type"], "shutdown_approved");
        assert_eq!(body["from"], "researcher");
        let researcher_inbox = read_json(&researcher_inbox_path);
        assert_eq!(researcher_inbox[0]["read"], true);
    }

    #[tokio::test]
    async fn monitor_reports_team_agent_status_from_task_ownership() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "review", "members": ["alice", "bob"] }),
                &mut context,
            )
            .await
            .unwrap();

        let tasks_root = context.app_state["tasks_root"].as_str().unwrap();
        let task_dir = Path::new(tasks_root).join("review");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(
            task_dir.join("1.json"),
            serde_json::to_string_pretty(&json!({
                "id": "1",
                "title": "Audit parser",
                "subject": "Audit parser",
                "owner": "alice",
                "status": "in_progress",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            task_dir.join("2.json"),
            serde_json::to_string_pretty(&json!({
                "id": "2",
                "title": "Old task",
                "subject": "Old task",
                "owner": "bob",
                "status": "completed",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1
            }))
            .unwrap(),
        )
        .unwrap();

        let monitored = MonitorTool::new()
            .call(&json!({ "limit": 1 }), &mut context)
            .await
            .unwrap();
        let agents = monitored.data["teamAgentStatuses"]["agents"]
            .as_array()
            .unwrap();
        let alice = agents
            .iter()
            .find(|agent| agent["name"] == "alice")
            .unwrap();
        let bob = agents.iter().find(|agent| agent["name"] == "bob").unwrap();
        assert_eq!(alice["status"], "busy");
        assert_eq!(alice["currentTasks"], json!(["1"]));
        assert_eq!(bob["status"], "idle");
        assert_eq!(monitored.data["team_agent_statuses"]["team_name"], "review");
    }

    #[tokio::test]
    async fn monitor_reports_resident_runtime_status_from_team_file() {
        let mut context = test_context();
        let created = TeamCreateTool::new()
            .call(
                &json!({ "team_name": "resident-review", "members": ["runner"] }),
                &mut context,
            )
            .await
            .unwrap();
        let team_file_path = created.data["team_file_path"].as_str().unwrap();
        let mut team_file = read_json(team_file_path);
        let runner = team_file["members"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|member| member["name"] == "runner")
            .unwrap();
        runner["isActive"] = json!(true);
        runner["is_active"] = json!(true);
        runner["lifecycleStatus"] = json!("running");
        runner["lifecycle_status"] = json!("running");
        fs::write(
            team_file_path,
            serde_json::to_string_pretty(&team_file).unwrap(),
        )
        .unwrap();

        let monitored = MonitorTool::new()
            .call(&json!({ "limit": 1 }), &mut context)
            .await
            .unwrap();
        let runner = monitored.data["teamAgentStatuses"]["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["name"] == "runner")
            .unwrap();
        assert_eq!(runner["status"], "running");
        assert_eq!(runner["lifecycleStatus"], "running");
        assert_eq!(runner["isActive"], true);

        let mut team_file = read_json(team_file_path);
        let runner = team_file["members"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|member| member["name"] == "runner")
            .unwrap();
        runner["lifecycleStatus"] = json!("idle");
        runner["lifecycle_status"] = json!("idle");
        fs::write(
            team_file_path,
            serde_json::to_string_pretty(&team_file).unwrap(),
        )
        .unwrap();

        let monitored = MonitorTool::new()
            .call(&json!({ "limit": 1 }), &mut context)
            .await
            .unwrap();
        let runner = monitored.data["teamAgentStatuses"]["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["name"] == "runner")
            .unwrap();
        assert_eq!(runner["status"], "idle");
        assert_eq!(runner["lifecycleStatus"], "idle");
    }

    #[tokio::test]
    async fn monitor_reports_pending_protocol_requests_from_lead_inbox() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "review", "members": ["alice", "bob"] }),
                &mut context,
            )
            .await
            .unwrap();

        let inbox_path = super::inbox_path(&context, "review", "team-lead");
        fs::create_dir_all(inbox_path.parent().unwrap()).unwrap();
        fs::write(
            &inbox_path,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "alice",
                    "text": "{\"type\":\"permission_request\",\"request_id\":\"perm-1\",\"agent_id\":\"alice@review\",\"tool_name\":\"Bash\",\"tool_use_id\":\"toolu_1\",\"description\":\"needs approval\",\"input\":{\"command\":\"git status\"},\"permission_suggestions\":[]}",
                    "summary": "permission_request",
                    "timestamp": "1",
                    "read": false
                },
                {
                    "from": "bob",
                    "text": "{\"type\":\"sandbox_permission_request\",\"requestId\":\"sandbox-1\",\"workerId\":\"bob@review\",\"workerName\":\"bob\",\"workerColor\":\"green\",\"hostPattern\":{\"host\":\"*\"},\"createdAt\":123}",
                    "summary": "sandbox_permission_request",
                    "timestamp": "2",
                    "read": false
                },
                {
                    "from": "alice",
                    "text": "{\"type\":\"plan_approval_request\",\"requestId\":\"plan-1\",\"from\":\"alice\",\"planFilePath\":\".kiana/plans/alice.md\",\"planContent\":\"1. Inspect\\n2. Implement\",\"timestamp\":\"3\"}",
                    "summary": "plan_approval_request",
                    "timestamp": "3",
                    "read": false
                },
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"shutdown_request\",\"requestId\":\"shutdown-1\",\"from\":\"team-lead\",\"reason\":\"done\",\"timestamp\":\"4\"}",
                    "summary": "shutdown_request",
                    "timestamp": "4",
                    "read": false
                },
                {
                    "from": "alice",
                    "text": "{\"type\":\"permission_request\",\"request_id\":\"perm-read\",\"tool_name\":\"Bash\"}",
                    "summary": "permission_request",
                    "timestamp": "5",
                    "read": true
                },
                {
                    "from": "alice",
                    "text": "plain status update",
                    "summary": "status",
                    "timestamp": "6",
                    "read": false
                },
                {
                    "from": "alice",
                    "text": "{\"type\":\"permission_response\",\"request_id\":\"perm-1\",\"subtype\":\"success\"}",
                    "summary": "permission_response",
                    "timestamp": "7",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();

        let monitored = MonitorTool::new()
            .call(&json!({ "limit": 1 }), &mut context)
            .await
            .unwrap();
        let pending = &monitored.data["pendingProtocolRequests"];

        assert_eq!(pending["teamName"], "review");
        assert_eq!(pending["total"], 4);
        assert_eq!(pending["counts"]["permissionRequests"], 1);
        assert_eq!(pending["counts"]["sandbox_permission_requests"], 1);
        assert_eq!(pending["counts"]["planApprovalRequests"], 1);
        assert_eq!(pending["counts"]["shutdown_requests"], 1);
        assert_eq!(
            monitored.data["pending_protocol_requests"]["counts"]["total"],
            4
        );

        let permission = pending["permissionRequests"].as_array().unwrap();
        assert_eq!(permission[0]["requestId"], "perm-1");
        assert_eq!(permission[0]["from"], "alice");
        assert_eq!(permission[0]["toolName"], "Bash");
        assert_eq!(permission[0]["input"]["command"], "git status");

        let sandbox = pending["sandboxPermissionRequests"].as_array().unwrap();
        assert_eq!(sandbox[0]["request_id"], "sandbox-1");
        assert_eq!(sandbox[0]["workerName"], "bob");
        assert_eq!(sandbox[0]["host"], "*");

        let plan = pending["planApprovalRequests"].as_array().unwrap();
        assert_eq!(plan[0]["requestId"], "plan-1");
        assert_eq!(plan[0]["planFilePath"], ".kiana/plans/alice.md");

        let shutdown = pending["shutdownRequests"].as_array().unwrap();
        assert_eq!(shutdown[0]["request_id"], "shutdown-1");
        assert_eq!(shutdown[0]["reason"], "done");

        let inbox_after = read_json(&inbox_path);
        assert_eq!(inbox_after[0]["read"], false);
    }

    #[tokio::test]
    async fn send_message_responses_resolve_leader_pending_protocol_requests() {
        let mut context = test_context();
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "review", "members": ["alice", "bob"] }),
                &mut context,
            )
            .await
            .unwrap();

        let inbox_path = super::inbox_path(&context, "review", "team-lead");
        fs::create_dir_all(inbox_path.parent().unwrap()).unwrap();
        fs::write(
            &inbox_path,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "alice",
                    "text": "{\"type\":\"permission_request\",\"request_id\":\"perm-1\",\"from\":\"alice\",\"tool_name\":\"Bash\",\"input\":{\"command\":\"cargo test\"}}",
                    "summary": "permission_request",
                    "timestamp": "1",
                    "read": false
                },
                {
                    "from": "bob",
                    "text": "{\"type\":\"sandbox_permission_request\",\"requestId\":\"sandbox-1\",\"from\":\"bob\",\"workerName\":\"bob\",\"hostPattern\":{\"host\":\"*\"}}",
                    "summary": "sandbox_permission_request",
                    "timestamp": "2",
                    "read": false
                },
                {
                    "from": "alice",
                    "text": "{\"type\":\"plan_approval_request\",\"requestId\":\"plan-1\",\"from\":\"alice\",\"planFilePath\":\".kiana/plans/alice.md\",\"planContent\":\"1. Inspect\"}",
                    "summary": "plan_approval_request",
                    "timestamp": "3",
                    "read": false
                },
                {
                    "from": "alice",
                    "text": "{\"type\":\"permission_request\",\"request_id\":\"perm-other\",\"from\":\"alice\",\"tool_name\":\"Bash\",\"input\":{\"command\":\"git status\"}}",
                    "summary": "permission_request",
                    "timestamp": "4",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        context.app_state.insert(
            "mailbox_permission_requests".to_string(),
            json!({
                "perm-1": {
                    "request_id": "perm-1",
                    "from": "alice",
                    "tool_name": "Bash",
                    "input": { "command": "cargo test" }
                },
                "perm-other": {
                    "request_id": "perm-other",
                    "from": "alice",
                    "tool_name": "Bash",
                    "input": { "command": "git status" }
                }
            }),
        );
        context.app_state.insert(
            "sandbox_permission_requests".to_string(),
            json!({
                "sandbox-1": {
                    "requestId": "sandbox-1",
                    "host": "*",
                    "workerName": "bob"
                }
            }),
        );

        let initial = MonitorTool::new()
            .call(&json!({ "limit": 1 }), &mut context)
            .await
            .unwrap();
        assert_eq!(initial.data["pendingProtocolRequests"]["total"], 4);

        let permission = SendMessageTool::new()
            .call(
                &json!({
                    "to": "alice",
                    "message": {
                        "type": "permission_response",
                        "request_id": "perm-1",
                        "approve": true
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(permission.data["resolved_request"], true);

        let sandbox = SendMessageTool::new()
            .call(
                &json!({
                    "to": "bob",
                    "message": {
                        "type": "sandbox_permission_response",
                        "request_id": "sandbox-1",
                        "allow": true
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(sandbox.data["resolved_request"], true);

        let plan = SendMessageTool::new()
            .call(
                &json!({
                    "to": "alice",
                    "message": {
                        "type": "plan_approval_response",
                        "request_id": "plan-1",
                        "approve": true
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(plan.data["resolved_request"], true);

        let pending = MonitorTool::new()
            .call(&json!({ "limit": 1 }), &mut context)
            .await
            .unwrap();
        assert_eq!(pending.data["pendingProtocolRequests"]["total"], 1);
        assert_eq!(
            pending.data["pendingProtocolRequests"]["permissionRequests"][0]["request_id"],
            "perm-other"
        );
        assert!(context.app_state["mailbox_permission_requests"]
            .as_object()
            .unwrap()
            .get("perm-1")
            .is_none());
        assert!(context.app_state["mailbox_permission_requests"]
            .as_object()
            .unwrap()
            .contains_key("perm-other"));
        assert!(context.app_state["sandbox_permission_requests"]
            .as_object()
            .unwrap()
            .is_empty());

        let inbox_after = read_json(&inbox_path);
        assert_eq!(inbox_after[0]["read"], true);
        assert_eq!(inbox_after[1]["read"], true);
        assert_eq!(inbox_after[2]["read"], true);
        assert_eq!(inbox_after[3]["read"], false);
    }

    #[tokio::test]
    async fn team_delete_refuses_active_non_lead_members() {
        let mut context = test_context();
        let created = TeamCreateTool::new()
            .call(
                &json!({ "team_name": "active-review", "members": ["alice"] }),
                &mut context,
            )
            .await
            .unwrap();
        let team_file_path = created.data["team_file_path"].as_str().unwrap();
        let mut team_file = read_json(team_file_path);
        if let Some(member) = team_file["members"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|member| member["name"] == "alice")
        {
            member["isActive"] = json!(true);
        }
        fs::write(
            team_file_path,
            serde_json::to_string_pretty(&team_file).unwrap(),
        )
        .unwrap();

        let deleted = TeamDeleteTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();

        assert_eq!(deleted.data["success"], false);
        assert!(deleted.data["message"]
            .as_str()
            .unwrap()
            .contains("active member(s): alice"));
        assert!(Path::new(team_file_path).is_file());
    }

    #[tokio::test]
    async fn cron_and_tool_search_are_stateful() {
        let mut context = test_context();
        let created = CronCreateTool::new()
            .call(
                &json!({ "cron": "0 9 * * *", "prompt": "daily check" }),
                &mut context,
            )
            .await
            .unwrap();
        let id = created.data["cron"]["id"].as_str().unwrap().to_string();

        let listed = CronListTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        assert_eq!(listed.data["crons"].as_array().unwrap().len(), 1);

        let search = ToolSearchTool::new()
            .call(&json!({ "query": "task" }), &mut context)
            .await
            .unwrap();
        assert!(!search.data["tools"].as_array().unwrap().is_empty());
        assert!(search.data["total_registered"].as_u64().unwrap() >= 40);

        let deleted = CronDeleteTool::new()
            .call(&json!({ "id": id }), &mut context)
            .await
            .unwrap();
        assert_eq!(deleted.data["deleted"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn worktree_tools_switch_and_restore_cwd() {
        let mut context = test_context();
        let original = context.cwd.clone();
        let target = std::env::temp_dir();

        EnterWorktreeTool::new()
            .call(
                &json!({ "path": target.to_string_lossy().to_string() }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(context.cwd, target.to_string_lossy());

        ExitWorktreeTool::new()
            .call(&json!({}), &mut context)
            .await
            .unwrap();
        assert_eq!(context.cwd, original);
    }

    #[tokio::test]
    async fn skill_tool_loads_skill_content_and_records_invocation() {
        let _guard = skill_test_lock().lock().await;
        kiana_skills::clear_caches();

        let root = std::env::temp_dir().join(format!("kiana-skill-tool-{}", Uuid::new_v4()));
        let skill = root.join(".claude").join("skills").join("refactor");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: Refactor Helper\ndescription: Improve Rust module structure\nallowed-tools:\n  - Read\n  - Edit\nmodel: sonnet\ncontext: inline\n---\n# Refactor helper\nDetailed instructions.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(root.to_string_lossy().to_string());
        context
            .app_state
            .insert("project_trusted".to_string(), json!(true));
        let tool = SkillTool::new();
        let validation = tool
            .validate_input(&json!({ "skill": "/refactor" }), &context)
            .await;
        assert!(validation.result, "{validation:?}");

        let output = tool
            .call(
                &json!({ "skill": "/refactor", "args": { "target": "team_create" } }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["success"], true);
        assert_eq!(output.data["commandName"], "refactor");
        assert_eq!(output.data["skill"]["display_name"], "Refactor Helper");
        assert_eq!(output.data["skill"]["allowed_tools"][1], "Edit");
        assert!(output.data["skill"]["content"]
            .as_str()
            .unwrap()
            .contains("Detailed instructions."));
        assert_eq!(
            context.app_state["skill_invocations"][0]["skill"],
            "refactor"
        );
        assert_eq!(
            context.app_state["skill_invocations"][0]["args"]["target"],
            "team_create"
        );
        let invoked_skill_path = Path::new(
            context.app_state["invoked_skills"][0]["path"]
                .as_str()
                .unwrap(),
        );
        assert!(invoked_skill_path.ends_with(Path::new("refactor").join("SKILL.md")));
        assert!(context.app_state["invoked_skills"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Detailed instructions."));

        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skill_tool_rejects_project_skill_when_project_is_untrusted() {
        let _guard = skill_test_lock().lock().await;
        kiana_skills::clear_caches();

        let unique = Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("kiana-skill-tool-{unique}"));
        let skill_name = format!("project-{unique}");
        let skill = root.join(".claude").join("skills").join(&skill_name);
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Project-only helper\n---\nProject instructions.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(root.to_string_lossy().to_string());
        context
            .app_state
            .insert("project_trusted".to_string(), json!(false));

        let validation = SkillTool::new()
            .validate_input(&json!({ "skill": skill_name }), &context)
            .await;
        assert!(!validation.result);
        assert!(validation
            .message
            .as_deref()
            .unwrap()
            .contains("Unknown skill"));

        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skill_tool_rejects_unknown_skill() {
        let _guard = skill_test_lock().lock().await;
        kiana_skills::clear_caches();

        let root = std::env::temp_dir().join(format!("kiana-skill-tool-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut context = test_context_with_cwd(root.to_string_lossy().to_string());
        let tool = SkillTool::new();

        let validation = tool
            .validate_input(&json!({ "skill": "missing" }), &context)
            .await;
        assert!(!validation.result);
        assert!(validation
            .message
            .as_deref()
            .unwrap()
            .contains("Unknown skill: missing"));
        let error = tool
            .call(&json!({ "skill": "missing" }), &mut context)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Unknown skill: missing"));

        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn skill_tool_rejects_model_disabled_skill() {
        let _guard = skill_test_lock().lock().await;
        kiana_skills::clear_caches();

        let root = std::env::temp_dir().join(format!("kiana-skill-tool-{}", Uuid::new_v4()));
        let skill = root.join(".claude").join("skills").join("deploy");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Deploy production\ndisable-model-invocation: true\n---\n# Deploy\nRun release steps.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(root.to_string_lossy().to_string());
        context
            .app_state
            .insert("project_trusted".to_string(), json!(true));
        let validation = SkillTool::new()
            .validate_input(&json!({ "skill": "deploy" }), &context)
            .await;
        assert!(!validation.result);
        assert!(validation
            .message
            .as_deref()
            .unwrap()
            .contains("disable-model-invocation"));

        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tool_search_uses_registered_tools_dynamically() {
        let mut context = test_context();
        let search = ToolSearchTool::new();

        let powershell = search
            .call(
                &json!({
                    "query": "powershell",
                    "include_schema": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(powershell.data["tools"][0]["name"], "PowerShell");
        assert!(powershell.data["tools"][0]["input_schema"]["properties"]
            .get("command")
            .is_some());

        let repl = search
            .call(&json!({ "query": "batch primitive" }), &mut context)
            .await
            .unwrap();
        assert_eq!(repl.data["tools"][0]["name"], "REPL");

        let all = search
            .call(&json!({ "query": "*", "max_results": 100 }), &mut context)
            .await
            .unwrap();
        let names = all.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"discover_skills"));
        assert!(names.contains(&"send_user_file"));
        assert_eq!(
            all.data["count"].as_u64(),
            all.data["total_registered"].as_u64()
        );
    }
}
