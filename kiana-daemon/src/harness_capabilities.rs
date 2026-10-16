//! Daemon 侧执行 Harness 授权后的 shell 与 patch capability。
//!
//! 上游由 `ControlPlane` 完成 capability 授权后调用 Broker；本文件通过 `register` 注册三个 handler，
//! 分别执行 shell、应用 patch 和预览 patch。Runner 只提交工具意图，不在自身进程内直接启动命令或写文件。
//!
//! shell 路径负责项目根目录/工作目录约束、sandbox 命令构造、超时与取消、进程组停止确认、输出截断和结果落盘；
//! patch 路径将路径授权与数据治理校验后委托给 `apply_patch` 模块。阅读时先区分 `execute_shell` 的
//! workspace-write 临时工作区发布流程与 `run_confined_cancellable` 的底层进程生命周期。

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
    // 操作名和 capability kind 必须与 Core/Broker 发来的授权请求一致；注册只建立分发，不授予调用权限。
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
    /// 普通执行不附加取消通道；Broker 仍通过同一 execute_shell 校验/执行路径。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        execute_shell(request, None).await
    }

    /// 带取消 watch 执行；取消必须进入同一进程监督路径并等待停止确认。
    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        execute_shell(request, Some(cancellation)).await
    }
}

/// 校验 shell capability、在指定 sandbox 内运行命令，并整理成可记录的 CapabilityResult。
///
/// workspace-write 通过临时 ExecutionWorkspace 运行，只有退出码、输出捕获和取消状态都成功时才发布改动；
/// read-only 则直接在 canonical 项目路径内运行。此函数会读取数据治理策略、创建/停止子进程，并写执行输出。
async fn execute_shell(
    mut request: AuthorizedCapabilityRequest,
    cancellation: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<CapabilityResult, PortError> {
    // handler 名称二次核对，避免误注册或错误路由把另一种 capability 当作 shell 执行。
    ensure_operation(&request, SHELL_OPERATION)?;
    let root = Path::new(argument_string(&request.request.arguments, "project_root")?);
    let revision = crate::data_governance::read_policy(root)?.revision;
    request.request.arguments["source_data_revision"] = json!(revision);
    let request_id = request.request.request_id;
    let arguments = &request.request.arguments;
    // sandbox/workdir/argv 都在启动进程前完成解析，不把未经校验的路径或命令传给 spawn。
    let sandbox = argument_sandbox(arguments)?;
    let project_root = canonical_project_root(argument_string(arguments, "project_root")?)?;
    let workdir = confined_workdir(&project_root, arguments.get("workdir"))?;
    let command_plan = ShellCommandPlan::from_value(arguments.get("command"))?;
    let argv = command_plan.argv.clone();
    let timeout = command_timeout(arguments);
    let mut output = if sandbox == HARNESS_SANDBOX_WORKSPACE_WRITE {
        // 写模式在独立临时工作区运行，后续只有判定 successful 后 workspace.finish 才发布结果。
        let owned = request.clone();
        let root = project_root.clone();
        let directory = workdir.clone();
        let mut workspace = tokio::task::spawn_blocking(move || {
            crate::execution_workspace::ExecutionWorkspace::prepare(&owned, &root, &directory)
        })
        .await
        .map_err(|_| PortError::Failed("execution_workspace_prepare_join_failed".to_owned()))??;
        // sandbox 内 path_allow 限定为临时 workspace 根；logical root 映射保留命令看到的项目路径语义。
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
                // 结果需要 reconciliation 时保留工作区，避免丢失可能已产生但未确认的改动。
                if kiana_domain::CapabilityErrorCode::from_reason(&error.to_string())
                    .policy()
                    .requires_reconciliation
                {
                    workspace.retain_unknown();
                }
                return Err(error);
            }
        };
        // 只有执行成功、未取消/超时、输出完整且取消 watch 未触发时才发布临时工作区写集。
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
        // publication receipt 描述临时工作区如何落到项目树，与 stdout/stderr 分开保存。
        output["workspace"] = publication;
        output
    } else {
        // read-only 模式不创建写工作区，但仍由 sandbox plan 限制项目路径和环境变量。
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
    // 执行输出交给 execution_control 统一存储、脱敏和缩减，再构造 capability 结果。
    let owned = request.clone();
    output = tokio::task::spawn_blocking(move || {
        crate::execution_control::store_output(&owned, &mut output)?;
        Ok::<_, PortError>(output)
    })
    .await
    .map_err(|_| PortError::Failed("result_unknown:output_store_join_failed".to_owned()))??;
    // shell plan 只用于解释命令分类，不会改变已执行 argv。
    output["shell_plan"] = command_plan.metadata();
    let mut result = CapabilityResult::success(request_id, output.clone());
    if output["exit_code"].as_i64().is_some_and(|code| code != 0) && output["cancelled"] != true {
        // 非零退出是已知执行失败，不是 handler crash；timeout 使用专门错误标签区分。
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
        // 即便进程退出码为 0，只要输出管道没有完整读取，结果就不能标成成功。
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

/// Broker handler：在 workspace-write 权限下执行 Codex patch，并把结果标记为已提交。
struct ApplyPatchHandler;

#[async_trait]
impl CapabilityHandler for ApplyPatchHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        // operation、sandbox、project root、patch 路径范围先校验，再把文件 I/O 移出 Tokio worker。
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

/// Broker handler：只计算 patch 预览，不把文件改动声明为已提交。
struct ApplyPatchPreviewHandler;

#[async_trait]
impl CapabilityHandler for ApplyPatchPreviewHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        // 预览与应用共享请求校验，但只调用纯预览入口，不执行文件写入。
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

/// 校验 patch 所需 sandbox、项目根、文件范围与数据治理撤销集合。
///
/// 通过后返回 canonical root 和原始 patch 文本，具体 diff 解析/应用由 `apply_codex_patch` 执行。
fn prepare_patch_request(
    arguments: &Value,
    _operation: &str,
) -> Result<(PathBuf, String), PortError> {
    // patch 有写副作用，只能在明确 workspace-write sandbox 下进行。
    let sandbox = argument_sandbox(arguments)?;
    if sandbox != HARNESS_SANDBOX_WORKSPACE_WRITE {
        return Err(PortError::Failed(
            "apply_patch_requires_workspace_write".to_owned(),
        ));
    }
    let project_root = canonical_project_root(argument_string(arguments, "project_root")?)?;
    let patch = argument_string(arguments, "patch")?.to_owned();
    // path_allow 是上游授权给本次 capability 的写范围；缺失时为空集合，后续 containment 会拒绝目标。
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
        // 只检查 Codex patch 的文件操作头；上下文行和增删内容不是目标路径。
        let target = [
            "*** Add File: ",
            "*** Update File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix));
        if let Some(target) = target {
            // 标准化拒绝绝对路径、parent traversal 等非项目相对目标。
            let target = kiana_domain::normalize_role_path(target)
                .ok_or_else(|| PortError::Failed("apply_patch_path_invalid".to_owned()))?;
            if target == ".git"
                || target.starts_with(".git/")
                || target == ".kiana"
                || target.starts_with(".kiana/")
            {
                // Git 状态与项目本地控制配置不允许由模型 patch 直接改写。
                return Err(PortError::Failed("apply_patch_protected_path".to_owned()));
            }
            // 写范围必须包含该目标，且被撤销数据源不能由 patch 绕过治理规则重新写入。
            if kiana_domain::enforce_path_containment(&paths, &target).is_err()
                || data_policy.revoked_sources.contains(&target)
            {
                return Err(PortError::Failed("apply_patch_scope_denied".to_owned()));
            }
        }
    }
    Ok((project_root, patch))
}

/// 验证请求 operation 与当前 handler 注册的常量一致，防止错误路由被当作本地工具执行。
fn ensure_operation(
    request: &AuthorizedCapabilityRequest,
    expected: &str,
) -> Result<(), PortError> {
    if request.request.operation != expected {
        return Err(PortError::Failed("harness_operation_mismatch".to_owned()));
    }
    Ok(())
}

/// 从 JSON 参数读取去除首尾空白后的非空字符串；错误带字段名便于定位调用契约问题。
fn argument_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, PortError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed(format!("harness_argument_required:{key}")))
}

/// 把请求中的 sandbox 名称限制到当前 Harness 明确支持的两种配置。
/// 缺省/空白按最小权限 read-only 处理，其它未识别值显式拒绝。
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

/// 将项目根路径解析为真实 canonical 路径，并确认目标确实是目录。
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

/// 解析 shell 工作目录并确保最终 canonical 目录仍位于 canonical project root 内。
///
/// 相对路径不允许 `..`、root 或平台 prefix；绝对路径可接受，但最终 containment 检查仍会拒绝越界。
pub(crate) fn confined_workdir(
    project_root: &Path,
    workdir: Option<&Value>,
) -> Result<PathBuf, PortError> {
    let Some(value) = workdir.filter(|value| !value.is_null()) else {
        // workdir 缺失/null 时直接在 canonical project root 执行。
        return Ok(project_root.to_path_buf());
    };
    let requested = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed("harness_workdir_invalid".to_owned()))?;
    let requested = Path::new(requested);
    let candidate = if requested.is_absolute() {
        // 绝对路径留给后面的 canonical containment 判定，避免只靠字符串前缀判断目录归属。
        requested.to_path_buf()
    } else {
        // 相对工作目录只能向项目根内追加，拒绝显式 parent/root/prefix 组件。
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
    // canonicalize 跟随符号链接后再次检查真实目标，防止项目内 symlink 指向项目外目录。
    kiana_domain::enforce_root_containment(project_root, &resolved)
        .map_err(|_| PortError::Failed("harness_workdir_outside_project".to_owned()))?;
    if !resolved.is_dir() {
        return Err(PortError::Failed(
            "harness_workdir_invalid:not_directory".to_owned(),
        ));
    }
    Ok(resolved)
}

/// 暴露与 shell handler 相同的命令计划解析，供调用前工具分析复用一致 argv 语义。
pub(crate) fn command_argv(command: Option<&Value>) -> Result<Vec<String>, PortError> {
    Ok(ShellCommandPlan::from_value(command)?.argv)
}

#[cfg(test)]
/// 测试用非取消封装，确保测试和生产共用相同的 sandbox/输出/停止实现。
async fn run_confined(
    argv: Vec<String>,
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    timeout: Duration,
) -> Result<Value, PortError> {
    run_confined_cancellable(argv, project_root, workdir, sandbox, timeout, None, None).await
}

/// 在 sandbox 子进程中执行 argv，支持取消、墙钟超时、进程组停止确认和受限输出采集。
///
/// 取消若在 spawn 前已到达，则返回 `not_executed/zero_effect`；spawn 后必须等 ProcessSupervisor
/// 确认整个进程组停止。无法确认时返回 `result_unknown` 错误，不把未知副作用包装成普通命令结果。
pub(crate) async fn run_confined_cancellable(
    argv: Vec<String>,
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    timeout: Duration,
    mut cancellation: Option<tokio::sync::watch::Receiver<bool>>,
    scope: Option<&Value>,
) -> Result<Value, PortError> {
    // spawn 前取消证明命令尚未开始，因此可以安全给出 zero_effect，而不需要启动子进程。
    if cancellation.as_ref().is_some_and(|rx| *rx.borrow()) {
        return Ok(
            json!({"cancelled":true,"not_executed":true,"effect_started":false,
                "effect_known":true,"zero_effect":true,"stop_state":"confirmed",
                "stop_confirmed":true,"fenced":false,"exit_code":130}),
        );
    }
    // sandbox 命令会配置 stdin/stdout/stderr；kill_on_drop 是 future 被丢弃时的最后一道子进程清理。
    let mut command = sandboxed_command_scoped(&argv, project_root, workdir, sandbox, scope)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| PortError::Failed(format!("shell_exec_failed:{error}")))?;
    // Tokio clears Child::id() after wait/reap. Keep the spawn-time host group identity so
    // every completion branch can still verify the whole group, including descendants.
    let process_group = child.id();
    // guard 在异常退出时保留进程监督责任；只有 stop report 已确认后才 disarm。
    let mut process_guard = ProcessSupervisor::guard(process_group);
    // take 将 pipe 所有权移给异步 reader，避免 stdout/stderr 缓冲区填满后子进程阻塞。
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PortError::Failed("shell_exec_failed:stdout_pipe_unavailable".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PortError::Failed("shell_exec_failed:stderr_pipe_unavailable".to_owned()))?;
    let output_budget = kiana_domain::ExecutionOutputBudget::default();
    // 并行读取两条管道，且 reader 内部受 output_budget 限制，防止命令无限输出占用内存。
    let mut stdout_handle = tokio::spawn(read_capped(stdout, output_budget.clone()));
    let mut stderr_handle = tokio::spawn(read_capped(stderr, output_budget.clone()));
    // timeout 是命令执行结果（124/timed_out），不是 capability 崩溃；之后由 supervisor 停整个进程组。
    // 输出 drain helper 有自己的 I/O drain 上限，避免子进程继承的 pipe 让 Agent 永远等待。
    let execution_id = scope
        .and_then(|value| value["execution_id"].as_str())
        .unwrap_or("shell.exec");
    // biased select 让已就绪的取消优先于 wait/timeout；三条分支都必须取得进程组停止报告。
    let (exit_code, timed_out, cancelled, stop_report) = tokio::select! {
        biased;
        _ = async {
            if let Some(rx) = cancellation.as_mut() { kiana_ports::wait_for_cancellation(rx).await; }
            else { std::future::pending::<()>().await; }
        } => {
            // 取消发生在命令已启动之后，效果已开始；只有 stop report confirmed 才能返回确定结果。
            let report = ProcessSupervisor::stop(execution_id, &mut child, process_group).await;
            if !report.confirmed { return Err(PortError::Failed("shell_result_unknown:cancel_stop_unconfirmed".to_owned())); }
            (130,false,true,report)
        }
        status = child.wait() => {
            // leader 退出后仍检查整个进程组，避免后台后代进程继续持有副作用或输出管道。
            let status = status
                .map_err(|error| PortError::Failed(format!("shell_exec_failed:{error}")))?;
            let report = ProcessSupervisor::stop(execution_id, &mut child, process_group).await;
            if !report.confirmed { return Err(PortError::Failed("shell_result_unknown:process_group_not_stopped".to_owned())); }
            (status.code().unwrap_or(-1), false, false, report)
        }
        _ = tokio::time::sleep(timeout) => {
            // 超时发出停止请求，无法证明已停止时把结果标为未知而非单纯 exit 124。
            let report = ProcessSupervisor::stop(execution_id, &mut child, process_group).await;
            if !report.confirmed {
                return Err(PortError::Failed(
                    "shell_result_unknown:process_group_not_stopped".to_owned(),
                ));
            }
            (EXEC_TIMEOUT_EXIT_CODE, true, false, report)
        }
    };
    // select 只会在 stop report 已确认时到达这里；guard 不再重复清理已停止的 group。
    process_guard.disarm();
    // 停止确认后再收尾输出 reader；reader 返回截断与读取元数据，稍后进入结果 receipt。
    let stdout = drain_capped(&mut stdout_handle).await;
    let stderr = drain_capped(&mut stderr_handle).await;
    // 资源回执记录本次允许的墙钟时间预算，实际 enforcement 由 ProcessSupervisor 提供。
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
        "stop_state": if cancelled || timed_out { if stop_report.confirmed { "confirmed" } else { "unconfirmed" } } else { "not_requested" },
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
/// 测试便利包装：不传 scope，也不支持取消，生产代码使用 scoped/cancellable 入口。
fn sandboxed_command(
    argv: &[String],
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
) -> Result<Command, PortError> {
    sandboxed_command_scoped(argv, project_root, workdir, sandbox, None)
}

/// 根据 argv、项目路径、sandbox 和可选执行 scope 构造受限 Tokio Command。
///
/// 该函数只准备进程，不 spawn；它用 bwrap plan 配置文件系统边界，清空宿主环境后仅注入 sandbox 白名单变量，
/// 再由 ProcessSupervisor 安装资源限制和进程组管理参数。
pub(crate) fn sandboxed_command_scoped(
    argv: &[String],
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
    scope: Option<&Value>,
) -> Result<Command, PortError> {
    // 空 argv 没有可启动程序，先拒绝以免 Command::new 在平台上产生不同异常。
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
    // scope 可缩小 path_allow；缺失时只允许项目根。bwrap plan 负责把这一范围变成文件系统挂载边界。
    let mut plan =
        crate::harness_sandbox::bwrap_plan_scoped(project_root, workdir, sandbox, &paths)?;
    if let Some(logical) = scope.and_then(|value| value["logical_project_root"].as_str()) {
        // 临时写工作区对进程提供逻辑项目路径，实际写入仍落在 workspace 临时 root。
        plan.remap_workspace(project_root, Path::new(logical));
    }
    let mut command = Command::new(&plan.program);
    plan.configure_command(&mut command);
    command.arg(&argv[0]);
    command.args(&argv[1..]);
    // 清空 bwrap helper 的宿主环境，避免宿主 secret 出现在其 /proc/pid/environ；
    // sandbox 内层命令环境由 bwrap plan 的 clearenv/setenv 重新构造。
    command.env_clear();
    for (key, value) in crate::harness_sandbox::sandbox_env(std::env::vars(), sandbox) {
        command.env(key, value);
    }
    let timeout_ms = scope
        .and_then(|value| value["timeout_ms"].as_u64())
        .unwrap_or(COMMAND_TIMEOUT.as_millis() as u64);
    // 资源限制与调用方实际 timeout 使用同一毫秒值，缺省则采用通用命令 timeout。
    let budget = kiana_domain::ProcessResourceBudget::for_wall_time_ms(timeout_ms);
    ProcessSupervisor::prepare_command(&mut command, &budget)?;
    Ok(command)
}

/// 解析请求 timeout；只接受正整数并限制到 60 秒，缺失/无效值回退为 30 秒。
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
    //! shell 与 patch capability 的本地集成测试。
    //!
    //! 测试用临时项目根和真实 sandbox backend 调 handler，覆盖环境隔离、路径拒绝、只读/写入边界、
    //! timeout、后代进程清理与输出限额；因此它们同时验证 handler 到 bwrap/ProcessSupervisor 的组合。
    use super::*;
    use kiana_domain::{CapabilityRequest, RequestId};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 创建本测试隔离使用的临时项目目录。
    fn temp_root() -> PathBuf {
        // 纳秒时间戳用于让并行测试拥有不同目录；测试自身负责清理需要保留写入结果的目录。
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-shell-{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// 构造经过“已授权”类型包装的测试请求，绕过策略引擎但保留 handler 的真实输入形状。
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

    /// 检查 sandbox 子进程获得必需 Kiana 环境变量，同时不继承 harness script/key/secret/token。
    #[tokio::test]
    async fn shell_exec_uses_codex_core_env_and_strips_secrets() {
        let root = temp_root();
        // printenv 让测试直接观察 sandbox 命令实际收到的环境，而非只检查 bwrap plan 的构造参数。
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
        // 分别检查白名单变量存在，以及宿主级 harness/凭据类变量已经从子进程环境移除。
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

    /// 未显式 workdir 时命令应以项目根为当前目录。
    #[tokio::test]
    async fn shell_exec_returns_stdout_from_project_root() {
        let root = temp_root();
        // 预置唯一标记文件，再运行 ls；输出包含它说明默认 cwd 是项目根。
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

    /// 相对 workdir 中的 parent traversal 在 spawn 前被拒绝。
    #[tokio::test]
    async fn shell_exec_rejects_workdir_escape() {
        let root = temp_root();
        // `..` 在路径组件阶段就应拒绝，因此用例不依赖 sandbox 是否能访问项目外部。
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

    /// read-only sandbox 即使 patch 内容合法，也不能执行有写副作用的 apply_patch。
    #[tokio::test]
    async fn apply_patch_is_rejected_in_read_only_sandbox() {
        let root = temp_root();
        // patch 文本本身是一个合法新增文件操作；拒绝原因应来自 read-only 权限。
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

    /// workspace-write 下应用 patch 后，项目文件内容应与补丁新增行一致。
    #[tokio::test]
    async fn apply_patch_writes_in_workspace_write_sandbox() {
        let root = temp_root();
        // 走正式应用 handler，再从项目树读取文件，确认结果已写入而不只是生成预览。
        let result = ApplyPatchHandler
            .execute(authorized(
                CapabilityKind::Filesystem,
                APPLY_PATCH_OPERATION,
                json!({
                    "patch": "*** Begin Patch\n*** Add File: created.txt\n+from-harness\n*** End Patch\n",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "workspace-write",
                    "path_allow": ["created.txt"],
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

    /// 已确认停止的命令失败必须保留完整 typed 失败证据，不能伪装成成功或 zero-effect。
    fn assert_failed_stopped_shell_result(result: &CapabilityResult, expected_error: &str) {
        assert!(!result.success);
        assert_eq!(result.output["error"], expected_error);
        assert_eq!(result.output["effect_started"], true);
        assert_eq!(result.output["effect_known"], true);
        assert_eq!(result.output["zero_effect"], false);
        assert_eq!(result.output["stop_confirmed"], true);
        let report: kiana_domain::StopReport =
            serde_json::from_value(result.output["stop_report"].clone()).unwrap();
        report.validate().unwrap();
        assert!(report.confirmed && report.leader_reaped);
        assert!(report.process_group_id.is_some());
        assert_eq!(report.process_group, kiana_domain::ProcessGroupState::Empty);
        let adapter = kiana_domain::AdapterResult::from_json(&result.output["adapter_result"])
            .expect("the handler failure carries a valid typed adapter result");
        assert_eq!(adapter.request_id, result.request_id);
        assert_eq!(
            adapter.process,
            kiana_domain::CapabilityProcessState::Exited
        );
        assert_eq!(adapter.stop, kiana_domain::CapabilityStopState::Confirmed);
        assert_eq!(adapter.effect, kiana_domain::CapabilityEffectState::Failed);
    }

    /// read-only 命令尝试重定向写入时应失败，且项目树中不出现目标文件。
    #[tokio::test]
    async fn shell_exec_read_only_cannot_write_workspace() {
        let root = temp_root();
        // shell 语法有效；失败应来自 sandbox 的只读挂载，并且文件系统中不产生目标文件。
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
        assert_failed_stopped_shell_result(&result, "execution_failed:shell_exit");
        assert_ne!(result.output["exit_code"], 0);
        assert_eq!(result.output["backend"], "bwrap");
        assert!(!root.join("written.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// 自然退出后仍使用 spawn 时捕获的 host 进程组身份确认整个组已经停止。
    #[tokio::test]
    async fn shell_stop_keeps_spawned_group_identity_after_natural_exit() {
        let root = temp_root();
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            run_confined(
                vec![
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "printf stopped".to_owned(),
                ],
                &root,
                &root,
                "read-only",
                Duration::from_secs(2),
            ),
        )
        .await
        .expect("supervisor must finish within the drain deadline")
        .expect("natural exit must retain the spawned process-group identity");
        assert_eq!(result["stdout"], "stopped");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["timed_out"], false);
        assert_eq!(result["cancelled"], false);
        let report: kiana_domain::StopReport =
            serde_json::from_value(result["stop_report"].clone()).unwrap();
        report.validate().unwrap();
        assert!(report.process_group_id.is_some());
        assert!(report.leader_reaped);
        assert_eq!(report.process_group, kiana_domain::ProcessGroupState::Empty);
        assert!(report.confirmed);
        assert_eq!(result["stop_confirmed"], true);
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn shell_stop_timeout_state_matches_confirmed_typed_report() {
        let root = temp_root();
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            run_confined(
                vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 8".to_owned()],
                &root,
                &root,
                "read-only",
                Duration::from_millis(250),
            ),
        )
        .await
        .expect("timeout stop must be bounded")
        .expect("the supervised process group must be confirmed stopped");
        let report: kiana_domain::StopReport =
            serde_json::from_value(result["stop_report"].clone()).unwrap();
        report.validate().unwrap();
        assert_eq!(result["timed_out"], true);
        assert_eq!(result["exit_code"], EXEC_TIMEOUT_EXIT_CODE);
        assert_eq!(result["stop_state"], "confirmed");
        assert_eq!(result["stop_confirmed"], true);
        assert!(report.confirmed && report.leader_reaped);
        assert!(report.term_sent || report.kill_sent);
        assert_eq!(report.process_group, kiana_domain::ProcessGroupState::Empty);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_stop_missing_group_identity_remains_unknown_after_leader_exit() {
        let mut command = Command::new("/bin/true");
        ProcessSupervisor::prepare_command(
            &mut command,
            &kiana_domain::ProcessResourceBudget::default(),
        )
        .unwrap();
        let mut child = command.spawn().unwrap();
        let spawned_group = child.id().expect("spawned leader has a host identity");
        tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("fixture leader must exit")
            .unwrap();
        assert_eq!(child.id(), None, "wait clears Tokio's live child identity");
        let missing = ProcessSupervisor::stop("missing-group", &mut child, None).await;
        missing.validate().unwrap();
        assert!(missing.leader_reaped);
        assert!(!missing.confirmed);
        assert_eq!(missing.process_group_id, None);
        assert_eq!(
            missing.process_group,
            kiana_domain::ProcessGroupState::Unknown
        );
        let known = ProcessSupervisor::stop("known-group", &mut child, Some(spawned_group)).await;
        known.validate().unwrap();
        assert!(known.confirmed && known.leader_reaped);
        assert_eq!(known.process_group_id, Some(spawned_group));
        assert_eq!(known.process_group, kiana_domain::ProcessGroupState::Empty);
    }

    /// 请求 timeout 转成 exit 124/timed_out 结果，并在 I/O drain 上限内结束。
    #[tokio::test]
    async fn shell_exec_timeout_ms_returns_codex_timed_out_result() {
        let root = temp_root();
        // sleep 时长大于请求 timeout，便于区分 timeout 结果和正常退出。
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
        assert_failed_stopped_shell_result(&result, "timed_out:shell");
        assert_eq!(result.output["timed_out"], true);
        assert_eq!(result.output["exit_code"], 124);
        assert_eq!(result.output["stop_state"], "confirmed");
        assert_eq!(result.output["backend"], "bwrap");
        assert!(
            elapsed.as_millis() < 4500,
            "timeout should kill the sandboxed sleep within Codex IO drain, elapsed={elapsed:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Unix 下验证 timeout 会停止 shell 启动的后台后代进程，而不只停止 leader。
    #[cfg(unix)]
    #[tokio::test]
    async fn shell_timeout_terminates_descendant_processes() {
        let root = temp_root();
        // shell 记录自身 PID 并启动长时间 sleep 后代，制造 leader 与子进程同时存活的情形。
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "printf '%s' $$ > group.pid; sleep 60 & child=$!; printf '%s' $child > child.pid; wait $child",
                    "timeout_ms": 250,
                    "project_root": root.to_string_lossy(),
                    "sandbox": "workspace-write",
                    "path_allow": ["group.pid", "child.pid"],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(result.output["timed_out"], true, "{}", result.output);
        // Timed-out staged changes are never published. The supervisor's typed receipt owns
        // the host process-group identity; shell PID text belongs to the child PID namespace.
        assert!(!result.success);
        assert_eq!(result.output["exit_code"], EXEC_TIMEOUT_EXIT_CODE);
        assert_eq!(result.output["workspace"]["published"], false);
        assert_eq!(result.output["workspace"]["host_effect"], "none");
        assert!(!root.join("group.pid").exists());
        assert!(!root.join("child.pid").exists());
        let report: kiana_domain::StopReport =
            serde_json::from_value(result.output["stop_report"].clone()).unwrap();
        report.validate().unwrap();
        assert!(report.confirmed);
        assert!(report.leader_reaped);
        assert_eq!(report.process_group, kiana_domain::ProcessGroupState::Empty);
        assert!(report.term_sent || report.kill_sent);
        let group_pid = report
            .process_group_id
            .expect("supervised host process group");
        // 停止确认可能有短暂系统调度延迟，因此在一秒 deadline 内轮询 process group。
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
        let _ = fs::remove_dir_all(&root);
    }

    /// 生成超出输出预算的数据，验证 reader 边读取边截断而非无限缓存后再压缩。
    #[tokio::test]
    async fn shell_exec_caps_stdout_at_codex_one_mib_while_reading() {
        let root = temp_root();
        // 输出 2MB 零字节，超过默认采集预算；同时验证截断不会把命令拖成 timeout。
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

    /// workspace-write shell 命令可写入隔离工作区，并在成功后把写集发布到项目根。
    #[tokio::test]
    async fn shell_exec_workspace_write_can_write_workspace() {
        let root = temp_root();
        // 写权限命令在临时 workspace 创建文件，成功后执行路径应把该写集发布到项目根。
        let result = ShellExecHandler
            .execute(authorized(
                CapabilityKind::Process,
                SHELL_OPERATION,
                json!({
                    "command": "echo from-shell > written.txt",
                    "project_root": root.to_string_lossy(),
                    "sandbox": "workspace-write",
                    "path_allow": ["written.txt"],
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
