use crate::tool::{ToolContext, ToolResult};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const TASKS_ROOT_KEY: &str = "tasks_root";
const TASKS_ROOT_ENV: &str = "KIANA_TASKS_ROOT";
const TEAMS_ROOT_KEY: &str = "teams_root";
const TEAMS_ROOT_ENV: &str = "KIANA_TEAMS_ROOT";

pub fn finalize_background_team_agent(
    context: &ToolContext,
    task: &mut Value,
    reason: &str,
) -> ToolResult<bool> {
    if value_bool(task, &["team_lifecycle_finalized"]).unwrap_or(false) {
        return Ok(false);
    }

    let Some(team_name) = value_string(task, &["team_name", "teamName"]) else {
        return Ok(false);
    };
    let agent_name =
        value_string(task, &["agent_name", "agentName"]).unwrap_or_else(|| "agent".to_string());
    let agent_id = value_string(task, &["teammate_id", "teammateId", "agent_id", "agentId"])
        .unwrap_or_else(|| format_agent_id(&agent_name, &team_name));
    let task_list_id = value_string(task, &["task_list_id", "taskListId"])
        .unwrap_or_else(|| sanitize_path_component(&team_name));
    let now = now_unix_seconds();

    let team_file_path = value_string(task, &["team_file_path", "teamFilePath"])
        .map(|path| resolve_context_path(context, path))
        .unwrap_or_else(|| team_file_path(context, &team_name));
    let member_marked_inactive =
        mark_member_inactive(&team_file_path, &agent_id, &agent_name, reason, now)?;
    let unassigned_tasks =
        unassign_teammate_tasks(context, &task_list_id, &agent_id, &agent_name, now)?;
    let notification_message = notification_message(&agent_name, reason, &unassigned_tasks);
    let leader_mailbox_path = append_leader_notification(
        context,
        &team_name,
        &team_file_path,
        &agent_name,
        &agent_id,
        reason,
        &notification_message,
        &unassigned_tasks,
        now,
    )?;

    if let Some(object) = task.as_object_mut() {
        object.insert("team_lifecycle_finalized".to_string(), json!(true));
        object.insert(
            "team_lifecycle".to_string(),
            json!({
                "team_name": team_name,
                "teamName": team_name,
                "agent_name": agent_name,
                "agentName": agent_name,
                "agent_id": agent_id,
                "agentId": agent_id,
                "reason": reason,
                "memberMarkedInactive": member_marked_inactive,
                "member_marked_inactive": member_marked_inactive,
                "unassignedTasks": unassigned_tasks,
                "unassigned_tasks": unassigned_tasks,
                "notificationMessage": notification_message,
                "notification_message": notification_message,
                "leaderMailboxPath": leader_mailbox_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                "leader_mailbox_path": leader_mailbox_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                "finalizedAt": now,
                "finalized_at": now
            }),
        );
    }

    Ok(true)
}

fn value_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn value_bool(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
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

fn sanitize_team_name(value: &str) -> String {
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

fn sanitize_agent_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
}

fn format_agent_id(agent_name: &str, team_name: &str) -> String {
    format!(
        "{}@{}",
        sanitize_agent_name(agent_name),
        sanitize_agent_name(team_name)
    )
}

fn resolve_context_path(context: &ToolContext, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(&context.cwd).join(path)
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
        return resolve_context_path(context, path.to_string());
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
        return resolve_context_path(context, path.to_string());
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

fn team_file_path(context: &ToolContext, team_name: &str) -> PathBuf {
    teams_root(context)
        .join(sanitize_team_name(team_name))
        .join("config.json")
}

fn read_json_file(path: &Path) -> ToolResult<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

fn write_json_file(path: &Path, value: &Value) -> ToolResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn mark_member_inactive(
    team_file_path: &Path,
    agent_id: &str,
    agent_name: &str,
    reason: &str,
    now: i64,
) -> ToolResult<bool> {
    let Some(mut team_file) = read_json_file(team_file_path)? else {
        return Ok(false);
    };
    let Some(members) = team_file
        .as_object_mut()
        .and_then(|object| object.get_mut("members"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(false);
    };

    let mut changed = false;
    for member in members {
        let matches_agent_id = member
            .get("agentId")
            .or_else(|| member.get("agent_id"))
            .and_then(Value::as_str)
            == Some(agent_id);
        let matches_agent_name = member.get("name").and_then(Value::as_str) == Some(agent_name);
        if !(matches_agent_id || matches_agent_name) {
            continue;
        }
        if let Some(member_object) = member.as_object_mut() {
            member_object.insert("isActive".to_string(), json!(false));
            member_object.insert("is_active".to_string(), json!(false));
            member_object.insert("lastExitReason".to_string(), json!(reason));
            member_object.insert("last_exit_reason".to_string(), json!(reason));
            member_object.insert("lastExitedAt".to_string(), json!(now));
            member_object.insert("last_exited_at".to_string(), json!(now));
            changed = true;
        }
    }

    if changed {
        write_json_file(team_file_path, &team_file)?;
    }
    Ok(changed)
}

fn unassign_teammate_tasks(
    context: &ToolContext,
    task_list_id: &str,
    agent_id: &str,
    agent_name: &str,
    now: i64,
) -> ToolResult<Vec<Value>> {
    let task_dir = tasks_root(context).join(sanitize_path_component(task_list_id));
    if !task_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut unassigned = Vec::new();
    for entry in fs::read_dir(task_dir)? {
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

        let mut task: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        if is_terminal_status(task.get("status").and_then(Value::as_str)) {
            continue;
        }
        let owner = task.get("owner").and_then(Value::as_str);
        if owner != Some(agent_id) && owner != Some(agent_name) {
            continue;
        }

        let id = task
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let subject = task
            .get("subject")
            .or_else(|| task.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(object) = task.as_object_mut() {
            object.remove("owner");
            object.insert("status".to_string(), json!("pending"));
            object.insert("updated_at".to_string(), json!(now));
        }
        write_json_file(&path, &task)?;
        unassigned.push(json!({
            "id": id,
            "subject": subject
        }));
    }
    Ok(unassigned)
}

fn is_terminal_status(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("completed" | "complete" | "success" | "failed" | "cancelled" | "canceled" | "killed")
    )
}

fn notification_message(agent_name: &str, reason: &str, unassigned_tasks: &[Value]) -> String {
    let action = match reason {
        "terminated" | "killed" | "cancelled" | "canceled" => "was terminated",
        "shutdown" | "completed" => "has shut down",
        "failed" => "failed",
        other => other,
    };
    let mut message = format!("{agent_name} {action}.");
    if !unassigned_tasks.is_empty() {
        let task_list = unassigned_tasks
            .iter()
            .map(|task| {
                let id = task.get("id").and_then(Value::as_str).unwrap_or_default();
                let subject = task
                    .get("subject")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                format!("#{id} \"{subject}\"")
            })
            .collect::<Vec<_>>()
            .join(", ");
        message.push_str(&format!(
            " {} task(s) were unassigned: {}. Use TaskList to check availability and TaskUpdate with owner to reassign them to idle teammates.",
            unassigned_tasks.len(),
            task_list
        ));
    }
    message
}

fn append_leader_notification(
    context: &ToolContext,
    team_name: &str,
    team_file_path: &Path,
    agent_name: &str,
    agent_id: &str,
    reason: &str,
    notification_message: &str,
    unassigned_tasks: &[Value],
    now: i64,
) -> ToolResult<Option<PathBuf>> {
    let team_dir = team_file_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| teams_root(context).join(sanitize_team_name(team_name)));
    let path = team_dir.join("inboxes").join("team-lead.json");
    let mut inbox = read_json_file(&path)?
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let body = json!({
        "type": "teammate_terminated",
        "message": notification_message,
        "reason": reason,
        "from": agent_name,
        "agentId": agent_id,
        "agent_id": agent_id,
        "unassignedTasks": unassigned_tasks,
        "unassigned_tasks": unassigned_tasks
    });
    inbox.push(json!({
        "from": agent_name,
        "text": serde_json::to_string(&body)?,
        "summary": "teammate_terminated",
        "timestamp": now.to_string(),
        "read": false
    }));
    write_json_file(&path, &Value::Array(inbox))?;
    Ok(Some(path))
}
