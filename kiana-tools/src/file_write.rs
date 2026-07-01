use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Map;
use serde_json::{json, Value};
use tokio::fs;

#[derive(Debug, Deserialize)]
struct FileWriteInput {
    file_path: String,
    content: String,
}

pub struct FileWriteTool;

impl FileWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        "Write a file to the local filesystem."
    }

    fn search_hint(&self) -> Option<&str> {
        Some("create or overwrite files")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["create", "update"] },
                "file_path": { "type": "string" },
                "content": { "type": "string" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: FileWriteInput = match serde_json::from_value(input.clone()) {
            Ok(i) => i,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        let path = match context.resolve_access_path(&input.file_path) {
            Ok(path) => path,
            Err(error) => return ValidationResult::err(error, 9),
        };
        if let Err(error) = context.ensure_file_editable(&path) {
            return ValidationResult::err(error, 10);
        }

        // If file exists, ensure it was previously read
        if path.exists() {
            let file_state = context.read_file_state.get(&input.file_path);
            if file_state.is_none() {
                return ValidationResult::err(
                    "File has not been read yet. Read it first before writing to it.".to_string(),
                    2,
                );
            }

            // Check mtime hasn't changed since last read
            if let Ok(metadata) = std::fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        let mtime = duration.as_secs() as i64;
                        if let Some(state) = file_state {
                            if mtime > state.timestamp {
                                return ValidationResult::err(
                                    "File has been modified since read. Read it again before writing.".to_string(),
                                    3
                                );
                            }
                        }
                    }
                }
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: FileWriteInput = serde_json::from_value(input.clone())?;
        let path = context
            .resolve_access_path(&input.file_path)
            .map_err(ToolError::PermissionDenied)?;
        context
            .ensure_file_editable(&path)
            .map_err(ToolError::PermissionDenied)?;

        let is_new_file = !path.exists();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let old_content = if !is_new_file {
            Some(fs::read_to_string(&path).await.unwrap_or_default())
        } else {
            None
        };

        fs::write(&path, &input.content).await?;

        // Update read file state
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        context.read_file_state.insert(
            input.file_path.clone(),
            FileState {
                content: input.content.clone(),
                timestamp,
                offset: None,
                limit: None,
            },
        );

        let op_type = if is_new_file { "create" } else { "update" };
        let mut metadata = Map::new();
        if let Err(error) =
            crate::lsp_tool::sync_lsp_file_saved(context, &path, &input.content).await
        {
            metadata.insert("lsp_warning".to_string(), json!(error.to_string()));
        }

        Ok(ToolOutput {
            data: json!({
                "type": op_type,
                "filePath": input.file_path,
                "content": input.content,
                "originalFile": old_content
            }),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata.into_iter().collect())
            },
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let op_type = output
            .data
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("create");
        let file_path = output
            .data
            .get("filePath")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let content = if op_type == "create" {
            format!("File created successfully at: {}", file_path)
        } else {
            format!("The file {} has been updated successfully.", file_path)
        };

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FileWriteTool;
    use crate::tool::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn write_rejects_files_marked_read_only() {
        let root = std::env::temp_dir().join(format!("kiana-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("locked.txt");
        std::fs::write(&path, "locked").unwrap();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([("read_only_files".to_string(), json!(["locked.txt"]))]),
            abort_signal: abort_rx,
        };

        let result = FileWriteTool::new()
            .validate_input(
                &json!({
                    "file_path": path.to_string_lossy(),
                    "content": "changed"
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("read-only"));

        let _ = std::fs::remove_dir_all(root);
    }
}
