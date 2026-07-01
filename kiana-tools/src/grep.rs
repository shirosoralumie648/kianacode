use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct GrepInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default = "default_output_mode")]
    output_mode: String,
    #[serde(rename = "-i", default)]
    case_insensitive: bool,
}

fn default_output_mode() -> String {
    "files_with_matches".to_string()
}

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search for patterns in files using ripgrep"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("search file contents with regex (ripgrep)")
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
                    "description": "The regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in. Defaults to current directory."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. '*.rs', '*.{ts,tsx}')"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Output mode: content (matching lines), files_with_matches (file paths), count (match counts)"
                },
                "-i": {
                    "type": "boolean",
                    "description": "Case insensitive search"
                }
            },
            "required": ["pattern"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string" },
                "numFiles": { "type": "number" },
                "filenames": { "type": "array", "items": { "type": "string" } },
                "content": { "type": "string" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: GrepInput = match serde_json::from_value(input.clone()) {
            Ok(i) => i,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        if let Some(path) = &input.path {
            if !std::path::Path::new(path).exists() {
                return ValidationResult::err(format!("Path does not exist: {}", path), 2);
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, _context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: GrepInput = serde_json::from_value(input.clone())?;
        let path = input.path.unwrap_or_else(|| ".".to_string());

        let mut args = vec!["--hidden", "--max-columns", "500"];
        let vcs_globs = vec!["!.git".to_string(), "!.svn".to_string(), "!.hg".to_string()];

        // Exclude VCS directories
        for glob in &vcs_globs {
            args.push("--glob");
            args.push(glob);
        }

        if input.case_insensitive {
            args.push("-i");
        }

        match input.output_mode.as_str() {
            "files_with_matches" => args.push("-l"),
            "count" => args.push("-c"),
            _ => {}
        }

        args.push(&input.pattern);

        if let Some(glob) = &input.glob {
            args.push("--glob");
            args.push(glob);
        }

        args.push(&path);

        let output = Command::new("rg")
            .args(&args)
            .output()
            .await
            .map_err(|e| ToolError::Other(format!("Failed to execute ripgrep: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        let result = match input.output_mode.as_str() {
            "content" => json!({
                "mode": "content",
                "numFiles": 0,
                "filenames": [],
                "content": stdout.trim()
            }),
            "count" => {
                let total_matches: usize = lines
                    .iter()
                    .filter_map(|line| line.split(':').last()?.parse::<usize>().ok())
                    .sum();
                json!({
                    "mode": "count",
                    "numFiles": lines.len(),
                    "filenames": [],
                    "content": stdout.trim(),
                    "numMatches": total_matches
                })
            }
            _ => json!({
                "mode": "files_with_matches",
                "numFiles": lines.len(),
                "filenames": lines
            }),
        };

        Ok(ToolOutput {
            data: result,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let mode = output
            .data
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("files_with_matches");
        let num_files = output
            .data
            .get("numFiles")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let content = match mode {
            "content" => output
                .data
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("No matches found")
                .to_string(),
            "count" => {
                let num_matches = output
                    .data
                    .get("numMatches")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                format!(
                    "Found {} occurrences across {} files.",
                    num_matches, num_files
                )
            }
            _ => {
                if num_files == 0 {
                    "No files found".to_string()
                } else {
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
                    format!("Found {} files\n{}", num_files, filenames)
                }
            }
        };

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}
