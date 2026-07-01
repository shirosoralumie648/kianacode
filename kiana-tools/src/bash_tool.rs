use crate::bash_sandbox::{
    bash_sandbox_allow_unsandboxed_commands, bash_sandbox_bwrap_path, bash_sandbox_enabled,
    bash_sandbox_fail_if_unavailable,
};
use crate::shell::preferred_bash_program;
use crate::tool::*;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct BashInput {
    command: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    run_in_background: bool,
    #[serde(default, alias = "sandboxPermissions")]
    sandbox_permissions: Option<String>,
    #[serde(default, alias = "additionalPermissions")]
    additional_permissions: Option<AdditionalPermissions>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AdditionalPermissions {
    #[serde(default)]
    file_system: Option<AdditionalFileSystemPermissions>,
    #[serde(default)]
    filesystem: Option<AdditionalFileSystemPermissions>,
    #[serde(default)]
    network: Option<AdditionalNetworkPermissions>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AdditionalFileSystemPermissions {
    #[serde(default, alias = "allow_read", alias = "allowRead")]
    read: Vec<String>,
    #[serde(default, alias = "allow_write", alias = "allowWrite")]
    write: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AdditionalNetworkPermissions {
    #[serde(default)]
    enabled: Option<bool>,
}

pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn description(&self) -> &str {
        "Execute bash commands"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 600000,
                    "description": "Maximum foreground runtime in milliseconds"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run the command in the background and read output later with TaskOutput"
                },
                "sandbox_permissions": {
                    "type": "string",
                    "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                    "description": "Sandbox override for this command. use_default runs in the configured Bash sandbox when enabled; with_additional_permissions adds per-command filesystem grants; require_escalated runs unsandboxed only if policy allows it."
                },
                "additional_permissions": {
                    "type": "object",
                    "description": "Additional sandboxed access for this command, only with sandbox_permissions=with_additional_permissions.",
                    "properties": {
                        "file_system": {
                            "type": "object",
                            "properties": {
                                "read": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "write": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            }
                        },
                        "network": {
                            "type": "object",
                            "properties": {
                                "enabled": { "type": "boolean" }
                            }
                        }
                    }
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
                "sandboxed": { "type": "boolean" },
                "sandbox": { "type": "string" },
                "backgroundTaskId": { "type": ["string", "null"] },
                "outputFile": { "type": ["string", "null"] }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: BashInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.command.trim().is_empty() {
            return ValidationResult::err("command cannot be empty".to_string(), 2);
        }
        if let Err(error) = crate::exec_policy::validate_bash_command(&input.command) {
            return ValidationResult::err(error, 6);
        }
        if input
            .timeout_ms
            .is_some_and(|timeout_ms| timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS)
        {
            return ValidationResult::err(
                format!("timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"),
                3,
            );
        }
        if let Err(error) = BashSandboxPermissions::parse(input.sandbox_permissions.as_deref()) {
            return ValidationResult::err(error, 4);
        }
        if let Err(error) = validate_additional_permissions(&input) {
            return ValidationResult::err(error, 5);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: BashInput = serde_json::from_value(input.clone())?;
        if input.command.trim().is_empty() {
            return Err(ToolError::ValidationError(
                "command cannot be empty".to_string(),
            ));
        }
        if let Err(error) = crate::exec_policy::validate_bash_command(&input.command) {
            return Err(ToolError::PermissionDenied(error));
        }
        if network_permission_requested(&input) && !consume_sandbox_permission_grant(context, "*") {
            let request_id = send_sandbox_permission_request(context, "*")?;
            return Err(ToolError::PermissionDenied(format!(
                "Sandbox permission request {request_id} sent to team-lead for network access. Wait for a sandbox_permission_response, then retry the Bash command."
            )));
        }
        if input.run_in_background {
            return spawn_background_bash(input, context);
        }

        let plan = bash_command_plan(
            &input,
            context,
            Vec::<PathBuf>::new(),
            vec![
                preferred_bash_program(),
                "-c".into(),
                input.command.clone().into(),
            ],
        )?;
        let mut command = Command::new(&plan.program);
        command
            .args(&plan.args)
            .current_dir(&context.cwd)
            .kill_on_drop(true);

        let timeout_ms = input.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let output =
            match run_foreground_command(command, timeout_ms, context.abort_signal.clone()).await {
                ForegroundCommandExit::Completed(output) => output,
                ForegroundCommandExit::Failed(error) => return Err(error.into()),
                ForegroundCommandExit::TimedOut => {
                    return Ok(ToolOutput {
                        data: json!({
                            "stdout": "",
                            "stderr": format!("Bash command timed out after {timeout_ms}ms"),
                            "exit_code": -1,
                            "interrupted": true,
                            "sandboxed": plan.sandboxed,
                            "sandbox": plan.sandbox_name,
                            "backgroundTaskId": Value::Null,
                            "outputFile": Value::Null
                        }),
                        metadata: None,
                    });
                }
                ForegroundCommandExit::Aborted => {
                    return Ok(ToolOutput {
                        data: json!({
                            "stdout": "",
                            "stderr": "Bash command aborted before completion",
                            "exit_code": -1,
                            "interrupted": true,
                            "sandboxed": plan.sandboxed,
                            "sandbox": plan.sandbox_name,
                            "backgroundTaskId": Value::Null,
                            "outputFile": Value::Null
                        }),
                        metadata: None,
                    });
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(ToolOutput {
            data: json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
                "interrupted": false,
                "sandboxed": plan.sandboxed,
                "sandbox": plan.sandbox_name,
                "backgroundTaskId": Value::Null,
                "outputFile": Value::Null
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
        let background_task_id = output
            .data
            .get("backgroundTaskId")
            .and_then(Value::as_str)
            .filter(|task_id| !task_id.is_empty());
        let output_file = output
            .data
            .get("outputFile")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty());

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
        if let Some(task_id) = background_task_id {
            let mut message = format!("Command running in background with ID: {task_id}.");
            if let Some(output_file) = output_file {
                message.push_str(&format!(" Output is being written to: {output_file}"));
            }
            parts.push(message);
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
const BACKGROUND_ROOT_KEY: &str = "background_root";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BashSandboxPermissions {
    UseDefault,
    WithAdditionalPermissions,
    RequireEscalated,
}

impl BashSandboxPermissions {
    fn parse(value: Option<&str>) -> Result<Option<Self>, String> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        match value {
            "use_default" | "use-default" | "default" => Ok(Some(Self::UseDefault)),
            "with_additional_permissions" | "with-additional-permissions" => {
                Ok(Some(Self::WithAdditionalPermissions))
            }
            "require_escalated" | "require-escalated" | "escalated" => {
                Ok(Some(Self::RequireEscalated))
            }
            other => Err(format!(
                "sandbox_permissions must be use_default, with_additional_permissions, or require_escalated, got {other}"
            )),
        }
    }
}

#[derive(Debug)]
struct BashCommandPlan {
    program: OsString,
    args: Vec<OsString>,
    sandboxed: bool,
    sandbox_name: &'static str,
}

#[derive(Debug, Default)]
struct AdditionalSandboxGrants {
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    network_enabled: bool,
}

fn bash_command_plan(
    input: &BashInput,
    context: &ToolContext,
    extra_writable_roots: Vec<PathBuf>,
    inner_args: Vec<OsString>,
) -> ToolResult<BashCommandPlan> {
    let sandbox_permissions = BashSandboxPermissions::parse(input.sandbox_permissions.as_deref())
        .map_err(ToolError::ValidationError)?;

    if matches!(
        sandbox_permissions,
        Some(BashSandboxPermissions::RequireEscalated)
    ) {
        if !bash_sandbox_allow_unsandboxed_commands(&context.app_state) {
            return Err(ToolError::PermissionDenied(
                "Bash requested require_escalated, but sandbox.allow_unsandboxed_commands is false"
                    .to_string(),
            ));
        }
        return Ok(BashCommandPlan {
            program: inner_args
                .first()
                .cloned()
                .unwrap_or_else(preferred_bash_program),
            args: inner_args.into_iter().skip(1).collect(),
            sandboxed: false,
            sandbox_name: "none",
        });
    }

    let explicitly_requested = sandbox_permissions.is_some();
    let additional_grants = additional_sandbox_grants(input, context)?;
    if !matches!(
        sandbox_permissions,
        Some(BashSandboxPermissions::WithAdditionalPermissions)
    ) && !additional_grants.is_empty()
    {
        return Err(ToolError::ValidationError(
            "additional_permissions requires sandbox_permissions=with_additional_permissions"
                .to_string(),
        ));
    }
    if !explicitly_requested && !bash_sandbox_enabled(&context.app_state) {
        return Ok(BashCommandPlan {
            program: inner_args
                .first()
                .cloned()
                .unwrap_or_else(preferred_bash_program),
            args: inner_args.into_iter().skip(1).collect(),
            sandboxed: false,
            sandbox_name: "none",
        });
    }

    let Some(bwrap) = bash_sandbox_bwrap_path(&context.app_state) else {
        if explicitly_requested || bash_sandbox_fail_if_unavailable(&context.app_state) {
            return Err(ToolError::Other(
                "Bash sandbox requested but bubblewrap (bwrap) is not available".to_string(),
            ));
        }
        if !bash_sandbox_allow_unsandboxed_commands(&context.app_state) {
            return Err(ToolError::PermissionDenied(
                "Bash sandbox is unavailable and sandbox.allow_unsandboxed_commands is false"
                    .to_string(),
            ));
        }
        return Ok(BashCommandPlan {
            program: inner_args
                .first()
                .cloned()
                .unwrap_or_else(preferred_bash_program),
            args: inner_args.into_iter().skip(1).collect(),
            sandboxed: false,
            sandbox_name: "none",
        });
    };

    let cwd = canonical_existing_dir(Path::new(&context.cwd))?;
    let writable_roots = bash_sandbox_writable_roots(
        context,
        extra_writable_roots,
        additional_grants.write_roots,
        &cwd,
    )?;
    let read_roots = bash_sandbox_read_roots(additional_grants.read_roots, &writable_roots)?;
    let mut args = vec![
        OsString::from("--die-with-parent"),
        OsString::from("--unshare-all"),
        OsString::from("--ro-bind"),
        OsString::from("/"),
        OsString::from("/"),
    ];
    if additional_grants.network_enabled {
        args.push(OsString::from("--share-net"));
    }
    for root in writable_roots {
        args.push(OsString::from("--bind"));
        args.push(root.as_os_str().to_os_string());
        args.push(root.as_os_str().to_os_string());
    }
    for root in read_roots {
        args.push(OsString::from("--ro-bind"));
        args.push(root.as_os_str().to_os_string());
        args.push(root.as_os_str().to_os_string());
    }
    args.extend([
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--proc"),
        OsString::from("/proc"),
        OsString::from("--tmpfs"),
        OsString::from("/tmp"),
        OsString::from("--chdir"),
        cwd.as_os_str().to_os_string(),
        OsString::from("--setenv"),
        OsString::from("KIANA_SANDBOX"),
        OsString::from("bwrap"),
    ]);
    args.extend(inner_args);

    Ok(BashCommandPlan {
        program: bwrap.into_os_string(),
        args,
        sandboxed: true,
        sandbox_name: "bwrap",
    })
}

fn bash_sandbox_writable_roots(
    context: &ToolContext,
    extra_roots: Vec<PathBuf>,
    additional_write_roots: Vec<PathBuf>,
    cwd: &Path,
) -> ToolResult<Vec<PathBuf>> {
    let mut roots = vec![cwd.to_path_buf()];
    if let Some(access_roots) = context.access_roots() {
        roots.extend(access_roots);
    }
    for root in extra_roots {
        roots.push(canonical_existing_dir(&root)?);
    }
    roots.extend(additional_write_roots);

    dedupe_and_validate_writable_roots(roots)
}

fn bash_sandbox_read_roots(
    read_roots: Vec<PathBuf>,
    writable_roots: &[PathBuf],
) -> ToolResult<Vec<PathBuf>> {
    let mut deduped: Vec<PathBuf> = Vec::new();
    for root in read_roots {
        if writable_roots
            .iter()
            .any(|writable| root.starts_with(writable))
        {
            continue;
        }
        if !deduped.iter().any(|existing| root.starts_with(existing)) {
            deduped.retain(|existing| !existing.starts_with(&root));
            deduped.push(root);
        }
    }
    Ok(deduped)
}

fn dedupe_and_validate_writable_roots(roots: Vec<PathBuf>) -> ToolResult<Vec<PathBuf>> {
    let mut deduped: Vec<PathBuf> = Vec::new();
    for root in roots {
        if root == Path::new("/") {
            return Err(ToolError::PermissionDenied(
                "Bash sandbox refuses to bind / as writable; use require_escalated for full-disk access"
                    .to_string(),
            ));
        }
        if !deduped.iter().any(|existing| existing == &root) {
            deduped.push(root);
        }
    }
    Ok(deduped)
}

fn validate_additional_permissions(input: &BashInput) -> Result<(), String> {
    let permissions = BashSandboxPermissions::parse(input.sandbox_permissions.as_deref())?;
    let Some(additional) = &input.additional_permissions else {
        return Ok(());
    };
    let network_enabled = additional
        .network
        .as_ref()
        .and_then(|network| network.enabled)
        == Some(true);
    let file_system = additional.file_system_permissions();
    let has_paths = file_system
        .as_ref()
        .is_some_and(|file_system| !file_system.read.is_empty() || !file_system.write.is_empty());
    if (has_paths || network_enabled)
        && !matches!(
            permissions,
            Some(BashSandboxPermissions::WithAdditionalPermissions)
        )
    {
        return Err(
            "additional_permissions requires sandbox_permissions=with_additional_permissions"
                .to_string(),
        );
    }
    Ok(())
}

fn additional_sandbox_grants(
    input: &BashInput,
    context: &ToolContext,
) -> ToolResult<AdditionalSandboxGrants> {
    let Some(additional) = &input.additional_permissions else {
        return Ok(AdditionalSandboxGrants::default());
    };
    let network_enabled = network_permission_requested(input);

    let Some(file_system) = additional.file_system_permissions() else {
        return Ok(AdditionalSandboxGrants {
            network_enabled,
            ..AdditionalSandboxGrants::default()
        });
    };
    Ok(AdditionalSandboxGrants {
        read_roots: canonical_permission_paths(&file_system.read, context)?,
        write_roots: canonical_permission_paths(&file_system.write, context)?,
        network_enabled,
    })
}

impl AdditionalPermissions {
    fn file_system_permissions(&self) -> Option<&AdditionalFileSystemPermissions> {
        self.file_system.as_ref().or(self.filesystem.as_ref())
    }
}

impl AdditionalSandboxGrants {
    fn is_empty(&self) -> bool {
        self.read_roots.is_empty() && self.write_roots.is_empty() && !self.network_enabled
    }
}

fn network_permission_requested(input: &BashInput) -> bool {
    input
        .additional_permissions
        .as_ref()
        .and_then(|additional| additional.network.as_ref())
        .and_then(|network| network.enabled)
        == Some(true)
}

fn consume_sandbox_permission_grant(context: &mut ToolContext, host: &str) -> bool {
    let grants = context
        .app_state
        .get("sandbox_permission_grants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut remaining = Vec::new();
    let mut consumed = false;
    for grant in grants {
        if !consumed && sandbox_permission_grant_matches(&grant, host) {
            consumed = true;
            continue;
        }
        remaining.push(grant);
    }
    if consumed {
        context.app_state.insert(
            "sandbox_permission_grants".to_string(),
            Value::Array(remaining),
        );
    }
    consumed
}

fn sandbox_permission_grant_matches(grant: &Value, host: &str) -> bool {
    if grant
        .get("allow")
        .or_else(|| grant.get("allowed"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return false;
    }
    let Some(grant_host) = grant.get("host").and_then(Value::as_str) else {
        return false;
    };
    grant_host == "*" || grant_host.eq_ignore_ascii_case(host)
}

fn send_sandbox_permission_request(context: &mut ToolContext, host: &str) -> ToolResult<String> {
    let Some(team_name) = active_team_name(context) else {
        return Err(ToolError::PermissionDenied(
            "Bash requested sandbox network access, but no active team context is available"
                .to_string(),
        ));
    };
    let worker_name = agent_name(context);
    if worker_name.eq_ignore_ascii_case("team-lead") {
        return Err(ToolError::PermissionDenied(
            "Bash requested sandbox network access; team-lead must grant it explicitly or avoid the network request"
                .to_string(),
        ));
    }
    if let Some(existing) = pending_sandbox_request_id(context, host) {
        return Ok(existing);
    }

    let request_id = format!(
        "sandbox-{}-{}",
        sanitize_path_component(&worker_name),
        Uuid::new_v4().simple()
    );
    let created_at = now_unix_seconds();
    let body = json!({
        "type": "sandbox_permission_request",
        "requestId": request_id,
        "request_id": request_id,
        "workerId": agent_id(context).unwrap_or_else(|| worker_name.clone()),
        "worker_id": agent_id(context).unwrap_or_else(|| worker_name.clone()),
        "workerName": worker_name,
        "worker_name": worker_name,
        "workerColor": agent_color(context),
        "worker_color": agent_color(context),
        "hostPattern": { "host": host },
        "host_pattern": { "host": host },
        "createdAt": created_at,
        "created_at": created_at
    });
    let content = serde_json::to_string(&body)?;
    let message = json!({
        "id": Uuid::new_v4().to_string(),
        "role": "assistant",
        "from": worker_name,
        "to": "team-lead",
        "recipients": ["team-lead"],
        "summary": "sandbox_permission_request",
        "content": content,
        "structured": body,
        "timestamp": created_at.to_string(),
        "created_at": created_at
    });
    append_mailbox_message(context, &team_name, "team-lead", &message)?;

    let mut pending = context
        .app_state
        .get("pending_sandbox_permission_requests")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    pending.insert(
        request_id.clone(),
        json!({
            "requestId": request_id,
            "request_id": request_id,
            "host": host,
            "team_name": team_name
        }),
    );
    context.app_state.insert(
        "pending_sandbox_permission_requests".to_string(),
        Value::Object(pending),
    );
    Ok(request_id)
}

fn pending_sandbox_request_id(context: &ToolContext, host: &str) -> Option<String> {
    context
        .app_state
        .get("pending_sandbox_permission_requests")
        .and_then(Value::as_object)?
        .iter()
        .find_map(|(request_id, request)| {
            if request
                .get("host")
                .and_then(Value::as_str)
                .is_some_and(|request_host| request_host == host)
            {
                Some(request_id.clone())
            } else {
                None
            }
        })
}

fn append_mailbox_message(
    context: &mut ToolContext,
    team_name: &str,
    recipient: &str,
    message: &Value,
) -> ToolResult<()> {
    let mut mailboxes = context
        .app_state
        .get("team_mailboxes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut inbox = mailboxes
        .remove(recipient)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    inbox.push(message.clone());
    mailboxes.insert(recipient.to_string(), Value::Array(inbox));
    context
        .app_state
        .insert("team_mailboxes".to_string(), Value::Object(mailboxes));

    let Some(teams_root) = teams_root(context) else {
        return Ok(());
    };
    let path = teams_root
        .join(sanitize_path_component(team_name))
        .join("inboxes")
        .join(format!("{}.json", sanitize_path_component(recipient)));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut inbox = if path.is_file() {
        serde_json::from_str::<Value>(&fs::read_to_string(&path)?)?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    inbox.push(json!({
        "from": message.get("from").and_then(Value::as_str).unwrap_or("agent"),
        "text": message
            .get("content")
            .or_else(|| message.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "summary": message.get("summary").cloned().unwrap_or(Value::Null),
        "timestamp": message
            .get("timestamp")
            .cloned()
            .unwrap_or_else(|| json!(now_unix_seconds().to_string())),
        "read": false
    }));
    fs::write(&path, serde_json::to_string_pretty(&Value::Array(inbox))?)?;
    Ok(())
}

fn active_team_name(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get("team_context")
        .or_else(|| context.app_state.get("teamContext"))
        .and_then(|team| team.get("team_name").or_else(|| team.get("teamName")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_TEAM_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_TEAM_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
}

fn agent_name(context: &ToolContext) -> String {
    context
        .app_state
        .get("agent_name")
        .or_else(|| context.app_state.get("agentName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_AGENT_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "team-lead".to_string())
}

fn agent_id(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get("agent_id")
        .or_else(|| context.app_state.get("agentId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_AGENT_ID")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_ID").ok())
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
        })
}

fn agent_color(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get("agent_color")
        .or_else(|| context.app_state.get("agentColor"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|color| !color.is_empty())
        .map(str::to_string)
}

fn teams_root(context: &ToolContext) -> Option<PathBuf> {
    context
        .app_state
        .get("teams_root")
        .or_else(|| context.app_state.get("teamsRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("KIANA_TEAMS_ROOT").ok().map(PathBuf::from))
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn canonical_permission_paths(paths: &[String], context: &ToolContext) -> ToolResult<Vec<PathBuf>> {
    let mut resolved = Vec::new();
    for path in paths {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let path = PathBuf::from(path);
        let absolute = if path.is_absolute() {
            path
        } else {
            Path::new(&context.cwd).join(path)
        };
        resolved.push(canonical_existing_dir(&absolute)?);
    }
    Ok(resolved)
}

fn canonical_existing_dir(path: &Path) -> ToolResult<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.is_dir() {
        return Err(ToolError::ValidationError(format!(
            "sandbox writable root is not a directory: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

fn spawn_background_bash(input: BashInput, context: &mut ToolContext) -> ToolResult<ToolOutput> {
    let root = background_root(context);
    ensure_background_dirs(&root)?;
    context.app_state.insert(
        BACKGROUND_ROOT_KEY.to_string(),
        Value::String(root.to_string_lossy().to_string()),
    );

    let task_id = Uuid::new_v4().to_string();
    let command_path = root.join("commands").join(format!("{task_id}.sh"));
    let runner_path = root.join("scripts").join(format!("{task_id}.sh"));
    let log_path = root.join("logs").join(format!("{task_id}.log"));
    let exit_path = root.join("exits").join(format!("{task_id}.exit"));
    let task_path = root.join("tasks").join(format!("{task_id}.json"));
    let now = now_unix_seconds();

    fs::write(&command_path, command_script(&input.command))?;
    fs::write(
        &runner_path,
        runner_script(&command_path, &log_path, &exit_path),
    )?;
    append_log(
        &log_path,
        &format!("created background bash task {task_id}\n"),
    )?;

    let plan = background_command_plan(&input, context, &runner_path, &root)?;
    let mut process = std::process::Command::new(&plan.program);
    process.args(&plan.args);
    let child = process
        .current_dir(&context.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let task = json!({
        "id": task_id,
        "prompt": input.command,
        "command": input.command,
        "cwd": context.cwd,
        "status": "running",
        "pid": child.id(),
        "created_at": now,
        "updated_at": now,
        "exit_code": Value::Null,
        "error": Value::Null,
        "task_type": "background_bash"
    });
    fs::write(&task_path, serde_json::to_string_pretty(&task)?)?;

    Ok(ToolOutput {
        data: json!({
            "stdout": "",
            "stderr": "",
            "exit_code": Value::Null,
            "interrupted": false,
            "sandboxed": plan.sandboxed,
            "sandbox": plan.sandbox_name,
            "backgroundTaskId": task_id,
            "outputFile": log_path.to_string_lossy()
        }),
        metadata: None,
    })
}

fn background_command_plan(
    input: &BashInput,
    context: &ToolContext,
    runner_path: &Path,
    root: &Path,
) -> ToolResult<BashCommandPlan> {
    let inner = background_inner_args(runner_path);
    bash_command_plan(input, context, vec![root.to_path_buf()], inner)
}

#[cfg(unix)]
fn background_inner_args(runner_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("setsid"),
        OsString::from("bash"),
        runner_path.as_os_str().to_os_string(),
    ]
}

#[cfg(not(unix))]
fn background_inner_args(runner_path: &Path) -> Vec<OsString> {
    vec![
        preferred_bash_program(),
        runner_path.as_os_str().to_os_string(),
    ]
}

fn command_script(command: &str) -> String {
    format!("#!/usr/bin/env bash\nset +e\n{command}\n")
}

fn runner_script(command_path: &Path, log_path: &Path, exit_path: &Path) -> String {
    format!(
        "#!/usr/bin/env bash\nset +e\nprintf '[%s] command started\\n' \"$(date +%s)\" >> {log}\n\"$BASH\" {command} >> {log} 2>&1\ncode=$?\nprintf '%s\\n' \"$code\" > {exit}\nif [ \"$code\" -eq 0 ]; then status=completed; else status=failed; fi\nprintf '[%s] command %s exit_code=%s\\n' \"$(date +%s)\" \"$status\" \"$code\" >> {log}\n",
        log = shell_quote_path(log_path),
        command = shell_quote_path(command_path),
        exit = shell_quote_path(exit_path)
    )
}

fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn background_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get(BACKGROUND_ROOT_KEY)
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_BG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path).join("bg-tasks");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".kiana").join("bg-tasks");
    }
    PathBuf::from(".kiana").join("bg-tasks")
}

fn ensure_background_dirs(root: &Path) -> ToolResult<()> {
    for dir in ["tasks", "logs", "commands", "scripts", "exits"] {
        fs::create_dir_all(root.join(dir))?;
    }
    Ok(())
}

fn append_log(path: &Path, line: &str) -> ToolResult<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("[{}] {}", now_unix_seconds(), line).as_bytes())?;
    Ok(())
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{shell_quote_path, BashTool};
    use crate::bash_sandbox::find_on_path;
    use crate::task_output::TaskOutputTool;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use uuid::Uuid;

    fn test_context(root: &std::path::Path) -> ToolContext {
        test_context_with_cwd(root, &std::env::current_dir().unwrap())
    }

    fn test_context_with_cwd(root: &std::path::Path, cwd: &std::path::Path) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: cwd.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([(
                "background_root".to_string(),
                Value::String(root.to_string_lossy().to_string()),
            )]),
            abort_signal: abort_rx,
        }
    }

    fn sandbox_context(root: &std::path::Path, cwd: &std::path::Path) -> ToolContext {
        let mut context = test_context_with_cwd(root, cwd);
        context.app_state.insert(
            "sandbox".to_string(),
            json!({
                "enabled": true,
                "failIfUnavailable": true,
                "allowUnsandboxedCommands": false
            }),
        );
        context
    }

    fn read_json(path: impl AsRef<std::path::Path>) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn validates_command_and_timeout() {
        let root = std::env::temp_dir().join(format!("kiana-bash-test-{}", Uuid::new_v4()));
        let context = test_context(&root);
        let tool = BashTool::new();

        assert!(
            !tool
                .validate_input(&json!({"command": ""}), &context)
                .await
                .result
        );
        assert!(
            !tool
                .validate_input(&json!({"command": "pwd", "timeout_ms": 0}), &context)
                .await
                .result
        );
    }

    #[tokio::test]
    async fn exec_policy_rejects_dangerous_bash_commands() {
        let root = std::env::temp_dir().join(format!("kiana-bash-policy-test-{}", Uuid::new_v4()));
        let context = test_context(&root);
        let tool = BashTool::new();

        for command in [
            "rm -rf /",
            "/bin/rm -rf /",
            "sudo apt update",
            "/usr/bin/sudo apt update",
            "bash -c 'rm -rf /'",
            "echo before; rm -rf /",
            "dd if=/dev/zero of=/dev/sda bs=1M",
            "mkfs.ext4 /dev/sda1",
            "chmod -R 777 /",
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
            .validate_input(&json!({ "command": "echo safe" }), &context)
            .await;
        assert!(allowed.result, "{:?}", allowed.message);

        for command in [r#"echo "rm -rf /""#, r#"printf '%s\n' 'sudo apt update'"#] {
            let allowed = tool
                .validate_input(&json!({ "command": command }), &context)
                .await;
            assert!(
                allowed.result,
                "{command} should be allowed: {:?}",
                allowed.message
            );
        }
    }

    #[tokio::test]
    async fn foreground_timeout_returns_interrupted_result() {
        let root = std::env::temp_dir().join(format!("kiana-bash-test-{}", Uuid::new_v4()));
        let mut context = test_context(&root);

        let output = BashTool::new()
            .call(
                &json!({"command": "sleep 1", "timeout_ms": 1}),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["interrupted"], true);
        assert_eq!(output.data["exit_code"], -1);
    }

    #[tokio::test]
    async fn foreground_abort_signal_returns_interrupted_result() {
        let root = std::env::temp_dir().join(format!("kiana-bash-test-{}", Uuid::new_v4()));
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = test_context(&root);
        context.abort_signal = abort_rx;
        let abort_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let _ = abort_tx.send(true);
        });
        let started = std::time::Instant::now();

        let output = BashTool::new()
            .call(
                &json!({"command": "sleep 5", "timeout_ms": 10_000}),
                &mut context,
            )
            .await
            .unwrap();

        abort_task.await.unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(output.data["interrupted"], true);
        assert_eq!(output.data["exit_code"], -1);
        assert!(output.data["stderr"].as_str().unwrap().contains("aborted"));
    }

    #[test]
    fn maps_stdout_and_stderr_to_model_facing_text() {
        let output = crate::tool::ToolOutput {
            data: json!({
                "stdout": "\nhello\n",
                "stderr": "warn\n",
                "exit_code": 0,
                "interrupted": false,
                "backgroundTaskId": Value::Null,
                "outputFile": Value::Null
            }),
            metadata: None,
        };

        let result = BashTool::new().map_to_api_result(&output, "toolu_bash");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_bash");
        assert_eq!(result["content"], "hello\nwarn");
        assert_eq!(result["is_error"], false);
        assert!(result["content"]["stdout"].is_null());
    }

    #[tokio::test]
    async fn background_bash_output_is_readable_with_task_output() {
        let root = std::env::temp_dir().join(format!("kiana-bash-test-{}", Uuid::new_v4()));
        let mut context = test_context(&root);

        let started = BashTool::new()
            .call(
                &json!({
                    "command": "printf 'hello background\\n'",
                    "run_in_background": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        let task_id = started.data["backgroundTaskId"]
            .as_str()
            .unwrap()
            .to_string();

        let output = TaskOutputTool::new()
            .call(
                &json!({
                    "task_id": task_id,
                    "block": true,
                    "timeout_ms": 5000
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["status"], "completed");
        assert_eq!(output.data["task"]["exit_code"], 0);
        assert!(output.data["task"]["output"]
            .as_str()
            .unwrap()
            .contains("hello background"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sandboxed_foreground_bash_writes_only_bound_workspace() {
        if find_on_path("bwrap").is_none() {
            return;
        }
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("kiana-bash-sandbox-test-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let blocked = root.join("blocked");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&blocked).unwrap();
        let mut context = sandbox_context(&root.join("bg"), &workspace);
        let blocked_file = blocked.join("denied.txt");

        let output = BashTool::new()
            .call(
                &json!({
                    "command": format!(
                        "printf allowed > allowed.txt; printf denied > {}",
                        shell_quote_path(&blocked_file)
                    ),
                    "sandbox_permissions": "use_default"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["sandboxed"], true);
        assert_eq!(output.data["sandbox"], "bwrap");
        assert_eq!(
            fs::read_to_string(workspace.join("allowed.txt")).unwrap(),
            "allowed"
        );
        assert!(!blocked_file.exists());
        assert_ne!(output.data["exit_code"], 0);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sandboxed_bash_honors_additional_write_permissions() {
        if find_on_path("bwrap").is_none() {
            return;
        }
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("kiana-bash-additional-test-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let extra = root.join("extra");
        let blocked = root.join("blocked");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&extra).unwrap();
        fs::create_dir_all(&blocked).unwrap();
        let mut context = sandbox_context(&root.join("bg"), &workspace);
        let extra_file = extra.join("allowed.txt");
        let blocked_file = blocked.join("denied.txt");

        let output = BashTool::new()
            .call(
                &json!({
                    "command": format!(
                        "printf extra > {}; printf denied > {}",
                        shell_quote_path(&extra_file),
                        shell_quote_path(&blocked_file)
                    ),
                    "sandbox_permissions": "with_additional_permissions",
                    "additional_permissions": {
                        "file_system": {
                            "write": [extra]
                        }
                    }
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["sandboxed"], true);
        assert_eq!(fs::read_to_string(extra_file).unwrap(), "extra");
        assert!(!blocked_file.exists());
        assert_ne!(output.data["exit_code"], 0);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn additional_permissions_require_matching_sandbox_mode_and_reject_network() {
        let root = std::env::temp_dir().join(format!("kiana-bash-test-{}", Uuid::new_v4()));
        let context = sandbox_context(&root, &std::env::current_dir().unwrap());
        let tool = BashTool::new();

        let invalid_mode = tool
            .validate_input(
                &json!({
                    "command": "printf no",
                    "sandbox_permissions": "use_default",
                    "additional_permissions": {
                        "file_system": {
                            "write": ["."]
                        }
                    }
                }),
                &context,
            )
            .await;
        assert!(!invalid_mode.result);
        assert!(invalid_mode
            .message
            .unwrap()
            .contains("with_additional_permissions"));

        let network = tool
            .validate_input(
                &json!({
                    "command": "printf yes",
                    "sandbox_permissions": "with_additional_permissions",
                    "additional_permissions": {
                        "network": {
                            "enabled": true
                        }
                    }
                }),
                &context,
            )
            .await;
        assert!(network.result);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sandbox_network_permission_request_is_sent_to_team_lead() {
        let root = std::env::temp_dir().join(format!("kiana-bash-sandbox-perm-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let teams_root = root.join("teams");
        fs::create_dir_all(&workspace).unwrap();
        let mut context = sandbox_context(&root.join("bg"), &workspace);
        context.app_state.insert(
            "teams_root".to_string(),
            json!(teams_root.to_string_lossy()),
        );
        context.app_state.insert(
            "team_context".to_string(),
            json!({ "team_name": "review", "teamName": "review" }),
        );
        context
            .app_state
            .insert("agent_name".to_string(), json!("researcher"));
        context
            .app_state
            .insert("agent_id".to_string(), json!("researcher@review"));
        context
            .app_state
            .insert("agent_color".to_string(), json!("green"));

        let error = BashTool::new()
            .call(
                &json!({
                    "command": "printf needs-network",
                    "sandbox_permissions": "with_additional_permissions",
                    "additional_permissions": {
                        "network": {
                            "enabled": true
                        }
                    }
                }),
                &mut context,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("Sandbox permission request"));
        let leader_inbox = read_json(teams_root.join("review/inboxes/team-lead.json"));
        let body: Value = serde_json::from_str(leader_inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["type"], "sandbox_permission_request");
        assert_eq!(body["workerName"], "researcher");
        assert_eq!(body["workerId"], "researcher@review");
        assert_eq!(body["hostPattern"]["host"], "*");
        assert_eq!(leader_inbox[0]["summary"], "sandbox_permission_request");

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sandbox_rejects_require_escalated_when_unsandboxed_commands_are_disabled() {
        let root = std::env::temp_dir().join(format!("kiana-bash-test-{}", Uuid::new_v4()));
        let mut context = sandbox_context(&root, &std::env::current_dir().unwrap());

        let error = BashTool::new()
            .call(
                &json!({
                    "command": "printf should-not-run",
                    "sandbox_permissions": "require_escalated"
                }),
                &mut context,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("allow_unsandboxed_commands is false"));
        let _ = fs::remove_dir_all(root);
    }
}
