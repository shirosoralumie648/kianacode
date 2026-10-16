//! 查询循环的停止钩子与工具前后置钩子编排。
//!
//! 每轮模型响应后，模块解析受信来源中的 hook 命令，按顺序执行并把进度、阻断错误、运行
//! 时错误和汇总通过 `mpsc` 流发送给调用方；最终结果通过 `oneshot` 返回。PreToolUse 和
//! PostToolUse 复用同一执行器，但只返回“允许、询问、更新输入或阻断”的决定，绝不直接
//! 授予能力。SessionStart/UserPromptSubmit 只产生上下文或更新后的输入。
//!
//! 在仓库中的位置和已核实的调用关系：
//!
//! ```text
//! 兼容工具执行 kiana-tools
//!   ├─ 工具执行前 -> run_pre_tool_use_hooks -> Allow / Ask / UpdateInput / Block
//!   │                -> 输入校验与权限处理 -> 工具
//!   └─ 工具执行后 -> run_post_tool_use_hooks -> 对已产生的结果给出受限决定
//!
//! handle_stop_hooks -> Stop 命令 -> StopHookEvent 流 + StopHookResult
//!                         (当前仓内未找到产品调用者；测试会直接调用)
//!
//! SessionStart / UserPromptSubmit -> execute_context_hook_set
//!   (entrypoints 中的包装函数目前标为 cfg(test)，生产调用未找到)
//! ```
//!
//! 因此，理解本文件时要区分“接口已经存在”和“产品路径已经接入”。Pre/PostToolUse
//! 当前仍由历史兼容工具执行路径调用，不是 CompanyOS 主路径的授权实现；它返回的 Allow
//! 也不能代替 ControlPlane 的策略判断。普通工具执行路径会继续验证输入和权限；预批准
//! 并发路径在 hook 改写输入后的权限复查情况需要结合调用方阅读，不能仅凭本文件推定。
//!
//! 输入通常由调用方从当前工具、提示或 hook 配置构造；配置选择器把 JSON 配置解析为
//! 命令列表；每条命令收到本文件序列化的 JSON stdin；stdout 再转成受限决定或上下文；
//! 最终由 `kiana-tools` 调用方、上下文调用方或 Stop 事件订阅者消费。Serde 是 Rust 的
//! 序列化库：这里的 `Serialize` / `Deserialize` 让事件和结果可以在 Rust 值与 JSON 等
//! 外部格式之间转换，但序列化本身不验证事件来源，也不会授予 hook 能力。
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
///
/// 它由执行器在每次启动命令后填充，并放入 `StopHookEvent::Summary`，供事件订阅者显示
/// 或诊断。它不是完整进程输出记录：stdout/stderr 不放在这里，`prompt_text` 是传入命令
/// 的 JSON stdin。该摘要也不会写入 EventLog，因此不能单独作为持久审计回执。
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
///
/// 事件通过 Tokio 的有界 `mpsc`（多生产者、单消费者）通道发送；调用方可边等最终结果
/// 边显示进度。事件接收端关闭时，当前实现会忽略发送失败并继续执行 hook，所以“没有
/// 收到事件”不等于“命令没有运行”。这些事件是本地执行期间的通知，不是 EventLog 事实。
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
///
/// 它只携带是否阻止自动继续以及需要反馈的错误文本，不包含已运行命令的完整输出，也
/// 不会自行改变 Query reducer、Run 状态或控制面授权。调用者必须明确决定如何消费它。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopHookResult {
    /// 需要反馈给模型的阻断错误文本。
    pub blocking_errors: Vec<String>,
    /// 为 `true` 时查询循环不得自动进入下一轮。
    pub prevent_continuation: bool,
}

impl StopHookResult {
    /// 构造无错误、允许继续的结果。
    ///
    /// 用于全部 hook 都没有提出阻断、Ask 或输入更新的情况。调用者收到它后仍需遵循自己
    /// 的查询状态机；这个值本身不会启动下一轮模型请求。
    pub fn clean() -> Self {
        StopHookResult {
            blocking_errors: vec![],
            prevent_continuation: false,
        }
    }

    /// 构造明确阻止继续的结果。
    ///
    /// 取消或 hook 的 prevent-continuation 决定会映射到该形态。它与 `with_errors` 的
    /// 区别是：这里没有可交给模型修复的错误文本，外层只知道本轮不能自动继续。
    pub fn prevented() -> Self {
        StopHookResult {
            blocking_errors: vec![],
            prevent_continuation: true,
        }
    }

    /// 构造带阻断错误、但允许模型尝试修复的结果。
    ///
    /// `errors` 由 hook 配置或执行失败产生，最终由调用方决定是否反馈给模型并允许用户
    /// 修正。此构造器不判断错误是否可恢复，也不触发重试。
    pub fn with_errors(errors: Vec<String>) -> Self {
        StopHookResult {
            blocking_errors: errors,
            prevent_continuation: false,
        }
    }
}

#[derive(Debug, Clone)]
/// PreToolUse hook 的最小输入上下文。
///
/// 兼容工具执行器在调用工具前提供工作目录、项目置信任、工具名及原始参数。hook 可以
/// 返回更新后的 JSON 参数；调用方必须继续做工具输入验证和适用的权限检查，不能把 hook
/// 的 Allow 当成授权凭证。`abort_signal` 是独立的 Tokio 通知句柄，是否与外层取消源相连
/// 由创建它的调用者决定。
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
///
/// 调用者在工具完成后把实际传入参数、结果和错误标记交给 hook。由于副作用此时可能已经
/// 发生，hook 返回 Block 只能影响后续结果呈现/控制流，不能回滚已完成的文件写入、网络
/// 请求或其他外部动作。
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
///
/// 这是 hook 与工具执行器之间的适配结果，不是 Kiana 的授权决定：`Allow` 表示本 hook
/// 没有增加限制，后续校验仍然必须运行；`Ask` 表示调用方应进入自己的审批流程；
/// `UpdateInput` 要求调用方使用新参数继续验证；`Block` 则停止该次兼容工具调用。
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
/// 【作用】在工具执行前运行受信 PreToolUse 命令，并把脚本输出收敛为受限 hook 决定。
///
/// 【调用者】`kiana-tools/src/tool_execution.rs` 中的兼容工具执行器；普通路径和预批准
/// 路径都会在实际工具调用前经过它。
///
/// 【输入】工具名、原始 JSON 参数、可选调用 ID、工作目录、项目 `ProjectTrust`、权限模式、
/// 查询来源以及调用方提供的取消通知。
///
/// 【输出】`Allow` 表示 hook 没有增加限制；`Ask` 携带审批原因和可选的新输入；
/// `UpdateInput` 携带替换参数；配置/运行错误、取消或显式阻断映射为 `Block`。
///
/// 【副作用】按来源解析并启动外部 shell 子进程；静默入口会排空事件，不把中间事件交给
/// `kiana-tools`。如果项目未信任，项目级 hook 文件不会进入普通候选列表。
///
/// 【失败与后续】启动、stdin、等待、超时及默认非零退出会形成阻断决定；不会在本函数内
/// 重试、禁用账号或写入 EventLog。调用方仍需验证参数并执行自己的权限流程；尤其要结合
/// 调用路径判断更新后的输入是否再次检查，不能把此函数的 `Allow` 当作授权。
///
/// 【核心流程】构造最小 StopHookContext -> 同步解析命令来源 -> 顺序执行命令 -> 解析
/// stdout -> 将聚合结果压缩为 `ToolHookDecision`。
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
/// 【作用】在工具执行后运行 PostToolUse 命令，并将结果压缩为调用方可消费的审计决定。
///
/// 【调用者】`kiana-tools/src/tool_execution.rs` 在工具成功返回后调用；失败结果是否也
/// 进入 hook 取决于对应执行分支。
///
/// 【输入/输出】输入包含工具名、已用参数、工具结果、错误标记和信任上下文；输出沿用
/// `ToolHookDecision`，但 `Ask` 被折叠为 `Block`，输入更新被忽略并折叠为 `Allow`。
///
/// 【为什么这样映射】工具已经执行后再 Ask 不能撤销或重新授权原动作，更新参数也不能
/// 改变已经产生的结果。Block 可能让调用方把成功结果改报为错误，但不能回滚外部副作用。
///
/// 【失败情况与副作用】hook 配置、启动、超时、非零退出或显式阻断可能形成 Block；本函数
/// 不重试、不撤销工具动作、不写持久审计记录，错误最终由兼容工具执行器消费。
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
///
/// 预期由新会话初始化阶段提供；当前 entrypoint 包装 helper 被 `cfg(test)` 限定，仓内未
/// 找到生产调用。成功输出会被当作待追加上下文，失败会返回错误而非悄悄跳过。
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
/// 【作用】执行 SessionStart 命令并提取成功 hook 追加的上下文文本。
///
/// 【调用者】目前只有本文件测试直接调用；`kiana-entrypoints/src/runner.rs` 中对应包装
/// helper 是测试专用，因此这里是已定义的接口，不代表生产会话初始化已接线。
///
/// 【输入/输出】使用当前工作目录、项目 trust、权限模式、入口来源和取消信号；成功返回
/// 按配置顺序收集的上下文字符串，失败返回合并后的错误文本。
///
/// 【副作用与失败】可能启动外部 hook 子进程。任一配置/运行/非零退出/Ask/阻断错误或取消
/// 都会使整个调用返回 `Err`；不会返回部分上下文，也不会在此处重试或写 EventLog。
///
/// 【核心流程】建立共享 hook 上下文 -> 复用严格上下文执行器 -> 只取 `add_contexts`。
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
///
/// `user_prompt` 是入口传来的原始提示文本。执行器可把前一个 hook 的更新值交给后一个
/// hook，因此返回值可能是整条 hook 链逐步变换后的提示，而不是单个命令的输出。
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
///
/// 返回给入口适配层的只读结果：`add_contexts` 会附加到会话输入，`updated_prompt` 仅在
/// hook 返回可识别且非空的字符串时出现。本结构不直接改写消息列表或启动模型请求。
pub struct UserPromptSubmitHookResult {
    /// hook 追加的上下文文本。
    pub add_contexts: Vec<String>,
    /// hook 请求替换后的提示文本。
    pub updated_prompt: Option<String>,
}

/// 执行 UserPromptSubmit hook，返回追加上下文和可选的新提示。
///
/// 【作用】按配置顺序运行 UserPromptSubmit 命令，汇总上下文并提取最后可识别的提示更新。
///
/// 【调用者】目前仓内直接调用者是本文件测试；entrypoint 的包装 helper 目前为 `cfg(test)`。
///
/// 【输入/输出】输入为原始用户提示和会话/信任元数据；输出为累积上下文及可选的更新提示。
/// 多个命令依配置顺序执行，前一条更新后的提示会成为下一条 stdin 中的提示。
///
/// 【失败情况】配置、启动、超时、非零退出、Ask、阻断输出和取消都作为 `Err` 返回，避免
/// 调用方误把“没有上下文”当成“检查成功”。部分收集的上下文不会随错误返回。
///
/// 【副作用/设计原因】会运行外部 shell 命令，但不直接调用模型、不修改消息列表、不持久化
/// 结果；分开返回更新提示和追加上下文，让入口决定如何应用这两个不同用途的数据。
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
///
/// 字段来源通常是上层工具/入口的当前会话快照。`agent_id`、`agent_name`、`team_name` 允许
/// 缺省，因为普通主线程没有 teammate 身份；`stop_hook_active` 和 `bare_mode` 是输入 JSON
/// 的说明性元数据，不会在此处关闭 hook 执行。
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
///
/// `events` 是容量固定为 64 条的有界 Tokio `mpsc`：生产者发送快于消费者读取时会等待，
/// 形成反压；`result` 是只允许发送一次的 `oneshot`，承载最终聚合结果。调用方通常要并行
/// 消费事件并等待结果，避免事件缓冲区占满后执行任务无法完成。
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
///
/// 【作用】把 Stop hook 工作异步交给 Tokio task，并立即交付事件流与最终结果两个接收端。
///
/// 【调用者】当前仓内产品调用者未找到；测试通过它检查异步执行和事件顺序。未来调用者应
/// 将它接入既有查询循环，而不是在此处创建第二套模型或授权循环。
///
/// 【输入/输出】接收 StopHookContext，立即返回事件接收端和最终结果接收端。真正结果在
/// 独立 Tokio task 完成后写入 `oneshot`。
///
/// 【副作用/失败】任务可能启动 shell 子进程；结果发送失败只表示接收方提前丢弃，不会
/// 撤销已经运行的 hook。事件接收方不读取会造成通道反压，取消通知只影响本适配器的等待。
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
///
/// Stop hook 先运行；若它阻断、要求审批、报错或取消，则 teammate 分支不会开始。只有 Stop
/// 结果为 Clean/UpdatedInput 时才检查 `is_teammate`，当前 teammate 分支实际上只执行
/// TeammateIdle。这里仅把内部结果折叠成 StopHookResult，不会推进外层查询 reducer。
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
///
/// 这些变体把多个公开入口统一成同一个命令执行/解析器；每种事件需要的字段不同，后续由
/// `hook_input_json()` 转换成脚本可读 JSON。`TaskCompleted` 虽有数据形状和解析分支，当前
/// `run_teammate_hooks()` 并未构造它，所以类型存在不等于该事件已经接通。
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
///
/// 它是执行器私有状态，不直接序列化给调用方。汇总顺序是错误优先、其次显式阻止继续、
/// 再次 Ask、输入更新、最后 Clean；`Aborted` 在循环中提前返回，不参与末尾优先级比较。
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

/// 按解析出的顺序执行同一事件的一组 hook，并把进度与最终汇总送入事件通道。
///
/// 【输入/输出】`event` 携带本次工具、提示或生命周期事件的数据；`ctx` 提供信任和工作目录；
/// `tx` 是可有界反压的事件发送端；`active` 只会写入 stdin JSON 元数据。返回的私有决定
/// 会由公共入口进一步折叠，不直接授予能力。
///
/// 【核心流程】先解析来源及配置错误；逐条发送 Progress、运行命令并处理退出码/stdout；
/// 最后发送 Summary，并按 BlockingErrors > Prevented > Ask > UpdatedInput > Clean 归并。
/// PreToolUse 中某条命令更新的参数会成为后续命令的输入；其他事件不会应用此更新。
///
/// 【错误与副作用】配置错误会发 RuntimeError 和 BlockingError，但当前仍继续运行已解析的
/// 命令；单条启动/等待错误会记入错误集合后继续后续命令。发送事件失败被忽略，不取消子进程。
/// 默认非零退出阻断，显式 warning 策略只让非零退出不阻断，错误仍保留在 Summary。
///
/// 【为什么要集中处理】多个事件入口共用相同的配置、取消、超时和错误优先级，避免静默
/// 与非静默路径各自形成不同的决定规则；上下文事件另有更严格的返回规则，见下方 helper。
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

    // 在发配置错误事件前也检查取消；已收到取消时不启动任何命令。
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

        // 该 ID 只用于当前事件流中的进度展示，不是全局唯一的工具调用授权 ID。
        let tool_use_id = format!("hook:{}:{}", hook_name, index + 1);
        let _ = tx
            .send(StopHookEvent::Progress {
                hook_name: hook_name.to_string(),
                tool_use_id,
            })
            .await;

        // 每个命令获得执行前 event 的快照；PreToolUse 的输入更新会传给后续命令。
        let input_json = hook_input_json(&event, ctx, active);
        let run = run_hook_command(
            command,
            &input_json,
            timeout_duration,
            &ctx.abort_signal,
            &ctx.cwd,
        )
        .await;
        hook_infos.push(HookInfo {
            command: command.clone(),
            prompt_text: Some(input_json.clone()),
            duration_ms: Some(run.duration_ms),
        });

        // 取消和普通命令错误不同：取消立即结束整组，不再启动后续 hook。
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

        // 运行成功与否由 shell 退出码判定；warning 模式仅改变阻断结果，不抹掉诊断记录。
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

        // stdout 的结构化决定会影响整组结果；stderr 仅在退出非零时用作错误详情。
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
    // 套不同的 hook 语义。排空任务与执行器并发运行是因为有界 mpsc 在无人接收时会反压；
    // drain 只丢弃通知，不会取消 hook，也不会改变最终 HookSetResult。
    let (tx, mut rx) = mpsc::channel::<StopHookEvent>(64);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let result = execute_hook_set(event, ctx, &tx, active).await;
    drop(tx);
    let _ = drain.await;
    result
}

/// 为 SessionStart/UserPromptSubmit 执行 hook，并将上下文结果直接返回给入口适配层。
///
/// 与 `execute_hook_set()` 的主要区别是：这里没有公开事件通道；配置错误、运行错误、非零
/// 退出、Ask 或阻断输出最终都会让整组返回 Err。命令执行期间仍会累积错误并继续尝试后续
/// 命令，只有取消会立即提前返回。此路径不应用 `KIANA_HOOK_NONZERO_POLICY` 的 warning 放行。
///
/// 对 UserPromptSubmit，上一条命令的可识别 prompt 更新会覆盖 event 中的当前提示，再成为
/// 下一条命令输入；通用 JSON 更新则保存在结果中。SessionStart 只消费上下文，不改提示。
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
        let run = run_hook_command(
            &command,
            &input_json,
            timeout_duration,
            &ctx.abort_signal,
            &ctx.cwd,
        )
        .await;
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

/// 为 teammate agent 运行当前已接入的 teammate hook。
///
/// 目前实际只构造并执行 TeammateIdle；TaskCompleted 的变体和输入字段存在，但没有从这里
/// 派发。此函数由 Stop 流程在普通 Stop hook 未阻断时调用，结果再合并回 StopHookResult。
async fn run_teammate_hooks(
    ctx: &StopHookContext,
    tx: &mpsc::Sender<StopHookEvent>,
) -> HookSetResult {
    // 不在这里发明自由消息总线或额外执行循环。
    execute_hook_set(HookEvent::TeammateIdle, ctx, tx, false).await
}

// ---------------------------------------------------------------------------
// 通知辅助函数
// ---------------------------------------------------------------------------

/// 构造展示给用户的 Stop hook 错误提示文本。
///
/// `expand_shortcut` 由入口层提供，因此本函数不解析快捷键，也不泄露 hook 的 stdout/stderr。
/// 仓内当前未找到产品调用者；函数只拼接展示文案，不读取配置、不发送事件、不改变错误状态。
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
    /// 立即检查是否已有通知可读，不等待未来通知。
    ///
    /// 返回 true 时消费一个已挂起的通知；没有通知时立即返回 false。该检查只作用于本
    /// hook 的 Rust 等待流程，不能证明子进程后代已退出。
    fn try_recv_aborted(&self) -> bool;
}

impl AbortSignalExt for Arc<tokio::sync::Notify> {
    fn try_recv_aborted(&self) -> bool {
        self.notified().now_or_never().is_some()
    }
}

impl HookEvent {
    /// 将内部事件变体映射到配置 JSON 中使用的精确事件名。
    ///
    /// 这个字符串同时用于环境变量选择、配置键查找和传给 hook 的输入；新增或改名必须
    /// 与 `kiana-types::hooks` 的配置解析契约同步。
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

/// 一次 shell 子进程运行后的本地采集结果。
///
/// `exit_code=None` 表示没有拿到正常退出码，需再看 `error`/`aborted` 区分启动、等待、超时
/// 或取消。该结构只在适配器内部传递，不是持久回执；stderr/stdout 已经过 UTF-8 转换和
/// `bounded_hook_output()` 的保留长度限制。
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

/// 面向事件流的结构化 hook 运行时错误。
///
/// `code` 供机器订阅者分类，`message` 用于展示，`details` 附带事件名、配置来源或超时值。
/// 该错误只描述本地 hook 适配器观察到的失败，不代表控制面策略拒绝。
#[derive(Debug, Clone)]
struct HookRuntimeError {
    code: Option<String>,
    message: String,
    details: Value,
}

/// 从单条 hook stdout 中提取出的有限决定字段。
///
/// 字段缺失时通常保留默认值；因此此旧解析器不是严格 schema 校验器。注意其行为与
/// `hook_outcome::parse_hook_outcome()` 不同，后者当前没有接入本文件的命令输出路径。
#[derive(Debug, Default)]
struct ParsedHookOutput {
    prevent_continuation: bool,
    stop_reason: Option<String>,
    blocking_error: Option<String>,
    ask_reason: Option<String>,
    add_contexts: Vec<String>,
    updated_input: Option<Value>,
}

/// 多条 SessionStart/UserPromptSubmit 命令成功后的累积结果。
///
/// `add_contexts` 按命令顺序追加；`updated_input` 保留最近一次被识别的输入更新。任何最终
/// 聚合错误会令执行器返回 Err，因此此值不会随失败一并返回。
#[derive(Debug, Default)]
struct ContextHookSetResult {
    add_contexts: Vec<String>,
    updated_input: Option<Value>,
}

/// 启动单条 hook shell 命令，传入 JSON，并把退出、超时和取消归一化为 HookRun。
///
/// 【输入/输出】命令来自已解析配置；stdin 是 `hook_input_json()` 生成的事件快照；timeout
/// 由 `KIANA_HOOK_TIMEOUT_MS` 解析；cwd 决定子进程工作目录。返回值包含退出码、有限长度的
/// 输出文本、耗时及结构化失败原因。
///
/// 【核心流程】配置隔离的 shell -> spawn -> 写 stdin -> 等待进程 -> 在等待阶段竞争取消和
/// timeout -> 收集输出并截断保留文本。
///
/// 【重要边界】计时从 spawn 前记录，但 `timeout(...)` 和取消 `select!` 在 stdin
/// `write_all()` 完成后才开始；因此大输入遇到不读取 stdin 的子进程时，写入等待不受这里的
/// timeout/取消 select 保护。`wait_with_output()` 会先收集完整 stdout/stderr，之后才调用
/// 64 KiB 文本截断，所以该阈值限制保留字符串，不限制进程输出的峰值内存。
///
/// 超时/取消离开等待 future 后，`kill_on_drop(true)` 会请求终止直接子进程；Unix 还创建
/// 了新进程组，但此处没有显式向整组发送信号或确认所有后代退出。不要把父 shell 退出等同
/// 于进程树清理完成。
async fn run_hook_command(
    command: &str,
    input_json: &str,
    timeout_duration: Duration,
    abort_signal: &Arc<tokio::sync::Notify>,
    cwd: &Path,
) -> HookRun {
    // 每个命令都在独立受控 shell 子进程中运行，清理宿主环境、固定 cwd、设置进程组并
    // 绑定 kill_on_drop；timeout/取消仍保持 Unknown 边界，不把 leader 退出当成整棵树已停。
    let started = Instant::now();
    let mut shell = shell_command(command, cwd);
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

    // stdin 写入在下面的 timeout/select 之外。hook 的输入一般是一个 JSON 对象，但其大小
    // 取决于调用上下文；如果调整序列化数据规模，应一并评估这里的阻塞范围。
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

    // wait_with_output 同时等待进程结束并读完 piped stdout/stderr；输出先进入内存，之后
    // 才会被 bounded_hook_output 截断，因此不能将后者误读为采集端的内存上限。
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
                stdout: bounded_hook_output(&output.stdout),
                stderr: bounded_hook_output(&output.stderr),
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

/// 为 hook 组装尽量小且可预测的 shell 子进程环境。
///
/// 两个平台都设置 cwd、清空继承环境并把 PATH 限制到系统目录；Windows 优先选择 Git Bash，
/// Unix 使用进程 SHELL 或 `/bin/sh`，并把子进程放入新进程组。环境清理降低 hook 意外读到
/// 调用进程密钥/配置的机会，但并不构成沙箱：命令仍拥有当前 OS 用户可用的文件与系统权限。
///
/// ⚠ 注意：Unix 的 `process_group(0)` 只创建组，不代表超时路径已经杀死并回收整个进程组；
/// 当前明确配置的 drop 行为作用于直接 child。若改动进程管理，必须同时考虑命令派生的后代。
fn shell_command(command: &str, cwd: &Path) -> Command {
    let path = if cfg!(windows) {
        r"C:\Windows\System32;C:\Windows"
    } else {
        "/usr/bin:/bin"
    };
    #[cfg(windows)]
    {
        let mut shell = Command::new(preferred_bash_program());
        shell
            .arg("-lc")
            .arg(command)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", path)
            .kill_on_drop(true);
        shell
    }

    #[cfg(not(windows))]
    {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut shell = Command::new(program);
        shell
            .arg("-lc")
            .arg(command)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", path)
            .kill_on_drop(true);
        shell.process_group(0);
        shell
    }
}

/// 将进程字节输出转成 UTF-8 展示文本，并限制最终保留字符串的长度。
///
/// 上限为 64 KiB；超长时预留 32 字节放置截断标记，并向后退到合法 UTF-8 边界，避免拆坏
/// 多字节字符。输入 `bytes` 已由 `wait_with_output()` 完整收集，所以这个函数控制的是
/// 后续保存/解析的文本长度，而不是子进程可以产生多少输出或峰值内存。
fn bounded_hook_output(bytes: &[u8]) -> String {
    const MAX_HOOK_OUTPUT_BYTES: usize = 64 * 1024;
    if bytes.len() <= MAX_HOOK_OUTPUT_BYTES {
        return String::from_utf8_lossy(bytes).to_string();
    }
    let mut end = MAX_HOOK_OUTPUT_BYTES.saturating_sub(32).min(bytes.len());
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    format!(
        "{}\n[truncated: hook_output_budget]",
        String::from_utf8_lossy(&bytes[..end])
    )
}

#[cfg(windows)]
/// 按显式设置、常见 Git 安装路径、PATH 的顺序选择 Windows Bash。
///
/// 这里的 `KIANA_BASH_PATH` 只用于挑选解释器，不会被传入清理后的 hook 子进程环境；如果
/// 都找不到，保留 `bash` 命令名让系统进程启动错误按正常 hook 失败路径处理。
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
/// 在当前进程 PATH 中依次查找 `bash.exe` 或 `bash`。
///
/// 这是 Windows 解释器选择的回退步骤；不会改变 Unix 分支使用 SHELL 的行为。
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
/// 排除 Windows 自带的旧 WSL Bash 路径，优先使用 Git Bash 或其他可执行 Bash。
fn is_windows_system_bash(path: &Path) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    canonical
        .to_string_lossy()
        .eq_ignore_ascii_case(r"C:\Windows\System32\bash.exe")
}

/// 配置解析阶段的结果：可运行命令与按来源归类的错误分开保存。
///
/// 即使一个来源出错，其他文件/插件仍可能产生命令；上层会同时保留错误并按当前策略
/// 执行解析成功的命令。因此“发现错误”不等于“这组命令一个都不会运行”。
#[derive(Debug, Default)]
struct HookCommandResolution {
    commands: Vec<String>,
    errors: Vec<HookRuntimeError>,
}

/// 按固定优先级解析某类 hook 的命令，并附加文件和插件来源。
///
/// 优先级是事件专用环境变量 > 通用 `KIANA_HOOKS` > 用户/KIANA_HOME/受信项目文件，再
/// 追加已安装且有 manifest 的插件。只要环境变量存在就会直接返回它的解析结果：空值可以
/// 表示空命令集；格式错误会成为错误且不会回退到文件。因而 env override 是替代来源，
/// 不是在文件命令后再加一层。
///
/// 【信任边界】项目文件路径由 `hooks_file_paths_with_trust()` 依 ProjectTrust 过滤；插件
/// 配置另由已安装插件清单发现逻辑处理。`KIANA_HOOKS_FILE` 是路径直连覆盖项，会在路径
/// helper 中提前返回，其项目 trust 语义需单独审慎理解。
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

/// 读取普通 hook 配置文件，将该事件对应的字符串命令追加到 resolution。
///
/// 每个文件独立解析；读文件、JSON 解码或 schema 验证失败会保留来源路径和错误，再继续看
/// 后续文件。解析成功的命令和其他文件失败可以同时出现在结果中，后续 executor 当前会
/// 先报告错误、仍执行这些有效命令。
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

/// 从启用的已安装插件中读取 `hooks/hooks.json` 并提取指定事件命令。
///
/// 插件根目录由 `kiana-types` 提供；没有已识别 manifest 的根目录跳过，没有 hooks 文件或
/// 文件读取失败也按“没有该插件 hook”处理。文件存在但 JSON/schema 无效时则保留结构化错误。
/// 插件来源在普通文件解析之后追加，因此配置顺序也决定命令运行顺序。
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

/// 将子进程运行结果包装成可发送给调用方的错误对象。
///
/// 错误码优先沿用运行阶段已经确定的稳定码；若没有则使用通用执行错误码。details 先带上
/// 事件名、命令和耗时，再合并 timeout 等阶段性细节，供上层诊断而不改变错误是否阻断。
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

/// 为配置文件读取失败生成带事件名和文件路径的结构化错误。
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

/// 为配置文件 JSON 语法错误生成结构化错误；它与 schema 错误分开，便于定位失败阶段。
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

/// 把 `kiana-types` 配置解析器的 schema 错误保留为 Query 层 hook 错误。
///
/// location 指向解析器报告的位置，source 标明环境变量或文件来源；本 helper 只包装诊断，
/// 不尝试修复配置或把无效字段忽略掉。
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

/// 把后续阶段的诊断字段并入已有 JSON 对象；同名键由后加入的 source 值覆盖。
///
/// 两边不是对象时直接不合并，避免为了错误诊断把任意 JSON 值强行改写成新形状。
fn merge_runtime_error_details(target: &mut Value, source: &Value) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

/// 复用 `kiana-types` 的插件发现与启用状态过滤，避免 Query 再维护一套插件目录规则。
fn installed_plugin_roots() -> Vec<PathBuf> {
    kiana_types::plugin::installed_plugin_roots()
}

/// 在支持的插件 manifest 位置中寻找第一个存在的清单文件。
///
/// 返回的路径只用于确认该目录符合已识别插件布局，不负责解析 manifest 内容或授予插件信任。
fn find_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// 按用户级、KIANA_HOME 级和项目级规则生成 hook 文件候选路径。
///
/// 默认情况下先选 `KIANA_HOME/hooks.json`，没有 KIANA_HOME 才选 `$HOME/.kiana/hooks.json`；
/// 然后仅在 `ProjectTrust::Trusted` 时加入 cwd 下 `.kiana/hooks.json`。相同路径不会重复加入。
///
/// ⚠ `KIANA_HOOKS_FILE` 是特殊覆盖：一旦设置就只返回该路径，直接跳过后续项目 trust
/// 过滤。它是显式环境配置而非自动发现的项目文件；维护其语义时要避免把“项目文件候选
/// 已过滤”的结论误套到这个 override 上。
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

/// 将内部事件和调用上下文序列化成 hook 脚本从 stdin 读取的 JSON 快照。
///
/// 通用字段描述事件名、权限模式、查询入口及 agent/team 上下文；事件专属字段只在相应
/// 变体中加入。兼容字段同时提供 snake_case 与 camelCase 名称，因此修改名称/值需要考虑
/// 已有脚本。`active` 只输出到 `stop_hook_active` 供脚本判断，不会让 Rust 执行器跳过命令。
/// 该 JSON 是上下文数据，不能作为 hook 授权、信任或参数验证的证明。
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

/// 读取单条 hook 等待进程退出时使用的超时。
///
/// 缺失、非整数或 0 毫秒时回退到 30 秒；任何正整数都被接受，没有额外最大值。因此注释
/// 中的默认 30 秒不是配置上限。超时实际包裹 `wait_with_output()`，stdin 写入位于其外。
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
/// hook 以非零退出码结束时，是否将其提升为阻断错误。
///
/// 该策略只用于 `execute_hook_set()` 的非零退出分支；结构化 JSON blocking_error、启动错误、
/// timeout 和配置错误仍按各自规则处理。上下文执行器不应用 Warning 放行。
enum HookNonzeroPolicy {
    Deny,
    Warning,
}

impl HookNonzeroPolicy {
    /// 返回该策略是否要求非零退出阻断当前 hook 组。
    fn is_deny(self) -> bool {
        self == HookNonzeroPolicy::Deny
    }
}

/// 将 `KIANA_HOOK_NONZERO_POLICY` 解析为 Deny 或 Warning。
///
/// 只有 warning/warn/non_blocking/non-blocking/allow（忽略大小写和首尾空格）才是 Warning；
/// 缺失或任何其他拼写都保守回到 Deny。它不会改变命令是否运行，只影响非零退出码是否阻断。
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

/// 解析兼容 hook stdout 中有限的一组 JSON 字段。
///
/// 【输入/输出】输入是已经 trim、且经过文本截断的 stdout；输出包括继续标记、停止原因、
/// 阻断错误、Ask、上下文以及更新输入。调用者先检查进程启动结果和退出码，再调用本函数。
///
/// 【宽松行为】无效 JSON 或非 object JSON 会得到默认结果；缺失/非布尔 continue 默认为 true，
/// 未识别字段和错误类型通常被忽略。也就是说，此旧解析器不把 malformed stdout 当作错误，
/// 可能使该条命令表现为“没有结构化决定”。不要把它与 `hook_outcome` 中更严格的解析器
/// 混为一谈；当前实际 shell 消费路径走的是本函数。
///
/// 【失败处理】有效 JSON 中识别出的阻断字段会进入 HookSetResult 并通过 BlockingError
/// 事件展示；本 helper 自身不记录日志、不重试、不执行权限检查。上下文入口还会把 Ask/阻断
/// 映射成整组 Err。
// 非 JSON stdout 视为没有结构化决定；命令退出码和 stderr 仍由上层先行处理。只有
// 可解析字段才会改变继续、Ask、错误或输入更新状态。
fn parse_hook_output(stdout: &str) -> ParsedHookOutput {
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

/// 从兼容字段名中取第一份非 null 的参数更新 JSON 值。
///
/// `null` 被视为“没有更新”，但对象、数组、字符串和其他 JSON 标量都可返回；本 helper
/// 不校验新值是否符合目标工具 schema，调用方需在继续工具执行前重新验证。
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

/// 从 UserPromptSubmit 更新值中提取一个可用提示字符串。
///
/// 支持直接字符串，或 object 内按 user_prompt/userPrompt/prompt/input 顺序查找第一个非空
/// 字符串。这里只认文本，不把数组、数字或对象转换成提示；因此复杂更新可能被忽略。
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

/// 保留原始提示文本，但拒绝去除空白后为空的字符串。
///
/// 非空时返回未经 trim 的原文，以免静默改写用户实际提交内容。
fn nonempty_prompt(text: &str) -> Option<String> {
    (!text.trim().is_empty()).then(|| text.to_string())
}

/// 从四种兼容字段名中汇总额外上下文；重复字段会按这里的查找顺序分别加入。
///
/// 实际数组元素过滤与 trim 由 `contexts_from_value()` / `clean_contexts()` 执行。
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

/// 将单个上下文字段规范为字符串列表。
///
/// 字符串成为一个元素；数组中非字符串会被静默丢弃；null、对象和数字都变为空列表。
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

/// 清除上下文两端空白并移除空字符串，保留其余文本和原始顺序。
fn clean_contexts(contexts: Vec<String>) -> Vec<String> {
    contexts
        .into_iter()
        .map(|context| context.trim().to_string())
        .filter(|context| !context.is_empty())
        .collect()
}

/// 从 decision、blocking_error 或 blocking_errors 字段提取阻断说明。
///
/// decision 识别 block/blocked/deny/denied/reject/rejected，并优先使用 reason；数组只拼接
/// 其中的字符串。其他值不视为阻断，空数组或全非字符串数组得到 None。
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

/// 从 decision/behavior 的 Ask 类取值提取审批原因，没有非空文案时给出固定默认说明。
///
/// 此处只产生待上层消费的文本，不弹审批界面，也不会建立 Approval 记录。
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

/// 将单调时钟上的耗时转换成毫秒整数；极端情况下饱和为 u64::MAX。
///
/// 单调时钟用于测量经过时间，不受系统日期调整影响；该值用于 hook 诊断，不是时间戳。
fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// 本模块的局部行为测试，使用进程环境变量、临时配置文件和短 shell 命令合成 hook 来源。
///
/// 这些测试检查命令选择、结果折叠、trust 路径和取消等适配器行为；它们不启动真实
/// ControlPlane，不验证 EventLog 持久化，也不证明 Stop 已接入产品查询循环。环境变量是
/// 进程级共享状态，因此本模块用异步互斥锁串行化自己的测试；这个锁不约束其他测试模块。
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    /// 持有到测试结束的模块内环境锁，避免两个测试同时改写相同 hook 环境变量。
    async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    /// 构造使用当前目录、Trusted 项目和空取消信号的 Stop 测试上下文。
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

    /// 消费完整事件流后等待最终结果；先持续读取是为了不让有界 mpsc 反压测试任务。
    async fn run_and_collect(ctx: StopHookContext) -> (Vec<StopHookEvent>, StopHookResult) {
        let mut handle = handle_stop_hooks(ctx);
        let mut events = Vec::new();
        while let Some(event) = handle.events.recv().await {
            events.push(event);
        }
        let result = handle.result.await.unwrap();
        (events, result)
    }

    /// 清除本模块涉及的 hook 配置，确保每个用例从可控的空环境开始。
    ///
    /// 该 helper 不保存并恢复测试进程原有值，所以调用点必须持有 env_guard。
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

    /// 为 PreToolUse trust 测试构造带固定 Bash 参数的输入。
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

    /// 为 PostToolUse trust 测试构造成功结果快照；不实际执行 Bash 工具。
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

    /// 构造 SessionStart 上下文 fixture，供不同 ProjectTrust 值复用。
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

    /// 构造带固定原始提示的 UserPromptSubmit fixture。
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

    /// 将一条 shell 命令编码成解析器要求的 JSON 命令数组环境变量。
    fn set_stop_hook(command: &str) {
        std::env::set_var(
            "KIANA_STOP_HOOKS",
            serde_json::to_string(&vec![command]).unwrap(),
        );
    }

    // 第一组：Stop 决定、退出码、warning 策略、timeout 与事件摘要。
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

    // 第二组：文件配置解析与受信/未受信项目路径矩阵。
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

    // 第三组：各 hook 类型共享同一项目 trust 来源边界，但分别验证各自输入/输出适配。
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

    // 第四组：插件启用状态、环境变量覆盖、取消及展示文案。
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
