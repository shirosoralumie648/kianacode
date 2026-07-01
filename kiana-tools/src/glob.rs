use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;

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
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern"
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
                    "description": "The glob pattern to match files against (e.g. '**/*.rs', 'src/**/*.txt')"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in. Defaults to current directory."
                }
            },
            "required": ["pattern"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "numFiles": { "type": "number" },
                "filenames": { "type": "array", "items": { "type": "string" } },
                "truncated": { "type": "boolean" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: GlobInput = match serde_json::from_value(input.clone()) {
            Ok(i) => i,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        if let Some(path) = &input.path {
            let p = std::path::Path::new(path);
            if !p.exists() {
                return ValidationResult::err(format!("Directory does not exist: {}", path), 2);
            }
            if !p.is_dir() {
                return ValidationResult::err(format!("Path is not a directory: {}", path), 3);
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, _context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: GlobInput = serde_json::from_value(input.clone())?;
        let search_path = input.path.unwrap_or_else(|| ".".to_string());

        // Use ripgrep with --files and --glob to match files
        let output = Command::new("rg")
            .args(&[
                "--files",
                "--hidden",
                "--glob",
                &input.pattern,
                &search_path,
            ])
            .output()
            .await
            .map_err(|e| ToolError::Other(format!("Failed to execute ripgrep: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();

        let limit = 100;
        let truncated = files.len() > limit;
        let filenames: Vec<String> = files.into_iter().take(limit).collect();

        Ok(ToolOutput {
            data: json!({
                "numFiles": filenames.len(),
                "filenames": filenames,
                "truncated": truncated
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let num_files = output
            .data
            .get("numFiles")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let truncated = output
            .data
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if num_files == 0 {
            return json!({
                "tool_use_id": tool_use_id,
                "type": "tool_result",
                "content": "No files found"
            });
        }

        let filenames = output
            .data
            .get("filenames")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let mut content = filenames;
        if truncated {
            content.push_str(
                "\n(Results are truncated. Consider using a more specific path or pattern.)",
            );
        }

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}
