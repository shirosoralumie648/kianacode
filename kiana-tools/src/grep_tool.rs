use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

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
    #[serde(rename = "-n", default = "default_true")]
    show_line_numbers: bool,
    #[serde(default)]
    head_limit: Option<usize>,
}

fn default_output_mode() -> String { "files_with_matches".to_string() }
fn default_true() -> bool { true }

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }

    async fn search(
        &self,
        input: &GrepInput,
        context: &ToolContext,
    ) -> ToolResult<(Vec<String>, usize)> {
        let path = match input.path.as_deref() {
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

        if !path.exists() {
            return Err(ToolError::Other(format!(
                "Path does not exist: {}",
                path.display()
            )));
        }

        let regex = if input.case_insensitive {
            regex::RegexBuilder::new(&input.pattern)
                .case_insensitive(true)
                .build()
                .map_err(|e| ToolError::Other(format!("Invalid regex: {}", e)))?
        } else {
            regex::Regex::new(&input.pattern)
                .map_err(|e| ToolError::Other(format!("Invalid regex: {}", e)))?
        };

        let mut matches = Vec::new();
        let mut total_matches = 0;

        let walker = walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Skip hidden directories and common excludes
                if let Some(name) = e.file_name().to_str() {
                    !name.starts_with('.')
                        && name != "node_modules"
                        && name != "target"
                        && name != "dist"
                } else {
                    true
                }
            });

        for entry in walker {
            let entry = entry.map_err(|e| ToolError::Other(e.to_string()))?;

            if !entry.file_type().is_file() {
                continue;
            }

            // Apply glob filter if specified
            if let Some(glob_pattern) = &input.glob {
                if let Some(name) = entry.file_name().to_str() {
                    if !glob::Pattern::new(glob_pattern)
                        .map_err(|e| ToolError::Other(e.to_string()))?
                        .matches(name)
                    {
                        continue;
                    }
                }
            }

            let path = entry.path();
            if context.ensure_path_access(path).is_err() {
                continue;
            }

            if let Ok(content) = tokio::fs::read_to_string(path).await {
                let file_path = path.display().to_string();
                let mut file_matched = false;

                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        total_matches += 1;

                        match input.output_mode.as_str() {
                            "content" => {
                                let line_str = if input.show_line_numbers {
                                    format!("{}:{}:{}", file_path, line_num + 1, line)
                                } else {
                                    format!("{}:{}", file_path, line)
                                };
                                matches.push(line_str);
                            }
                            "count" => {
                                if !file_matched {
                                    file_matched = true;
                                }
                            }
                            _ => { // files_with_matches
                                if !file_matched {
                                    matches.push(file_path.clone());
                                    file_matched = true;
                                    break;
                                }
                            }
                        }

                        // Apply head_limit
                        if let Some(limit) = input.head_limit {
                            if matches.len() >= limit {
                                return Ok((matches, total_matches));
                            }
                        }
                    }
                }

                if input.output_mode == "count" && file_matched {
                    let count = content.lines().filter(|l| regex.is_match(l)).count();
                    matches.push(format!("{}:{}", file_path, count));
                }
            }
        }

        Ok((matches, total_matches))
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents with regex (ripgrep)"
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
                    "description": "File or directory to search in"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.js\")"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "default": "files_with_matches"
                },
                "-i": {
                    "type": "boolean",
                    "description": "Case insensitive search"
                },
                "-n": {
                    "type": "boolean",
                    "description": "Show line numbers"
                },
                "head_limit": {
                    "type": "integer",
                    "description": "Limit output to first N entries"
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
                "numFiles": { "type": "integer" },
                "filenames": { "type": "array", "items": { "type": "string" } },
                "content": { "type": "string" },
                "numMatches": { "type": "integer" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: GrepInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        if let Some(path) = input.path.as_deref() {
            match context.resolve_access_path(path) {
                Ok(path) => {
                    if !path.exists() {
                        return ValidationResult::err(
                            format!("Path does not exist: {}", path.display()),
                            2,
                        );
                    }
                }
                Err(error) => return ValidationResult::err(error, 9),
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: GrepInput = serde_json::from_value(input.clone())?;
        let (matches, total_matches) = self.search(&input, context).await?;

        let output = match input.output_mode.as_str() {
            "content" => json!({
                "mode": "content",
                "numFiles": 0,
                "filenames": [],
                "content": matches.join("\n"),
                "numLines": matches.len()
            }),
            "count" => json!({
                "mode": "count",
                "numFiles": matches.len(),
                "filenames": [],
                "content": matches.join("\n"),
                "numMatches": total_matches
            }),
            _ => json!({
                "mode": "files_with_matches",
                "numFiles": matches.len(),
                "filenames": matches,
            }),
        };

        Ok(ToolOutput {
            data: output,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let mode = output.data.get("mode").and_then(|v| v.as_str()).unwrap_or("files_with_matches");
        let num_files = output.data.get("numFiles").and_then(|v| v.as_u64()).unwrap_or(0);

        let content = match mode {
            "content" => {
                output.data.get("content").and_then(|v| v.as_str()).unwrap_or("No matches found").to_string()
            }
            "count" => {
                let num_matches = output.data.get("numMatches").and_then(|v| v.as_u64()).unwrap_or(0);
                let raw = output.data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                format!("{}\n\nFound {} total occurrences across {} files.", raw, num_matches, num_files)
            }
            _ => {
                if num_files == 0 {
                    "No files found".to_string()
                } else {
                    let filenames = output.data.get("filenames")
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
