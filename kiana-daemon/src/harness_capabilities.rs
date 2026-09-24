//! Daemon-owned execution for Codex-shaped harness tools.
//!
//! `shell.exec` and `apply_patch` are registered on the capability broker.
//! The runner never executes these tools.

use crate::apply_patch::{apply_codex_patch, preview_codex_patch};
use crate::execution_output::{drain_capped, metadata, read_capped, render_capped_for};
use crate::process_supervisor::ProcessSupervisor;
use crate::shell_plan::ShellCommandPlan;
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AdapterCommitState, AdapterResultKind, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityResult,
};
use kiana_ports::PortError;
use kiana_runner_protocol::{DEFAULT_HARNESS_SANDBOX, HARNESS_SANDBOX_WORKSPACE_WRITE};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

const SHELL_OPERATION: &str = "shell.exec";
const APPLY_PATCH_OPERATION: &str = "apply_patch";
const APPLY_PATCH_PREVIEW_OPERATION: &str = "apply_patch.preview";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const EXEC_TIMEOUT_EXIT_CODE: i32 = 124;

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
    broker.register_static(
        CapabilityKind::Filesystem,
        APPLY_PATCH_PREVIEW_OPERATION,
        Arc::new(ApplyPatchPreviewHandler),
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
        execute_shell(request, None).await
    }

    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        execute_shell(request, Some(cancellation)).await
    }
}

async fn execute_shell(
    mut request: AuthorizedCapabilityRequest,
    cancellation: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<CapabilityResult, PortError> {
    ensure_operation(&request, SHELL_OPERATION)?;
    let root = Path::new(argument_string(&request.request.arguments, "project_root")?);
    let revision = crate::data_governance::read_policy(root)?.revision;
    request.request.arguments["source_data_revision"] = json!(revision);
    let request_id = request.request.request_id;
    let arguments = &request.request.arguments;
    let sandbox = argument_sandbox(arguments)?;
    let project_root = canonical_project_root(argument_string(arguments, "project_root")?)?;
    let workdir = confined_workdir(&project_root, arguments.get("workdir"))?;
    let command_plan = ShellCommandPlan::from_value(arguments.get("command"))?;
    let argv = command_plan.argv.clone();
    let timeout = command_timeout(arguments);
    let mut output = if sandbox == HARNESS_SANDBOX_WORKSPACE_WRITE {
        let owned = request.clone();
        let root = project_root.clone();
        let directory = workdir.clone();
        let mut workspace = tokio::task::spawn_blocking(move || {
            crate::execution_workspace::ExecutionWorkspace::prepare(&owned, &root, &directory)
        })
        .await
        .map_err(|_| PortError::Failed("execution_workspace_prepare_join_failed".to_owned()))??;
        let mut execution_scope = arguments.clone();
        execution_scope["path_allow"] = json!(["."]);
        execution_scope["logical_project_root"] = json!(project_root);
        let mut output = match run_confined_cancellable(
            argv,
            &workspace.root,
            &workspace.workdir,
            sandbox,
            timeout,
            cancellation.clone(),
            Some(&execution_scope),
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                if kiana_domain::CapabilityErrorCode::from_reason(&error.to_string())
                    .policy()
                    .requires_reconciliation
                {
                    workspace.retain_unknown();
                }
                return Err(error);
            }
        };
        let successful = output["exit_code"] == 0
            && output["cancelled"] != true
            && output["timed_out"] != true
            && output["stdout_metadata"]["read_error"].is_null()
            && output["stderr_metadata"]["read_error"].is_null()
            && !cancellation
                .as_ref()
                .is_some_and(|receiver| *receiver.borrow());
        let publication = tokio::task::spawn_blocking(move || workspace.finish(successful))
            .await
            .map_err(|_| {
                PortError::Failed("result_unknown:workspace_publication_join_failed".to_owned())
            })??;
        output["workspace"] = publication;
        output
    } else {
        run_confined_cancellable(
            argv,
            &project_root,
            &workdir,
            sandbox,
            timeout,
            cancellation,
            Some(arguments),
        )
        .await?
    };
    let owned = request.clone();
    output = tokio::task::spawn_blocking(move || {
        crate::execution_control::store_output(&owned, &mut output)?;
        Ok::<_, PortError>(output)
    })
    .await
    .map_err(|_| PortError::Failed("result_unknown:output_store_join_failed".to_owned()))??;
    output["shell_plan"] = command_plan.metadata();
    let mut result = CapabilityResult::success(request_id, output.clone());
    if output["exit_code"].as_i64().is_some_and(|code| code != 0) && output["cancelled"] != true {
        result.success = false;
        output["error"] = json!(if output["timed_out"] == true {
            "timed_out:shell"
        } else {
            "execution_failed:shell_exit"
        });
        result.output = output;
    } else if !output["stdout_metadata"]["read_error"].is_null()
        || !output["stderr_metadata"]["read_error"].is_null()
    {
        result.success = false;
        result.output["error"] = json!("execution_failed:output_capture_incomplete");
    }
    kiana_domain::attach_adapter_result(
        result,
        AdapterResultKind::Shell,
        AdapterCommitState::Committed,
    )
    .map_err(|error| PortError::Failed(format!("adapter_result_invalid:{error}")))
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
        let (project_root, patch) = prepare_patch_request(arguments, APPLY_PATCH_OPERATION)?;
        let output = tokio::task::spawn_blocking(move || apply_codex_patch(&project_root, &patch))
            .await
            .map_err(|error| PortError::Failed(format!("apply_patch_join_failed:{error}")))??;
        let result = CapabilityResult::success(request_id, output);
        kiana_domain::attach_adapter_result(
            result,
            AdapterResultKind::Patch,
            AdapterCommitState::Committed,
        )
        .map_err(|error| PortError::Failed(format!("adapter_result_invalid:{error}")))
    }
}

struct ApplyPatchPreviewHandler;

#[async_trait]
impl CapabilityHandler for ApplyPatchPreviewHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, APPLY_PATCH_PREVIEW_OPERATION)?;
        let request_id = request.request.request_id;
        let arguments = &request.request.arguments;
        let (project_root, patch) =
            prepare_patch_request(arguments, APPLY_PATCH_PREVIEW_OPERATION)?;
        let output =
            tokio::task::spawn_blocking(move || preview_codex_patch(&project_root, &patch))
                .await
                .map_err(|error| {
                    PortError::Failed(format!("apply_patch_preview_join_failed:{error}"))
                })??;
        let result = CapabilityResult::success(request_id, output);
        kiana_domain::attach_adapter_result(
            result,
            AdapterResultKind::Patch,
            AdapterCommitState::NotStarted,
        )
        .map_err(|error| PortError::Failed(format!("adapter_result_invalid:{error}")))
    }
}

fn prepare_patch_request(
    arguments: &Value,
    _operation: &str,
) -> Result<(PathBuf, String), PortError> {
    let sandbox = argument_sandbox(arguments)?;
    if sandbox != HARNESS_SANDBOX_WORKSPACE_WRITE {
        return Err(PortError::Failed(
            "apply_patch_requires_workspace_write".to_owned(),
        ));
    }
    let project_root = canonical_project_root(argument_string(arguments, "project_root")?)?;
    let patch = argument_string(arguments, "patch")?.to_owned();
    let paths = arguments["path_allow"]
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let data_policy = crate::data_governance::read_policy(&project_root)?;
    for line in patch.lines() {
        let target = [
            "*** Add File: ",
            "*** Update File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix));
        if let Some(target) = target {
            let target = kiana_domain::normalize_role_path(target)
                .ok_or_else(|| PortError::Failed("apply_patch_path_invalid".to_owned()))?;
            if target == ".git"
                || target.starts_with(".git/")
                || target == ".kiana"
                || target.starts_with(".kiana/")
            {
                return Err(PortError::Failed("apply_patch_protected_path".to_owned()));
            }
            if kiana_domain::enforce_path_containment(&paths, &target).is_err()
                || data_policy.revoked_sources.contains(&target)
            {
                return Err(PortError::Failed("apply_patch_scope_denied".to_owned()));
            }
        }
    }
    Ok((project_root, patch))
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

pub(crate) fn confined_workdir(
    project_root: &Path,
    workdir: Option<&Value>,
) -> Result<PathBuf, PortError> {
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
    kiana_domain::enforce_root_containment(project_root, &resolved)
        .map_err(|_| PortError::Failed("harness_workdir_outside_project".to_owned()))?;
    if !resolved.is_dir() {
        return Err(PortError::Failed(
            "harness_workdir_invalid:not_directory".to_owned(),
        ));
    }
    Ok(resolved)
}

pub(crate) fn command_argv(command: Option<&Value>) -> Result<Vec<String>, PortError> {
    Ok(ShellCommandPlan::from_value(command)?.argv)
}

#[cfg(test)]
async fn run_confined(
    argv: Vec<String>,
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    timeout: Duration,
) -> Result<Value, PortError> {
    run_confined_cancellable(argv, project_root, workdir, sandbox, timeout, None, None).await
}

pub(crate) async fn run_confined_cancellable(
    argv: Vec<String>,
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    timeout: Duration,
    mut cancellation: Option<tokio::sync::watch::Receiver<bool>>,
    scope: Option<&Value>,
) -> Result<Value, PortError> {
    if cancellation.as_ref().is_some_and(|rx| *rx.borrow()) {
        return Ok(
            json!({"cancelled":true,"not_executed":true,"effect_started":false,
                "effect_known":true,"zero_effect":true,"stop_state":"confirmed",
                "stop_confirmed":true,"fenced":false,"exit_code":130}),
        );
    }
    let mut command = sandboxed_command_scoped(&argv, project_root, workdir, sandbox, scope)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| PortError::Failed(format!("shell_exec_failed:{error}")))?;
    let mut process_guard = ProcessSupervisor::guard(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PortError::Failed("shell_exec_failed:stdout_pipe_unavailable".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PortError::Failed("shell_exec_failed:stderr_pipe_unavailable".to_owned()))?;
    let output_budget = kiana_domain::ExecutionOutputBudget::default();
    let mut stdout_handle = tokio::spawn(read_capped(stdout, output_budget.clone()));
    let mut stderr_handle = tokio::spawn(read_capped(stderr, output_budget.clone()));
    // Codex exec.rs consume_output: timeout is an exec outcome (exit 124,
    // timed_out=true), not a capability crash. Kill the child, then drain
    // pipes with IO_DRAIN_TIMEOUT so inherited fds cannot hang the agent.
    let execution_id = scope
        .and_then(|value| value["execution_id"].as_str())
        .unwrap_or("shell.exec");
    let (exit_code, timed_out, cancelled, stop_report) = tokio::select! {
        biased;
        _ = async {
            if let Some(rx) = cancellation.as_mut() { kiana_ports::wait_for_cancellation(rx).await; }
            else { std::future::pending::<()>().await; }
        } => {
            let process_group = child.id();
            let report = ProcessSupervisor::stop(execution_id, &mut child, process_group).await;
            if !report.confirmed { return Err(PortError::Failed("shell_result_unknown:cancel_stop_unconfirmed".to_owned())); }
            (130,false,true,report)
        }
        status = child.wait() => {
            let status = status
                .map_err(|error| PortError::Failed(format!("shell_exec_failed:{error}")))?;
            let process_group = child.id();
            let report = ProcessSupervisor::stop(execution_id, &mut child, process_group).await;
            if !report.confirmed { return Err(PortError::Failed("shell_result_unknown:process_group_not_stopped".to_owned())); }
            (status.code().unwrap_or(-1), false, false, report)
        }
        _ = tokio::time::sleep(timeout) => {
            let process_group = child.id();
            let report = ProcessSupervisor::stop(execution_id, &mut child, process_group).await;
            if !report.confirmed {
                return Err(PortError::Failed(
                    "shell_result_unknown:process_group_not_stopped".to_owned(),
                ));
            }
            (EXEC_TIMEOUT_EXIT_CODE, true, false, report)
        }
    };
    process_guard.disarm();
    let stdout = drain_capped(&mut stdout_handle).await;
    let stderr = drain_capped(&mut stderr_handle).await;
    let process_budget = kiana_domain::ProcessResourceBudget::for_wall_time_ms(
        timeout.as_millis().min(u128::from(u64::MAX)) as u64,
    );
    Ok(json!({
        "stdout": render_capped_for(kiana_domain::SecretScanChannel::Stdout, &stdout.bytes, stdout.truncated, output_budget.preview_max_bytes),
        "stderr": render_capped_for(kiana_domain::SecretScanChannel::Stderr, &stderr.bytes, stderr.truncated, output_budget.preview_max_bytes),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "cancelled": cancelled,
        "effect_started": true,
        "effect_known": true,
        "zero_effect": false,
        "stop_state": if cancelled { if stop_report.confirmed { "confirmed" } else { "unconfirmed" } } else { "not_requested" },
        "stop_confirmed": stop_report.confirmed,
        "stop_report": stop_report,
        "resource_budget": ProcessSupervisor::resource_receipt(&process_budget),
        "stdout_metadata": metadata(&stdout, &output_budget),
        "stderr_metadata": metadata(&stderr, &output_budget),
        "sandbox": sandbox,
        "backend": crate::harness_sandbox::SANDBOX_BACKEND,
    }))
}

#[cfg(test)]
fn sandboxed_command(
    argv: &[String],
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
) -> Result<Command, PortError> {
    sandboxed_command_scoped(argv, project_root, workdir, sandbox, None)
}

pub(crate) fn sandboxed_command_scoped(
    argv: &[String],
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    scope: Option<&Value>,
) -> Result<Command, PortError> {
    if argv.is_empty() {
        return Err(PortError::Failed("harness_command_required".to_owned()));
    }
    let paths = scope
        .and_then(|scope| scope.get("path_allow"))
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![".".to_owned()]);
    let mut plan =
        crate::harness_sandbox::bwrap_plan_scoped(project_root, workdir, sandbox, &paths)?;
    if let Some(logical) = scope.and_then(|value| value["logical_project_root"].as_str()) {
        plan.remap_workspace(project_root, Path::new(logical));
    }
    let mut command = Command::new(&plan.program);
    plan.configure_command(&mut command);
    command.arg(&argv[0]);
    command.args(&argv[1..]);
    // Codex spawn.rs: env_clear the helper process so host secrets never sit
    // in bwrap's /proc/pid/environ. Inner command env still comes from
    // bwrap --clearenv/--setenv in the sandbox plan.
    command.env_clear();
    for (key, value) in crate::harness_sandbox::sandbox_env(std::env::vars(), sandbox) {
        command.env(key, value);
    }
    let timeout_ms = scope
        .and_then(|value| value["timeout_ms"].as_u64())
        .unwrap_or(COMMAND_TIMEOUT.as_millis() as u64);
    let budget = kiana_domain::ProcessResourceBudget::for_wall_time_ms(timeout_ms);
    ProcessSupervisor::prepare_command(&mut command, &budget)?;
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
        while ProcessSupervisor::process_group_exists(group_pid)
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            !ProcessSupervisor::process_group_exists(group_pid),
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
            stdout.len()
                <= kiana_domain::ExecutionOutputBudget::default().collect_max_bytes
                    + "...truncated...".len()
                    + 1,
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
