use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct NotebookEditInput {
    path: String,
    cell_index: usize,
    source: String,
}

pub struct NotebookEditTool;

impl NotebookEditTool {
    pub fn new() -> Self {
        Self
    }
}

fn validate_notebook_edit_request(
    input: &NotebookEditInput,
    context: &ToolContext,
) -> Result<PathBuf, ValidationResult> {
    if input.path.trim().is_empty() {
        return Err(ValidationResult::err("path cannot be empty".to_string(), 2));
    }
    if !is_ipynb_path(&input.path) {
        return Err(ValidationResult::err(
            "path must point to a .ipynb notebook".to_string(),
            3,
        ));
    }
    let path = context
        .resolve_access_path(&input.path)
        .map_err(|error| ValidationResult::err(error, 9))?;
    if let Err(error) = context.ensure_file_editable(&path) {
        return Err(ValidationResult::err(error, 10));
    }
    if !path.exists() {
        return Err(ValidationResult::err(
            format!("Notebook does not exist: {}", input.path),
            4,
        ));
    }
    if !path.is_file() {
        return Err(ValidationResult::err(
            format!("Path is not a file: {}", input.path),
            5,
        ));
    }

    let file_state = context.read_file_state.get(&input.path);
    if file_state.is_none() {
        return Err(ValidationResult::err(
            "Notebook has not been read yet. Read it first before editing it.".to_string(),
            6,
        ));
    }

    if let Ok(metadata) = fs::metadata(&path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                let mtime = duration.as_secs() as i64;
                if let Some(state) = file_state {
                    if mtime > state.timestamp {
                        return Err(ValidationResult::err(
                            "Notebook has been modified since read. Read it again before editing."
                                .to_string(),
                            7,
                        ));
                    }
                }
            }
        }
    }

    Ok(path)
}

fn is_ipynb_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or(false, |extension| extension.eq_ignore_ascii_case("ipynb"))
}

fn validation_error(result: ValidationResult) -> ToolError {
    ToolError::ValidationError(
        result
            .message
            .unwrap_or_else(|| "notebook edit validation failed".to_string()),
    )
}

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "NotebookEdit"
    }

    fn description(&self) -> &str {
        "Edit the source of a Jupyter notebook cell"
    }

    fn workbench(&self) -> Option<&str> {
        Some("notebook")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "cell_index": { "type": "integer", "minimum": 0 },
                "source": { "type": "string" }
            },
            "required": ["path", "cell_index", "source"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "cell_index": { "type": "integer" },
                "cell": { "type": "object" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: NotebookEditInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        match validate_notebook_edit_request(&input, context) {
            Ok(_) => ValidationResult::ok(),
            Err(result) => result,
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: NotebookEditInput = serde_json::from_value(input.clone())?;
        let path = validate_notebook_edit_request(&input, context).map_err(validation_error)?;
        let contents = fs::read_to_string(&path)?;
        let mut notebook: Value = serde_json::from_str(&contents)?;
        let cells = notebook
            .get_mut("cells")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| ToolError::Other("notebook is missing cells array".to_string()))?;
        let cell = cells
            .get_mut(input.cell_index)
            .ok_or_else(|| ToolError::Other(format!("cell {} not found", input.cell_index)))?;
        let Some(cell_object) = cell.as_object_mut() else {
            return Err(ToolError::Other(format!(
                "cell {} is not an object",
                input.cell_index
            )));
        };
        cell_object.insert(
            "source".to_string(),
            source_to_notebook_lines(&input.source),
        );
        let updated_cell = Value::Object(cell_object.clone());

        let updated_content = serde_json::to_string_pretty(&notebook)?;
        fs::write(&path, &updated_content)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        context.read_file_state.insert(
            input.path.clone(),
            FileState {
                content: updated_content,
                timestamp,
                offset: None,
                limit: None,
            },
        );

        Ok(ToolOutput {
            data: json!({
                "path": path.to_string_lossy(),
                "cell_index": input.cell_index,
                "cell": updated_cell
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let path = output
            .data
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("unknown notebook");
        let cell_index = output
            .data
            .get("cell_index")
            .and_then(Value::as_u64)
            .map(|index| index.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let cell_type = output
            .data
            .get("cell")
            .and_then(|cell| cell.get("cell_type"))
            .and_then(Value::as_str)
            .unwrap_or("cell");

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format!(
                "Notebook updated successfully: {path}\nCell index: {cell_index}\nCell type: {cell_type}"
            ),
            "changed_files": [{
                "path": path,
                "operation": "edit",
                "source": "NotebookEdit"
            }]
        })
    }
}

fn source_to_notebook_lines(source: &str) -> Value {
    let mut lines = source
        .split_inclusive('\n')
        .map(|line| Value::String(line.to_string()))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(Value::String(String::new()));
    }
    Value::Array(lines)
}

#[cfg(test)]
mod tests {
    use super::NotebookEditTool;
    use crate::tool::FileState;
    use crate::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn fresh_read_state(content: String) -> FileState {
        FileState {
            content,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 60,
            offset: None,
            limit: None,
        }
    }

    fn notebook_json(source: &str) -> String {
        serde_json::to_string_pretty(&json!({
            "cells": [
                {
                    "cell_type": "code",
                    "source": [source],
                    "metadata": {},
                    "outputs": []
                }
            ],
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn edits_notebook_cell_source() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let notebook_path = root.join("test.ipynb");
        let original = notebook_json("print('old')\n");
        fs::write(&notebook_path, &original).unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::from([(
                "test.ipynb".to_string(),
                fresh_read_state(original.clone()),
            )]),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let output = NotebookEditTool::new()
            .call(
                &json!({
                    "path": "test.ipynb",
                    "cell_index": 0,
                    "source": "print('new')\n"
                }),
                &mut context,
            )
            .await
            .unwrap();

        let notebook: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&notebook_path).unwrap()).unwrap();
        assert_eq!(notebook["cells"][0]["source"][0], "print('new')\n");

        let api_result = NotebookEditTool::new().map_to_api_result(&output, "toolu_notebook");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_notebook");
        assert_eq!(api_result["changed_files"][0]["operation"], "edit");
        assert_eq!(api_result["changed_files"][0]["source"], "NotebookEdit");
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("Notebook updated successfully:"));
        assert!(content.contains("test.ipynb"));
        assert!(content.contains("Cell index: 0"));
        assert!(content.contains("Cell type: code"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn notebook_edit_requires_read_state_before_editing() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test.ipynb"), notebook_json("print('old')\n")).unwrap();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let result = NotebookEditTool::new()
            .validate_input(
                &json!({
                    "path": "test.ipynb",
                    "cell_index": 0,
                    "source": "print('new')\n"
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("not been read"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_edit_accepts_ipynb_extension_case_insensitively() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let original = notebook_json("print('old')\n");
        fs::write(root.join("CASE.IPYNB"), &original).unwrap();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::from([(
                "CASE.IPYNB".to_string(),
                fresh_read_state(original),
            )]),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let result = NotebookEditTool::new()
            .validate_input(
                &json!({
                    "path": "CASE.IPYNB",
                    "cell_index": 0,
                    "source": "print('new')\n"
                }),
                &context,
            )
            .await;

        assert!(result.result);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_edit_rejects_files_marked_read_only() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let original = notebook_json("print('old')\n");
        fs::write(root.join("locked.ipynb"), &original).unwrap();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::from([(
                "locked.ipynb".to_string(),
                fresh_read_state(original),
            )]),
            app_state: HashMap::from([("read_only_files".to_string(), json!(["locked.ipynb"]))]),
            abort_signal: abort_rx,
        };

        let result = NotebookEditTool::new()
            .validate_input(
                &json!({
                    "path": "locked.ipynb",
                    "cell_index": 0,
                    "source": "print('new')\n"
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("read-only"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_edit_rejects_files_not_listed_as_editable() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let original = notebook_json("print('old')\n");
        fs::write(root.join("blocked.ipynb"), &original).unwrap();
        fs::write(
            root.join("allowed.ipynb"),
            notebook_json("print('allowed')\n"),
        )
        .unwrap();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::from([(
                "blocked.ipynb".to_string(),
                fresh_read_state(original),
            )]),
            app_state: HashMap::from([("editable_files".to_string(), json!(["allowed.ipynb"]))]),
            abort_signal: abort_rx,
        };

        let result = NotebookEditTool::new()
            .validate_input(
                &json!({
                    "path": "blocked.ipynb",
                    "cell_index": 0,
                    "source": "print('new')\n"
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("editable"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_edit_rejects_notebooks_modified_after_read() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let original = notebook_json("print('old')\n");
        fs::write(root.join("test.ipynb"), &original).unwrap();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::from([(
                "test.ipynb".to_string(),
                FileState {
                    content: original,
                    timestamp: 0,
                    offset: None,
                    limit: None,
                },
            )]),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };

        let result = NotebookEditTool::new()
            .validate_input(
                &json!({
                    "path": "test.ipynb",
                    "cell_index": 0,
                    "source": "print('new')\n"
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("modified since read"));

        let _ = fs::remove_dir_all(root);
    }
}
