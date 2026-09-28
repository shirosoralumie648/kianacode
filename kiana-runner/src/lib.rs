//! 受 Kiana Runner 协议约束的模型循环运行时。
//!
//! 生产 `KianaHarness` 归本 crate 所有；inbox、compact 和工具协议在这里统一定义，避免
//! daemon 或入口层再包一层第二模型循环。模型只能看到固定的 `shell`、`apply_patch`、
//! `mcp`、`memory.search`、`memory.write` 工具名；每次调用都会回到 ControlPlane 形成
//! `CapabilityRequest`，真正执行仍由受控 Broker 完成。
//!
//! 当前模型客户端可以是脚本或显式不可用实现；脚本通过测试/本地 cassette 证明行为，
//! 不等于已接入 live provider。`reference/` 中的上游源码只作为审计对照，不是 workspace
//! 成员或运行时依赖。
//!
//! ---------------------------------------------------------------------------
//! 【下面这一段是给「刚接手本 crate」的读者做的导读】
//!
//! ## 本 crate 在系统里的位置
//!
//! `kiana-runner` 是**规范 agent 循环（canonical agent loop）**：整条执行脊柱
//! （execution spine）上唯一负责「跑模型 + 决定下一步做什么」的那一段。
//! 它**不执行任何副作用**——模型的工具调用会被翻译成能力请求（CapabilityRequest）
//! 交回 ControlPlane 审批，再由 Broker 真正执行。
//!
//! * 上游（谁构造我）：`kiana-daemon` 的 `DaemonHost`（组合根），
//!   通过 `KianaHarness::with_config(...)` 构造，见 `kiana-daemon/src/lib.rs:1098`。
//! * 下游（我喂给谁）：ControlPlane → Broker → 事件日志（EventLog）→ 收据（Receipt）。
//! * 入口：对外只暴露本文件底部的 `pub use` 列表；生产入口是 `KianaHarness`。
//!
//! ## 12 个私有模块各自负责什么
//!
//! 所有模块都是 `mod`（私有）而不是 `pub mod`——**模块实现不对外暴露，
//! 对外只暴露类型本身**。这样上层能用的符号集合被这份 `pub use` 清单穷尽，
//! 以后想改内部结构不会变成破坏性变更。
//!
//! | 模块 | 职责 | 被谁调用（crate 内） |
//! |---|---|---|
//! | `model` | 模型请求/响应值对象 + `ModelClient` trait + 脚本/不可用实现 | 被 `harness`、`compact`、`tools` 引用 |
//! | `tools` | 五个冻结工具面 → 能力请求的翻译（**冻结项，只读**） | `harness` |
//! | `harness` | `KianaHarness`：规范循环本体、运行表、取消、检查点 | `protocol_runner`；被 daemon 构造 |
//! | `state_driver` | 纯函数状态机：阶段迁移与意图（**无 I/O**） | `harness` |
//! | `inbox` | 暂停期间注入消息的暂存与认领 | `harness` |
//! | `compact` | 上下文超限时的历史压缩 | `harness` |
//! | `stream_normalizer` | 流式增量归一化：分片 → 完整输出 | `harness` |
//! | `progress` | 进度追踪，防模型空转重复干活 | `harness` |
//! | `budget` | 预算台账（步数/token/压缩次数/修复次数上限） | `harness` |
//! | `retry` | 模型调用的重试判定与退避 | `harness` |
//! | `protocol_runner` | `RunnerPort` 的接线适配，把 wire 命令翻译成 harness 调用 | 被 daemon 持有 |
//!
//! ## 模块依赖图（箭头 = 「被谁依赖」，从上往下依赖）
//!
//! ```text
//!   kiana-daemon::DaemonHost  ──构造──▶  KianaHarness
//!                                            │
//!              ┌──────────┬──────────┬───────┴───────┬──────────┬─────────┐
//!              ▼          ▼          ▼               ▼          ▼         ▼
//!          state_driver  inbox   stream_normalizer  compact  progress  budget
//!            (纯函数)                              │       (retry 同级)
//!              │          │          │             │
//!              └──────────┴─────┬────┴─────────────┘
//!                               ▼
//!                            model ◀── tools
//!                               │
//!                               └──▶ kiana-domain（值对象、摘要、校验）
//! ```
//!
//! 注意方向：`model` 位于底层（不含循环逻辑），`harness` 位于顶层并依赖其余全部。
//! `state_driver` / `inbox` / `compact` / `stream_normalizer` 都不依赖 `harness`，
//! 也不互相依赖——它们各自是**可单测的纯构件**。
//!
//! ## 为什么要显式 `pub use` 出去
//!
//! 只有 `kiana-daemon` 和 `kiana-core` 两个 crate 依赖本 crate（实测自各自的 Cargo.toml）。
//! 把它们需要的符号集中在一处 `pub use`，外部就只需要 `use kiana_runner::{...}` 一个入口，
//! 不必知道符号住在哪个私有模块里——内部重构（合并/拆分模块）不会波及调用方。
//!
//! ## 刻意「没有」导出的东西
//!
//! 这份清单里**没有** `EventStorePort` 之类的符号。原因是刻意的：
//! runner 只负责**产生**待记录的事实（`RunnerEvent`），**不碰事件存储**。
//! 事件只追加（append-only）的落盘、CAS（compare-and-swap，比较并交换）版本单调、
//! 幂等键去重，全部由 `kiana-eventlog` 负责，经 ControlPlane 走。
//! 如果 runner 自己去写事件，就等于开了第二条写事实的通路，破坏「唯一执行脊柱」。

mod budget;
mod compact;
mod harness;
mod inbox;
mod model;
mod progress;
mod protocol_runner;
mod retry;
mod state_driver;
mod stream_normalizer;
mod tools;

// ---------------------------------------------------------------------------
// 下面每组 `pub use` 前的说明回答两个问题：
//   1. 这个模块对外承诺了什么；
//   2. 谁在用（只列实测到的调用点，不猜）。
// ---------------------------------------------------------------------------

// 预算：限制单次运行最多花多少步 / 多少 token / 压缩多少次 / 修复几次。
// 上限值以 `DEFAULT_*` 常量形式导出，是为了让 daemon 能读到并展示同一组默认值，
// 而不是让调用方自己再写一份数字。
pub use budget::{
    BudgetUsageSnapshot, HarnessBudgetConfig, HarnessBudgetSource, DEFAULT_MAX_ATTEMPTS_PER_TASK,
    DEFAULT_MAX_COMPACTIONS_PER_TASK, DEFAULT_MAX_MODEL_STEPS_PER_TURN,
    DEFAULT_MAX_REPAIRS_PER_TASK, DEFAULT_MAX_TOKENS_PER_TASK, DEFAULT_MAX_TOOL_CALLS_PER_TASK,
    HARNESS_BUDGET_SCHEMA,
};
// harness：生产入口。`KianaHarness` 由 daemon 的 DaemonHost 用
// `KianaHarness::with_config(model_client::from_env(), config)` 构造
// （kiana-daemon/src/lib.rs:1098 / :1668），测试里则用 `ScriptedModel` 构造
// （kiana-core/tests/control_plane.rs:317）。`RuntimeConfig` 允许覆盖步数、重复工具调用阈值、
// 挂钟预算和压缩阈值。
pub use harness::{
    KianaHarness, KianaHarnessError, RuntimeConfig, HARNESS_ID, HARNESS_RESULT_SCHEMA,
};
// inbox：收件箱。运行被 capability 请求挂起、或跨 turn 注入文本时，消息先落在这里，
// 在下一个时间边界被 `claim` 走并变成一条 `user` 模型消息。
// 三个 `INBOX_MAX_*` 也导出，是为了让 daemon/测试能用**同一组上限**构造反压场景。
pub use inbox::{
    Inbox, InboxMessage, InboxTarget, INBOX_MAX_CLAIMED_IDS, INBOX_MAX_MESSAGES,
    INBOX_MAX_TEXT_BYTES,
};
// model：模型边界。`ModelClient` 是 trait（接口），
// 上层提供实现——生产是 `kiana-provider` 的网关包装，
// 离线/测试是 `ScriptedModel`（脚本化）和 `UnavailableModel`（明确不可用）。
// `UnavailableModel` 的存在是 fail-closed（失败即关闭）的体现：
// 配置冲突时宁可返回「不可用」并给出稳定错误码，也不空转、不伪造一次成功响应。
pub use model::{
    ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelRequestContext,
    ModelRole, ModelToolCall, ModelUsage, ScriptedModel, UnavailableModel,
};
// progress：防空转。模型反复给出同样的工具调用时，循环会停下来而不是无限烧预算。
// `failure_digest` 用摘要（digest）而不是原文记录失败，便于比对而不泄漏内容。
pub use progress::{
    failure_digest, ProgressAction, ProgressDecision, ProgressEvidence, ProgressInput,
    ProgressTracker, DEFAULT_NO_PROGRESS_LIMIT, DEFAULT_PROGRESS_WINDOW,
};
// protocol_runner：`RunnerPort`（kiana-ports 定义的端口契约）的接线实现，
// 把版本化的 wire 命令（`RunnerCommand`）翻译成 harness 的方法调用。
// 它是「协议层 ↔ 循环层」之间唯一的翻译点，循环逻辑本身不知道 wire 格式。
pub use protocol_runner::ProtocolRunner;
// state_driver：纯状态机。**没有任何 I/O、时钟或 ID 分配**，
// 因此可以被确定性测试，也因此状态机本身不会成为「偷偷做副作用」的地方。
// 注意 `transition` 是自由函数（只吃 `&RunFrame` 返回新帧），
// 而 `RunDriver` 只是给它套了一层可变外壳。
pub use state_driver::{
    transition, DriverError, DriverInput, DriverIntent, DriverTerminal, DriverTransition,
    HarnessPhase, RunDriver, RunFrame, TurnFrame, DEFAULT_MAILBOX_CAPACITY, RUN_FRAME_SCHEMA,
    TURN_FRAME_SCHEMA,
};
// stream_normalizer：把流式分片拼成完整 `ModelOutput`。
// 导出的四个 `STREAM_NORMALIZER_MAX_*` 是有界输入的硬上限——
// 流的另一半是外部 provider，必须有界，否则一个异常对端就能耗尽内存。
pub use stream_normalizer::{
    ModelStreamAccumulator, STREAM_NORMALIZER_MAX_DELTAS, STREAM_NORMALIZER_MAX_TEXT_BYTES,
    STREAM_NORMALIZER_MAX_TOOL_BLOCKS, STREAM_NORMALIZER_MAX_TOOL_BYTES,
};

// tools：五个冻结工具面到能力请求的翻译入口。
// 模型只能看到 `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write`。
// 这里**只做翻译，不做执行**：产物是交给 ControlPlane 过审的申请单。
pub use tools::{capability_for_tool, capability_for_tool_with_request_id};
