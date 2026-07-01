use crate::team_lifecycle::finalize_background_team_agent;
use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct TaskOutputInput {
    task_id: String,
    #[serde(default = "default_block")]
    block: bool,
    #[serde(default = "default_timeout_ms", alias = "timeout")]
    timeout_ms: u64,
}

pub struct TaskOutputTool;

impl TaskOutputTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn description(&self) -> &str {
        "Read status and output for a session or background task"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("read output logs from a task")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "block": {
                    "type": "boolean",
                    "description": "Whether to wait until a running task reaches a terminal state"
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 600000
                }
            },
            "required": ["task_id"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "retrieval_status": {
                    "type": "string",
                    "enum": ["success", "timeout", "not_ready", "not_found"]
                },
                "task": { "type": ["object", "null"] }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: TaskOutputInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.task_id.trim().is_empty()
            || input.task_id.contains('/')
            || input.task_id.contains('\\')
            || input.task_id.contains("..")
        {
            return ValidationResult::err("task_id is invalid".to_string(), 2);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TaskOutputInput = serde_json::from_value(input.clone())?;
        let timeout_ms = input.timeout_ms.min(600_000);
        let task_id = input.task_id.trim();
        let started = Instant::now();

        loop {
            if let Some(task) = find_session_task(context, task_id) {
                let retrieval_status =
                    if is_terminal_status(task.get("status").and_then(Value::as_str)) {
                        "success"
                    } else if input.block {
                        "not_ready"
                    } else {
                        "not_ready"
                    };
                return Ok(ToolOutput {
                    data: json!({
                        "retrieval_status": retrieval_status,
                        "task": task
                    }),
                    metadata: None,
                });
            }

            if let Some(task) = read_background_task(context, task_id)? {
                let terminal = is_terminal_status(task.get("status").and_then(Value::as_str));
                if terminal || !input.block {
                    return Ok(ToolOutput {
                        data: json!({
                            "retrieval_status": if terminal { "success" } else { "not_ready" },
                            "task": task
                        }),
                        metadata: None,
                    });
                }
            }

            if !input.block {
                return Ok(ToolOutput {
                    data: json!({
                        "retrieval_status": "not_found",
                        "task": Value::Null
                    }),
                    metadata: None,
                });
            }

            if started.elapsed() >= Duration::from_millis(timeout_ms) {
                let task = read_background_task(context, task_id)?
                    .or_else(|| find_session_task(context, task_id));
                return Ok(ToolOutput {
                    data: json!({
                        "retrieval_status": "timeout",
                        "task": task
                    }),
                    metadata: None,
                });
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let mut parts = Vec::new();
        let retrieval_status = output
            .data
            .get("retrieval_status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        parts.push(format!(
            "<retrieval_status>{}</retrieval_status>",
            escape_xml_text(retrieval_status)
        ));

        if let Some(task) = output.data.get("task").filter(|task| !task.is_null()) {
            push_tag_from_str(&mut parts, "task_id", task, &["task_id", "id"]);
            push_tag_from_str(&mut parts, "task_type", task, &["task_type"]);
            push_tag_from_str(&mut parts, "status", task, &["status"]);
            push_tag_from_number(&mut parts, "exit_code", task, &["exit_code", "exitCode"]);
            push_tag_from_str(
                &mut parts,
                "description",
                task,
                &["description", "prompt", "title"],
            );
            if let Some(output_text) = task.get("output").and_then(Value::as_str) {
                if !output_text.trim().is_empty() {
                    parts.push(format!(
                        "<output>\n{}\n</output>",
                        escape_xml_text(output_text.trim_end())
                    ));
                }
            }
            push_tag_from_str(&mut parts, "error", task, &["error"]);
        }

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": parts.join("\n\n")
        })
    }
}

fn push_tag_from_str(parts: &mut Vec<String>, tag: &str, object: &Value, keys: &[&str]) {
    if let Some(value) = keys
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("<{tag}>{}</{tag}>", escape_xml_text(value)));
    }
}

fn push_tag_from_number(parts: &mut Vec<String>, tag: &str, object: &Value, keys: &[&str]) {
    if let Some(value) = keys.iter().find_map(|key| object.get(*key)) {
        if let Some(number) = value.as_i64().map(|number| number.to_string()) {
            parts.push(format!("<{tag}>{number}</{tag}>"));
        }
    }
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn default_block() -> bool {
    true
}

fn default_timeout_ms() -> u64 {
    30_000
}

fn find_session_task(context: &ToolContext, task_id: &str) -> Option<Value> {
    context
        .app_state
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|tasks| {
            tasks.iter().find_map(|task| {
                let id = task
                    .get("id")
                    .or_else(|| task.get("task_id"))
                    .and_then(Value::as_str)?;
                if id == task_id {
                    Some(json!({
                        "task_id": id,
                        "task_type": "session",
                        "status": task.get("status").cloned().unwrap_or_else(|| json!("unknown")),
                        "description": task
                            .get("description")
                            .or_else(|| task.get("title"))
                            .cloned()
                            .unwrap_or_else(|| json!("")),
                        "output": task.get("output").cloned().unwrap_or_else(|| json!("")),
                        "task": task
                    }))
                } else {
                    None
                }
            })
        })
}

fn read_background_task(context: &ToolContext, task_id: &str) -> ToolResult<Option<Value>> {
    let root = background_root(context);
    let task_path = root.join("tasks").join(format!("{task_id}.json"));
    if !task_path.is_file() {
        return Ok(None);
    }
    let mut task: Value = serde_json::from_str(&std::fs::read_to_string(&task_path)?)?;
    let mut changed = apply_exit_marker(&root, task_id, &mut task)?;
    if is_terminal_status(task.get("status").and_then(Value::as_str)) {
        let reason = task
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("completed")
            .to_string();
        changed |= finalize_background_team_agent(context, &mut task, &reason)?;
    }
    if changed {
        std::fs::write(&task_path, serde_json::to_string_pretty(&task)?)?;
    }
    let output = std::fs::read_to_string(root.join("logs").join(format!("{task_id}.log")))
        .unwrap_or_default();
    if let Some(task_object) = task.as_object_mut() {
        task_object.insert("task_id".to_string(), json!(task_id));
        task_object.insert("task_type".to_string(), json!("background"));
        task_object.insert("output".to_string(), json!(output));
    }
    Ok(Some(task))
}

fn apply_exit_marker(root: &std::path::Path, task_id: &str, task: &mut Value) -> ToolResult<bool> {
    let exit_path = root.join("exits").join(format!("{task_id}.exit"));
    if !exit_path.is_file() || is_terminal_status(task.get("status").and_then(Value::as_str)) {
        return Ok(false);
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
    }
    Ok(true)
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

fn is_terminal_status(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("completed" | "complete" | "success" | "failed" | "killed" | "cancelled" | "canceled")
    )
}

#[cfg(test)]
mod tests {
    use super::TaskOutputTool;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use uuid::Uuid;

    fn test_context() -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        }
    }

    #[tokio::test]
    async fn reads_session_task_output_from_app_state() {
        let mut context = test_context();
        context.app_state.insert(
            "tasks".to_string(),
            json!([{
                "id": "task-1",
                "title": "Inspect",
                "status": "completed",
                "output": "done"
            }]),
        );

        let output = TaskOutputTool::new()
            .call(
                &json!({ "task_id": "task-1", "block": false }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["output"], "done");
    }

    #[test]
    fn maps_task_output_to_model_facing_xml_text() {
        let output = crate::tool::ToolOutput {
            data: json!({
                "retrieval_status": "success",
                "task": {
                    "task_id": "bg-1",
                    "task_type": "background",
                    "status": "completed",
                    "exit_code": 0,
                    "description": "Inspect repo",
                    "output": "line <one>\nline & two\n"
                }
            }),
            metadata: None,
        };

        let result = TaskOutputTool::new().map_to_api_result(&output, "toolu_task_output");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_task_output");
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("<retrieval_status>success</retrieval_status>"));
        assert!(content.contains("<task_id>bg-1</task_id>"));
        assert!(content.contains("<exit_code>0</exit_code>"));
        assert!(content.contains("<output>\nline &lt;one&gt;\nline &amp; two\n</output>"));
        assert!(result["content"]["task"].is_null());
    }

    #[tokio::test]
    async fn reads_background_task_json_and_log() {
        let root = std::env::temp_dir().join(format!("kiana-bg-tool-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("tasks")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(
            root.join("tasks").join("bg-1.json"),
            r#"{"id":"bg-1","prompt":"hello","status":"completed"}"#,
        )
        .unwrap();
        fs::write(root.join("logs").join("bg-1.log"), "worker completed\n").unwrap();
        std::env::set_var("KIANA_BG_DIR", &root);

        let mut context = test_context();
        let output = TaskOutputTool::new()
            .call(&json!({ "task_id": "bg-1", "block": false }), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["task_type"], "background");
        assert!(output.data["task"]["output"]
            .as_str()
            .unwrap()
            .contains("worker completed"));

        std::env::remove_var("KIANA_BG_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn output_finalizes_background_teammate_and_unassigns_tasks() {
        let root = std::env::temp_dir().join(format!("kiana-bg-team-{}", Uuid::new_v4()));
        let bg_root = root.join("bg");
        let teams_root = root.join("teams");
        let tasks_root = root.join("tasks");
        fs::create_dir_all(bg_root.join("tasks")).unwrap();
        fs::create_dir_all(bg_root.join("logs")).unwrap();
        fs::create_dir_all(bg_root.join("exits")).unwrap();
        fs::create_dir_all(teams_root.join("review")).unwrap();
        fs::create_dir_all(tasks_root.join("review")).unwrap();

        let team_file_path = teams_root.join("review/config.json");
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
            tasks_root.join("review/1.json"),
            serde_json::to_string_pretty(&json!({
                "id": "1",
                "title": "Finish report",
                "subject": "Finish report",
                "owner": "runner",
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
            bg_root.join("tasks/bg-1.json"),
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
        fs::write(bg_root.join("logs/bg-1.log"), "worker completed\n").unwrap();
        fs::write(bg_root.join("exits/bg-1.exit"), "0\n").unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                (
                    "background_root".to_string(),
                    json!(bg_root.to_string_lossy().to_string()),
                ),
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
        };

        let output = TaskOutputTool::new()
            .call(&json!({ "task_id": "bg-1", "block": false }), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["status"], "completed");
        assert_eq!(output.data["task"]["team_lifecycle_finalized"], true);
        assert_eq!(output.data["task"]["team_lifecycle"]["reason"], "completed");
        assert_eq!(
            output.data["task"]["team_lifecycle"]["unassignedTasks"][0]["id"],
            "1"
        );

        let team_file: Value =
            serde_json::from_str(&fs::read_to_string(&team_file_path).unwrap()).unwrap();
        let runner = team_file["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["name"] == "runner")
            .unwrap();
        assert_eq!(runner["isActive"], false);
        assert_eq!(runner["lastExitReason"], "completed");

        let task: Value =
            serde_json::from_str(&fs::read_to_string(tasks_root.join("review/1.json")).unwrap())
                .unwrap();
        assert!(task.get("owner").is_none());
        assert_eq!(task["status"], "pending");

        let inbox: Value = serde_json::from_str(
            &fs::read_to_string(teams_root.join("review/inboxes/team-lead.json")).unwrap(),
        )
        .unwrap();
        let body: Value = serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["type"], "teammate_terminated");
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("1 task(s) were unassigned"));

        TaskOutputTool::new()
            .call(&json!({ "task_id": "bg-1", "block": false }), &mut context)
            .await
            .unwrap();
        let inbox_after: Value = serde_json::from_str(
            &fs::read_to_string(teams_root.join("review/inboxes/team-lead.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(inbox_after.as_array().unwrap().len(), 1);

        let persisted = serde_json::from_str::<Value>(
            &fs::read_to_string(Path::new(&bg_root).join("tasks/bg-1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted["team_lifecycle_finalized"], true);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_nonblocking_task_returns_not_found() {
        let mut context = test_context();
        let output = TaskOutputTool::new()
            .call(
                &json!({ "task_id": "missing", "block": false }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "not_found");
        assert!(output.data["task"].is_null());
    }
}
