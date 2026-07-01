use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct GlobInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self {
        Self
    }

    async fn glob_search(
        &self,
        pattern: &str,
        base_path: &Path,
        context: &ToolContext,
    ) -> ToolResult<Vec<String>> {
        let full_pattern = if Path::new(pattern).is_absolute() {
            pattern.to_string()
        } else {
            base_path.join(pattern).to_string_lossy().to_string()
        };

        let mut results = Vec::new();

        for entry in glob::glob(&full_pattern)
            .map_err(|e| ToolError::Other(format!("Invalid glob pattern: {}", e)))?
        {
            match entry {
                Ok(path) => {
                    if path.is_file() && context.ensure_path_access(&path).is_ok() {
                        results.push(path.display().to_string());
                    }
                }
                Err(e) => {
                    // Log but continue on individual errors
                    eprintln!("Glob entry error: {}", e);
                }
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Find files by name pattern or wildcard"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("find files by name pattern or wildcard")
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
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match files against"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in"
                }
            },
            "required": ["pattern"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "numFiles": { "type": "integer" },
                "filenames": { "type": "array", "items": { "type": "string" } },
                "truncated": { "type": "boolean" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: GlobInput = match serde_json::from_value(input.clone()) {
            Ok(i) => i,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        if let Some(path) = &input.path {
            let p = match context.resolve_access_path(path) {
                Ok(path) => path,
                Err(error) => return ValidationResult::err(error, 9),
            };
            if !p.exists() {
                return ValidationResult::err(
                    format!("Directory does not exist: {}", path),
                    2
                );
            }
            if !p.is_dir() {
                return ValidationResult::err(
                    format!("Path is not a directory: {}", path),
                    3
                );
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: GlobInput = serde_json::from_value(input.clone())?;
        let base_path = match input.path.as_deref() {
            Some(path) => context
                .resolve_access_path(path)
                .map_err(ToolError::PermissionDenied)?,
            None => {
                let path = PathBuf::from(&context.cwd);
                context
                    .ensure_path_access(&path)
                    .map_err(ToolError::PermissionDenied)?;
                path
            }
        };

        let mut filenames = self
            .glob_search(&input.pattern, &base_path, context)
            .await?;

        // Limit to 100 files
        let truncated = filenames.len() > 100;
        if truncated {
            filenames.truncate(100);
        }

        Ok(ToolOutput {
            data: json!({
                "numFiles": filenames.len(),
                "filenames": filenames,
                "truncated": truncated,
                "durationMs": 0
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let filenames = output.data.get("filenames")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let truncated = output.data.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);

        if filenames.is_empty() {
            return json!({
                "tool_use_id": tool_use_id,
                "type": "tool_result",
                "content": "No files found"
            });
        }

        let mut lines = filenames.iter().map(|s| s.as_ref() as &str).collect::<Vec<_>>();
        if truncated {
            lines.push("(Results are truncated. Consider using a more specific path or pattern.)");
        }

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": lines.join("\n")
        })
    }
}
