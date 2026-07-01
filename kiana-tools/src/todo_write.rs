use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct TodoWriteInput {
    todos: Vec<Value>,
}

pub struct TodoWriteTool;

impl TodoWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn description(&self) -> &str {
        "Create and update the current session task checklist"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("manage the session task checklist")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The complete updated todo list for the current session",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["low", "medium", "high"]
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "oldTodos": { "type": "array" },
                "newTodos": { "type": "array" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: TodoWriteInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        for (idx, todo) in input.todos.iter().enumerate() {
            let Some(object) = todo.as_object() else {
                return ValidationResult::err(
                    format!("Todo at index {} must be an object", idx),
                    2,
                );
            };
            let Some(content) = object.get("content").and_then(|value| value.as_str()) else {
                return ValidationResult::err(
                    format!("Todo at index {} is missing content", idx),
                    3,
                );
            };
            if content.trim().is_empty() {
                return ValidationResult::err(
                    format!("Todo at index {} has empty content", idx),
                    4,
                );
            }
            let Some(status) = object.get("status").and_then(|value| value.as_str()) else {
                return ValidationResult::err(
                    format!("Todo at index {} is missing status", idx),
                    5,
                );
            };
            if !matches!(status, "pending" | "in_progress" | "completed") {
                return ValidationResult::err(
                    format!("Todo at index {} has invalid status", idx),
                    6,
                );
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: TodoWriteInput = serde_json::from_value(input.clone())?;
        let old_todos = context
            .app_state
            .get("todos")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let new_todos = json!(input.todos);

        context
            .app_state
            .insert("todos".to_string(), new_todos.clone());

        Ok(ToolOutput {
            data: json!({
                "oldTodos": old_todos,
                "newTodos": new_todos,
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, _output: &ToolOutput, tool_use_id: &str) -> Value {
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": "Todos have been modified successfully. Continue using the todo list to track progress."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TodoWriteTool;
    use crate::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn writes_todos_into_tool_context_state() {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let tool = TodoWriteTool::new();
        let input = json!({
            "todos": [
                {
                    "content": "Make workspace compile",
                    "status": "completed",
                    "priority": "high"
                }
            ]
        });

        let validation = tool.validate_input(&input, &context).await;
        assert!(validation.result, "{:?}", validation.message);

        let output = tool.call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["oldTodos"], json!([]));
        assert_eq!(
            context.app_state["todos"][0]["content"],
            "Make workspace compile"
        );
    }
}
