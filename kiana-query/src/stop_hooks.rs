//! 查询循环的停止钩子与工具前后置钩子编排。
//!
//! 每轮模型响应后，模块解析受信来源中的 hook 命令，按顺序执行并把进度、阻断错误、运行
//! 时错误和汇总通过 `mpsc` 流发送给调用方；最终结果通过 `oneshot` 返回。PreToolUse 和
//! PostToolUse 复用同一执行器，但只返回“允许、询问、更新输入或阻断”的决定，绝不直接
//! 授予能力。SessionStart/UserPromptSubmit 只产生上下文或更新后的输入。
//!
//! 项目级 hook/plugin 配置必须先经过 `ProjectTrust`；未信任项目只能使用允许的用户级或
//! KIANA_HOME 级来源。命令超时、启动失败、格式错误和默认非零退出都按阻断/运行时错误
//! 处理，除非显式配置 warning 策略。该模块的本地执行结果会进入 hook 事件流，但不是
//! EventLog 的替代事实源，也不等同于整个 Kiana 能力已授权。
use futures::FutureExt;
use kiana_types::{
    hooks::{hook_commands_for_event, parse_hook_commands, parse_hook_config, HookConfigError},
    ProjectTrust,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(windows)]
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

// ---------------------------------------------------------------------------
// 公共类型
// ---------------------------------------------------------------------------

/// 本轮实际启动过的单个 hook 命令摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInfo {
    /// 解析后实际执行的 shell 命令。
    pub command: String,
    /// 传给命令 stdin 的 JSON 文本；可能为空或包含上下文输入。
    pub prompt_text: Option<String>,
    /// 命令从启动到结束/超时的耗时毫秒。
    pub duration_ms: Option<u64>,
}

/// `handle_stop_hooks` 执行期间按顺序发出的事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopHookEvent {
    /// hook 产生阻断错误；调用方应把内容反馈给用户/模型并停止自动继续。
    BlockingError { content: String },
    /// hook 解析或执行产生结构化运行时错误。
    RuntimeError {
        /// 稳定错误码（若可识别）。
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// 清理后的错误文本。
        message: String,
        /// 额外诊断字段；默认为空对象。
        #[serde(default)]
        details: Value,
    },
    /// hook 明确要求查询循环不要继续。
    ContinuationPrevented { reason: String },
    /// hook 开始/完成过程中的提示事件。
    Progress {
        /// hook 事件名称。
        hook_name: String,
        /// 本次 hook 的稳定展示 ID。
        tool_use_id: String,
    },
    /// 本组 hook 全部处理后的汇总统计。
    Summary {
        /// 实际执行的命令数量。
        hook_count: usize,
        /// 每个命令的摘要。
        hook_infos: Vec<HookInfo>,
        /// 阻断或退出错误列表。
        errors: Vec<String>,
        /// 是否有 hook 阻止继续。
        prevented_continuation: bool,
        /// hook 提供的停止原因。
        stop_reason: Option<String>,
        /// 是否至少收到一段非空 stdout。
        has_output: bool,
    },
    /// hook 执行期间查询被中止。
    Aborted,
}

/// 所有停止 hook 处理完成后返回给 reducer/外层的最终结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopHookResult {
    /// 需要反馈给模型的阻断错误文本。
    pub blocking_errors: Vec<String>,
    /// 为 `true` 时查询循环不得自动进入下一轮。
    pub prevent_continuation: bool,
}

impl StopHookResult {
    /// 构造无错误、允许继续的结果。
    pub fn clean() -> Self {
        StopHookResult {
            blocking_errors: vec![],
            prevent_continuation: false,
        }
    }

    /// 构造明确阻止继续的结果。
    pub fn prevented() -> Self {
        StopHookResult {
            blocking_errors: vec![],
            prevent_continuation: true,
        }
    }

    /// 构造带阻断错误、但允许模型尝试修复的结果。
    pub fn with_errors(errors: Vec<String>) -> Self {
        StopHookResult {
            blocking_errors: errors,
            prevent_continuation: false,
        }
    }
}

#[derive(Debug, Clone)]
/// PreToolUse hook 的最小输入上下文。
pub struct PreToolUseHookContext {
    /// 可被 hook 执行轮询的取消信号。
    pub abort_signal: Arc<tokio::sync::Notify>,
    /// 解析项目级 hook 的工作目录。
    pub cwd: PathBuf,
    /// 项目信任级别，决定是否加载项目资源。
    pub project_trust: ProjectTrust,
    /// 当前权限模式展示值。
    pub permission_mode: String,
    /// 查询入口来源展示值。
    pub query_source: String,
    /// 即将调用的工具名。
    pub tool_name: String,
    /// 即将发送给工具的参数。
    pub tool_input: Value,
    /// 可选的工具调用 ID。
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone)]
/// PostToolUse hook 的最小输入上下文。
pub struct PostToolUseHookContext {
    /// 可被 hook 执行轮询的取消信号。
    pub abort_signal: Arc<tokio::sync::Notify>,
    /// 解析项目级 hook 的工作目录。
    pub cwd: PathBuf,
    /// 项目信任级别。
    pub project_trust: ProjectTrust,
    /// 当前权限模式展示值。
    pub permission_mode: String,
    /// 查询入口来源展示值。
    pub query_source: String,
    /// 已执行的工具名。
    pub tool_name: String,
    /// 工具原始输入。
    pub tool_input: Value,
    /// 可选的工具调用 ID。
    pub tool_use_id: Option<String>,
    /// 工具返回的结构化结果。
    pub tool_result: Value,
    /// 工具是否报告错误。
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 工具前后置 hook 对调用方给出的受限决定。
pub enum ToolHookDecision {
    /// 不增加额外限制；仍需通过 ControlPlane 的策略和 Broker。
    Allow,
    /// 暂停并请求审批，可选携带经过 hook 更新的输入。
    Ask {
        /// 审批原因。
        reason: String,
        /// hook 修改后的输入。
        updated_input: Option<Value>,
    },
    /// 阻断工具调用。
    Block(String),
    /// 仅更新输入，调用方可在后续策略检查中重新评估。
    UpdateInput(Value),
}

/// 执行受信 PreToolUse hook 并把结果收敛为工具调用决定。
///
/// 任一 hook 启动/超时/非零错误默认会转成 `Block`；项目未信任时不会加载项目 hook。
pub async fn run_pre_tool_use_hooks(ctx: PreToolUseHookContext) -> ToolHookDecision {
    let stop_ctx = StopHookContext {
        abort_signal: ctx.abort_signal,
        agent_id: None,
        permission_mode: ctx.permission_mode,
        query_source: ctx.query_source,
        stop_hook_active: false,
        bare_mode: false,
        is_teammate: false,
        agent_name: None,
        team_name: None,
        cwd: ctx.cwd,
        project_trust: ctx.project_trust,
    };

    match execute_hook_set_silent(
        HookEvent::PreToolUse {
            tool_name: ctx.tool_name,
            tool_input: ctx.tool_input,
            tool_use_id: ctx.tool_use_id,
        },
        &stop_ctx,
        false,
    )
    .await
    {
        HookSetResult::Clean => ToolHookDecision::Allow,
        HookSetResult::Aborted => ToolHookDecision::Block("PreToolUse hook aborted".to_string()),
        HookSetResult::Prevented { reason } => ToolHookDecision::Block(reason),
        HookSetResult::BlockingErrors(errors) => ToolHookDecision::Block(errors.join("\n")),
        HookSetResult::Ask {
            reason,
            updated_input,
        } => ToolHookDecision::Ask {
            reason,
            updated_input,
        },
        HookSetResult::UpdatedInput(input) => ToolHookDecision::UpdateInput(input),
    }
}

/// 执行 PostToolUse hook 并返回审计后的受限决定。
///
/// PostToolUse 的 `Ask` 不会把已经完成的工具重新授权，因此当前适配器将其折叠为阻断；
/// hook 返回的更新输入也不会改变已经产生的工具结果。
pub async fn run_post_tool_use_hooks(ctx: PostToolUseHookContext) -> ToolHookDecision {
    let stop_ctx = StopHookContext {
        abort_signal: ctx.abort_signal,
        agent_id: None,
        permission_mode: ctx.permission_mode,
        query_source: ctx.query_source,
        stop_hook_active: false,
        bare_mode: false,
        is_teammate: false,
        agent_name: None,
        team_name: None,
        cwd: ctx.cwd,
        project_trust: ctx.project_trust,
    };

    match execute_hook_set_silent(
        HookEvent::PostToolUse {
            tool_name: ctx.tool_name,
            tool_input: ctx.tool_input,
            tool_use_id: ctx.tool_use_id,
            tool_result: ctx.tool_result,
            is_error: ctx.is_error,
        },
        &stop_ctx,
        false,
    )
    .await
    {
        HookSetResult::Clean => ToolHookDecision::Allow,
        HookSetResult::Aborted => ToolHookDecision::Block("PostToolUse hook aborted".to_string()),
        HookSetResult::Prevented { reason } => ToolHookDecision::Block(reason),
        HookSetResult::BlockingErrors(errors) => ToolHookDecision::Block(errors.join("\n")),
        HookSetResult::Ask { reason, .. } => ToolHookDecision::Block(reason),
        HookSetResult::UpdatedInput(_) => ToolHookDecision::Allow,
    }
}

#[derive(Debug, Clone)]
/// SessionStart hook 的最小输入上下文。
pub struct SessionStartHookContext {
    /// 取消信号。
    pub abort_signal: Arc<tokio::sync::Notify>,
    /// hook 配置工作目录。
    pub cwd: PathBuf,
    /// 项目信任级别。
    pub project_trust: ProjectTrust,
    /// 权限模式展示值。
    pub permission_mode: String,
    /// 查询入口来源。
    pub query_source: String,
}

/// 执行 SessionStart hook 并返回追加到会话上下文的文本。
///
/// 任何 hook 错误、超时或取消都会以 `Err` 返回；不会静默当作“没有上下文”。
pub async fn run_session_start_hooks(ctx: SessionStartHookContext) -> Result<Vec<String>, String> {
    let stop_ctx = StopHookContext {
        abort_signal: ctx.abort_signal,
        agent_id: None,
        permission_mode: ctx.permission_mode,
        query_source: ctx.query_source,
        stop_hook_active: false,
        bare_mode: false,
        is_teammate: false,
        agent_name: None,
        team_name: None,
        cwd: ctx.cwd,
        project_trust: ctx.project_trust,
    };
    execute_context_hook_set(HookEvent::SessionStart, &stop_ctx)
        .await
        .map(|result| result.add_contexts)
}

#[derive(Debug, Clone)]
/// UserPromptSubmit hook 的最小输入上下文。
pub struct UserPromptSubmitHookContext {
    /// 取消信号。
    pub abort_signal: Arc<tokio::sync::Notify>,
    /// hook 配置工作目录。
    pub cwd: PathBuf,
    /// 项目信任级别。
    pub project_trust: ProjectTrust,
    /// 权限模式展示值。
    pub permission_mode: String,
    /// 查询入口来源。
    pub query_source: String,
    /// 用户原始提示。
    pub user_prompt: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// UserPromptSubmit hook 处理后的上下文和提示结果。
pub struct UserPromptSubmitHookResult {
    /// hook 追加的上下文文本。
    pub add_contexts: Vec<String>,
    /// hook 请求替换后的提示文本。
    pub updated_prompt: Option<String>,
}

/// 执行 UserPromptSubmit hook，返回追加上下文和可选的新提示。
///
/// 多个 hook 按配置顺序执行；前一个 hook 的提示更新会成为后一个 hook 的输入。错误不会
/// 被吞掉，以免模型在缺失安全/业务前置检查时继续运行。
pub async fn run_user_prompt_submit_hooks(
    ctx: UserPromptSubmitHookContext,
) -> Result<UserPromptSubmitHookResult, String> {
    let stop_ctx = StopHookContext {
        abort_signal: ctx.abort_signal,
        agent_id: None,
        permission_mode: ctx.permission_mode,
        query_source: ctx.query_source,
        stop_hook_active: false,
        bare_mode: false,
        is_teammate: false,
        agent_name: None,
        team_name: None,
        cwd: ctx.cwd,
        project_trust: ctx.project_trust,
    };
    let result = execute_context_hook_set(
        HookEvent::UserPromptSubmit {
            user_prompt: ctx.user_prompt,
        },
        &stop_ctx,
    )
    .await?;
    Ok(UserPromptSubmitHookResult {
        add_contexts: result.add_contexts,
        updated_prompt: result
            .updated_input
            .as_ref()
            .and_then(updated_prompt_text_from_value),
    })
}

// ---------------------------------------------------------------------------
// Hook 执行上下文
// ---------------------------------------------------------------------------

/// 在 hook 执行链中传递的调用方上下文。
///
/// 结构只暴露 hook 所需的最小字段；它不是完整的请求授权上下文。项目资源开关在这里
/// 只作为解析输入，真正的能力授权仍由控制面完成。
#[derive(Debug, Clone)]
pub struct StopHookContext {
    /// hook 执行期间轮询的取消信号。
    pub abort_signal: Arc<tokio::sync::Notify>,
    /// 子代理运行时的 agent ID。
    pub agent_id: Option<String>,
    /// 权限模式字符串。
    pub permission_mode: String,
    /// 查询入口来源。
    pub query_source: String,
    /// 是否启用 Stop hook 主流程。
    pub stop_hook_active: bool,
    /// 是否为 bare/simple 模式。
    pub bare_mode: bool,
    /// 当前进程是否作为 teammate agent 运行。
    pub is_teammate: bool,
    /// teammate 名称。
    pub agent_name: Option<String>,
    /// team 名称。
    pub team_name: Option<String>,
    /// 解析项目 hook 文件时使用的工作目录。
    pub cwd: PathBuf,
    /// 项目级 hook 是否因 trust 允许参与。
    pub project_trust: ProjectTrust,
}

// ---------------------------------------------------------------------------
// 核心编排
// ---------------------------------------------------------------------------

/// 异步 hook 执行返回给调用方的两个通道。
pub struct StopHookHandle {
    /// hook 执行期间的有序事件流。
    pub events: mpsc::Receiver<StopHookEvent>,
    /// 全部 hook 结束后解析的最终结果。
    pub result: oneshot::Receiver<StopHookResult>,
}

/// 异步启动 Stop hook 主流程并立即返回两个结果通道。
///
/// 调用方应在等待 `result` 的同时持续消费 `events`，否则有界通道填满后 hook 任务可能
/// 反压等待。向 `ctx.abort_signal` 发送通知会中断当前循环，发出 `Aborted` 并以
/// `StopHookResult::prevented()` 结束；这只是 hook 层取消，不替代控制面的 Run fencing。
pub fn handle_stop_hooks(ctx: StopHookContext) -> StopHookHandle {
    let (event_tx, event_rx) = mpsc::channel::<StopHookEvent>(64);
    let (result_tx, result_rx) = oneshot::channel::<StopHookResult>();

    tokio::spawn(async move {
        let result = run_stop_hooks(ctx, event_tx.clone()).await;
        let _ = result_tx.send(result);
    });

    StopHookHandle {
        events: event_rx,
        result: result_rx,
    }
}

/// 在独立 Tokio 任务中执行 Stop hook 的内部实现。
async fn run_stop_hooks(ctx: StopHookContext, tx: mpsc::Sender<StopHookEvent>) -> StopHookResult {
    // 先执行主 Stop hook；只有它没有阻断/错误时才会继续到 teammate 分支。
    let stop_result = execute_hook_set(HookEvent::Stop, &ctx, &tx, ctx.stop_hook_active).await;

    // 在进入后续 hook 前再次检查取消，避免主 hook 返回后仍启动新的命令。
    if ctx.abort_signal.try_recv_aborted() {
        let _ = tx.send(StopHookEvent::Aborted).await;
        return StopHookResult::prevented();
    }

    match stop_result {
        HookSetResult::Aborted => {
            let _ = tx.send(StopHookEvent::Aborted).await;
            return StopHookResult::prevented();
        }
        HookSetResult::Prevented { reason: _ } => {
            return StopHookResult::prevented();
        }
        HookSetResult::BlockingErrors(errors) => {
            return StopHookResult::with_errors(errors);
        }
        HookSetResult::Ask { reason, .. } => {
            return StopHookResult::with_errors(vec![reason]);
        }
        HookSetResult::UpdatedInput(_) => {}
        HookSetResult::Clean => {}
    }

    // teammate 只额外运行受限的 TeammateIdle/TaskCompleted 流程。
    if ctx.is_teammate {
        let teammate_result = run_teammate_hooks(&ctx, &tx).await;
        match teammate_result {
            HookSetResult::Aborted => {
                let _ = tx.send(StopHookEvent::Aborted).await;
                return StopHookResult::prevented();
            }
            HookSetResult::Prevented { reason: _ } => {
                return StopHookResult::prevented();
            }
            HookSetResult::BlockingErrors(errors) => {
                return StopHookResult::with_errors(errors);
            }
            HookSetResult::Ask { reason, .. } => {
                return StopHookResult::with_errors(vec![reason]);
            }
            HookSetResult::UpdatedInput(_) => {}
            HookSetResult::Clean => {}
        }
    }

    StopHookResult::clean()
}

// ---------------------------------------------------------------------------
// Hook 集合辅助函数
// ---------------------------------------------------------------------------

#[derive(Debug)]
#[allow(dead_code)]
/// hook 执行器内部使用的事件输入。
enum HookEvent {
    Stop,
    SessionStart,
    UserPromptSubmit {
        user_prompt: String,
    },
    PreToolUse {
        tool_name: String,
        tool_input: Value,
        tool_use_id: Option<String>,
    },
    PostToolUse {
        tool_name: String,
        tool_input: Value,
        tool_use_id: Option<String>,
        tool_result: Value,
        is_error: bool,
    },
    TaskCompleted {
        task_id: String,
        task_subject: String,
    },
    TeammateIdle,
}

#[derive(Debug)]
#[allow(dead_code)]
/// 单组 hook 汇总后的内部决定。
enum HookSetResult {
    Clean,
    Aborted,
    Prevented {
        reason: String,
    },
    BlockingErrors(Vec<String>),
    Ask {
        reason: String,
        updated_input: Option<Value>,
    },
    UpdatedInput(Value),
}

async fn execute_hook_set(
    mut event: HookEvent,
    ctx: &StopHookContext,
    tx: &mpsc::Sender<StopHookEvent>,
    active: bool,
) -> HookSetResult {
    // 配置解析与命令执行分开：配置错误先进入错误事件流，不能在无法解释的 schema 下
    // 猜测命令或放宽项目资源边界。
    let hook_name = event.name();
    let hook_resolution = hook_commands_for(hook_name, ctx.project_trust, &ctx.cwd);
    let commands = hook_resolution.commands;
    let timeout_duration = hook_timeout_duration();
    let mut hook_infos = Vec::new();
    let mut runtime_errors = hook_resolution.errors;
    let mut blocking_errors = runtime_errors
        .iter()
        .map(|error| error.message.clone())
        .collect::<Vec<_>>();
    let mut summary_errors = blocking_errors.clone();
    let mut prevented_continuation = false;
    let mut stop_reason = None;
    let mut has_output = false;
    let mut ask_reason = None;
    let mut updated_input = None;

    if ctx.abort_signal.try_recv_aborted() {
        return HookSetResult::Aborted;
    }

    // 同一个配置错误同时发 RuntimeError 和 BlockingError，分别服务机器订阅者与模型/用户。
    for error in &runtime_errors {
        let _ = tx
            .send(StopHookEvent::RuntimeError {
                code: error.code.clone(),
                message: error.message.clone(),
                details: error.details.clone(),
            })
            .await;
        let _ = tx
            .send(StopHookEvent::BlockingError {
                content: error.message.clone(),
            })
            .await;
    }

    for (index, command) in commands.iter().enumerate() {
        if ctx.abort_signal.try_recv_aborted() {
            return HookSetResult::Aborted;
        }

        let tool_use_id = format!("hook:{}:{}", hook_name, index + 1);
        let _ = tx
            .send(StopHookEvent::Progress {
                hook_name: hook_name.to_string(),
                tool_use_id,
            })
            .await;

        // 每个命令获得执行前 event 的快照；PreToolUse 的输入更新会传给后续命令。
        let input_json = hook_input_json(&event, ctx, active);
        let run = run_hook_command(command, &input_json, timeout_duration, &ctx.abort_signal).await;
        hook_infos.push(HookInfo {
            command: command.clone(),
            prompt_text: Some(input_json.clone()),
            duration_ms: Some(run.duration_ms),
        });

        if run.aborted {
            return HookSetResult::Aborted;
        }

        if let Some(error) = run.error.as_ref() {
            let runtime_error = hook_command_runtime_error(hook_name, command, &run, error.clone());
            let _ = tx
                .send(StopHookEvent::RuntimeError {
                    code: runtime_error.code.clone(),
                    message: runtime_error.message.clone(),
                    details: runtime_error.details.clone(),
                })
                .await;
            blocking_errors.push(runtime_error.message.clone());
            summary_errors.push(runtime_error.message.clone());
            runtime_errors.push(runtime_error);
            continue;
        }

        if run.exit_code != Some(0) {
            let detail = if run.stderr.trim().is_empty() {
                run.stdout.trim().to_string()
            } else {
                run.stderr.trim().to_string()
            };
            let exit_error = format!(
                "{} exited with {}{}",
                command,
                run.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown status".to_string()),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", detail)
                }
            );
            summary_errors.push(exit_error.clone());
            // 默认非零退出阻断；仅显式 warning 配置允许继续，并把错误保留在汇总中。
            if hook_nonzero_policy().is_deny() {
                blocking_errors.push(exit_error);
            } else {
                has_output = true;
            }
            continue;
        }

        let stdout = run.stdout.trim();
        if stdout.is_empty() {
            continue;
        }
        has_output = true;

        let parsed = parse_hook_output(stdout);
        if let Some(error) = parsed.blocking_error {
            let _ = tx
                .send(StopHookEvent::BlockingError {
                    content: error.clone(),
                })
                .await;
            blocking_errors.push(error.clone());
            summary_errors.push(error);
        }
        if parsed.prevent_continuation {
            let reason = parsed
                .stop_reason
                .clone()
                .unwrap_or_else(|| "Hook prevented continuation".to_string());
            let _ = tx
                .send(StopHookEvent::ContinuationPrevented {
                    reason: reason.clone(),
                })
                .await;
            prevented_continuation = true;
            stop_reason = Some(reason);
        }
        if let Some(reason) = parsed.ask_reason {
            ask_reason = Some(reason);
        }
        if let Some(next_input) = parsed.updated_input {
            if let HookEvent::PreToolUse { tool_input, .. } = &mut event {
                *tool_input = next_input.clone();
                updated_input = Some(next_input);
            }
        }
    }

    let _ = tx
        .send(StopHookEvent::Summary {
            hook_count: hook_infos.len(),
            hook_infos,
            errors: summary_errors,
            prevented_continuation,
            stop_reason: stop_reason.clone(),
            has_output,
        })
        .await;

    // 结果优先级固定为：阻断错误 > 明确禁止继续 > Ask > 仅更新输入 > clean。
    // 这样后续“允许继续”的字段不会冲掉更早发现的配置或执行错误。
    if !blocking_errors.is_empty() {
        HookSetResult::BlockingErrors(blocking_errors)
    } else if prevented_continuation {
        HookSetResult::Prevented {
            reason: stop_reason.unwrap_or_else(|| "Hook prevented continuation".to_string()),
        }
    } else if let Some(reason) = ask_reason {
        HookSetResult::Ask {
            reason,
            updated_input,
        }
    } else if let Some(input) = updated_input {
        HookSetResult::UpdatedInput(input)
    } else {
        HookSetResult::Clean
    }
}

async fn execute_hook_set_silent(
    event: HookEvent,
    ctx: &StopHookContext,
    active: bool,
) -> HookSetResult {
    // 静默入口仍然完整执行同一套规则，只把事件流排空而不暴露给调用方，避免出现第二
    // 套不同的 hook 语义。
    let (tx, mut rx) = mpsc::channel::<StopHookEvent>(64);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let result = execute_hook_set(event, ctx, &tx, active).await;
    drop(tx);
    let _ = drain.await;
    result
}

async fn execute_context_hook_set(
    mut event: HookEvent,
    ctx: &StopHookContext,
) -> Result<ContextHookSetResult, String> {
    // SessionStart/UserPromptSubmit 需要传递上下文或修改后的输入，因此沿用相同的命令
    // 顺序和取消检查，但不产生公开 StopHookEvent 流。
    let hook_name = event.name();
    let hook_resolution = hook_commands_for(hook_name, ctx.project_trust, &ctx.cwd);
    let commands = hook_resolution.commands;
    let timeout_duration = hook_timeout_duration();
    let mut errors = hook_resolution
        .errors
        .into_iter()
        .map(|error| error.message)
        .collect::<Vec<_>>();
    let mut contexts = Vec::new();
    let mut updated_input = None;

    if ctx.abort_signal.try_recv_aborted() {
        return Err(format!("{hook_name} hook aborted"));
    }

    for command in commands {
        if ctx.abort_signal.try_recv_aborted() {
            return Err(format!("{hook_name} hook aborted"));
        }

        let input_json = hook_input_json(&event, ctx, false);
        let run =
            run_hook_command(&command, &input_json, timeout_duration, &ctx.abort_signal).await;
        if run.aborted {
            return Err(format!("{hook_name} hook aborted"));
        }
        if let Some(error) = run.error {
            errors.push(format!("{command}: {error}"));
            continue;
        }
        if run.exit_code != Some(0) {
            let detail = if run.stderr.trim().is_empty() {
                run.stdout.trim().to_string()
            } else {
                run.stderr.trim().to_string()
            };
            errors.push(format!(
                "{command} exited with {}{}",
                run.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown status".to_string()),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            ));
            continue;
        }

        let stdout = run.stdout.trim();
        if stdout.is_empty() {
            continue;
        }
        let parsed = parse_hook_output(stdout);
        if let Some(error) = parsed.blocking_error {
            errors.push(error);
        }
        if let Some(reason) = parsed.ask_reason {
            errors.push(reason);
        }
        contexts.extend(parsed.add_contexts);
        if let Some(next_input) = parsed.updated_input {
            if let HookEvent::UserPromptSubmit { user_prompt } = &mut event {
                if let Some(prompt) = updated_prompt_text_from_value(&next_input) {
                    *user_prompt = prompt.clone();
                    updated_input = Some(Value::String(prompt));
                    continue;
                }
            }
            updated_input = Some(next_input);
        }
    }

    if errors.is_empty() {
        Ok(ContextHookSetResult {
            add_contexts: contexts,
            updated_input,
        })
    } else {
        Err(errors.join("\n"))
    }
}

/// 为 teammate agent 运行 TeammateIdle 和 TaskCompleted hook。
async fn run_teammate_hooks(
    ctx: &StopHookContext,
    tx: &mpsc::Sender<StopHookEvent>,
) -> HookSetResult {
    // 当前实现只触发一次 TeammateIdle；TaskCompleted 的任务枚举尚未接入此适配器。
    // 不在这里发明自由消息总线或额外执行循环。
    execute_hook_set(HookEvent::TeammateIdle, ctx, tx, false).await
}

// ---------------------------------------------------------------------------
// 通知辅助函数
// ---------------------------------------------------------------------------

/// 构造展示给用户的 Stop hook 错误提示文本。
///
/// `expand_shortcut` 由入口层提供，因此本函数不解析快捷键，也不泄露 hook 的 stdout/stderr。
pub fn stop_hook_error_notification(expand_shortcut: &str) -> String {
    format!("Stop hook error occurred · {expand_shortcut} to see")
}

// ---------------------------------------------------------------------------
// 取消信号辅助函数（近似 AbortController.signal.aborted）
// ---------------------------------------------------------------------------

/// 为 `tokio::sync::Notify` 增加非阻塞取消检查。
///
/// 当前用 `Notify` 作为轻量取消原语；它只表示“曾收到取消通知”，不负责杀死控制面中的
/// 其他进程或撤销已发出的能力授权。更完整的集成可替换为 `CancellationToken`。
trait AbortSignalExt {
    fn try_recv_aborted(&self) -> bool;
}

impl AbortSignalExt for Arc<tokio::sync::Notify> {
    fn try_recv_aborted(&self) -> bool {
        self.notified().now_or_never().is_some()
    }
}

impl HookEvent {
    fn name(&self) -> &'static str {
        match self {
            HookEvent::Stop => "Stop",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::UserPromptSubmit { .. } => "UserPromptSubmit",
            HookEvent::PreToolUse { .. } => "PreToolUse",
            HookEvent::PostToolUse { .. } => "PostToolUse",
            HookEvent::TaskCompleted { .. } => "TaskCompleted",
            HookEvent::TeammateIdle => "TeammateIdle",
        }
    }
}

#[derive(Debug)]
struct HookRun {
    duration_ms: u64,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
    error_code: Option<String>,
    error_details: Value,
    aborted: bool,
}

#[derive(Debug, Clone)]
struct HookRuntimeError {
    code: Option<String>,
    message: String,
    details: Value,
}

#[derive(Debug, Default)]
struct ParsedHookOutput {
    prevent_continuation: bool,
    stop_reason: Option<String>,
    blocking_error: Option<String>,
    ask_reason: Option<String>,
    add_contexts: Vec<String>,
    updated_input: Option<Value>,
}

#[derive(Debug, Default)]
struct ContextHookSetResult {
    add_contexts: Vec<String>,
    updated_input: Option<Value>,
}

async fn run_hook_command(
    command: &str,
    input_json: &str,
    timeout_duration: Duration,
    abort_signal: &Arc<tokio::sync::Notify>,
) -> HookRun {
    // 每个命令都在独立 shell 子进程中运行，stdin 接收结构化 JSON；timeout 或取消时
    // `kill_on_drop` 负责回收子进程句柄，但不声称能清理所有外部孙进程。
    let started = Instant::now();
    let mut shell = shell_command(command);
    shell
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match shell.spawn() {
        Ok(child) => child,
        Err(error) => {
            return HookRun {
                duration_ms: elapsed_ms(started),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
                error_code: Some("hook_spawn_error".to_string()),
                error_details: json!({}),
                aborted: false,
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input_json.as_bytes()).await {
            return HookRun {
                duration_ms: elapsed_ms(started),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("failed to write hook stdin: {}", error)),
                error_code: Some("hook_stdin_error".to_string()),
                error_details: json!({}),
                aborted: false,
            };
        }
    }

    let wait = child.wait_with_output();
    tokio::pin!(wait);
    tokio::select! {
        _ = abort_signal.notified() => HookRun {
            duration_ms: elapsed_ms(started),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            error_code: None,
            error_details: json!({}),
            aborted: true,
        },
        result = timeout(timeout_duration, &mut wait) => match result {
            Ok(Ok(output)) => HookRun {
                duration_ms: elapsed_ms(started),
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                error: None,
                error_code: None,
                error_details: json!({}),
                aborted: false,
            },
            Ok(Err(error)) => HookRun {
                duration_ms: elapsed_ms(started),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
                error_code: Some("hook_wait_error".to_string()),
                error_details: json!({}),
                aborted: false,
            },
            Err(_) => HookRun {
                duration_ms: elapsed_ms(started),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("hook timed out after {}ms", timeout_duration.as_millis())),
                error_code: Some("hook_timeout".to_string()),
                error_details: json!({
                    "timeout_ms": timeout_duration.as_millis()
                }),
                aborted: false,
            },
        },
    }
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut shell = Command::new(preferred_bash_program());
        shell.arg("-lc").arg(command);
        shell
    }

    #[cfg(not(windows))]
    {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut shell = Command::new(program);
        shell.arg("-lc").arg(command);
        shell
    }
}

#[cfg(windows)]
fn preferred_bash_program() -> OsString {
    if let Some(configured) = std::env::var_os("KIANA_BASH_PATH").filter(|value| !value.is_empty())
    {
        return configured;
    }

    for candidate in [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
    ] {
        let path = Path::new(candidate);
        if path.is_file() {
            return path.as_os_str().to_os_string();
        }
    }

    find_bash_on_path()
        .filter(|path| !is_windows_system_bash(path))
        .map(|path| path.into_os_string())
        .unwrap_or_else(|| OsString::from("bash"))
}

#[cfg(windows)]
fn find_bash_on_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| {
        for name in ["bash.exe", "bash"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

#[cfg(windows)]
fn is_windows_system_bash(path: &Path) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    canonical
        .to_string_lossy()
        .eq_ignore_ascii_case(r"C:\Windows\System32\bash.exe")
}

#[derive(Debug, Default)]
struct HookCommandResolution {
    commands: Vec<String>,
    errors: Vec<HookRuntimeError>,
}

fn hook_commands_for(
    hook_name: &str,
    project_trust: ProjectTrust,
    cwd: &Path,
) -> HookCommandResolution {
    // 配置优先级：事件专用环境变量 > 通用环境变量 > 用户/受信项目文件 > 已安装插件。
    // 事件专用变量存在但格式错误时直接报错，不静默回退到其他来源。
    let specific_env = match hook_name {
        "Stop" => "KIANA_STOP_HOOKS",
        "SessionStart" => "KIANA_SESSION_START_HOOKS",
        "UserPromptSubmit" => "KIANA_USER_PROMPT_SUBMIT_HOOKS",
        "PreToolUse" => "KIANA_PRE_TOOL_USE_HOOKS",
        "PostToolUse" => "KIANA_POST_TOOL_USE_HOOKS",
        "TaskCompleted" => "KIANA_TASK_COMPLETED_HOOKS",
        "TeammateIdle" => "KIANA_TEAMMATE_IDLE_HOOKS",
        _ => "KIANA_HOOKS",
    };

    if let Ok(value) = std::env::var(specific_env) {
        return match parse_hook_commands(&value) {
            Ok(commands) => HookCommandResolution {
                commands,
                errors: Vec::new(),
            },
            Err(error) => HookCommandResolution {
                commands: Vec::new(),
                errors: vec![hook_config_error(hook_name, specific_env, &error)],
            },
        };
    }

    if let Ok(value) = std::env::var("KIANA_HOOKS") {
        return match parse_hook_commands(&value) {
            Ok(commands) => HookCommandResolution {
                commands,
                errors: Vec::new(),
            },
            Err(error) => HookCommandResolution {
                commands: Vec::new(),
                errors: vec![hook_config_error(hook_name, "KIANA_HOOKS", &error)],
            },
        };
    }

    let mut resolution = hook_commands_from_files(hook_name, project_trust, cwd);
    let plugin_resolution = plugin_hook_commands(hook_name);
    resolution.commands.extend(plugin_resolution.commands);
    resolution.errors.extend(plugin_resolution.errors);
    resolution
}

fn hook_commands_from_files(
    hook_name: &str,
    project_trust: ProjectTrust,
    cwd: &Path,
) -> HookCommandResolution {
    // 项目文件的候选路径已经由 trust 过滤；每个文件的 JSON/schema 错误分别保留来源，
    // 便于审计究竟是哪一份配置阻断了请求。
    let mut resolution = HookCommandResolution::default();
    for path in hooks_file_paths_with_trust(project_trust, cwd) {
        if !path.is_file() {
            continue;
        }
        let source = path.display().to_string();
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                resolution.errors.push(hook_io_error(
                    hook_name,
                    &source,
                    format!("failed to read hook config {source}: {error}"),
                ));
                continue;
            }
        };
        let value = match serde_json::from_str::<Value>(&contents) {
            Ok(value) => value,
            Err(error) => {
                resolution.errors.push(hook_json_error(
                    hook_name,
                    &source,
                    format!("invalid hook JSON in {source}: {error}"),
                ));
                continue;
            }
        };
        match parse_hook_config(value) {
            Ok(config) => resolution
                .commands
                .extend(hook_commands_for_event(&config, hook_name)),
            Err(error) => resolution
                .errors
                .push(hook_config_error(hook_name, &source, &error)),
        };
    }
    resolution
}

fn plugin_hook_commands(hook_name: &str) -> HookCommandResolution {
    // 插件必须有可识别 manifest 才会被考虑；缺失 hooks.json 的插件被视为没有该事件，
    // 而不是错误地执行目录中的任意文件。
    let mut resolution = HookCommandResolution::default();
    for plugin in installed_plugin_roots() {
        if find_manifest_path(&plugin).is_none() {
            continue;
        }
        let path = plugin.join("hooks").join("hooks.json");
        let source = path.display().to_string();
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let value = match serde_json::from_str::<Value>(&contents) {
            Ok(value) => value,
            Err(error) => {
                resolution.errors.push(hook_json_error(
                    hook_name,
                    &source,
                    format!("invalid hook JSON in {source}: {error}"),
                ));
                continue;
            }
        };
        match parse_hook_config(value) {
            Ok(config) => resolution
                .commands
                .extend(hook_commands_for_event(&config, hook_name)),
            Err(error) => resolution
                .errors
                .push(hook_config_error(hook_name, &source, &error)),
        }
    }
    resolution
}

fn hook_command_runtime_error(
    hook_name: &str,
    command: &str,
    run: &HookRun,
    error: String,
) -> HookRuntimeError {
    let mut details = json!({
        "hook_event": hook_name,
        "command": command,
        "duration_ms": run.duration_ms,
    });
    merge_runtime_error_details(&mut details, &run.error_details);
    HookRuntimeError {
        code: run
            .error_code
            .clone()
            .or_else(|| Some("hook_execution_error".to_string())),
        message: format!("{command}: {error}"),
        details,
    }
}

fn hook_io_error(hook_name: &str, source: &str, message: String) -> HookRuntimeError {
    HookRuntimeError {
        code: Some("hook_config_io_error".to_string()),
        message,
        details: json!({
            "hook_event": hook_name,
            "source": source
        }),
    }
}

fn hook_json_error(hook_name: &str, source: &str, message: String) -> HookRuntimeError {
    HookRuntimeError {
        code: Some("hook_config_json_error".to_string()),
        message,
        details: json!({
            "hook_event": hook_name,
            "source": source
        }),
    }
}

fn hook_config_error(hook_name: &str, source: &str, error: &HookConfigError) -> HookRuntimeError {
    let message = format!(
        "invalid hook schema in {} at {}: {}",
        source, error.location, error.message
    );
    HookRuntimeError {
        code: Some("hook_config_error".to_string()),
        message,
        details: json!({
            "hook_event": hook_name,
            "source": source,
            "location": error.location,
            "schema_error": error.message
        }),
    }
}

fn merge_runtime_error_details(target: &mut Value, source: &Value) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

fn installed_plugin_roots() -> Vec<PathBuf> {
    kiana_types::plugin::installed_plugin_roots()
}

fn find_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn hooks_file_paths_with_trust(project_trust: ProjectTrust, cwd: &Path) -> Vec<PathBuf> {
    // 用户级/KIANA_HOME 级配置与项目级配置分开处理；只有 trust 允许项目资源时才加入
    // `.kiana/hooks.json`，防止未信任目录注入命令。
    if let Ok(path) = std::env::var("KIANA_HOOKS_FILE") {
        return vec![PathBuf::from(path)];
    }
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("KIANA_HOME") {
        paths.push(PathBuf::from(path).join("hooks.json"));
    } else if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(home).join(".kiana").join("hooks.json"));
    }
    if project_trust.allows_project_resources() {
        let project_path = cwd.join(".kiana").join("hooks.json");
        if !paths.contains(&project_path) {
            paths.push(project_path);
        }
    }
    paths
}

fn hook_input_json(event: &HookEvent, ctx: &StopHookContext, active: bool) -> String {
    // 同时输出 snake_case 和兼容 camelCase 字段，保证既有 hook 脚本能迁移；输入只是
    // 命令上下文，不会把它变成能力授权或扩大写集。
    let mut input = json!({
        "hook_event_name": event.name(),
        "permission_mode": &ctx.permission_mode,
        "query_source": &ctx.query_source,
        "stop_hook_active": active,
        "bare_mode": ctx.bare_mode,
        "is_teammate": ctx.is_teammate,
        "agent_id": &ctx.agent_id,
        "agent_name": &ctx.agent_name,
        "team_name": &ctx.team_name,
    });

    if let HookEvent::TaskCompleted {
        task_id,
        task_subject,
    } = event
    {
        input["task_id"] = json!(task_id);
        input["task_subject"] = json!(task_subject);
    }

    if let HookEvent::UserPromptSubmit { user_prompt } = event {
        input["user_prompt"] = json!(user_prompt);
        input["userPrompt"] = json!(user_prompt);
    }

    if let HookEvent::PreToolUse {
        tool_name,
        tool_input,
        tool_use_id,
    } = event
    {
        input["tool_name"] = json!(tool_name);
        input["toolName"] = json!(tool_name);
        input["tool_input"] = tool_input.clone();
        input["toolInput"] = tool_input.clone();
        input["tool_use_id"] = json!(tool_use_id);
        input["toolUseID"] = json!(tool_use_id);
    }

    if let HookEvent::PostToolUse {
        tool_name,
        tool_input,
        tool_use_id,
        tool_result,
        is_error,
    } = event
    {
        input["tool_name"] = json!(tool_name);
        input["toolName"] = json!(tool_name);
        input["tool_input"] = tool_input.clone();
        input["toolInput"] = tool_input.clone();
        input["tool_use_id"] = json!(tool_use_id);
        input["toolUseID"] = json!(tool_use_id);
        input["tool_result"] = tool_result.clone();
        input["toolResult"] = tool_result.clone();
        input["is_error"] = json!(is_error);
        input["isError"] = json!(is_error);
    }

    input.to_string()
}

fn hook_timeout_duration() -> Duration {
    // 非法、零值或缺失配置统一使用 30 秒，避免把无限等待暴露给查询循环。
    let millis = std::env::var("KIANA_HOOK_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30_000);
    Duration::from_millis(millis)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookNonzeroPolicy {
    Deny,
    Warning,
}

impl HookNonzeroPolicy {
    fn is_deny(self) -> bool {
        self == HookNonzeroPolicy::Deny
    }
}

fn hook_nonzero_policy() -> HookNonzeroPolicy {
    match std::env::var("KIANA_HOOK_NONZERO_POLICY") {
        Ok(value)
            if matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "warning" | "warn" | "non_blocking" | "non-blocking" | "allow"
            ) =>
        {
            HookNonzeroPolicy::Warning
        }
        _ => HookNonzeroPolicy::Deny,
    }
}

fn parse_hook_output(stdout: &str) -> ParsedHookOutput {
    // 非 JSON stdout 视为没有结构化决定；命令退出码和 stderr 仍由上层先行处理。只有
    // 可解析字段才会改变继续、Ask、错误或输入更新状态。
    let Ok(value) = serde_json::from_str::<Value>(stdout) else {
        return ParsedHookOutput::default();
    };

    let continue_processing = value
        .get("continue")
        .or_else(|| value.get("continue_processing"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let stop_reason = value
        .get("stopReason")
        .or_else(|| value.get("stop_reason"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let blocking_error = blocking_error_from_output(&value);
    let ask_reason = ask_reason_from_output(&value);
    let add_contexts = add_contexts_from_output(&value);
    let updated_input = updated_input_from_output(&value);

    ParsedHookOutput {
        prevent_continuation: !continue_processing,
        stop_reason,
        blocking_error,
        ask_reason,
        add_contexts,
        updated_input,
    }
}

fn updated_input_from_output(value: &Value) -> Option<Value> {
    [
        value.get("update_input"),
        value.get("updateInput"),
        value.get("updated_input"),
        value.get("updatedInput"),
    ]
    .into_iter()
    .flatten()
    .find(|input| !input.is_null())
    .cloned()
}

fn updated_prompt_text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => nonempty_prompt(text),
        Value::Object(object) => [
            object.get("user_prompt"),
            object.get("userPrompt"),
            object.get("prompt"),
            object.get("input"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(nonempty_prompt),
        _ => None,
    }
}

fn nonempty_prompt(text: &str) -> Option<String> {
    (!text.trim().is_empty()).then(|| text.to_string())
}

fn add_contexts_from_output(value: &Value) -> Vec<String> {
    [
        value.get("add_context"),
        value.get("addContext"),
        value.get("additional_context"),
        value.get("additionalContext"),
    ]
    .into_iter()
    .flatten()
    .flat_map(contexts_from_value)
    .collect()
}

fn contexts_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => clean_contexts(vec![text.to_string()]),
        Value::Array(items) => clean_contexts(
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        ),
        _ => Vec::new(),
    }
}

fn clean_contexts(contexts: Vec<String>) -> Vec<String> {
    contexts
        .into_iter()
        .map(|context| context.trim().to_string())
        .filter(|context| !context.is_empty())
        .collect()
}

fn blocking_error_from_output(value: &Value) -> Option<String> {
    if let Some(decision) = value.get("decision").and_then(Value::as_str) {
        let fallback = match decision.to_ascii_lowercase().as_str() {
            "block" | "blocked" => Some("Hook blocked execution"),
            "deny" | "denied" | "reject" | "rejected" => Some("Hook denied execution"),
            _ => None,
        };
        if let Some(fallback) = fallback {
            return value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some(fallback.to_string()));
        }
    }

    if let Some(error) = value.get("blocking_error").and_then(Value::as_str) {
        return Some(error.to_string());
    }

    value
        .get("blocking_errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|errors| !errors.is_empty())
}

fn ask_reason_from_output(value: &Value) -> Option<String> {
    let decision = value
        .get("decision")
        .or_else(|| value.get("behavior"))
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        decision.as_str(),
        "ask" | "asked" | "prompt" | "prompt_user" | "prompt-user" | "require_approval"
    ) {
        return None;
    }
    value
        .get("reason")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(str::to_string)
        .or_else(|| Some("Hook requested approval".to_string()))
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    fn make_ctx() -> StopHookContext {
        StopHookContext {
            abort_signal: Arc::new(tokio::sync::Notify::new()),
            agent_id: None,
            permission_mode: "default".into(),
            query_source: "repl_main_thread".into(),
            stop_hook_active: false,
            bare_mode: false,
            is_teammate: false,
            agent_name: None,
            team_name: None,
            cwd: std::env::current_dir().unwrap(),
            project_trust: kiana_types::ProjectTrust::Trusted,
        }
    }

    async fn run_and_collect(ctx: StopHookContext) -> (Vec<StopHookEvent>, StopHookResult) {
        let mut handle = handle_stop_hooks(ctx);
        let mut events = Vec::new();
        while let Some(event) = handle.events.recv().await {
            events.push(event);
        }
        let result = handle.result.await.unwrap();
        (events, result)
    }

    fn clear_hook_env() {
        for key in [
            "KIANA_HOOKS",
            "KIANA_STOP_HOOKS",
            "KIANA_SESSION_START_HOOKS",
            "KIANA_USER_PROMPT_SUBMIT_HOOKS",
            "KIANA_PRE_TOOL_USE_HOOKS",
            "KIANA_POST_TOOL_USE_HOOKS",
            "KIANA_TASK_COMPLETED_HOOKS",
            "KIANA_TEAMMATE_IDLE_HOOKS",
            "KIANA_HOOK_TIMEOUT_MS",
            "KIANA_HOOK_NONZERO_POLICY",
            "KIANA_HOOKS_FILE",
            "KIANA_HOME",
            "KIANA_PLUGINS_DIR",
        ] {
            std::env::remove_var(key);
        }
    }

    fn make_pre_tool_ctx(
        cwd: PathBuf,
        project_trust: kiana_types::ProjectTrust,
    ) -> PreToolUseHookContext {
        PreToolUseHookContext {
            abort_signal: Arc::new(tokio::sync::Notify::new()),
            cwd,
            project_trust,
            permission_mode: "default".into(),
            query_source: "repl_main_thread".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({ "command": "echo ok" }),
            tool_use_id: Some("toolu_test".into()),
        }
    }

    fn make_post_tool_ctx(
        cwd: PathBuf,
        project_trust: kiana_types::ProjectTrust,
    ) -> PostToolUseHookContext {
        PostToolUseHookContext {
            abort_signal: Arc::new(tokio::sync::Notify::new()),
            cwd,
            project_trust,
            permission_mode: "default".into(),
            query_source: "repl_main_thread".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({ "command": "echo ok" }),
            tool_use_id: Some("toolu_test".into()),
            tool_result: serde_json::json!({ "content": "ok" }),
            is_error: false,
        }
    }

    fn make_session_start_ctx(
        cwd: PathBuf,
        project_trust: kiana_types::ProjectTrust,
    ) -> SessionStartHookContext {
        SessionStartHookContext {
            abort_signal: Arc::new(tokio::sync::Notify::new()),
            cwd,
            project_trust,
            permission_mode: "default".into(),
            query_source: "repl_main_thread".into(),
        }
    }

    fn make_user_prompt_submit_ctx(
        cwd: PathBuf,
        project_trust: kiana_types::ProjectTrust,
    ) -> UserPromptSubmitHookContext {
        UserPromptSubmitHookContext {
            abort_signal: Arc::new(tokio::sync::Notify::new()),
            cwd,
            project_trust,
            permission_mode: "default".into(),
            query_source: "repl_main_thread".into(),
            user_prompt: "original prompt".into(),
        }
    }

    fn set_stop_hook(command: &str) {
        std::env::set_var(
            "KIANA_STOP_HOOKS",
            serde_json::to_string(&vec![command]).unwrap(),
        );
    }

    #[tokio::test]
    async fn clean_run_returns_no_errors() {
        let _guard = env_guard().await;
        clear_hook_env();
        let ctx = make_ctx();
        let (events, result) = run_and_collect(ctx).await;

        assert!(events
            .iter()
            .any(|event| matches!(event, StopHookEvent::Summary { hook_count: 0, .. })));
        assert!(!result.prevent_continuation);
        assert!(result.blocking_errors.is_empty());
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hook_can_prevent_continuation() {
        let _guard = env_guard().await;
        clear_hook_env();
        set_stop_hook("printf '%s' '{\"continue\":false,\"stopReason\":\"needs review\"}'");

        let (_events, result) = run_and_collect(make_ctx()).await;

        assert!(result.prevent_continuation);
        assert!(result.blocking_errors.is_empty());
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hook_block_decision_returns_blocking_error() {
        let _guard = env_guard().await;
        clear_hook_env();
        set_stop_hook("printf '%s' '{\"decision\":\"block\",\"reason\":\"policy failed\"}'");

        let (events, result) = run_and_collect(make_ctx()).await;

        assert!(!result.prevent_continuation);
        assert_eq!(result.blocking_errors, vec!["policy failed"]);
        assert!(events.iter().any(|event| matches!(
            event,
            StopHookEvent::BlockingError { content } if content == "policy failed"
        )));
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hook_nonzero_exit_returns_blocking_error() {
        let _guard = env_guard().await;
        clear_hook_env();
        set_stop_hook("printf '%s' 'bad hook' >&2; exit 7");

        let (_events, result) = run_and_collect(make_ctx()).await;

        assert!(!result.prevent_continuation);
        assert_eq!(result.blocking_errors.len(), 1);
        assert!(result.blocking_errors[0].contains("exited with 7"));
        assert!(result.blocking_errors[0].contains("bad hook"));
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hook_nonzero_exit_warning_policy_does_not_block() {
        let _guard = env_guard().await;
        clear_hook_env();
        std::env::set_var("KIANA_HOOK_NONZERO_POLICY", "warning");
        set_stop_hook("printf '%s' 'warn hook' >&2; exit 7");

        let (events, result) = run_and_collect(make_ctx()).await;

        assert!(!result.prevent_continuation);
        assert!(result.blocking_errors.is_empty(), "{:?}", result);
        assert!(events.iter().any(|event| matches!(
            event,
            StopHookEvent::Summary {
                errors,
                has_output: true,
                ..
            } if errors.iter().any(|error| error.contains("exited with 7") && error.contains("warn hook"))
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event, StopHookEvent::BlockingError { .. })));
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hook_timeout_emits_runtime_error_event() {
        let _guard = env_guard().await;
        clear_hook_env();
        std::env::set_var("KIANA_HOOK_TIMEOUT_MS", "1");
        set_stop_hook("sleep 1");

        let (events, result) = run_and_collect(make_ctx()).await;

        assert_eq!(result.blocking_errors.len(), 1);
        assert!(
            result.blocking_errors[0].contains("hook timed out after 1ms"),
            "{:?}",
            result.blocking_errors
        );
        assert!(events.iter().any(|event| matches!(
            event,
            StopHookEvent::RuntimeError { code, message, details }
                if code.as_deref() == Some("hook_timeout")
                    && message.contains("hook timed out after 1ms")
                    && details["hook_event"] == "Stop"
                    && details["timeout_ms"] == 1
        )));
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_load_from_persistent_hooks_file() {
        let _guard = env_guard().await;
        clear_hook_env();
        let path = std::env::temp_dir().join(format!(
            "kiana-stop-hooks-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &path,
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"file hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let (_events, result) = run_and_collect(make_ctx()).await;

        assert_eq!(result.blocking_errors, vec!["file hook blocked"]);
        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_report_malformed_hooks_file_as_blocking_error() {
        let _guard = env_guard().await;
        clear_hook_env();
        let path = std::env::temp_dir().join(format!(
            "kiana-malformed-stop-hooks-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, r#"{"Stop":[{"command":"echo nested"}]}"#).unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let (events, result) = run_and_collect(make_ctx()).await;

        assert_eq!(result.blocking_errors.len(), 1);
        assert!(
            result.blocking_errors[0].contains("invalid hook schema"),
            "{:?}",
            result.blocking_errors
        );
        assert!(
            result.blocking_errors[0].contains("Stop[0]"),
            "{:?}",
            result.blocking_errors
        );
        assert!(events.iter().any(|event| matches!(
            event,
            StopHookEvent::BlockingError { content } if content.contains("invalid hook schema")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            StopHookEvent::RuntimeError { code, message, details }
                if code.as_deref() == Some("hook_config_error")
                    && message.contains("invalid hook schema")
                    && details["hook_event"] == "Stop"
                    && details["source"].as_str().is_some_and(|source| source == path.to_string_lossy())
        )));
        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_report_invalid_json_as_runtime_error_event() {
        let _guard = env_guard().await;
        clear_hook_env();
        let path = std::env::temp_dir().join(format!(
            "kiana-invalid-stop-hooks-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, r#"{"Stop":["echo missing end quote]}"#).unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let (events, result) = run_and_collect(make_ctx()).await;

        assert_eq!(result.blocking_errors.len(), 1);
        assert!(
            result.blocking_errors[0].contains("invalid hook JSON"),
            "{:?}",
            result.blocking_errors
        );
        assert!(events.iter().any(|event| matches!(
            event,
            StopHookEvent::RuntimeError { code, message, details }
                if code.as_deref() == Some("hook_config_json_error")
                    && message.contains("invalid hook JSON")
                    && details["hook_event"] == "Stop"
                    && details["source"].as_str().is_some_and(|source| source == path.to_string_lossy())
        )));
        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_load_trusted_project_hooks_alongside_home_hooks() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-trusted-project-stop-hooks-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir_all(home.join(".kiana")).unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::write(
            home.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"home hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            project.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"project hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let mut ctx = make_ctx();
        ctx.cwd = project;

        let (_events, result) = run_and_collect(ctx).await;

        assert_eq!(
            result.blocking_errors,
            vec!["home hook blocked", "project hook blocked"]
        );
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_ignore_project_hooks_file_when_project_is_untrusted() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-project-stop-hooks-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let project = root.join("project");
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::write(
            project.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"project hook should not run\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::remove_var("HOME");
        let mut trusted_ctx = make_ctx();
        trusted_ctx.cwd = project.clone();
        let (_events, trusted_result) = run_and_collect(trusted_ctx).await;
        assert_eq!(
            trusted_result.blocking_errors,
            vec!["project hook should not run"]
        );
        let mut ctx = make_ctx();
        ctx.cwd = project;
        ctx.project_trust = kiana_types::ProjectTrust::Untrusted;

        let (_events, result) = run_and_collect(ctx).await;

        assert!(result.blocking_errors.is_empty());
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn pre_tool_use_hooks_respect_home_project_plugin_and_trust_sources() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-pre-tool-source-matrix-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let project = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin = plugins_dir.join("policy-plugin");
        std::fs::create_dir_all(home.join(".kiana")).unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(plugin.join("hooks")).unwrap();
        std::fs::write(
            home.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "PreToolUse": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"home pre hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            project.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "PreToolUse": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"project pre hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            plugin.join(".codex-plugin").join("plugin.json"),
            serde_json::json!({ "name": "policy-plugin" }).to_string(),
        )
        .unwrap();
        std::fs::write(
            plugin.join("hooks").join("hooks.json"),
            serde_json::json!({
                "PreToolUse": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"plugin pre hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let trusted = run_pre_tool_use_hooks(make_pre_tool_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Trusted,
        ))
        .await;
        let ToolHookDecision::Block(trusted_reason) = trusted else {
            panic!("expected trusted hooks to block, got {trusted:?}");
        };
        assert!(trusted_reason.contains("home pre hook blocked"));
        assert!(trusted_reason.contains("project pre hook blocked"));
        assert!(trusted_reason.contains("plugin pre hook blocked"));

        let untrusted = run_pre_tool_use_hooks(make_pre_tool_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Untrusted,
        ))
        .await;
        let ToolHookDecision::Block(untrusted_reason) = untrusted else {
            panic!("expected untrusted hooks to block on home/plugin, got {untrusted:?}");
        };
        assert!(untrusted_reason.contains("home pre hook blocked"));
        assert!(!untrusted_reason.contains("project pre hook blocked"));
        assert!(untrusted_reason.contains("plugin pre hook blocked"));

        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "policy-plugin", false).unwrap();
        let disabled_plugin = run_pre_tool_use_hooks(make_pre_tool_ctx(
            project,
            kiana_types::ProjectTrust::Untrusted,
        ))
        .await;
        let ToolHookDecision::Block(disabled_reason) = disabled_plugin else {
            panic!("expected disabled-plugin case to block on home, got {disabled_plugin:?}");
        };
        assert!(disabled_reason.contains("home pre hook blocked"));
        assert!(!disabled_reason.contains("project pre hook blocked"));
        assert!(!disabled_reason.contains("plugin pre hook blocked"));

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn session_start_hooks_respect_project_trust_source_boundary() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-session-start-trust-matrix-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir_all(home.join(".kiana")).unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::write(
            home.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "SessionStart": ["printf '%s' '{\"add_context\":\"home session context\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            project.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "SessionStart": ["printf '%s' '{\"add_context\":\"project session context\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let trusted = run_session_start_hooks(make_session_start_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Trusted,
        ))
        .await
        .unwrap();
        assert_eq!(
            trusted,
            vec![
                "home session context".to_string(),
                "project session context".to_string()
            ]
        );

        let untrusted = run_session_start_hooks(make_session_start_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Untrusted,
        ))
        .await
        .unwrap();
        assert_eq!(untrusted, vec!["home session context".to_string()]);

        let unknown = run_session_start_hooks(make_session_start_ctx(
            project,
            kiana_types::ProjectTrust::Unknown,
        ))
        .await
        .unwrap();
        assert_eq!(unknown, vec!["home session context".to_string()]);

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn user_prompt_submit_hooks_respect_project_trust_source_boundary() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-user-prompt-submit-trust-matrix-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir_all(home.join(".kiana")).unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::write(
            home.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "UserPromptSubmit": ["printf '%s' '{\"add_context\":\"home prompt context\",\"update_input\":\"home prompt\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            project.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "UserPromptSubmit": ["printf '%s' '{\"add_context\":\"project prompt context\",\"update_input\":\"project prompt\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let trusted = run_user_prompt_submit_hooks(make_user_prompt_submit_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Trusted,
        ))
        .await
        .unwrap();
        assert_eq!(
            trusted.add_contexts,
            vec![
                "home prompt context".to_string(),
                "project prompt context".to_string()
            ]
        );
        assert_eq!(trusted.updated_prompt.as_deref(), Some("project prompt"));

        let untrusted = run_user_prompt_submit_hooks(make_user_prompt_submit_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Untrusted,
        ))
        .await
        .unwrap();
        assert_eq!(
            untrusted.add_contexts,
            vec!["home prompt context".to_string()]
        );
        assert_eq!(untrusted.updated_prompt.as_deref(), Some("home prompt"));

        let unknown = run_user_prompt_submit_hooks(make_user_prompt_submit_ctx(
            project,
            kiana_types::ProjectTrust::Unknown,
        ))
        .await
        .unwrap();
        assert_eq!(
            unknown.add_contexts,
            vec!["home prompt context".to_string()]
        );
        assert_eq!(unknown.updated_prompt.as_deref(), Some("home prompt"));

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn post_tool_use_hooks_respect_project_trust_source_boundary() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-post-tool-use-trust-matrix-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir_all(home.join(".kiana")).unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::write(
            home.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "PostToolUse": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"home post hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            project.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "PostToolUse": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"project post hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let trusted = run_post_tool_use_hooks(make_post_tool_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Trusted,
        ))
        .await;
        let ToolHookDecision::Block(trusted_reason) = trusted else {
            panic!("expected trusted hooks to block, got {trusted:?}");
        };
        assert!(trusted_reason.contains("home post hook blocked"));
        assert!(trusted_reason.contains("project post hook blocked"));

        let untrusted = run_post_tool_use_hooks(make_post_tool_ctx(
            project.clone(),
            kiana_types::ProjectTrust::Untrusted,
        ))
        .await;
        let ToolHookDecision::Block(untrusted_reason) = untrusted else {
            panic!("expected untrusted hooks to block on home only, got {untrusted:?}");
        };
        assert!(untrusted_reason.contains("home post hook blocked"));
        assert!(!untrusted_reason.contains("project post hook blocked"));

        let unknown = run_post_tool_use_hooks(make_post_tool_ctx(
            project,
            kiana_types::ProjectTrust::Unknown,
        ))
        .await;
        let ToolHookDecision::Block(unknown_reason) = unknown else {
            panic!("expected unknown-trust hooks to block on home only, got {unknown:?}");
        };
        assert!(unknown_reason.contains("home post hook blocked"));
        assert!(!unknown_reason.contains("project post hook blocked"));

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_load_from_installed_plugin_hooks_file() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-stop-hooks-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let plugin = root.join("plugins").join("policy-plugin");
        std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin.join(".codex-plugin").join("plugin.json"),
            serde_json::json!({ "name": "policy-plugin" }).to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(plugin.join("hooks")).unwrap();
        std::fs::write(
            plugin.join("hooks").join("hooks.json"),
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"plugin hook blocked\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("plugins"));

        let (_events, result) = run_and_collect(make_ctx()).await;

        assert_eq!(result.blocking_errors, vec!["plugin hook blocked"]);
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hooks_ignore_disabled_plugin_hooks_file() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-disabled-plugin-stop-hooks-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let plugins_dir = root.join("plugins");
        let plugin = plugins_dir.join("policy-plugin");
        std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin.join(".codex-plugin").join("plugin.json"),
            serde_json::json!({ "name": "policy-plugin" }).to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(plugin.join("hooks")).unwrap();
        std::fs::write(
            plugin.join("hooks").join("hooks.json"),
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"disabled plugin hook should not run\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "policy-plugin", false).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let (events, result) = run_and_collect(make_ctx()).await;

        assert!(result.blocking_errors.is_empty());
        assert!(!result.prevent_continuation);
        assert!(events
            .iter()
            .any(|event| matches!(event, StopHookEvent::Summary { hook_count: 0, .. })));
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn stop_hook_env_overrides_plugin_hooks() {
        let _guard = env_guard().await;
        clear_hook_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-stop-hooks-env-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let plugin = root.join("plugins").join("policy-plugin");
        std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin.join(".codex-plugin").join("plugin.json"),
            serde_json::json!({ "name": "policy-plugin" }).to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(plugin.join("hooks")).unwrap();
        std::fs::write(
            plugin.join("hooks").join("hooks.json"),
            serde_json::json!({
                "Stop": ["printf '%s' '{\"decision\":\"block\",\"reason\":\"plugin hook should not run\"}'"]
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("plugins"));
        set_stop_hook("printf '%s' '{\"decision\":\"block\",\"reason\":\"env hook blocked\"}'");

        let (_events, result) = run_and_collect(make_ctx()).await;

        assert_eq!(result.blocking_errors, vec!["env hook blocked"]);
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn abort_signal_prevents_hook_run() {
        let _guard = env_guard().await;
        clear_hook_env();
        set_stop_hook("printf '%s' '{\"decision\":\"block\",\"reason\":\"should not run\"}'");
        let ctx = make_ctx();
        ctx.abort_signal.notify_one();

        let (_events, result) = run_and_collect(ctx).await;

        assert!(result.prevent_continuation);
        assert!(result.blocking_errors.is_empty());
        clear_hook_env();
    }

    #[tokio::test]
    async fn notification_text_contains_shortcut() {
        let note = stop_hook_error_notification("ctrl+o");
        assert!(note.contains("ctrl+o"));
    }
}
