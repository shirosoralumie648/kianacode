//! 受 Kiana Runner 协议约束的模型循环运行时。
//!
//! `KianaHarness` 是主路径的模型循环，由 `kiana-daemon::DaemonHost` 构造。
//! 它调用注入的 `ModelClient`，但不直接执行模型要求的工具副作用：固定的
//! `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write` 调用先转换为
//! `CapabilityRequest`，再回到 ControlPlane 授权，由 Broker 执行。
//!
//! 脚本和 fake provider 可证明本地行为，不代表已验证 live provider 或 UI token
//! streaming。`reference/` 仅供审计对照，不是 workspace 成员或运行时依赖。
//!
//! ## 13 个私有模块
//!
//! 模块路径保持私有；下方 `pub use` 集中导出类型、函数和常量。内部可以调整文件组织，
//! 但导出签名、协议字段和可观察行为仍需保持兼容。
//!
//! | 模块 | 职责与边界 |
//! |---|---|
//! | `model` | re-export domain 值对象与 ports 的 `ModelClient`，提供脚本/不可用实现 |
//! | `tools` | 五个固定工具名及参数到能力请求的翻译；不执行工具 |
//! | `harness` | 模型循环、运行表、取消、Continue 与 checkpoint |
//! | `state_driver` | 纯状态转移及意图输出；不做 I/O |
//! | `step_context` | 每步编译 ContextPlan 并冻结 ResolvedStepContext；不发请求、不执行工具、不授予权限 |
//! | `inbox` | 按 next-turn/next-step 分队列的输入接收与认领 |
//! | `compact` | 根据预算压缩历史，并保留完整的近期消息组 |
//! | `stream_normalizer` | 每个 attempt 的增量累积与最终输出验证 |
//! | `progress` | domain 进度 reducer 的导出层与证据适配器；harness 在已知工具结果/空 turn 上调用 observe |
//! | `budget` | 按调用方 scope 共享的内存预算账本，不授予执行权限 |
//! | `retry` | 重试条件、次数/deadline 检查与退避计算，不另起重试循环 |
//! | `tool_batch_scheduler` | 有界并行只读组与独占屏障的批次执行器；只调度调用方闭包，不执行工具 |
//! | `protocol_runner` | 无状态命令到事件的 helper；不调用 `KianaHarness` |
//!
//! ## 阅读顺序与依赖
//!
//! 先读 `model` 的接口边界，再读 `harness::invoke_model` 和工具结果回灌路径；
//! `state_driver`、`inbox`、`stream_normalizer` 直接使用 domain 类型，
//! `compact` 和 `tools` 还使用 `model` 的 re-export。这些 helper 不反向调用 harness。
//!
//! 当前直接依赖本 crate 的 workspace crate 是 daemon 和 core；Core 使用 ports 的
//! `RunnerPort` 接口，daemon 负责注入具体 harness。`ProtocolRunner` 只有自身的
//! `send` 方法，当前没有实现 `RunnerPort`，也不是生产循环的协议包装层。
//!
//! Runner 产生 `RunnerEvent`，不持有 `EventStorePort`。事件持久化、版本与幂等校验
//! 由 ControlPlane 调用 EventStore 完成，不能从 Runner 开第二条写事实的通路。

// 私有实现构件；公开符号由下方的 pub use 清单限定。
mod budget;
mod compact;
mod harness;
mod inbox;
mod model;
mod progress;
mod protocol_runner;
mod retry;
mod state_driver;
mod step_context;
mod stream_normalizer;
mod tool_batch_scheduler;
mod tools;

// ■ 预算配置与快照
// daemon 可配置 HarnessBudgetConfig；harness 再与 run 的步数及 assignment 限额取交集。
// 账户当前按项目和角色共享，不是每个 run 一份。修复计数接口存在不代表修复循环已接入。
// 导出默认值使调用方和测试不必复制数字；快照不是持久预算租约。
pub use budget::{
    BudgetUsageSnapshot, HarnessBudgetConfig, HarnessBudgetSource, DEFAULT_MAX_ATTEMPTS_PER_TASK,
    DEFAULT_MAX_COMPACTIONS_PER_TASK, DEFAULT_MAX_MODEL_STEPS_PER_TURN,
    DEFAULT_MAX_REPAIRS_PER_TASK, DEFAULT_MAX_TOKENS_PER_TASK, DEFAULT_MAX_TOOL_CALLS_PER_TASK,
    HARNESS_BUDGET_SCHEMA,
};

// ■ 上下文压缩
// 三段式流程：纯函数规划（外置大结果、去重、选择较旧完整组）→ 用准入模型生成摘要
// （purpose=compaction、tools 为空、独立有界预算）→ 绑定服务端权威证据后校验并替换视图。
// 失败一律保留原视图并暂停，不用本地占位摘要继续执行；解释对象只含摘要与计数。
pub use compact::{
    approx_token_count, build_compacted_history, build_compacted_history_with_summary,
    build_compacted_view, compact_if_needed, explain_compaction, history_tokens,
    is_summary_message, parse_compaction_summary, plan_compaction, summary_request_messages,
    validate_summary_for_plan, CompactOutcome, CompactionExplanation, CompactionPlan,
    DuplicatedResult, ExternalizedResult, COMPACTION_EXPLANATION_SCHEMA, COMPACTION_PLAN_SCHEMA,
    COMPACT_SUMMARY_BUDGET_TOKENS, COMPACT_SUMMARY_INSTRUCTION, COMPACT_SUMMARY_MAX_TOKENS,
    COMPACT_SUMMARY_PROMPT_MAX_TOKENS, COMPACT_USER_MESSAGE_MAX_TOKENS,
    DEFAULT_COMPACT_TRIGGER_TOKENS, EXTERNALIZE_RESULT_TOKENS, SUMMARY_PREFIX,
};

// ■ Harness 本体
// DaemonHost 通过 with_config 构造，Core 通过 RunnerPort 调用。
// RuntimeConfig 控制步数、连续重复调用阈值、挂钟和压缩等参数；
// KianaHarnessError 是错误类型，两个 HARNESS_* 常量用于标识实现和结果格式。
pub use harness::{
    KianaHarness, KianaHarnessError, RuntimeConfig, HARNESS_ID, HARNESS_RESULT_SCHEMA,
};

// ■ 收件箱
// insert/insert_for_run 接收文本，claim 在指定边界取走消息；这里不解释命令或授予权限。
// 容量限制会产生反压。ProtocolRunner 不持有 inbox，也不负责这些限制。
pub use inbox::{
    Inbox, InboxMessage, InboxTarget, INBOX_MAX_CLAIMED_IDS, INBOX_MAX_MESSAGES,
    INBOX_MAX_TEXT_BYTES,
};

// ■ 模型边界
// 值对象定义在 domain，ModelClient 定义在 ports；这里导出统一访问入口。
// ScriptedModel/UnavailableModel 是本地实现，真实适配器由 daemon 侧注入。
// 配置不可用时明确失败，不能把空响应伪装成模型完成。
pub use model::{
    ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelRequestContext,
    ModelRole, ModelToolCall, ModelUsage, ScriptedModel, UnavailableModel,
};

// ■ 进度 reducer
// observe 消费 ProgressInput 证据并返回 ProgressDecision；默认值限定证据窗口与停滞次数。
// harness 在每次已知工具结果与空 turn 时调用 observe，并按决定注入一次性反馈、等待澄清
// 或停止 turn；checkpoint/restore 携带 tracker，连续重复工具调用检查仍在 record_tool_call。
// failure_digest 生成失败原因摘要，不提供完整脱敏保证。
pub use progress::{
    failure_digest, ProgressAction, ProgressDecision, ProgressEvidence, ProgressInput,
    ProgressTracker, DEFAULT_NO_PROGRESS_LIMIT, DEFAULT_PROGRESS_WINDOW,
};

// ■ 无状态协议 helper
// send 直接从 RunnerCommand 构造事件：Start 报不可用，Continue/Resume/Inject 报不存在，
// Cancel 报取消；CapabilityResult 只按传入结果回传。它不调用模型，不做 wire 反序列化，
// 不持有运行状态，也不代替 KianaHarness 的 RunnerPort 实现。
pub use protocol_runner::ProtocolRunner;

// ■ 状态机
// transition 接收旧帧与输入，返回新帧及意图；RunDriver 的便捷方法更新帧并丢弃意图列表，
// harness 自己编排 I/O。DEFAULT_MAILBOX_CAPACITY 限定 driver 的输入计数，
// 与 Inbox 的 INBOX_MAX_MESSAGES 是两道不同限制，不能互换。
pub use state_driver::{
    transition, DriverError, DriverInput, DriverIntent, DriverTerminal, DriverTransition,
    HarnessPhase, RunDriver, RunFrame, TurnFrame, DEFAULT_MAILBOX_CAPACITY, RUN_FRAME_SCHEMA,
    TURN_FRAME_SCHEMA,
};

// ■ 单步不可变上下文
// harness 在每条模型请求前用同一份 bundle、消息快照与 wire 预算编译 ContextPlan 并冻结为
// ResolvedStepContext；路由/目录/workspace/data 漂移在预留预算前拒绝发送（fail closed）。
// 解释对象记录摘要与剔除原因，不包含消息全文；module 不发请求、不执行工具，也不授予权限，
// 其中 scope 是 runner 侧占位值（真实授权仍在 ControlPlane）。
pub use step_context::{
    compile_step_context, compile_step_context_with_prefix, compile_wire_budget,
    compile_wire_budget_with_accounting, decide_stable_prefix_reuse, derive_data_binding,
    derive_workspace_binding, recheck_step_context_bindings, wire_budget_part_items,
    wire_budget_parts, StablePrefixReuse, StepBudgetPart, StepContextExplanation,
    StepContextInputs, StepContextOmission, WireBudgetParts, CONSERVATIVE_WIRE_ACCOUNTING,
    STEP_CONTEXT_EXPLANATION_SCHEMA, STEP_CONTEXT_MAX_MESSAGE_CANDIDATES,
};

// ■ 流式归一化
// ModelStreamAccumulator::push 返回 Result<(), String>，只积累/检查增量；
// finish 消费 accumulator 与最终 ModelOutput 后才返回完整输出或错误。
// 四个 MAX 值分别限定 delta 数、文本字节、工具块数与单个工具参数字节数。
pub use stream_normalizer::{
    ModelStreamAccumulator, STREAM_NORMALIZER_MAX_DELTAS, STREAM_NORMALIZER_MAX_TEXT_BYTES,
    STREAM_NORMALIZER_MAX_TOOL_BLOCKS, STREAM_NORMALIZER_MAX_TOOL_BYTES,
};

// ■ 工具翻译
// 两个函数把 ModelToolCall 映射成 CapabilityRequest，后者允许调用方指定 request_id。
// 模型可见工具名仍是五个冻结名称；不能借注释维护扩展工具面或绕过 ControlPlane。
pub use tools::{capability_for_tool, capability_for_tool_with_request_id};

// ■ 有界并行工具批次调度
// 按 domain 的 ToolBatchPlan 顺序调度调用方提供的 admit/execute 闭包：只读组有界并发，
// 独占组前后各 drain 一次形成屏障，历史按源 ordinal 排列。执行器不执行工具副作用，
// 也不授予权限；每个 execute 仍必须走 ControlPlane → Broker。
pub use tool_batch_scheduler::{
    ToolBatchAdmission, ToolBatchCallOutcome, ToolBatchCallState, ToolBatchRunReport,
    ToolBatchScheduler,
};
