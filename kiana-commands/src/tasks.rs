use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct TasksCommand;

#[async_trait]
impl Command for TasksCommand {
    fn name(&self) -> &str {
        "tasks"
    }

    fn description(&self) -> &str {
        "Manage tasks"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("list") {
            "" | "list" | "status" => list_tasks(&context, optional_task_list_arg(rest)?),
            "json" => tasks_json(&context, optional_task_list_arg(rest)?),
            "show" | "get" => show_task(&context, rest),
            "path" => Ok(CommandResult::text(
                task_list_dir(&context, optional_task_list_arg(rest)?)
                    .display()
                    .to_string(),
            )),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => {
                if looks_like_status(other) {
                    list_tasks_with_status(&context, other, optional_task_list_arg(rest)?)
                } else {
                    Err(anyhow!("unknown tasks command '{}'\n\n{}", other, usage()))
                }
            }
        }
    }
}

fn list_tasks(context: &CommandContext, task_list: Option<&str>) -> Result<CommandResult> {
    let tasks = load_tasks(context, task_list)?;
    let task_list_id = task_list_id(context, task_list);
    if tasks.is_empty() {
        return Ok(CommandResult::text(format!(
            "No tasks.\ntask_list_id: {}\ntasks_dir: {}\nTasks created by TaskCreate are stored in this task list.",
            task_list_id,
            task_list_dir(context, task_list).display()
        )));
    }

    Ok(CommandResult::text(format_task_list(
        &tasks,
        &task_list_id,
        None,
    )))
}

fn list_tasks_with_status(
    context: &CommandContext,
    status: &str,
    task_list: Option<&str>,
) -> Result<CommandResult> {
    let mut tasks = load_tasks(context, task_list)?;
    tasks.retain(|task| task_status(task).is_some_and(|value| value == status));
    Ok(CommandResult::text(format_task_list(
        &tasks,
        &task_list_id(context, task_list),
        Some(status),
    )))
}

fn tasks_json(context: &CommandContext, task_list: Option<&str>) -> Result<CommandResult> {
    Ok(CommandResult::text(serde_json::to_string_pretty(
        &tasks_report(context, task_list)?,
    )?))
}

pub fn tasks_report(context: &CommandContext, task_list: Option<&str>) -> Result<Value> {
    let tasks = load_tasks(context, task_list)?;
    let task_list_id = task_list_id(context, task_list);
    let tasks_dir = task_list_dir(context, task_list);
    let mut status_counts = BTreeMap::new();
    for task in &tasks {
        let status = task_status(task).unwrap_or("unknown").to_string();
        *status_counts.entry(status).or_insert(0usize) += 1;
    }
    Ok(json!({
        "schema": "kiana.tasks.v1",
        "task_list_id": task_list_id,
        "tasks_dir": tasks_dir.display().to_string(),
        "count": tasks.len(),
        "status_counts": status_counts,
        "tasks": tasks,
    }))
}

fn show_task(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (task_id, task_list) = parse_show_args(rest)?;
    let tasks = load_tasks(context, task_list)?;
    let Some(task) = tasks.iter().find(|task| task_id_matches(task, &task_id)) else {
        return Err(anyhow!(
            "task '{}' was not found in task list '{}'",
            task_id,
            task_list_id(context, task_list)
        ));
    };
    Ok(CommandResult::text(serde_json::to_string_pretty(task)?))
}

fn load_tasks(context: &CommandContext, task_list: Option<&str>) -> Result<Vec<Value>> {
    let mut tasks = read_disk_tasks(&task_list_dir(context, task_list))?;
    if task_list.is_none() {
        let state_tasks = context
            .app_state
            .get("tasks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for task in state_tasks {
            if !tasks
                .iter()
                .any(|existing| task_id(existing) == task_id(&task))
            {
                tasks.push(task);
            }
        }
    }

    tasks.sort_by(compare_tasks);
    Ok(tasks)
}

fn read_disk_tasks(dir: &Path) -> Result<Vec<Value>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut value: Value = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if value.get("subject").is_none() {
            if let Some(title) = value.get("title").cloned() {
                value["subject"] = title;
            }
        }
        tasks.push(value);
    }
    Ok(tasks)
}

fn task_list_dir(context: &CommandContext, task_list: Option<&str>) -> PathBuf {
    tasks_root(context).join(sanitize_path_component(&task_list_id(context, task_list)))
}

fn tasks_root(context: &CommandContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get("tasks_root")
        .or_else(|| context.app_state.get("tasksRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return resolve_path(context, path);
    }
    if let Some(path) = std::env::var_os("KIANA_TASKS_ROOT") {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            cwd(context).join(path)
        };
    }
    cwd(context).join(".kiana").join("tasks")
}

fn task_list_id(context: &CommandContext, explicit: Option<&str>) -> String {
    explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            context
                .app_state
                .get("task_list_id")
                .or_else(|| context.app_state.get("taskListId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
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
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            context
                .app_state
                .get("session_id")
                .or_else(|| context.app_state.get("sessionId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default".to_string())
}

fn cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_path(context: &CommandContext, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd(context).join(path)
    }
}

fn format_task_list(tasks: &[Value], task_list_id: &str, status: Option<&str>) -> String {
    let mut lines = vec![match status {
        Some(status) => format!(
            "{} task(s) in '{}' with status '{}':",
            tasks.len(),
            task_list_id,
            status
        ),
        None => format!("{} task(s) in '{}':", tasks.len(), task_list_id),
    }];

    for task in tasks.iter().take(20) {
        let id = task_id(task).unwrap_or("<unknown>");
        let title = task_title(task);
        let status = task_status(task).unwrap_or("unknown");
        let owner = task
            .get("owner")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("-");
        let blocked_by = task
            .get("blockedBy")
            .or_else(|| task.get("blocked_by"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        lines.push(format!(
            "- {} [{}] owner={} blocked_by={} {}",
            id, status, owner, blocked_by, title
        ));
    }
    if tasks.len() > 20 {
        lines.push(format!("... {} more", tasks.len() - 20));
    }
    lines.push("usage: kiana tasks show <id> | json | <status>".to_string());
    lines.join("\n")
}

fn task_id(task: &Value) -> Option<&str> {
    task.get("id")
        .or_else(|| task.get("task_id"))
        .or_else(|| task.get("taskId"))
        .and_then(Value::as_str)
}

fn task_id_matches(task: &Value, expected_id: &str) -> bool {
    task_id(task).is_some_and(|id| id == expected_id)
}

fn task_title(task: &Value) -> &str {
    task.get("subject")
        .or_else(|| task.get("title"))
        .or_else(|| task.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("<untitled>")
}

fn task_status(task: &Value) -> Option<&str> {
    task.get("status").and_then(Value::as_str)
}

fn compare_tasks(a: &Value, b: &Value) -> std::cmp::Ordering {
    let a_id = task_id(a).unwrap_or_default();
    let b_id = task_id(b).unwrap_or_default();
    let a_num = a_id.parse::<i64>().ok();
    let b_num = b_id.parse::<i64>().ok();
    a_num.cmp(&b_num).then_with(|| a_id.cmp(b_id))
}

fn optional_task_list_arg(rest: &str) -> Result<Option<&str>> {
    let mut parts = rest.trim().split_whitespace();
    let task_list = parts.next();
    if parts.next().is_some() {
        return Err(anyhow!("{}", usage()));
    }
    Ok(task_list)
}

fn parse_show_args(rest: &str) -> Result<(String, Option<&str>)> {
    let mut parts = rest.split_whitespace();
    let task_id = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("tasks show requires a task id"))?;
    if task_id.contains('/') || task_id.contains('\\') || task_id.contains("..") {
        return Err(anyhow!("task id is invalid"));
    }
    let task_list = parts.next();
    if parts.next().is_some() {
        return Err(anyhow!("{}", usage()));
    }
    Ok((task_id.to_string(), task_list))
}

fn split_word(value: &str) -> (Option<&str>, &str) {
    let value = value.trim_start();
    if value.is_empty() {
        return (None, "");
    }
    let Some((word, rest)) = value.split_once(char::is_whitespace) else {
        return (Some(value), "");
    };
    (Some(word), rest.trim_start())
}

fn looks_like_status(value: &str) -> bool {
    matches!(
        value,
        "pending" | "in_progress" | "running" | "completed" | "failed" | "cancelled" | "killed"
    )
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

fn usage() -> &'static str {
    "Usage:\n  kiana tasks [list|status] [task_list_id]\n  kiana tasks json [task_list_id]\n  kiana tasks show <task_id> [task_list_id]\n  kiana tasks path [task_list_id]\n  kiana tasks <pending|in_progress|running|completed|failed|cancelled|killed> [task_list_id]"
}

#[cfg(test)]
mod tests {
    use super::TasksCommand;
    use crate::{Command, CommandContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-tasks-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, app_state: HashMap<String, Value>) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    fn write_task(root: &std::path::Path, list_id: &str, id: &str, status: &str, title: &str) {
        let dir = root.join(".kiana").join("tasks").join(list_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string_pretty(&json!({
                "id": id,
                "title": title,
                "subject": title,
                "description": null,
                "status": status,
                "owner": null,
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 2
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn tasks_lists_session_state_tasks() {
        let mut app_state = HashMap::new();
        app_state.insert(
            "tasks".to_string(),
            json!([{ "id": "t1", "title": "Inspect", "status": "pending" }]),
        );

        let result = TasksCommand.execute(context("", app_state)).await.unwrap();

        assert!(result.value.contains("1 task(s) in 'default'"));
        assert!(result.value.contains("t1 [pending]"));
        assert!(result.value.contains("Inspect"));
    }

    #[tokio::test]
    async fn tasks_read_persisted_session_task_list() {
        let root = temp_root();
        write_task(&root, "session-1", "1", "pending", "Persisted task");
        let app_state = HashMap::from([
            ("cwd".to_string(), json!(root.to_string_lossy().to_string())),
            ("session_id".to_string(), json!("session-1")),
        ]);

        let result = TasksCommand
            .execute(context("list", app_state.clone()))
            .await
            .unwrap();
        assert!(result.value.contains("1 task(s) in 'session-1'"));
        assert!(result.value.contains("Persisted task"));

        let shown = TasksCommand
            .execute(context("show 1", app_state))
            .await
            .unwrap();
        assert!(shown.value.contains("\"id\": \"1\""));
        assert!(shown.value.contains("\"subject\": \"Persisted task\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tasks_can_filter_and_select_explicit_task_list() {
        let root = temp_root();
        write_task(&root, "review", "1", "completed", "Done task");
        write_task(&root, "review", "2", "pending", "Open task");
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);

        let result = TasksCommand
            .execute(context("pending review", app_state.clone()))
            .await
            .unwrap();
        assert!(result
            .value
            .contains("1 task(s) in 'review' with status 'pending'"));
        assert!(result.value.contains("Open task"));
        assert!(!result.value.contains("Done task"));

        let json = TasksCommand
            .execute(context("json review", app_state))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&json.value).unwrap();
        assert_eq!(parsed["schema"], "kiana.tasks.v1");
        assert_eq!(parsed["task_list_id"], "review");
        assert_eq!(parsed["count"], 2);
        assert_eq!(parsed["status_counts"]["completed"], 1);
        assert_eq!(parsed["status_counts"]["pending"], 1);
        assert!(json.value.contains("\"id\": \"1\""));
        assert!(json.value.contains("\"id\": \"2\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tasks_json_reports_schema_status_counts_and_task_order() {
        let root = temp_root();
        write_task(&root, "session-2", "2", "completed", "Second task");
        write_task(&root, "session-2", "1", "pending", "First task");
        let app_state = HashMap::from([
            ("cwd".to_string(), json!(root.to_string_lossy().to_string())),
            ("session_id".to_string(), json!("session-2")),
        ]);

        let result = TasksCommand
            .execute(context("json", app_state))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.tasks.v1");
        assert_eq!(report["task_list_id"], "session-2");
        assert_eq!(report["count"], 2);
        assert_eq!(report["status_counts"]["pending"], 1);
        assert_eq!(report["status_counts"]["completed"], 1);
        assert_eq!(report["tasks"][0]["id"], "1");
        assert_eq!(report["tasks"][1]["id"], "2");
        assert!(report["tasks_dir"].as_str().unwrap().contains("session-2"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tasks_reject_extra_words_after_task_list_id() {
        let root = temp_root();
        write_task(&root, "review", "1", "pending", "Open task");
        let app_state =
            HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]);

        for args in [
            "list review extra",
            "json review extra",
            "path review extra",
            "pending review extra",
            "show 1 review extra",
        ] {
            let error = TasksCommand
                .execute(context(args, app_state.clone()))
                .await
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("Usage:"),
                "{args} returned unexpected error: {error}"
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }
}
