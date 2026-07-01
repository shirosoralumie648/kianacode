use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;

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
        if input.path.trim().is_empty() {
            return ValidationResult::err("path cannot be empty".to_string(), 2);
        }
        if let Err(error) = context.resolve_access_path(&input.path) {
            return ValidationResult::err(error, 9);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: NotebookEditInput = serde_json::from_value(input.clone())?;
        if input.path.trim().is_empty() {
            return Err(ToolError::ValidationError(
                "path cannot be empty".to_string(),
            ));
        }

        let path = context
            .resolve_access_path(&input.path)
            .map_err(ToolError::PermissionDenied)?;
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

        fs::write(&path, serde_json::to_string_pretty(&notebook)?)?;

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
            )
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
    use crate::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use uuid::Uuid;

    #[tokio::test]
    async fn edits_notebook_cell_source() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let notebook_path = root.join("test.ipynb");
        fs::write(
            &notebook_path,
            serde_json::to_string_pretty(&json!({
                "cells": [
                    {
                        "cell_type": "code",
                        "source": ["print('old')\n"],
                        "metadata": {},
                        "outputs": []
                    }
                ],
                "metadata": {},
                "nbformat": 4,
                "nbformat_minor": 5
            }))
            .unwrap(),
        )
        .unwrap();

        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
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
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("Notebook updated successfully:"));
        assert!(content.contains("test.ipynb"));
        assert!(content.contains("Cell index: 0"));
        assert!(content.contains("Cell type: code"));

        let _ = fs::remove_dir_all(&root);
    }
}
