//! Daemon-owned execution for Codex-shaped harness tools.
//!
//! `shell.exec` and `apply_patch` are registered on the capability broker.
//! The runner never executes these tools.

use crate::apply_patch::apply_codex_patch;
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult};
use kiana_ports::PortError;
use kiana_runner_protocol::{DEFAULT_HARNESS_SANDBOX, HARNESS_SANDBOX_WORKSPACE_WRITE};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

const SHELL_OPERATION: &str = "shell.exec";
const APPLY_PATCH_OPERATION: &str = "apply_patch";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const EXEC_TIMEOUT_EXIT_CODE: i32 = 124;
const EXEC_OUTPUT_MAX_BYTES: usize = 1024 * 1024;
const IO_DRAIN_TIMEOUT: Duration = Duration::from_millis(2000);
const PROCESS_GROUP_TERM_GRACE: Duration = Duration::from_millis(100);
const PROCESS_GROUP_EXIT_GRACE: Duration = Duration::from_millis(250);
const READ_CHUNK_SIZE: usize = 8192;

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Process,
        SHELL_OPERATION,
        Arc::new(ShellExecHandler),
    )?;
    broker.register_static(
        CapabilityKind::Filesystem,
        APPLY_PATCH_OPERATION,
        Arc::new(ApplyPatchHandler),
    )?;
    Ok(())
}

struct ShellExecHandler;

#[async_trait]
impl CapabilityHandler for ShellExecHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, SHELL_OPERATION)?;
        let request_id = request.request.request_id;
        let arguments = &request.request.arguments;
        let sandbox = argument_sandbox(arguments)?;
        let project_root = canonical_project_root(argument_string(arguments, "project_root")?)?;
        let workdir = confined_workdir(&project_root, arguments.get("workdir"))?;
        let argv = command_argv(arguments.get("command"))?;
        let timeout = command_timeout(arguments);
        let output = run_confined(argv, &project_root, &workdir, sandbox, timeout).await?;
        Ok(CapabilityResult::success(request_id, output))
    }
}

struct ApplyPatchHandler;

#[async_trait]
impl CapabilityHandler for ApplyPatchHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, APPLY_PATCH_OPERATION)?;
        let request_id = request.request.request_id;
        let arguments = &request.request.arguments;
        let sandbox = argument_sandbox(arguments)?;
        if sandbox != HARNESS_SANDBOX_WORKSPACE_WRITE {
            return Err(PortError::Failed(
                "apply_patch_requires_workspace_write".to_owned(),
            ));
        }
        let project_root = canonical_project_root(argument_string(arguments, "project_root")?)?;
        let patch = argument_string(arguments, "patch")?.to_owned();
        let output = tokio::task::spawn_blocking(move || apply_codex_patch(&project_root, &patch))
            .await
            .map_err(|error| PortError::Failed(format!("apply_patch_join_failed:{error}")))??;
        Ok(CapabilityResult::success(request_id, output))
    }
}

fn ensure_operation(
    request: &AuthorizedCapabilityRequest,
    expected: &str,
) -> Result<(), PortError> {
    if request.request.operation != expected {
        return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
    }
    Ok(())
}

fn argument_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, PortError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed(format!("harness_argument_required:{key}")))
}

fn argument_sandbox(arguments: &Value) -> Result<&str, PortError> {
    match arguments
        .get("sandbox")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_HARNESS_SANDBOX)
        .trim()
    {
        "" | DEFAULT_HARNESS_SANDBOX => Ok(DEFAULT_HARNESS_SANDBOX),
        HARNESS_SANDBOX_WORKSPACE_WRITE => Ok(HARNESS_SANDBOX_WORKSPACE_WRITE),
        other => Err(PortError::Failed(format!("sandbox_unsupported:{other}"))),
    }
}

fn canonical_project_root(project_root: &str) -> Result<PathBuf, PortError> {
    let project_root = PathBuf::from(project_root)
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("harness_project_root_invalid:{error}")))?;
    if !project_root.is_dir() {
        return Err(PortError::Failed(
            "harness_project_root_invalid:not_directory".to_owned(),
        ));
    }
    Ok(project_root)
}

fn confined_workdir(project_root: &Path, workdir: Option<&Value>) -> Result<PathBuf, PortError> {
    let Some(value) = workdir.filter(|value| !value.is_null()) else {
        return Ok(project_root.to_path_buf());
    };
    let requested = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed("harness_workdir_invalid".to_owned()))?;
    let requested = Path::new(requested);
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        if requested.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(PortError::Failed("harness_workdir_not_relative".to_owned()));
        }
        project_root.join(requested)
    };
    let resolved = candidate
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("harness_workdir_invalid:{error}")))?;
    if !resolved.starts_with(project_root) {
        return Err(PortError::Failed(
            "harness_workdir_outside_project".to_owned(),
        ));
    }
    if !resolved.is_dir() {
        return Err(PortError::Failed(
            "harness_workdir_invalid:not_directory".to_owned(),
        ));
    }
    Ok(resolved)
}

fn command_argv(command: Option<&Value>) -> Result<Vec<String>, PortError> {
    let command =
        command.ok_or_else(|| PortError::Failed("harness_command_required".to_owned()))?;
    if let Some(value) = command.as_str() {
        let value = value.trim();
        if value.is_empty() {
            return Err(PortError::Failed("harness_command_required".to_owned()));
        }
        return Ok(vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            value.to_owned(),
        ]);
    }
    if let Some(items) = command.as_array() {
        let argv = items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| PortError::Failed("harness_command_invalid".to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if argv.is_empty() || argv.iter().all(|item| item.trim().is_empty()) {
            return Err(PortError::Failed("harness_command_required".to_owned()));
        }
        return Ok(argv);
    }
    Err(PortError::Failed("harness_command_invalid".to_owned()))
}

async fn run_confined(
    argv: Vec<String>,
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    timeout: Duration,
) -> Result<Value, PortError> {
    let mut command = sandboxed_command(&argv, project_root, workdir, sandbox)?;
    prepare_process_group(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| PortError::Failed(format!("shell_exec_failed:{error}")))?;
    let mut process_group = ProcessGroupGuard::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PortError::Failed("shell_exec_failed:stdout_pipe_unavailable".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PortError::Failed("shell_exec_failed:stderr_pipe_unavailable".to_owned()))?;
    let mut stdout_handle = tokio::spawn(read_capped(stdout, EXEC_OUTPUT_MAX_BYTES));
    let mut stderr_handle = tokio::spawn(read_capped(stderr, EXEC_OUTPUT_MAX_BYTES));
    // Codex exec.rs consume_output: timeout is an exec outcome (exit 124,
    // timed_out=true), not a capability crash. Kill the child, then drain
    // pipes with IO_DRAIN_TIMEOUT so inherited fds cannot hang the agent.
    let (exit_code, timed_out, stop_confirmed) = tokio::select! {
        status = child.wait() => {
            let status = status
                .map_err(|error| PortError::Failed(format!("shell_exec_failed:{error}")))?;
            (status.code().unwrap_or(-1), false, true)
        }
        _ = tokio::time::sleep(timeout) => {
            let stop_confirmed = terminate_process_group(&mut child, process_group.id()).await;
            if !stop_confirmed {
                return Err(PortError::Failed(
                    "shell_result_unknown:process_group_not_stopped".to_owned(),
                ));
            }
            (EXEC_TIMEOUT_EXIT_CODE, true, stop_confirmed)
        }
    };
    process_group.finish_if_stopped();
    let stdout = drain_capped(&mut stdout_handle).await;
    let stderr = drain_capped(&mut stderr_handle).await;
    Ok(json!({
        "stdout": render_capped(&stdout.bytes, stdout.truncated),
        "stderr": render_capped(&stderr.bytes, stderr.truncated),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "stop_confirmed": stop_confirmed,
        "sandbox": sandbox,
        "backend": crate::harness_sandbox::SANDBOX_BACKEND,
    }))
}

struct CappedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn prepare_process_group(command: &mut Command) {
    #[cfg(unix)]
    command.process_group(0);
}

struct ProcessGroupGuard {
    pid: Option<u32>,
    active: bool,
}

impl ProcessGroupGuard {
    fn new(pid: Option<u32>) -> Self {
        Self { pid, active: true }
    }

    fn id(&self) -> Option<u32> {
        self.pid
    }

    fn finish_if_stopped(&mut self) {
        #[cfg(unix)]
        {
            if self.pid.is_none_or(|pid| !process_group_exists(pid)) {
                self.active = false;
            }
        }
        #[cfg(not(unix))]
        {
            self.active = false;
        }
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            signal_process_group(pid, libc::SIGKILL);
        }
    }
}

async fn terminate_process_group(child: &mut Child, pid: Option<u32>) -> bool {
    let Some(pid) = pid else {
        if child.start_kill().is_err() {
            return false;
        }
        return child.wait().await.is_ok();
    };

    #[cfg(unix)]
    {
        if !signal_process_group(pid, libc::SIGTERM) {
            let _ = child.start_kill();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.start_kill();
    }

    let mut leader_reaped = false;
    if tokio::time::timeout(PROCESS_GROUP_TERM_GRACE, child.wait())
        .await
        .is_ok()
    {
        leader_reaped = true;
    }

    #[cfg(unix)]
    if process_group_exists(pid) {
        signal_process_group(pid, libc::SIGKILL);
    }
    if !leader_reaped {
        let _ = child.wait().await;
    }

    #[cfg(unix)]
    {
        let deadline = tokio::time::Instant::now() + PROCESS_GROUP_EXIT_GRACE;
        while process_group_exists(pid) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        return leader_reaped && !process_group_exists(pid);
    }
    #[cfg(not(unix))]
    {
        leader_reaped
    }
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: libc::c_int) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    unsafe { libc::kill(-pgid, signal) == 0 }
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    let result = unsafe { libc::kill(-pgid, 0) };
    result == 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

async fn read_capped<R>(mut reader: R, max_bytes: usize) -> std::io::Result<CappedOutput>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(READ_CHUNK_SIZE.min(max_bytes));
    let mut tmp = [0u8; READ_CHUNK_SIZE];
    let mut truncated = false;
    loop {
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        if bytes.len() >= max_bytes {
            truncated = true;
            continue;
        }
        let take = (max_bytes - bytes.len()).min(n);
        bytes.extend_from_slice(&tmp[..take]);
        if take < n {
            truncated = true;
        }
    }
    Ok(CappedOutput { bytes, truncated })
}

async fn drain_capped(handle: &mut JoinHandle<std::io::Result<CappedOutput>>) -> CappedOutput {
    match tokio::time::timeout(IO_DRAIN_TIMEOUT, &mut *handle).await {
        Ok(Ok(Ok(output))) => output,
        Ok(Ok(Err(_))) | Ok(Err(_)) => CappedOutput {
            bytes: Vec::new(),
            truncated: false,
        },
        Err(_) => {
            handle.abort();
            CappedOutput {
                bytes: Vec::new(),
                truncated: false,
            }
        }
    }
}

fn render_capped(bytes: &[u8], truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if truncated {
        text.push_str("\n...truncated...");
    }
    text
}

fn sandboxed_command(
    argv: &[String],
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
) -> Result<Command, PortError> {
    if argv.is_empty() {
        return Err(PortError::Failed("harness_command_required".to_owned()));
    }
    let plan = crate::harness_sandbox::bwrap_plan(project_root, workdir, sandbox)?;
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    command.arg(&argv[0]);
    command.args(&argv[1..]);
    // Codex spawn.rs: env_clear the helper process so host secrets never sit
    // in bwrap's /proc/pid/environ. Inner command env still comes from
    // bwrap --clearenv/--setenv in the sandbox plan.
    command.env_clear();
    for (key, value) in crate::harness_sandbox::sandbox_env(std::env::vars(), sandbox) {
        command.env(key, value);
    }
    Ok(command)
}

fn command_timeout(arguments: &Value) -> Duration {
    arguments
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .map(|value| Duration::from_millis(value.min(60_000)))
        .unwrap_or(COMMAND_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{CapabilityRequest, RequestId};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-shell-{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn authorized(
        kind: CapabilityKind,
        operation: &str,
        arguments: Value,
    ) -> AuthorizedCapabilityRequest {
        AuthorizedCapabilityRequest::new(
            "policy:test",
            CapabilityRequest::new(RequestId::new(), kind, operation, arguments),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn shell_exec_uses_codex_core_env_and_strips_secrets() {
        let root = temp_root();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": ["printenv"],
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap();
        assert!(result.success, "{}", result.output);
        let stdout = result.output["stdout"].as_str().unwrap_or_default();
        let names: Vec<&str> = stdout
            .lines()
            .filter_map(|line| line.split_once('=').map(|(name, _)| name))
            .collect();
        assert!(
            names.iter().any(|name| name.eq_ignore_ascii_case("PATH")),
            "{stdout}"
        );
        let values: std::collections::BTreeMap<&str, &str> = stdout
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        assert_eq!(
            values.get("KIANA_SANDBOX").copied(),
            Some("read-only"),
            "{stdout}"
        );
        assert_eq!(
            values.get("KIANA_SANDBOX_BACKEND").copied(),
            Some("bwrap"),
            "{stdout}"
        );
        assert_eq!(
            values.get("KIANA_SANDBOX_NETWORK_DISABLED").copied(),
            Some("1"),
            "{stdout}"
        );
        assert!(
            !names.iter().any(|name| *name == "KIANA_HARNESS_SCRIPT"),
            "{stdout}"
        );
        assert!(
            names.iter().all(|name| {
                let upper = name.to_ascii_uppercase();
                !(upper.contains("KEY") || upper.contains("SECRET") || upper.contains("TOKEN"))
            }),
            "{stdout}"
        );
    }

    #[tokio::test]
    async fn shell_exec_returns_stdout_from_project_root() {
        let root = temp_root();
        fs::write(root.join("marker.txt"), "ok").unwrap();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "ls",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            result.output["stdout"]
                .as_str()
                .unwrap_or_default()
                .contains("marker.txt"),
            "{}",
            result.output
        );
    }

    #[tokio::test]
    async fn shell_exec_rejects_workdir_escape() {
        let root = temp_root();
        let error = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "ls",
                    "workdir": "..",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Failed("harness_workdir_not_relative".to_owned())
        );
    }

    #[tokio::test]
    async fn apply_patch_is_rejected_in_read_only_sandbox() {
        let root = temp_root();
        let error = ApplyPatchHandler
            .execute(authorized(
                CapabilityKind::Filesystem,
                APPLY_PATCH_OPERATION,
                json!({
                    "patch": "*** Begin Patch\n*** Add File: x.txt\n+x\n*** End Patch\n",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Failed("apply_patch_requires_workspace_write".to_owned())
        );
    }

    #[tokio::test]
    async fn apply_patch_writes_in_workspace_write_sandbox() {
        let root = temp_root();
        let result = ApplyPatchHandler
            .execute(authorized(
                CapabilityKind::Filesystem,
                APPLY_PATCH_OPERATION,
                json!({
                    "patch": "*** Begin Patch\n*** Add File: created.txt\n+from-harness\n*** End Patch\n",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "workspace-write",
                }),
            ))
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(
            fs::read_to_string(root.join("created.txt")).unwrap(),
            "from-harness\n"
        );
    }

    #[tokio::test]
    async fn shell_exec_read_only_cannot_write_workspace() {
        let root = temp_root();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "echo x > written.txt",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap();
        assert!(result.success);
        assert_ne!(result.output["exit_code"], 0);
        assert_eq!(result.output["backend"], "bwrap");
        assert!(!root.join("written.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn shell_exec_timeout_ms_returns_codex_timed_out_result() {
        let root = temp_root();
        let started = std::time::Instant::now();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "sleep 8",
                    "timeout_ms": 250,
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap();
        let elapsed = started.elapsed();
        assert!(result.success, "{}", result.output);
        assert_eq!(result.output["timed_out"], true);
        assert_eq!(result.output["exit_code"], 124);
        assert_eq!(result.output["backend"], "bwrap");
        assert!(
            elapsed.as_millis() < 4500,
            "timeout should kill the sandboxed sleep within Codex IO drain, elapsed={elapsed:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_timeout_terminates_descendant_processes() {
        let root = temp_root();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "printf '%s' $$ > group.pid; sleep 60 & child=$!; printf '%s' $child > child.pid; wait $child",
                    "timeout_ms": 250,
                    "project_root": root.to_string_lossy(),
                    "sandbox": "workspace-write",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(result.output["timed_out"], true, "{}", result.output);
        let group_pid = fs::read_to_string(root.join("group.pid"))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while process_group_exists(group_pid) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            !process_group_exists(group_pid),
            "process group {group_pid} survived"
        );
    }

    #[tokio::test]
    async fn shell_exec_caps_stdout_at_codex_one_mib_while_reading() {
        let root = temp_root();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": ["head", "-c", "2000000", "/dev/zero"],
                    "timeout_ms": 5000,
                    "project_root": root.to_string_lossy(),
                    "sandbox": "read-only",
                }),
            ))
            .await
            .unwrap();
        assert!(result.success, "{}", result.output);
        assert_eq!(result.output["timed_out"], false, "{}", result.output);
        let stdout = result.output["stdout"].as_str().unwrap_or_default();
        assert!(
            stdout.contains("...truncated..."),
            "expected Codex-style truncation marker: {}",
            result.output
        );
        assert!(
            stdout.len() <= EXEC_OUTPUT_MAX_BYTES + "...truncated...".len() + 1,
            "stdout exceeded Codex 1MiB cap: {}",
            stdout.len()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn shell_exec_workspace_write_can_write_workspace() {
        let root = temp_root();
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "echo from-shell > written.txt",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "workspace-write",
                }),
            ))
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output["exit_code"], 0);
        assert_eq!(
            fs::read_to_string(root.join("written.txt")).unwrap(),
            "from-shell\n"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
