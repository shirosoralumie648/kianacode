use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct PowerShellInput {
    command: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    shell: Option<String>,
}

pub struct PowerShellTool;

impl PowerShellTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        "PowerShell"
    }

    fn description(&self) -> &str {
        "Execute Windows PowerShell or PowerShell Core commands"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("execute PowerShell commands")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The PowerShell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 600000,
                    "description": "Maximum command runtime in milliseconds"
                },
                "shell": {
                    "type": "string",
                    "description": "Optional explicit PowerShell executable path"
                }
            },
            "required": ["command"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "stdout": { "type": "string" },
                "stderr": { "type": "string" },
                "exit_code": { "type": "integer" },
                "interrupted": { "type": "boolean" },
                "shell": { "type": ["string", "null"] }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: PowerShellInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.command.trim().is_empty() {
            return ValidationResult::err("command cannot be empty".to_string(), 2);
        }
        if let Err(error) = crate::exec_policy::validate_powershell_command(&input.command) {
            return ValidationResult::err(error, 5);
        }
        if let Some(timeout_ms) = input.timeout_ms {
            if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
                return ValidationResult::err(
                    format!("timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"),
                    3,
                );
            }
        }
        if input
            .shell
            .as_deref()
            .is_some_and(|shell| shell.trim().is_empty())
        {
            return ValidationResult::err("shell cannot be empty".to_string(), 4);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: PowerShellInput = serde_json::from_value(input.clone())?;
        if let Err(error) = crate::exec_policy::validate_powershell_command(&input.command) {
            return Err(ToolError::PermissionDenied(error));
        }
        let timeout_ms = input.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let Some(shell) = resolve_powershell_shell(input.shell.as_deref()) else {
            return Ok(unavailable_runtime_output());
        };

        let mut command = Command::new(&shell);
        command
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(&input.command)
            .current_dir(&context.cwd)
            .kill_on_drop(true);

        let output = match run_foreground_command(command, timeout_ms, context.abort_signal.clone())
            .await
        {
            ForegroundCommandExit::Completed(output) => output,
            ForegroundCommandExit::Failed(error) => {
                return Ok(ToolOutput {
                    data: json!({
                        "stdout": "",
                        "stderr": format!("failed to start PowerShell shell '{shell}': {error}"),
                        "exit_code": -1,
                        "interrupted": false,
                        "shell": shell,
                    }),
                    metadata: None,
                });
            }
            ForegroundCommandExit::TimedOut => {
                return Ok(ToolOutput {
                    data: json!({
                        "stdout": "",
                        "stderr": format!("PowerShell command timed out after {timeout_ms}ms"),
                        "exit_code": -1,
                        "interrupted": true,
                        "shell": shell,
                    }),
                    metadata: None,
                });
            }
            ForegroundCommandExit::Aborted => {
                return Ok(ToolOutput {
                    data: json!({
                        "stdout": "",
                        "stderr": "PowerShell command aborted before completion",
                        "exit_code": -1,
                        "interrupted": true,
                        "shell": shell,
                    }),
                    metadata: None,
                });
            }
        };

        Ok(ToolOutput {
            data: json!({
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "exit_code": output.status.code().unwrap_or(-1),
                "interrupted": false,
                "shell": shell,
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let stdout = output
            .data
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_start_matches(|ch: char| ch == '\n' || ch.is_whitespace() && ch != ' ')
            .trim_end()
            .to_string();
        let stderr = output
            .data
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let interrupted = output
            .data
            .get("interrupted")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut parts = Vec::new();
        if !stdout.is_empty() {
            parts.push(stdout);
        }
        if !stderr.is_empty() {
            parts.push(stderr);
        }
        if interrupted {
            parts.push("<error>Command was aborted before completion</error>".to_string());
        }
        if parts.is_empty() {
            if let Some(exit_code) = output.data.get("exit_code").and_then(Value::as_i64) {
                if exit_code != 0 {
                    parts.push(format!("Command exited with code {exit_code}."));
                }
            }
        }

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": parts.join("\n"),
            "is_error": interrupted
        })
    }
}

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;

enum ForegroundCommandExit {
    Completed(std::process::Output),
    Failed(std::io::Error),
    TimedOut,
    Aborted,
}

async fn run_foreground_command(
    mut command: Command,
    timeout_ms: u64,
    mut abort_signal: tokio::sync::watch::Receiver<bool>,
) -> ForegroundCommandExit {
    if *abort_signal.borrow() {
        return ForegroundCommandExit::Aborted;
    }

    let output = command.output();
    let timeout = tokio::time::sleep(Duration::from_millis(timeout_ms));
    tokio::pin!(output);
    tokio::pin!(timeout);
    let mut abort_open = true;

    loop {
        tokio::select! {
            result = &mut output => {
                return match result {
                    Ok(output) => ForegroundCommandExit::Completed(output),
                    Err(error) => ForegroundCommandExit::Failed(error),
                };
            }
            _ = &mut timeout => {
                return ForegroundCommandExit::TimedOut;
            }
            changed = abort_signal.changed(), if abort_open => {
                if changed.is_err() {
                    abort_open = false;
                } else if *abort_signal.borrow() {
                    return ForegroundCommandExit::Aborted;
                }
            }
        }
    }
}

fn unavailable_runtime_output() -> ToolOutput {
    ToolOutput {
        data: json!({
            "stdout": "",
            "stderr": "PowerShell runtime not found. Install PowerShell Core (`pwsh`) or provide a `shell` path.",
            "exit_code": -1,
            "interrupted": false,
            "shell": Value::Null,
        }),
        metadata: None,
    }
}

fn resolve_powershell_shell(shell: Option<&str>) -> Option<String> {
    if let Some(shell) = shell.map(str::trim).filter(|shell| !shell.is_empty()) {
        return Some(shell.to_string());
    }
    if let Ok(shell) = std::env::var("KIANA_POWERSHELL") {
        let shell = shell.trim();
        if !shell.is_empty() {
            return Some(shell.to_string());
        }
    }
    find_executable("pwsh").or_else(|| find_executable("powershell"))
}

fn find_executable(name: &str) -> Option<String> {
    if Path::new(name).components().count() > 1 {
        return executable_path(name).map(|path| path.to_string_lossy().to_string());
    }

    let path = std::env::var_os("PATH")?;
    let extensions = executable_extensions();
    for dir in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if executable_path(&candidate).is_some() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn executable_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        std::env::var("PATHEXT")
            .ok()
            .map(|value| {
                value
                    .split(';')
                    .filter(|extension| !extension.trim().is_empty())
                    .map(|extension| extension.to_ascii_lowercase())
                    .collect()
            })
            .filter(|extensions: &Vec<String>| !extensions.is_empty())
            .unwrap_or_else(|| vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()])
    }
    #[cfg(not(windows))]
    {
        vec!["".to_string()]
    }
}

fn executable_path(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref();
    if path.is_file() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::PowerShellTool;
    use crate::tool::{Tool, ToolContext, ToolOutput};
    use serde_json::json;
    use std::collections::HashMap;

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
    async fn validates_required_command() {
        let tool = PowerShellTool::new();
        let context = test_context(".".to_string());

        let result = tool.validate_input(&json!({"command": ""}), &context).await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("command cannot be empty"));
    }

    #[tokio::test]
    async fn rejects_out_of_range_timeout() {
        let tool = PowerShellTool::new();
        let context = test_context(".".to_string());

        let result = tool
            .validate_input(
                &json!({"command": "Get-Location", "timeout_ms": 0}),
                &context,
            )
            .await;

        assert!(!result.result);
        assert!(result.message.unwrap().contains("timeout_ms"));
    }

    #[tokio::test]
    async fn exec_policy_rejects_dangerous_powershell_commands() {
        let tool = PowerShellTool::new();
        let context = test_context(".".to_string());

        for command in [
            "Remove-Item -Recurse -Force C:\\",
            "Clear-Disk -Number 0 -RemoveData",
            "Restart-Computer -Force",
            "Format-Volume -DriveLetter C",
            "pwsh -Command \"Remove-Item -Recurse -Force C:\\\\\"",
        ] {
            let result = tool
                .validate_input(&json!({ "command": command }), &context)
                .await;

            assert!(!result.result, "{command} should be denied");
            assert!(result
                .message
                .unwrap_or_default()
                .contains("exec policy denied"));
        }

        let allowed = tool
            .validate_input(&json!({ "command": "Write-Output hello" }), &context)
            .await;
        assert!(allowed.result, "{:?}", allowed.message);

        let allowed_literal = tool
            .validate_input(
                &json!({ "command": "Write-Output \"Remove-Item -Recurse -Force C:\\\\\"" }),
                &context,
            )
            .await;
        assert!(allowed_literal.result, "{:?}", allowed_literal.message);
    }

    #[tokio::test]
    async fn returns_structured_result_when_explicit_shell_is_missing() {
        let tool = PowerShellTool::new();
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut context = test_context(cwd);

        let output = tool
            .call(
                &json!({
                    "command": "Get-Location",
                    "shell": "/definitely/not/a/powershell/runtime"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["exit_code"], -1);
        assert_eq!(output.data["interrupted"], false);
        let stderr = output.data["stderr"].as_str().unwrap();
        assert!(stderr.contains("failed to start PowerShell shell"));
        assert!(stderr.contains("/definitely/not/a/powershell/runtime"));
    }

    #[test]
    fn maps_stdout_and_stderr_to_model_facing_text() {
        let output = ToolOutput {
            data: json!({
                "stdout": "\nps output\n",
                "stderr": "ps warning\n",
                "exit_code": 0,
                "interrupted": false,
                "shell": "pwsh"
            }),
            metadata: None,
        };

        let result = PowerShellTool::new().map_to_api_result(&output, "toolu_powershell");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_powershell");
        assert_eq!(result["content"], "ps output\nps warning");
        assert_eq!(result["is_error"], false);
        assert!(result["content"]["stdout"].is_null());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn executes_explicit_shell_path() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir =
            std::env::temp_dir().join(format!("kiana-powershell-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let shell = temp_dir.join("fake-pwsh");
        fs::write(
            &shell,
            "#!/bin/sh\nprintf 'cmd:%s\\n' \"$5\"\nprintf 'err:%s\\n' \"$4\" >&2\nexit 7\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&shell).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shell, permissions).unwrap();

        let tool = PowerShellTool::new();
        let mut context = test_context(temp_dir.to_string_lossy().to_string());
        let output = tool
            .call(
                &json!({
                    "command": "Write-Output hello",
                    "shell": shell.to_string_lossy()
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["exit_code"], 7);
        assert_eq!(output.data["stdout"], "cmd:Write-Output hello\n");
        assert_eq!(output.data["stderr"], "err:-Command\n");

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn abort_signal_returns_interrupted_result_for_explicit_shell() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = std::env::temp_dir().join(format!(
            "kiana-powershell-abort-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let shell = temp_dir.join("fake-pwsh");
        fs::write(&shell, "#!/bin/sh\nsleep 5\nprintf 'done\\n'\n").unwrap();
        let mut permissions = fs::metadata(&shell).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shell, permissions).unwrap();
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = test_context(temp_dir.to_string_lossy().to_string());
        context.abort_signal = abort_rx;
        let abort_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let _ = abort_tx.send(true);
        });
        let started = std::time::Instant::now();

        let output = PowerShellTool::new()
            .call(
                &json!({
                    "command": "Write-Output hello",
                    "shell": shell.to_string_lossy(),
                    "timeout_ms": 10_000
                }),
                &mut context,
            )
            .await
            .unwrap();

        abort_task.await.unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(output.data["interrupted"], true);
        assert_eq!(output.data["exit_code"], -1);
        assert!(output.data["stderr"].as_str().unwrap().contains("aborted"));

        fs::remove_dir_all(temp_dir).unwrap();
    }
}
