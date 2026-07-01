use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Map;
use serde_json::{json, Value};
use tokio::fs;

#[derive(Debug, Deserialize)]
struct FileEditInput {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

pub struct FileEditTool;

impl FileEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "A tool for editing files"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("modify file contents in place")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filePath": { "type": "string" },
                "oldString": { "type": "string" },
                "newString": { "type": "string" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: FileEditInput = match serde_json::from_value(input.clone()) {
            Ok(i) => i,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        if input.old_string == input.new_string {
            return ValidationResult::err(
                "No changes to make: old_string and new_string are exactly the same.".to_string(),
                2,
            );
        }

        let path = match context.resolve_access_path(&input.file_path) {
            Ok(path) => path,
            Err(error) => return ValidationResult::err(error, 9),
        };
        if let Err(error) = context.ensure_file_editable(&path) {
            return ValidationResult::err(error, 10);
        }

        // File must exist (unless old_string is empty = new file creation)
        if !path.exists() {
            if input.old_string.is_empty() {
                return ValidationResult::ok();
            }
            return ValidationResult::err(format!("File does not exist: {}", input.file_path), 3);
        }

        // Check file was read before editing
        if !input.old_string.is_empty() {
            if context.read_file_state.get(&input.file_path).is_none() {
                return ValidationResult::err(
                    "File has not been read yet. Read it first before writing to it.".to_string(),
                    4,
                );
            }

            // Check mtime
            if let Ok(metadata) = std::fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        let mtime = duration.as_secs() as i64;
                        if let Some(state) = context.read_file_state.get(&input.file_path) {
                            if mtime > state.timestamp {
                                return ValidationResult::err(
                                    "File has been modified since read. Read it again before editing.".to_string(),
                                    5
                                );
                            }
                        }
                    }
                }
            }

            // Validate old_string exists in file
            if let Ok(content) = std::fs::read_to_string(&path) {
                let content = content.replace("\r\n", "\n");
                if !content.contains(&input.old_string) {
                    return ValidationResult::err(
                        format!(
                            "String to replace not found in file.\nString: {}",
                            input.old_string
                        ),
                        6,
                    );
                }

                // Multiple matches with replace_all=false
                let matches = content.matches(&input.old_string).count();
                if matches > 1 && !input.replace_all {
                    return ValidationResult::err(
                        format!(
                            "Found {} matches of the string to replace, but replace_all is false.",
                            matches
                        ),
                        7,
                    );
                }
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: FileEditInput = serde_json::from_value(input.clone())?;
        let path = context
            .resolve_access_path(&input.file_path)
            .map_err(ToolError::PermissionDenied)?;
        context
            .ensure_file_editable(&path)
            .map_err(ToolError::PermissionDenied)?;

        // Create parent directory if needed (new file creation)
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let original_content = if path.exists() {
            let content = fs::read_to_string(&path).await?;
            content.replace("\r\n", "\n")
        } else {
            String::new()
        };

        let updated_content = if input.replace_all {
            original_content.replace(&input.old_string, &input.new_string)
        } else {
            original_content.replacen(&input.old_string, &input.new_string, 1)
        };

        fs::write(&path, &updated_content).await?;

        // Update file state cache
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        context.read_file_state.insert(
            input.file_path.clone(),
            FileState {
                content: updated_content.clone(),
                timestamp,
                offset: None,
                limit: None,
            },
        );
        let mut metadata = Map::new();
        if let Err(error) =
            crate::lsp_tool::sync_lsp_file_saved(context, &path, &updated_content).await
        {
            metadata.insert("lsp_warning".to_string(), json!(error.to_string()));
        }

        Ok(ToolOutput {
            data: json!({
                "filePath": input.file_path,
                "oldString": input.old_string,
                "newString": input.new_string,
                "replaceAll": input.replace_all,
                "originalFile": original_content,
                "updatedContent": updated_content
            }),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata.into_iter().collect())
            },
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let file_path = output
            .data
            .get("filePath")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let replace_all = output
            .data
            .get("replaceAll")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = if replace_all {
            format!(
                "The file {} has been updated. All occurrences were successfully replaced.",
                file_path
            )
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
    use super::FileEditTool;
    use crate::tool::{FileState, Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn edit_rejects_files_not_listed_as_editable() {
        let root = std::env::temp_dir().join(format!("kiana-edit-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let blocked = root.join("blocked.txt");
        fs::write(&blocked, "blocked\n").unwrap();
        let allowed = root.join("allowed.txt");
        fs::write(&allowed, "allowed\n").unwrap();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 60;
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([("editable_files".to_string(), json!(["allowed.txt"]))]),
            abort_signal: abort_rx,
        };
        context.read_file_state.insert(
            "blocked.txt".to_string(),
            FileState {
                content: "blocked\n".to_string(),
                timestamp,
                offset: None,
                limit: None,
            },
        );

        let result = FileEditTool::new()
            .validate_input(
                &json!({
                    "file_path": blocked.to_string_lossy(),
                    "old_string": "blocked",
                    "new_string": "changed"
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("editable"));

        let _ = fs::remove_dir_all(root);
    }
}
