use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::fs;

#[derive(Debug, Deserialize)]
struct FileDeleteInput {
    file_path: String,
}

pub struct FileDeleteTool;

impl FileDeleteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileDeleteTool {
    fn name(&self) -> &str {
        "Delete"
    }

    fn description(&self) -> &str {
        "Delete a file from the local filesystem."
    }

    fn search_hint(&self) -> Option<&str> {
        Some("remove files")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to delete"
                }
            },
            "required": ["file_path"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filePath": { "type": "string" },
                "originalFile": { "type": "string" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: FileDeleteInput = match serde_json::from_value(input.clone()) {
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

        if !path.exists() {
            return ValidationResult::err(format!("File does not exist: {}", input.file_path), 2);
        }
        if !path.is_file() {
            return ValidationResult::err(format!("Path is not a file: {}", input.file_path), 3);
        }

        let file_state = context.read_file_state.get(&input.file_path);
        if file_state.is_none() {
            return ValidationResult::err(
                "File has not been read yet. Read it first before deleting it.".to_string(),
                4,
            );
        }

        if let Ok(metadata) = std::fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    let mtime = duration.as_secs() as i64;
                    if let Some(state) = file_state {
                        if mtime > state.timestamp {
                            return ValidationResult::err(
                                "File has been modified since read. Read it again before deleting."
                                    .to_string(),
                                5,
                            );
                        }
                    }
                }
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: FileDeleteInput = serde_json::from_value(input.clone())?;
        let path = context
            .resolve_access_path(&input.file_path)
            .map_err(ToolError::PermissionDenied)?;
        context
            .ensure_file_editable(&path)
            .map_err(ToolError::PermissionDenied)?;

        if !path.exists() {
            return Err(ToolError::ValidationError(format!(
                "File does not exist: {}",
                input.file_path
            )));
        }
        if !path.is_file() {
            return Err(ToolError::ValidationError(format!(
                "Path is not a file: {}",
                input.file_path
            )));
        }

        let original_content = fs::read_to_string(&path).await.unwrap_or_default();
        fs::remove_file(&path).await?;
        context.read_file_state.remove(&input.file_path);

        Ok(ToolOutput {
            data: json!({
                "filePath": input.file_path,
                "originalFile": original_content
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let file_path = output
            .data
            .get("filePath")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format!("The file {} has been deleted successfully.", file_path),
            "changed_files": [{
                "path": file_path,
                "operation": "delete",
                "source": "Delete"
            }]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FileDeleteTool;
    use crate::tool::{FileState, Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_context(cwd: String) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd,
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        }
    }

    #[tokio::test]
    async fn delete_api_result_reports_changed_file_metadata() {
        let output = crate::ToolOutput {
            data: json!({
                "filePath": "src/lib.rs",
                "originalFile": "old"
            }),
            metadata: None,
        };

        let result = FileDeleteTool::new().map_to_api_result(&output, "toolu_delete");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["changed_files"][0]["path"], "src/lib.rs");
        assert_eq!(result["changed_files"][0]["operation"], "delete");
        assert_eq!(result["changed_files"][0]["source"], "Delete");
    }

    #[tokio::test]
    async fn delete_requires_file_to_be_read_first() {
        let root = std::env::temp_dir().join(format!("kiana-delete-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("note.txt");
        fs::write(&path, "delete me\n").unwrap();
        let context = test_context(root.to_string_lossy().to_string());

        let result = FileDeleteTool::new()
            .validate_input(&json!({ "file_path": path.to_string_lossy() }), &context)
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("not been read"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn delete_rejects_files_not_listed_as_editable() {
        let root = std::env::temp_dir().join(format!("kiana-delete-{}", uuid::Uuid::new_v4()));
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
            blocked.to_string_lossy().to_string(),
            FileState {
                content: "blocked\n".to_string(),
                timestamp,
                offset: None,
                limit: None,
            },
        );

        let result = FileDeleteTool::new()
            .validate_input(&json!({ "file_path": blocked.to_string_lossy() }), &context)
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("editable"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn delete_removes_file_and_clears_read_state() {
        let root = std::env::temp_dir().join(format!("kiana-delete-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("note.txt");
        fs::write(&path, "delete me\n").unwrap();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 60;
        let mut context = test_context(root.to_string_lossy().to_string());
        context.read_file_state.insert(
            path.to_string_lossy().to_string(),
            FileState {
                content: "delete me\n".to_string(),
                timestamp,
                offset: None,
                limit: None,
            },
        );

        let output = FileDeleteTool::new()
            .call(
                &json!({ "file_path": path.to_string_lossy() }),
                &mut context,
            )
            .await
            .unwrap();

        assert!(!path.exists());
        assert!(context
            .read_file_state
            .get(&path.to_string_lossy().to_string())
            .is_none());
        assert_eq!(output.data["filePath"], path.to_string_lossy().to_string());
        assert_eq!(output.data["originalFile"], "delete me\n");

        let _ = fs::remove_dir_all(root);
    }
}
