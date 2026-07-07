use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 200_000;

#[derive(Debug, Deserialize)]
struct NotebookExecuteInput {
    path: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_output_bytes: Option<usize>,
    #[serde(default)]
    stop_on_error: Option<bool>,
}

pub struct NotebookExecuteTool;

impl NotebookExecuteTool {
    pub fn new() -> Self {
        Self
    }
}

fn validate_notebook_execute_request(
    input: &NotebookExecuteInput,
    context: &ToolContext,
) -> Result<PathBuf, ValidationResult> {
    if !notebook_execution_allowed(context) {
        return Err(ValidationResult::err(
            "Notebook execution is disabled. Set notebook_execution_allowed=true or KIANA_NOTEBOOK_EXECUTION_ALLOWED=1 to enable it for this run.".to_string(),
            8,
        ));
    }
    if input.path.trim().is_empty() {
        return Err(ValidationResult::err("path cannot be empty".to_string(), 2));
    }
    if !is_ipynb_path(&input.path) {
        return Err(ValidationResult::err(
            "path must point to a .ipynb notebook".to_string(),
            3,
        ));
    }
    if input
        .timeout_ms
        .is_some_and(|timeout_ms| timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS)
    {
        return Err(ValidationResult::err(
            format!("timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"),
            6,
        ));
    }
    if input
        .max_output_bytes
        .is_some_and(|max_output_bytes| max_output_bytes < 1024)
    {
        return Err(ValidationResult::err(
            "max_output_bytes must be at least 1024".to_string(),
            7,
        ));
    }
    let path = context
        .resolve_access_path(&input.path)
        .map_err(|error| ValidationResult::err(error, 9))?;
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
    Ok(path)
}

fn notebook_execution_allowed(context: &ToolContext) -> bool {
    bool_app_state(&context.app_state, "notebook_execution_allowed")
        || bool_app_state(&context.app_state, "notebookExecutionAllowed")
        || std::env::var("KIANA_NOTEBOOK_EXECUTION_ALLOWED")
            .ok()
            .is_some_and(|value| truthy_flag(&value))
}

fn bool_app_state(app_state: &std::collections::HashMap<String, Value>, key: &str) -> bool {
    app_state.get(key).is_some_and(|value| match value {
        Value::Bool(value) => *value,
        Value::String(value) => truthy_flag(value),
        _ => false,
    })
}

fn truthy_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
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
            .unwrap_or_else(|| "notebook execute validation failed".to_string()),
    )
}

fn notebook_code_cells(notebook: &Value) -> ToolResult<Vec<Value>> {
    let cells = notebook
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::Other("notebook is missing cells array".to_string()))?;
    let mut code_cells = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        if cell.get("cell_type").and_then(Value::as_str) != Some("code") {
            continue;
        }
        let source = cell
            .get("source")
            .map(notebook_source_to_string)
            .unwrap_or_default();
        code_cells.push(json!({
            "index": index,
            "source": source
        }));
    }
    Ok(code_cells)
}

fn notebook_source_to_string(source: &Value) -> String {
    match source {
        Value::String(text) => text.clone(),
        Value::Array(lines) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

fn resolve_python_command(context: &ToolContext) -> ToolResult<String> {
    if let Some(command) = context
        .app_state
        .get("notebook_python")
        .or_else(|| context.app_state.get("notebookPython"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
    {
        return Ok(command.to_string());
    }
    if let Ok(command) = std::env::var("KIANA_NOTEBOOK_PYTHON") {
        let command = command.trim();
        if !command.is_empty() {
            return Ok(command.to_string());
        }
    }
    for candidate in ["python3", "python", "py"] {
        if std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok()
        {
            return Ok(candidate.to_string());
        }
    }
    Err(ToolError::ValidationError(
        "No Python runtime found for notebook execution. Set notebook_python or KIANA_NOTEBOOK_PYTHON.".to_string(),
    ))
}

async fn run_python_notebook(
    python: &str,
    payload_path: &Path,
    runner_path: &Path,
    work_dir: &Path,
    timeout_ms: u64,
    max_output_bytes: usize,
) -> ToolResult<Value> {
    let mut command = Command::new(python);
    command
        .arg(runner_path)
        .arg(payload_path)
        .current_dir(work_dir)
        .kill_on_drop(true)
        .env("PYTHONNOUSERSITE", "1")
        .env("KIANA_NOTEBOOK_NETWORK", "disabled")
        .env_remove("PYTHONPATH");
    let output =
        match tokio::time::timeout(Duration::from_millis(timeout_ms), command.output()).await {
            Ok(output) => output?,
            Err(_) => {
                return Ok(json!({
                    "ok": false,
                    "timed_out": true,
                    "timeout_ms": timeout_ms,
                    "results": []
                }))
            }
        };
    let stdout = truncate_bytes(String::from_utf8_lossy(&output.stdout), max_output_bytes);
    let stderr = truncate_bytes(String::from_utf8_lossy(&output.stderr), max_output_bytes);
    if !output.status.success() {
        return Ok(json!({
            "ok": false,
            "timed_out": false,
            "runner_exit_code": output.status.code().unwrap_or(-1),
            "stdout": stdout,
            "stderr": stderr,
            "results": []
        }));
    }
    let mut report: Value = serde_json::from_str(&stdout)?;
    if !stderr.trim().is_empty() {
        report["runner_stderr"] = json!(stderr);
    }
    Ok(report)
}

fn truncate_bytes(text: std::borrow::Cow<'_, str>, max_output_bytes: usize) -> String {
    let text = text.as_ref();
    if text.len() <= max_output_bytes {
        return text.to_string();
    }
    let mut end = max_output_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &text[..end])
}

fn python_runner_source() -> &'static str {
    r#"import contextlib
import io
import json
import sys
import traceback

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)

namespace = {"__name__": "__main__"}
results = []
stop_on_error = bool(payload.get("stop_on_error", True))
for cell in payload.get("cells", []):
    index = cell.get("index")
    source = cell.get("source", "")
    stdout = io.StringIO()
    stderr = io.StringIO()
    ok = True
    error = None
    try:
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            exec(compile(source, f"<kiana-notebook-cell-{index}>", "exec"), namespace)
    except BaseException as exc:
        ok = False
        error = {
            "type": exc.__class__.__name__,
            "message": str(exc),
            "traceback": traceback.format_exc(limit=8),
        }
    results.append({
        "cell_index": index,
        "ok": ok,
        "stdout": stdout.getvalue(),
        "stderr": stderr.getvalue(),
        "error": error,
    })
    if (not ok) and stop_on_error:
        break

print(json.dumps({
    "ok": all(item.get("ok") for item in results),
    "timed_out": False,
    "results": results,
}, ensure_ascii=False))
"#
}

#[async_trait]
impl Tool for NotebookExecuteTool {
    fn name(&self) -> &str {
        "NotebookExecute"
    }

    fn description(&self) -> &str {
        "Execute Jupyter notebook code cells in an isolated temporary Python working directory"
    }

    fn workbench(&self) -> Option<&str> {
        Some("notebook")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_TIMEOUT_MS
                },
                "max_output_bytes": {
                    "type": "integer",
                    "minimum": 1024
                },
                "stop_on_error": {
                    "type": "boolean",
                    "default": true
                }
            },
            "required": ["path"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "ok": { "type": "boolean" },
                "timed_out": { "type": "boolean" },
                "code_cell_count": { "type": "integer" },
                "executed_cell_count": { "type": "integer" },
                "results": { "type": "array" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: NotebookExecuteInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        match validate_notebook_execute_request(&input, context) {
            Ok(_) => ValidationResult::ok(),
            Err(result) => result,
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: NotebookExecuteInput = serde_json::from_value(input.clone())?;
        let path = validate_notebook_execute_request(&input, context).map_err(validation_error)?;
        let notebook: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        let code_cells = notebook_code_cells(&notebook)?;
        let code_cell_count = code_cells.len();
        let timeout_ms = input.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let max_output_bytes = input.max_output_bytes.unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
        let python = resolve_python_command(context)?;
        let execution_root =
            std::env::temp_dir().join(format!("kiana-notebook-exec-{}", Uuid::new_v4()));
        fs::create_dir_all(&execution_root)?;
        let payload_path = execution_root.join("payload.json");
        let runner_path = execution_root.join("runner.py");
        let stop_on_error = input.stop_on_error.unwrap_or(true);
        fs::write(
            &payload_path,
            serde_json::to_string_pretty(&json!({
                "cells": code_cells,
                "stop_on_error": stop_on_error
            }))?,
        )?;
        fs::write(&runner_path, python_runner_source())?;
        let report = run_python_notebook(
            &python,
            &payload_path,
            &runner_path,
            &execution_root,
            timeout_ms,
            max_output_bytes,
        )
        .await;
        let _ = fs::remove_dir_all(&execution_root);
        let mut report = report?;
        report["path"] = json!(path.to_string_lossy().to_string());
        report["code_cell_count"] = json!(code_cell_count);
        report["executed_cell_count"] = json!(report["results"].as_array().map_or(0, Vec::len));
        report["isolated_cwd"] = json!("temporary");
        report["network"] = json!("disabled-env");

        Ok(ToolOutput {
            data: report,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let ok = output
            .data
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let timed_out = output
            .data
            .get("timed_out")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let code_cell_count = output
            .data
            .get("code_cell_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format!(
                "Notebook execution completed: ok={ok}, timed_out={timed_out}, code_cells={code_cell_count}"
            ),
            "execution": output.data
        })
    }
}

#[cfg(test)]
mod tests {
    use super::NotebookExecuteTool;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use uuid::Uuid;

    fn test_context(root: &std::path::Path, allowed: bool) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: root.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: if allowed {
                HashMap::from([("notebook_execution_allowed".to_string(), json!(true))])
            } else {
                HashMap::new()
            },
            abort_signal: abort_rx,
        }
    }

    fn notebook_json(cells: Vec<Value>) -> String {
        serde_json::to_string_pretty(&json!({
            "cells": cells,
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        }))
        .unwrap()
    }

    fn code_cell(source: &str) -> Value {
        json!({
            "cell_type": "code",
            "source": [source],
            "metadata": {},
            "outputs": []
        })
    }

    fn python_available() -> bool {
        ["python3", "python", "py"].iter().any(|candidate| {
            std::process::Command::new(candidate)
                .arg("--version")
                .output()
                .is_ok()
        })
    }

    #[tokio::test]
    async fn notebook_execute_requires_explicit_enablement() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-exec-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("test.ipynb"),
            notebook_json(vec![code_cell("print(1)\n")]),
        )
        .unwrap();
        let context = test_context(&root, false);

        let result = NotebookExecuteTool::new()
            .validate_input(&json!({ "path": "test.ipynb" }), &context)
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("disabled"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_execute_rejects_too_small_output_limit() {
        let root = std::env::temp_dir().join(format!("kiana-notebook-exec-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("test.ipynb"),
            notebook_json(vec![code_cell("print(1)\n")]),
        )
        .unwrap();
        let context = test_context(&root, true);

        let result = NotebookExecuteTool::new()
            .validate_input(
                &json!({
                    "path": "test.ipynb",
                    "max_output_bytes": 1
                }),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("max_output_bytes"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_execute_runs_code_cells_in_isolated_temp_cwd() {
        if !python_available() {
            eprintln!("skipping notebook execution test because Python is unavailable");
            return;
        }
        let root = std::env::temp_dir().join(format!("kiana-notebook-exec-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("test.ipynb"),
            notebook_json(vec![
                code_cell("x = 41\nprint('first', x)\n"),
                code_cell("import os\nprint('second', x + 1)\nprint(os.getcwd())\n"),
            ]),
        )
        .unwrap();
        let mut context = test_context(&root, true);

        let output = NotebookExecuteTool::new()
            .call(
                &json!({
                    "path": "test.ipynb",
                    "timeout_ms": 5000
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["ok"], true);
        assert_eq!(output.data["timed_out"], false);
        assert_eq!(output.data["code_cell_count"], 2);
        assert_eq!(output.data["executed_cell_count"], 2);
        assert_eq!(output.data["results"][0]["stdout"], "first 41\n");
        let second_stdout = output.data["results"][1]["stdout"].as_str().unwrap();
        assert!(second_stdout.contains("second 42"));
        assert!(!second_stdout.contains(root.to_string_lossy().as_ref()));
        assert_eq!(output.data["isolated_cwd"], "temporary");
        assert_eq!(output.data["network"], "disabled-env");

        let api_result = NotebookExecuteTool::new().map_to_api_result(&output, "toolu_notebook");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_notebook");
        assert!(api_result["content"].as_str().unwrap().contains("ok=true"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn notebook_execute_reports_cell_errors_without_writing_notebook() {
        if !python_available() {
            eprintln!("skipping notebook execution test because Python is unavailable");
            return;
        }
        let root = std::env::temp_dir().join(format!("kiana-notebook-exec-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let notebook = notebook_json(vec![code_cell("raise RuntimeError('boom')\n")]);
        fs::write(root.join("test.ipynb"), &notebook).unwrap();
        let mut context = test_context(&root, true);

        let output = NotebookExecuteTool::new()
            .call(&json!({ "path": "test.ipynb" }), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["ok"], false);
        assert_eq!(output.data["code_cell_count"], 1);
        assert_eq!(output.data["executed_cell_count"], 1);
        assert_eq!(output.data["results"][0]["ok"], false);
        assert_eq!(output.data["results"][0]["error"]["type"], "RuntimeError");
        assert_eq!(
            fs::read_to_string(root.join("test.ipynb")).unwrap(),
            notebook
        );
        let _ = fs::remove_dir_all(root);
    }
}
