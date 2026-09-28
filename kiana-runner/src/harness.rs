//! Owned in-process agent harness.
//!
//! Loop and inbox semantics are derived from DeepSeek Harness (MIT)
//! `packages/core/agent-loop` + `packages/core/agent`. Model-visible tool names
//! and item shapes are derived from OpenAI Codex (Apache-2.0) `codex-rs/exec`
//! and `codex-rs/protocol` (`shell`, `apply_patch`).
//!
//! Copied into Kiana; `reference/` is audit-only and is not a workspace member.
//! Tools never execute here. Each call becomes a `CapabilityRequest` for the
//! control plane broker.
//!
//! ---------------------------------------------------------------------------
//! 【本文件负责什么】
//! ---------------------------------------------------------------------------
//! 这是 Kiana 的**规范 Agent loop（canonical agent loop，即全系统唯一的、
//! 被认可的那条「模型 ↔ 工具」循环）**的内存态实现。
//! 所谓「规范」，是说 Kiana 里**不允许存在第二条执行循环**：
//! 任何想驱动模型、想让模型干活的路径，都必须经过本文件。
//!
//! 它负责的六件事：
//! 1. **组装消息**（assemble messages）：把 system prompt、用户提示、
//!    历史对话、工具观测、收件箱注入按顺序拼成一次模型请求。
//! 2. **决定何时调模型**（何时 / 调几次）：受 `max_steps_per_turn`、
//!    预算、墙钟、取消信号、工具目录摘要共同约束。
//! 3. **翻译工具调用**：把模型吐出的 `ModelToolCall` 翻成
//!    `CapabilityRequest`（能力请求 = 「申请干活的单子」，不是执行本身）。
//! 4. **上下文超限就压缩**（compaction）：消息太长时把旧内容折叠成摘要。
//! 5. **暂停与恢复**（inbox / continue）：等 Broker 回话时挂起，暂停期间
//!    到达的注入消息存在收件箱里，下次模型步取走。
//! 6. **防空转**（repeated tool call）：同一个工具同一组参数反复调用就拒绝。
//!
//! **它自己一个副作用都不执行。** 这是本文件最重要的性质。
//! 模型的工具调用在这里**只会被转换成事件**，真正的执行发生在
//! kiana-capability-broker 那侧。
//!
//! ---------------------------------------------------------------------------
//! 【在系统里的位置 / 上下游】
//! ---------------------------------------------------------------------------
//! ```
//!   kiana-entrypoints (CLI / workbench / web / desktop / MCP)
//!        │  RunnerCommand (版本化 wire 命令)
//!        ▼
//!   kiana-daemon::DaemonHost  ← 唯一组合根，生产上唯一构造本文件类型的地方
//!        │  持有 Arc<KianaHarness>
//!        ▼
//!   kiana-core::ControlPlane  ← 授权与生命周期权威
//!        │  通过 kiana_ports::RunnerPort 这个 trait 驱动本文件
//!        ▼
//!   kiana-runner::KianaHarness  ←【本文件】规范 Agent loop
//!        │  只产出 RunnerEvent（kiana_runner_protocol 的 wire 类型）
//!        ▼
//!   Broker 执行副作用 → 回 CapabilityResult → ControlPlane → 事件日志
//! ```
//!
//! * **上游调用者（已核实）**：生产上只有 `kiana-daemon` 的 `DaemonHost`
//!   构造 `KianaHarness`
//!   （`kiana-daemon/src/lib.rs:1098`、`:1668-1670` `configured_env_harness()`）。
//!   测试里 `kiana-core/tests/control_plane.rs:317-318` 与多个
//!   `kiana-daemon/tests/*.rs` 也构造它。
//!   `ControlPlane` 通过 `impl RunnerPort for KianaHarness`（:1812）调用它，
//!   具体调用点在 `kiana-core/src/lifecycle.rs`（Start / Continue / Inject / Cancel）、
//!   `kiana-core/src/dispatch.rs:241`（CapabilityResult 回灌）、
//!   `kiana-core/src/recovery.rs:273/588`（checkpoint / restore）。
//! * **下游依赖（本文件调用谁）**：同 crate 的兄弟文件
//!   `crate::budget`、`crate::compact`、`crate::inbox`、`crate::model`、
//!   `crate::progress`、`crate::retry`、`crate::state_driver`、
//!   `crate::stream_normalizer`、`crate::tools`。
//!   另有 `kiana-domain`（值对象与脱敏/摘要）、`kiana-ports`（trait 契约与错误类型）。
//!   依赖方向永远是 domain/ports 在底层。
//!
//! ---------------------------------------------------------------------------
//! 【入口在哪】
//! ---------------------------------------------------------------------------
//! 唯一入口是 `RunnerPort::send` / `send_with_events`（:2060 / :2064），
//! 二者都转调私有 `dispatch`（:549）。`dispatch` 把 `RunnerCommand` 分派到五个处理函数：
//!
//! | RunnerCommand | 处理函数 | 语义 |
//! |---|---|---|
//! | `Start` | `start` :615 | 开一个新 run，组装消息，跑第一个模型步 |
//! | `CapabilityResult` | `on_capability_result` :754 | Broker 回话，**唤醒挂起的 run** |
//! | `Inject` | `enqueue_injected` :433 | 往收件箱塞一条消息 |
//! | `Continue` | `continue_run` :869 | 对已有 run 追加一轮 |
//! | `Cancel` | `cancel` :923 | 取消 |
//!
//! ---------------------------------------------------------------------------
//! 【数据如何流过】
//! ---------------------------------------------------------------------------
//! ```
//!   RunnerCommand::Start
//!         │
//!         ▼
//!   start()  组装 ActiveRun { messages, inbox, driver, ... }
//!         │
//!         ▼
//!   model_step()  ──循环──▶  model_step_once()        ← 「一个模型步」
//!                                │
//!                                │ 1. 取消 / 步数 / 墙钟 / 预算 / 工具目录摘要 检查
//!                                │ 2. 取走收件箱 NextStep 消息 → messages
//!                                │ 3. compact_if_needed()  上下文压缩
//!                                │ 4. invoke_model()  流式调模型，Delta 事件逐条外发
//!                                │
//!                     ┌──────────┴───────────┐
//!                     │ 有工具调用？            │ 无工具调用
//!                     ▼                       ▼
//!        emit_tool_request()  ──▶ 发出         messages 里没有待注入消息？
//!        （不发执行命令！）       CapabilityRequested       有 ─▶ 继续循环
//!        pending_tools.front()        事件，然后【返回】，        无 ─▶ 发出
//!        标记 Dispatched              run 存回 runs 表 =【挂起】   Completed
//!                     │                                          事件
//!        ═══ 控制权交回 ControlPlane / Broker ═══
//!                     │
//!        on_capability_result()  ← Broker 执行完，回灌结果
//!                     │
//!         ├─ 还有下一个 pending_tool？ ──▶ 再 emit_tool_request()，继续串行
//!         └─ 没有？ ──▶ 回到 model_step()，跑下一个模型步
//! ```
//!
//! 【为什么工具调用后要「挂起」而不是直接返回给调用方？】
//! 因为执行权不在本文件。发出 `CapabilityRequested` 之后，本文件就把 run
//! 从 `runs` 表里拿出来（`take_run`），整个状态存进事件流返回给上层。
//! 上层（ControlPlane）把申请单过审、交给 Broker 执行，再把
//! `CapabilityResult` 用新命令送回来。只有这时循环才继续。
//! 这样做的直接好处是：**一次只有一把钥匙在系统里流转**，
//! 不存在「模型一边等审批一边继续往下跑」的可能。
//!
//! 【注意 `model_step_once` 返回 `StepProgress::Finished` 的两种含义】
//! * 「这一轮真的结束了」（失败 / 取消 / Completed）
//! * 「挂起了，等 `CapabilityResult`」
//! 两者共用 `Finished`，因为对 `model_step` 的循环来说，
//! 「不再继续循环」这两者没有区别。区分靠的是**有没有发出
//! `CapabilityRequested` 事件**，以及 run 有没有被 `store_unless_terminal` 存回去。

use crate::budget::{BudgetLedger, HarnessBudgetConfig};
use crate::compact::{
    compact_if_needed, COMPACT_USER_MESSAGE_MAX_TOKENS, DEFAULT_COMPACT_TRIGGER_TOKENS,
};
use crate::inbox::{Inbox, InboxMessage, InboxTarget};
use crate::model::{
    ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelToolCall, ScriptedModel,
    UnavailableModel,
};
use crate::progress::ProgressTracker;
use crate::state_driver::RunDriver;
use crate::stream_normalizer::ModelStreamAccumulator;
use crate::tools::{capability_for_tool_with_request_id, tool_schemas};
use async_trait::async_trait;
use kiana_domain::{
    derived_request_id, json_digest, redact_text, scan_secret_sentinels, AttemptId,
    CapabilityResult, InputId, ModelAttemptId, ModelAttemptIdentity, PromptBundle,
    QuotaReservationId, RequestId, RetryAttemptReservation, RunId, SecretScanChannel, StepId,
    StepIdentity, StreamingRedactor, ToolObservation, ToolObservationStatus, TurnId,
};
use kiana_ports::{PortError, RunnerPort};
use kiana_runner_protocol::{
    RunnerCommand, RunnerEvent, DEFAULT_HARNESS_SANDBOX, HARNESS_SANDBOX_WORKSPACE_WRITE,
};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 本文件在事件与收据里的稳定身份（stable id）。
///
/// 它的作用是让「这份结果/这份进度**是哪个 Agent loop 产生的**」可以被机器判定，
/// 而不是靠调用方口头声称。写进 `Completed` 事件的 `source` 字段和
/// `HarnessCheckpoint.schema` 里的 `kiana.harness-` 前缀都来自这里。
///
/// 值 `"kiana-harness"` 而不是别的：它是**跨版本、跨语言、跨进程**都要保持不变的
/// 标识符，所以不带任何实现细节、版本号或主机信息。
pub const HARNESS_ID: &str = "kiana-harness";
/// 完成的 harness 结果（harness result）所遵循的 JSON 结构版本号。
///
/// 带 `.v1` 后缀是**刻意的版本化约定**：将来结构要改时新增 `v2`，
/// 旧消费方看到 `v1` 仍能正确解析，而不是被静默地按新结构误读。
/// 写进 `RunnerEvent::Completed` 的 `output.schema` 字段。
pub const HARNESS_RESULT_SCHEMA: &str = "kiana.harness-result.v1";
/// 环境变量名：指向一个 cassette（本地录制脚本）文件路径。
///
/// 设置它 = 用 `ScriptedModel` 回放录制好的模型输出，而不是真的调 provider。
/// 用途是**确定性重放**（把一次真实执行录下来，之后每次都跑出同样结果），
/// 用来写测试和做可复现的排障。
const ENV_HARNESS_SCRIPT: &str = "KIANA_HARNESS_SCRIPT";
/// 稳定的结构化错误码：墙钟预算（wall time budget）超了。
///
/// **为什么必须是常量字符串而不是格式化生成的？**
/// 上层按错误码做分支处理（「超时」可以 `continue` 重来，其它失败就是终局）。
/// `store_unless_terminal`（:1684）就靠**精确字符串相等**判断
/// 「这个失败只终止当前 turn，run 仍要留下来给 Continue 用」。
/// 一旦这个值被改成动态拼接，run 就会在超时后被丢弃，
/// 表现为「Continue 之后 run_not_found」——一个极难定位的 bug。
const RUN_BUDGET_EXCEEDED_WALL_TIME: &str = "run_budget_exceeded:wall_time";

/// 运行时可调参数的快照（runtime config）。
///
/// 【为什么是 `Copy`】
/// 它只是一组标量 + 一个 `Option<Duration>`，没有堆分配。
/// `KianaHarness` 存一份、`config()` 又返回一份，全程按值传递，
/// 不需要 clone 出第二个所有权。
///
/// 【为什么 `max_steps_per_turn` 和 `repeated_tool_call_threshold` 是 `u32`】
/// 两者都是「次数」：没有分数、不会为负。用 `u32` 而不是 `usize` 是为了
/// 跨平台宽度一致，并且能直接和事件里的 `step: u32` 字段对齐，
/// 避免跨 crate 传递时的隐式转换。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    /// 单个 turn 内**最多跑多少个模型步**。
    ///
    /// 一个「模型步」（model step）= 调一次模型 + 处理它的输出。
    /// 默认 32（见 `Default`）。32 是个经验值：一个正常的多文件改动任务
    /// 通常用 5～15 步；32 留足了余量，但又不是无限，所以卡死的循环会在这里被掐断。
    /// 还有一个更重要的含义：**它是防挂死的兜底**——
    /// 即便 `repeated_tool_call_threshold` 因为工具参数每次都变而没触发，
    /// 32 步也足够把一个失控循环停下来。
    pub max_steps_per_turn: u32,
    /// **连续**几次发出「同一个工具 + 同一组参数」就判定为空转并拒绝。
    ///
    /// 默认 3。为什么不是 2：合法的工作流里重试完全正常
    /// （文件刚被另一个进程改了就再读一次）；为什么不是 4+：
    /// 真正有用的重试很少超过 2 次，第三次还在做同一件事基本就是死循环了。
    /// 精确语义见 `record_tool_call`（:1768）的注释。
    pub repeated_tool_call_threshold: u32,
    /// 单个 run 的**墙钟预算**：从 `start` 起经过多久就强制停止。
    ///
    /// **为什么是 `Option<Duration>` 而不是 `Duration`**：
    /// `None` = 不设墙钟上限（由别的预算维度兜底，例如模型分配里的
    /// `max_wall_time_ms`）。测试用的 `ScriptedModel` 是本地回放、毫秒级完成，
    /// 挂表对它毫无意义；生产环境则由 daemon 显式设置。
    /// 另外注意 `Duration` 是**相对时长**而不是绝对时间戳——
    /// `Instant` 本身就不可序列化进 checkpoint（见 :1929 那里存的是
    /// `wall_time_elapsed_ms` 而不是 `Instant`），所以这里是「已经过了多久」。
    pub wall_time_budget: Option<Duration>,
    /// 通用预算额度（token 数、工具调用次数、压缩次数等），见 `crate::budget`。
    pub budget: HarnessBudgetConfig,
    /// 消息总 token 数**超过**这个值就触发上下文压缩（compaction）。
    ///
    /// 默认取 `DEFAULT_COMPACT_TRIGGER_TOKENS`（32_000），见 `crate::compact`。
    pub compact_trigger_tokens: usize,
    /// 压缩时，每条用户消息最多保留多少 token。
    ///
    /// 默认取 `COMPACT_USER_MESSAGE_MAX_TOKENS`（20_000），见 `crate::compact`。
    /// 单独设一个值而不是复用触发阈值，是因为「触发压缩」和「压缩到什么粒度」
    /// 是两个独立决策：前者关心总窗口，后者关心单条消息别被砍得太碎。
    pub compact_user_message_max_tokens: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            // 32：见字段注释。留足余量，同时是失控循环的硬上限。
            max_steps_per_turn: 32,
            // 3：允许 2 次正常重试，第 3 次判定为空转。
            repeated_tool_call_threshold: 3,
            // None：默认不设墙钟。调用方（daemon）显式设置才生效。
            wall_time_budget: None,
            budget: HarnessBudgetConfig::default(),
            compact_trigger_tokens: DEFAULT_COMPACT_TRIGGER_TOKENS,
            compact_user_message_max_tokens: COMPACT_USER_MESSAGE_MAX_TOKENS,
        }
    }
}

/// 本文件能产生的两类错误。
///
/// 【为什么只有 Unavailable / Failed 两档，而不是几十个变体】
/// 真正的错误**内容**（哪个工具被拒、哪个字段非法）不放在 enum 变体里，
/// 而是编码进字符串消息（例如 `"invalid_arguments:shell:cmd"`）。
/// 理由有两点：
/// 1. 错误码要能一路透传到事件流、CLI 和 UI，用 `enum` 反而会被
///    `#[derive(Error)]` 的格式化层拦下来做额外转换。
/// 2. 变体数量一多就必然漏 `match` 分支。分两档之后，
///    **调用方只需区分「可重试的环境问题」和「已决定的失败」**，
///    具体含义从消息里读。
///
/// 两档的语义边界（对应 `kiana_ports::PortError`）：
/// * `Unavailable` = 环境不支持 / 还没就绪（可以稍后重试）
/// * `Failed` = 本次操作已经确定失败（重试同一个输入也没用）
#[derive(Debug, thiserror::Error)]
pub enum KianaHarnessError {
    /// 运行时不可用。消息会原样拼成 `kiana_harness_unavailable:<消息>`。
    #[error("kiana_harness_unavailable:{0}")]
    Unavailable(String),
    /// 本次操作失败。消息会原样拼成 `kiana_harness_failed:<消息>`。
    #[error("kiana_harness_failed:{0}")]
    Failed(String),
}

/// 事件收集器：把一次命令处理期间产生的 `RunnerEvent` **同时**送到两个地方。
///
/// 【为什么需要两个去处】
/// 1. `events`（`Vec`）：最终作为 `send` 的返回值交给 ControlPlane，
///    用来做投影和落盘。
/// 2. `sink`（`FnMut`）：**流式**回调，让上层在事件产生的那一刻就拿到它
///    （比如推到 SSE 推给 UI）。没有 sink 的话，用户要等整个 run 跑完
///    才看得到第一个字。
///
/// 【为什么把 sink 放在结构体里而不是单独传参数】
/// 因为 `emit` 需要同时写这两个去处，而几乎所有处理函数都要发事件。
/// 绑在 `&mut self` 上就不用在每个函数签名里拖着一个 `&mut dyn FnMut`。
struct EventEmitter<'a> {
    events: Vec<RunnerEvent>,
    sink: Option<&'a mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send)>,
}

impl<'a> EventEmitter<'a> {
    fn new(sink: Option<&'a mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send)>) -> Self {
        Self {
            events: Vec::new(),
            sink,
        }
    }

    /// 发一条事件：先交给 sink（流式推送），再收进 `events`。
    ///
    /// 【顺序为什么是 sink 在前】
    /// sink 返回 `Err` 表示**下游取消了**或背压了。这时候必须立刻停，
    /// 不能再往 `events` 里堆——那些事件永远不会被交付出去了，
    /// 收下来只会造成「看起来发生过」的假象。
    ///
    /// 【为什么失败码是 `runner_event_sink_failed:<原文>`】
    /// 前缀让上层一眼分清「这是下游推送失败」和「这是执行失败」，
    /// 两者的处置完全不同：前者是基础设施问题，后者是任务结果。
    fn emit(&mut self, event: RunnerEvent) -> Result<(), String> {
        if let Some(sink) = self.sink.as_mut() {
            sink(event.clone()).map_err(|error| format!("runner_event_sink_failed:{error}"))?;
        }
        self.events.push(event);
        Ok(())
    }

    /// `emit` 的便捷包装：把 sink 的 `String` 错误转成本文件的错误类型。
    ///
    /// 注意这里**只转换错误类型，不改错误码**——`runner_event_sink_failed:` 前缀
    /// 依然保留在消息里，信息没有丢。
    fn emit_event(&mut self, event: RunnerEvent) -> Result<(), KianaHarnessError> {
        self.emit(event).map_err(KianaHarnessError::Failed)
    }

    /// **回滚**到 `checkpoint` 下标，再发新事件。用于「撤销已发的不该发的消息」。
    ///
    /// 【什么时候需要回滚】
    /// 流式 Delta 是在**还不知道模型这次会不会成功**的情况下就发出去的。
    /// 如果这次模型步后来发现工具参数非法、或者工具名不存在，
    /// 前面那几段 Delta 就不该留在事件流里——它们描述的是一个
    /// 最终会被拒绝的模型步的输出。回滚让「已交付的事件」和
    /// 「最终成立的结论」保持一致。
    ///
    /// 【为什么有 sink 时不回滚】
    /// sink 已经把事件推给上游了，撤回不了。
    /// 这种情况下改为**追加一条 `Failed` 事件**（见 `model_step_once` 里的
    /// `replace_event_since` 调用点），让下游自己能判断
    /// 「之前那批 Delta 属于一个失败的步」。
    /// 宁可让下游多处理一条消息，也不谎称消息没发过。
    fn replace_since(&mut self, checkpoint: usize, event: RunnerEvent) -> Result<(), String> {
        if self.sink.is_none() {
            self.events.truncate(checkpoint);
        }
        self.emit(event)
    }

    /// `replace_since` 的便捷包装，错误类型转换规则同 `emit_event`。
    fn replace_event_since(
        &mut self,
        checkpoint: usize,
        event: RunnerEvent,
    ) -> Result<(), KianaHarnessError> {
        self.replace_since(checkpoint, event)
            .map_err(KianaHarnessError::Failed)
    }

    /// 交出已收集的全部事件，结束 emitter 的生命周期。
    fn into_events(self) -> Vec<RunnerEvent> {
        self.events
    }
}

/// 一个「正在跑或者挂起着的 run」的**全部内存状态**。
///
/// 【架构角色：这是 harness 的「寄存器堆」】
/// Kiana 的 run 是**可挂起**的：跑到一半要等 Broker 回话时，
/// 整个 `ActiveRun` 会从 `KianaHarness.runs` 表里被取出来，
/// 序列化进事件流交给上层；上层把结果送回来时再整个装回去。
/// 所以这个结构体必须**完整到足以自洽地恢复一个 run**——
/// 少一个字段，恢复出来的 run 就和崩溃前不是同一个 run。
///
/// 【为什么用 `Mutex<HashMap<RunId, ActiveRun>>` 而不是每 run 一个 task】
/// 因为 run 的生命周期**跨命令**。`Start` 命令结束时它可能还没跑完
/// （挂起等能力结果），此时没有 task 持有它了。
/// 用表来存，才能在下一条 `CapabilityResult` 命令里找回来。
/// 代价是**单个 run 的状态不能被两个命令同时改**——
/// `take_run`（:1625）就是「把整个 run 原子地从表里摘走」的动作，
/// 第二个命令再来就会拿到 `run_not_found`。
struct ActiveRun {
    /// 这个 run 的全局唯一 id。跨进程、跨重启稳定。
    run_id: RunId,
    /// 当前 turn 的 id。`Option` = 还没有 turn。
    ///
    /// **为什么可能为 `None`**：`RunnerCommand::Start` 的 `turn_id` 字段本身
    /// 就是 `Option` —— 老协议允许不带 turn 启动。带上之后，
    /// 所有派生 id（`model.call`、`harness.invocation`）都带 turn 维度，
    /// 不同 turn 的同名调用不会撞 id。
    turn_id: Option<TurnId>,
    /// 当前 step 的 id。`Option` = 还没开始任何 step。
    ///
    /// 由 `model_step_once` 在每一步开始时新建（:1064），
    /// 之后所有工具请求都带着它。
    step_id: Option<StepId>,
    /// 状态机驱动器（`crate::state_driver::RunDriver`），
    /// 负责「这一步到底走到哪了」的合法转换检查。
    ///
    /// 【为什么需要它】harness 的所有对外操作
    /// （`begin_turn` / `begin_step` / `model_output` / `tool_result` / `release`）
    /// 都要先跟它说话。非法顺序（例如没 `begin_step` 就发工具请求）
    /// 会在这一步就被挡住，而不是留到事件流里才暴露。
    driver: RunDriver,
    /// 进度记录器（`crate::progress::ProgressTracker`）：
    /// 记「这个 run 干过哪些外部观测」，用于向用户展示已完成的工作。
    progress_tracker: ProgressTracker,
    /// The server-owned question remains attached to the same run across checkpoint/restore.
    /// 待澄清的问题。`Option` = 当前没有。
    ///
    /// 【为什么它能跨 checkpoint/restore 存活】
    /// 因为它在 `HarnessCheckpoint`（:191）里，而且**权限属于服务端**：
    /// 模型既不能读它也不能改它，只能等上层把答案通过 inject 送进来。
    pending_clarification: Option<kiana_domain::ClarificationRequest>,
    /// 沙箱档位，只可能是 `"read-only"` 或 `"workspace-write"`。
    ///
    /// 经过 `normalize_sandbox`（:2109）归一化，未知值直接拒绝，
    /// 所以这里没有「脏值」。它**只是参数上下文，不是授权凭证**——
    /// ControlPlane 和 Broker 会独立再验一次。
    sandbox: String,
    /// 项目根目录。**同样只是参数上下文**，
    /// 会被抄进 `shell` 工具的 `CapabilityRequest` 里（见 :1248），
    /// 不构成对任何路径的访问授权。
    project_root: String,
    /// 收件箱（inbox，暂停期间到达的注入消息），见 `crate::inbox`。
    inbox: Inbox,
    /// 累积的消息历史，**下一次调模型时整体作为 `messages` 传出去**。
    ///
    /// 这是 run 的「思考草稿纸」：包含 system、用户、助手、工具观测。
    /// 可能被 `compact_if_needed` 折叠变短，所以类型是 `Vec` 而不是只增的追加日志。
    messages: Vec<ModelMessage>,
    /// prompt 的来源出处（provenance），来自 `PromptBundle`。
    /// 原样写进 `ModelTurn` 事件，用于事后追查「这段上下文是谁给的」。
    prompt_sources: Vec<serde_json::Value>,
    /// 控制面下发的模型分配（model assignment）：角色、配额、模型路由权限。
    ///
    /// **由 `bind_model_assignment`（:1813）在 run 启动前单独写入**，
    /// 模型对它没有写入路径——模型文本永远无法给自己提权。
    /// `start` 时用 `remove` 把它从待绑定表里取走并搬进本字段。
    model_assignment: Option<kiana_domain::ModelAssignment>,
    /// 第一次调模型时由 provider 解析出来的实际路由（route）。
    ///
    /// 【为什么第一次之后要钉住】
    /// 一个 run 生命周期内模型路由不能变。
    /// 如果中途 provider 把请求路由到了别的模型，同一个 run 的前后两段输出
    /// 就来自不同模型，却顶着同一个 `run_id` 混在一起——
    /// 事后根本无法解释结果是怎么来的。所以这里不一致就报
    /// `model_route_changed_during_run`（:1403）。
    model_route: Option<kiana_domain::ModelRoute>,
    /// 本次模型步要执行的工具队列（先进先出）。
    ///
    /// **一个模型步里的多个工具在这里排队，但只发一个**：
    /// `emit_tool_request` 永远只看 `front()`，
    /// 要等前一个的 `CapabilityResult` 回来（`on_capability_result`）
    /// 才发下一个。测试
    /// `serial_tools_in_one_model_step_request_one_capability_at_a_time`（:2949）
    /// 锁定了这条规格。
    pending_tools: VecDeque<PendingTool>,
    /// 上一条工具调用的指纹（工具名 + 规范化参数）和连续次数，
    /// 用于空转检测。`None` = 本轮还没调过工具。见 `record_tool_call`。
    last_tool_call: Option<RepeatedToolCall>,
    /// 本 turn 已经跑过的模型步数。`model_step_once` 每次进来 `+= 1`。
    steps: u32,
    /// 本 run 的步数上限 = `Start` 命令带来的值与 harness 全局值的较小者。
    ///
    /// 【为什么取 min 而不是覆盖】
    /// 一次具体请求可以要求更严（`max_steps_per_turn` 调小），
    /// 但**不能要求更松** —— 否则调用方就能绕过 harness 的全局保护。
    max_steps_per_turn: u32,
    /// run 启动那一刻的**工具目录摘要**。
    ///
    /// 每次模型步都和当前摘要比对（:1045），不一致就报
    /// `tool_catalog_changed` 并停止：
    /// 同一轮对话里工具集忽然变了，模型之前学到的用法可能已经不成立，
    /// 继续跑下去只会产生无法解释的结果。
    tool_catalog_digest: String,
    /// 墙钟计时的起点。用 `Instant`（单调时钟）而不是 `SystemTime`，
    /// 因为后者会被 NTP 校时和手动改表影响，可能算出负的耗时。
    wall_time_started_at: Instant,
    /// 本次模型步最后一段文本（已脱敏），用于填 `Completed` 事件的 `text` 字段。
    last_text: String,
    /// 取消令牌。用 `Arc` 是因为 cancel 命令和正在跑的模型步
    /// 要能各自拿到同一份（`cancel` :923 走 `in_flight` 表拿的就是它的 clone）。
    cancellation: Arc<RunCancellation>,
}

/// 一个待处理工具调用的阶段。
///
/// 【为什么是三态而不是布尔 `in_flight`】
/// 区分「还没发出去」和「发出去了但没回话」至关重要：
/// 前者可以安全地取消（证明没执行过），
/// 后者**不能**假设没执行 —— 见 `cancel`（:947）那里对队首的特殊处理。
/// 用 `bool` 会把这两者压成同一个值，丢掉「能否证明未执行」这条信息。
///
/// 顺带 `#[serde(rename_all = "snake_case")]` 是为了让 checkpoint 的 JSON
/// 可读且稳定：字段序列化成 `"queued"` / `"dispatched"` / `"settled"`。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum PendingToolPhase {
    /// 已进队列，`CapabilityRequest` 还没发出。可以证明**尚未执行**。
    Queued,
    /// `CapabilityRequested` 事件已发出，等 Broker 回话。
    /// 此时**无法证明有没有执行**，取消它只能等回执。
    Dispatched,
    /// 结果已回填给模型。这个状态不该出现在待处理队列里，
    /// `restore`（:2005）会显式拒绝它。
    Settled,
}

/// 一个待处理的工具调用。
///
/// 【为什么 `deny_unknown_fields`】
/// checkpoint 是要被反序列化的持久化数据。加这个属性后，
/// 任何拼错的字段名都会**立刻报错**而不是被静默忽略。
/// 对状态恢复来说，静默丢字段 = 恢复出一个残缺的 run，比直接失败危险得多。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingTool {
    /// 能力请求 id。**必须稳定**：审批、发出、恢复三处用的是同一个值，
    /// 否则每次都造新 invocation，审批就失效了。见 `stable_invocation_request_id`。
    request_id: kiana_domain::RequestId,
    /// 模型的原始工具调用。
    call: ModelToolCall,
    /// 当前阶段。
    phase: PendingToolPhase,
}

/// 一次 `model_step_once` 之后循环要不要继续。
///
/// 借用标准库 `ControlFlow` 的形状，但独立定义是为了不额外引依赖，
/// 并且语义更明确：`Finished` 覆盖了「真结束」和「挂起等回话」两种情况（见文件头）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepProgress {
    /// 还有下一步（收到新注入消息等），继续循环。
    Continue,
    /// 停止循环。真正结束，或者是挂起等 `CapabilityResult`。
    Finished,
}

/// 「连续重复调用同一工具」的指纹和计数。
///
/// 需要 `Serialize + Deserialize` 是因为它必须能进 checkpoint：
/// 崩溃重启后要接着数，否则「已经空转 2 次」这个事实会丢。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RepeatedToolCall {
    /// 工具名（如 `shell`）。
    name: String,
    /// 规范化后的参数字符串（键已排序），见 `canonical_json`。
    /// 比较必须基于它而不是原始 JSON，否则键序不同会被误判成两次不同调用。
    canonical_arguments: String,
    /// 连续相同调用的次数。第 1 次调用后为 1。
    count: u32,
}

/// 一个 run 的**完整快照**，用于崩溃/重启后接着跑。
///
/// 【为什么 Agent loop 需要快照能力】
/// run 是可挂起的：它会等 Broker 审批、等用户回答澄清问题。
/// 在这个等待窗口里进程可能崩溃、被杀、被机器重启。
/// 没有快照的话，这个 run 就永远停在原地——已经发生的对话、
/// 已排队的工具、已用的步数全部丢失，只能从头重来。
///
/// 【`deny_unknown_fields` 的理由同上】
/// 快照来自**不可信的持久化存储**（事件日志重放出来的东西），
/// 宁可因为多一个字段就明确报错，也不要悄悄按不完整的理解恢复。
///
/// 【`#[serde(default)]` 的取舍】
/// 凡是标了 `default` 的字段都是**后加的**。给老快照留后路，
/// 让新版本能读旧版本写的数据。这是显式的向前兼容，
/// 不是通用兜底 —— 没标 `default` 的字段（如 `messages`、`sandbox`）
/// 缺失即报错。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessCheckpoint {
    /// 快照结构版本。写死 `"kiana.harness-checkpoint.v1"`，由 `restore`（:1942）校验。
    schema: String,
    /// 这个快照属于哪个 run。`restore` 会核对它和参数 `run_id` 一致。
    run_id: RunId,
    #[serde(default)]
    turn_id: Option<TurnId>,
    #[serde(default)]
    step_id: Option<StepId>,
    /// 状态机状态。`None` 时 `restore` 会造一个新的 `RunDriver`。
    #[serde(default)]
    driver: Option<RunDriver>,
    #[serde(default)]
    progress_tracker: Option<ProgressTracker>,
    #[serde(default)]
    pending_clarification: Option<kiana_domain::ClarificationRequest>,
    /// 沙箱档位。`restore` 会重新过一遍 `normalize_sandbox` —— 绝不信任快照里的值。
    sandbox: String,
    /// 项目根目录。同样**只是参数上下文**，restore 时不重新校验其真实性。
    project_root: String,
    #[serde(default)]
    inbox: Inbox,
    /// 完整消息历史。这是最占体积的字段，也是压缩后重新可能变长的原因。
    messages: Vec<ModelMessage>,
    #[serde(default)]
    prompt_sources: Vec<serde_json::Value>,
    #[serde(default)]
    model_assignment: Option<kiana_domain::ModelAssignment>,
    #[serde(default)]
    model_route: Option<kiana_domain::ModelRoute>,
    pending_tools: VecDeque<PendingTool>,
    last_tool_call: Option<RepeatedToolCall>,
    steps: u32,
    max_steps_per_turn: u32,
    #[serde(default)]
    tool_catalog_digest: String,
    /// 快照时刻**已经过去的墙钟毫秒数**，而不是 `Instant` 本身。
    ///
    /// 【为什么不能存 `Instant`】
    /// `Instant` 是进程内的单调时钟，跨进程没有意义，也序列化不出来。
    /// 只能存「已用掉多少」这个相对量；`restore` 再用
    /// `Instant::now().checked_sub(elapsed)` 把它还原成一个「等价的起点」（:2028）。
    /// 这样重启后墙钟预算**不会因为停机时间而白送**——
    /// 停了 1 小时，预算还是只扣了停机前真正用掉的那部分。
    wall_time_elapsed_ms: u64,
    last_text: String,
}

struct RunCancellation {
    error: Mutex<Option<String>>,
    changed: tokio::sync::Notify,
    signal: tokio::sync::watch::Sender<bool>,
}

impl Default for RunCancellation {
    fn default() -> Self {
        let (signal, _receiver) = tokio::sync::watch::channel(false);
        Self {
            error: Mutex::new(None),
            changed: tokio::sync::Notify::new(),
            signal,
        }
    }
}

impl RunCancellation {
    async fn cancelled(&self) -> String {
        loop {
            if let Ok(Some(error)) = self.error() {
                return error;
            }
            self.changed.notified().await;
        }
    }
    fn request(&self, error: String) -> Result<bool, KianaHarnessError> {
        let mut state = self
            .error
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        if state.is_some() {
            return Ok(false);
        }
        *state = Some(error);
        let _ = self.signal.send(true);
        self.changed.notify_one();
        Ok(true)
    }

    fn error(&self) -> Result<Option<String>, KianaHarnessError> {
        self.error
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))
            .map(|state| state.clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<String>>, KianaHarnessError> {
        self.error
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))
    }

    fn subscribe(&self) -> tokio::sync::watch::Receiver<bool> {
        self.signal.subscribe()
    }
}

struct StartInput {
    run_id: RunId,
    turn_id: Option<TurnId>,
    prompt: String,
    history: Vec<kiana_domain::ConversationMessage>,
    sandbox: String,
    project_root: String,
    instructions: String,
    max_steps_per_turn: u32,
}

pub struct KianaHarness {
    model: Arc<dyn ModelClient>,
    model_budget: Mutex<Option<Arc<dyn kiana_ports::ModelBudgetPort>>>,
    budget_ledger: BudgetLedger,
    budget_config: HarnessBudgetConfig,
    budget_config_error: Option<String>,
    runs: Mutex<HashMap<RunId, ActiveRun>>,
    assignments: Mutex<HashMap<RunId, kiana_domain::ModelAssignment>>,
    histories: Mutex<HashMap<RunId, Vec<ModelMessage>>>,
    in_flight: Mutex<HashMap<RunId, Arc<RunCancellation>>>,
    deferred_inputs: Mutex<HashMap<RunId, Vec<(InboxTarget, InboxMessage)>>>,
    compact_trigger_tokens: usize,
    compact_user_message_max_tokens: usize,
    max_steps_per_turn: u32,
    repeated_tool_call_threshold: u32,
    wall_time_budget: Option<Duration>,
}

impl Default for KianaHarness {
    fn default() -> Self {
        Self::from_env()
    }
}

impl KianaHarness {
    pub fn new(model: Arc<dyn ModelClient>) -> Self {
        Self::with_config(model, RuntimeConfig::default())
    }

    pub fn with_config(model: Arc<dyn ModelClient>, config: RuntimeConfig) -> Self {
        let budget_config = config.budget;
        let budget_config_error = budget_config.validate().err();
        Self {
            model,
            model_budget: Mutex::new(None),
            budget_ledger: BudgetLedger::default(),
            budget_config,
            budget_config_error,
            runs: Mutex::new(HashMap::new()),
            assignments: Mutex::new(HashMap::new()),
            histories: Mutex::new(HashMap::new()),
            in_flight: Mutex::new(HashMap::new()),
            deferred_inputs: Mutex::new(HashMap::new()),
            compact_trigger_tokens: config.compact_trigger_tokens,
            compact_user_message_max_tokens: config.compact_user_message_max_tokens,
            max_steps_per_turn: config.max_steps_per_turn.max(1),
            repeated_tool_call_threshold: config.repeated_tool_call_threshold.max(1),
            wall_time_budget: config.wall_time_budget,
        }
    }

    pub fn config(&self) -> RuntimeConfig {
        RuntimeConfig {
            max_steps_per_turn: self.max_steps_per_turn,
            repeated_tool_call_threshold: self.repeated_tool_call_threshold,
            compact_trigger_tokens: self.compact_trigger_tokens,
            compact_user_message_max_tokens: self.compact_user_message_max_tokens,
            wall_time_budget: self.wall_time_budget,
            budget: self.budget_config,
        }
    }

    fn effective_budget(&self, run: &ActiveRun) -> Result<HarnessBudgetConfig, String> {
        if let Some(error) = &self.budget_config_error {
            return Err(format!("runtime_config_invalid:budget:{error}"));
        }
        let mut budget = self.budget_config;
        budget.max_model_steps_per_turn =
            budget.max_model_steps_per_turn.min(run.max_steps_per_turn);
        if let Some(authority) = run
            .model_assignment
            .as_ref()
            .and_then(|assignment| assignment.runtime_budget.as_ref())
        {
            authority
                .validate()
                .map_err(|error| format!("effective_budget_invalid:{error}"))?;
            budget.max_model_steps_per_turn = budget
                .max_model_steps_per_turn
                .min(authority.max_model_calls.min(u64::from(u32::MAX)) as u32);
            budget.max_tokens_per_task = budget.max_tokens_per_task.min(authority.max_tokens);
            let authority_wall_time = Duration::from_millis(authority.max_wall_time_ms);
            budget.max_wall_time_per_task = Some(
                budget
                    .max_wall_time_per_task
                    .map_or(authority_wall_time, |configured| {
                        configured.min(authority_wall_time)
                    }),
            );
        }
        budget
            .validate()
            .map_err(|error| format!("effective_budget_invalid:{error}"))?;
        Ok(budget)
    }

    fn budget_scope(run: &ActiveRun) -> String {
        let role = run
            .model_assignment
            .as_ref()
            .map(|assignment| assignment.role_id.as_str())
            .unwrap_or("unassigned");
        format!("project:{}:role:{role}", run.project_root)
    }

    pub fn with_compact_budget(mut self, trigger_tokens: usize, retain_tokens: usize) -> Self {
        self.compact_trigger_tokens = trigger_tokens;
        self.compact_user_message_max_tokens = retain_tokens;
        self
    }

    pub fn with_max_steps(mut self, max_steps_per_turn: u32) -> Self {
        self.max_steps_per_turn = max_steps_per_turn.max(1);
        self
    }

    pub fn with_repeated_tool_call_threshold(mut self, threshold: u32) -> Self {
        self.repeated_tool_call_threshold = threshold.max(1);
        self
    }

    pub fn with_wall_time_budget(mut self, wall_time_budget: impl Into<Option<Duration>>) -> Self {
        self.wall_time_budget = wall_time_budget.into();
        self
    }

    pub fn from_env() -> Self {
        let model: Arc<dyn ModelClient> = match std::env::var(ENV_HARNESS_SCRIPT) {
            Ok(path) if !path.trim().is_empty() => match ScriptedModel::from_json_path(path.trim())
            {
                Ok(model) => Arc::new(model),
                Err(error) => Arc::new(UnavailableModel::new(error)),
            },
            _ => Arc::new(UnavailableModel::default()),
        };
        Self::new(model)
    }

    /// DeepSeek `inject`: stage steering text for the next step boundary.
    /// The run must already be stored (typically paused on a capability).
    pub fn inject(&self, run_id: RunId, text: impl Into<String>) -> Result<(), KianaHarnessError> {
        self.insert_next_step(run_id, text)
    }

    /// DeepSeek `steer`: same inbox target as inject. Wakeup is the next
    /// `model_step` after the in-flight capability returns; we do not expand
    /// the runner protocol with a concurrent wake command.
    pub fn steer(&self, run_id: RunId, text: impl Into<String>) -> Result<(), KianaHarnessError> {
        self.insert_next_step(run_id, text)
    }

    fn enqueue_injected(
        &self,
        run_id: RunId,
        input_id: InputId,
        source: String,
        target: InboxTarget,
        target_turn_id: Option<TurnId>,
        text: String,
    ) -> Result<(), KianaHarnessError> {
        let mut message = InboxMessage::from_source(source, text);
        message.input_id = input_id;
        message.target_turn_id = target_turn_id;
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        if let Some(run) = runs.get_mut(&run_id) {
            run.driver
                .queue_input()
                .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
            run.inbox
                .insert_for_run(run_id, target, message)
                .map_err(KianaHarnessError::Failed)?;
            return Ok(());
        }
        drop(runs);
        if !self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .contains_key(&run_id)
        {
            return Err(KianaHarnessError::Failed("run_not_found".to_owned()));
        }
        self.deferred_inputs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .entry(run_id)
            .or_default()
            .push((target, message));
        Ok(())
    }

    fn insert_next_step(
        &self,
        run_id: RunId,
        text: impl Into<String>,
    ) -> Result<(), KianaHarnessError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(KianaHarnessError::Failed(
                "inbox_message_required".to_owned(),
            ));
        }
        let runs = self
            .runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        let current_turn = runs.get(&run_id).and_then(|run| run.turn_id);
        if runs.get(&run_id).is_none()
            && !self
                .in_flight
                .lock()
                .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
                .contains_key(&run_id)
        {
            return Err(KianaHarnessError::Failed("run_not_found".to_owned()));
        }
        drop(runs);
        if current_turn.is_none() {
            return self.enqueue_injected(
                run_id,
                InputId::new(),
                "runner".to_owned(),
                InboxTarget::NextStep,
                None,
                text,
            );
        }
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        let run = runs
            .get_mut(&run_id)
            .ok_or_else(|| KianaHarnessError::Failed("run_not_found".to_owned()))?;
        run.driver
            .queue_input()
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        let mut message = InboxMessage::user(text);
        message.target_turn_id = run.turn_id;
        run.inbox
            .insert_for_run(run_id, InboxTarget::NextStep, message)
            .map_err(KianaHarnessError::Failed)?;
        Ok(())
    }

    pub async fn send(
        &self,
        command: RunnerCommand,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        self.dispatch(command, None).await
    }

    /// 发送命令，并在每个协议事件产生时同步交付给 `sink`。
    ///
    /// 成功返回时，`sink` 收到的序列与返回值逐项一致。`sink` 返回错误表示下游取消或背压，
    /// harness 会立即停止并返回 `runner_event_sink_failed:*`，不会继续产生后续事件。
    pub async fn send_with_events(
        &self,
        command: RunnerCommand,
        sink: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        self.dispatch(command, Some(sink)).await
    }

    async fn dispatch(
        &self,
        command: RunnerCommand,
        sink: Option<&mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send)>,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        let mut emitter = EventEmitter::new(sink);
        match command {
            RunnerCommand::Start {
                run_id,
                turn_id,
                prompt,
                history,
                project_root,
                sandbox,
                instructions,
                project_trusted: _,
                max_steps_per_turn,
            } => {
                self.start(
                    StartInput {
                        run_id,
                        turn_id,
                        prompt,
                        history,
                        sandbox,
                        project_root,
                        instructions,
                        max_steps_per_turn,
                    },
                    &mut emitter,
                )
                .await
            }
            RunnerCommand::CapabilityResult { run_id, result } => {
                self.on_capability_result(run_id, result, &mut emitter)
                    .await
            }
            RunnerCommand::Inject {
                run_id,
                input_id,
                source,
                target,
                target_turn_id,
                text,
            } => {
                let target = match target.as_str() {
                    "next-turn" | "next_turn" => InboxTarget::NextTurn,
                    "next-step" | "next_step" => InboxTarget::NextStep,
                    _ => {
                        emitter.emit_event(RunnerEvent::Failed {
                            run_id,
                            error: "inbox_target_invalid".to_owned(),
                        })?;
                        return Ok(emitter.into_events());
                    }
                };
                self.enqueue_injected(run_id, input_id, source, target, target_turn_id, text)
            }
            RunnerCommand::Continue { run_id, prompt } => {
                self.continue_run(run_id, prompt, &mut emitter).await
            }
            RunnerCommand::Cancel { run_id, reason } => self.cancel(run_id, reason, &mut emitter),
        }?;
        Ok(emitter.into_events())
    }

    async fn start(
        &self,
        input: StartInput,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let wall_time_started_at = Instant::now();
        let StartInput {
            run_id,
            turn_id,
            prompt,
            history,
            sandbox,
            project_root,
            instructions,
            max_steps_per_turn,
        } = input;
        let prompt = safe_channel_text(SecretScanChannel::Prompt, &prompt);
        if max_steps_per_turn == 0 {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "runtime_config_invalid:max_steps_per_turn".to_owned(),
            });
        }
        if self.has_run(run_id)? || self.has_in_flight(run_id)? {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "run_already_exists".to_owned(),
            });
        }
        let sandbox = normalize_sandbox(&sandbox)?;
        let tool_catalog = kiana_domain::current_tool_catalog();
        tool_catalog.validate().map_err(KianaHarnessError::Failed)?;
        let cancellation = Arc::new(RunCancellation::default());
        let mut run = ActiveRun {
            run_id,
            turn_id,
            step_id: None,
            driver: RunDriver::new(run_id, turn_id),
            progress_tracker: ProgressTracker::default(),
            pending_clarification: None,
            sandbox: sandbox.to_owned(),
            project_root,
            inbox: Inbox::default(),
            messages: Vec::new(),
            prompt_sources: Vec::new(),
            model_route: None,
            model_assignment: self
                .assignments
                .lock()
                .map_err(|_| {
                    KianaHarnessError::Failed("harness_assignment_lock_poisoned".to_owned())
                })?
                .remove(&run_id),
            pending_tools: VecDeque::new(),
            last_tool_call: None,
            steps: 0,
            max_steps_per_turn: max_steps_per_turn.min(self.max_steps_per_turn),
            tool_catalog_digest: tool_catalog.digest,
            wall_time_started_at,
            last_text: String::new(),
            cancellation: cancellation.clone(),
        };
        run.driver
            .begin_turn()
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        if let (Some(turn_id), Some(assignment)) = (run.turn_id, run.model_assignment.as_ref()) {
            if assignment.turn_id != turn_id {
                return emitter.emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "model_assignment_turn_mismatch".to_owned(),
                });
            }
        }
        if !instructions.trim().is_empty() {
            if instructions.trim_start().starts_with('{') {
                let bundle =
                    PromptBundle::decode(&instructions).map_err(KianaHarnessError::Failed)?;
                run.prompt_sources = bundle.provenance();
                let system = bundle.system_prompt();
                if !system.is_empty() {
                    run.messages
                        .push(ModelMessage::system(bundle.encode().map_err(|error| {
                            KianaHarnessError::Failed(format!("prompt_bundle_invalid:{error}"))
                        })?));
                }
                let context = bundle.context_prompt();
                if !context.is_empty() {
                    run.messages.push(ModelMessage::user(context));
                }
            } else {
                // Legacy in-process instructions carry no role or resource authority.
                run.messages
                    .push(ModelMessage::system(kiana_domain::PRODUCT_SYSTEM_PROMPT));
                run.messages.push(ModelMessage::user(instructions));
            }
        }
        let exact = self
            .histories
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_history_lock_poisoned".to_owned()))?
            .remove(&run_id);
        if let Some(exact) = exact {
            run.messages.extend(exact);
        } else {
            // Wire v1 has no assistant tool declarations. Preserve supplied observations as
            // untrusted context; never invent protocol call identities for these legacy records.
            run.messages
                .extend(history.into_iter().map(|message| match message.role {
                    kiana_domain::ConversationRole::User => ModelMessage::user(message.text),
                    kiana_domain::ConversationRole::Assistant => {
                        ModelMessage::assistant(message.text)
                    }
                    kiana_domain::ConversationRole::Tool => ModelMessage::user(format!(
                        "[Legacy tool observation; no replay authority]\n{}",
                        message.text
                    )),
                }));
        }
        run.driver
            .queue_input()
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        run.inbox
            .insert_for_run(run_id, InboxTarget::NextTurn, InboxMessage::user(prompt))
            .map_err(KianaHarnessError::Failed)?;
        for message in run.inbox.claim(InboxTarget::NextTurn) {
            run.messages.push(ModelMessage::user(message.text));
        }

        self.register_in_flight(run_id, cancellation)?;
        let result = match emitter.emit_event(RunnerEvent::Started { run_id }) {
            Ok(()) => self.model_step(&mut run, emitter).await,
            Err(error) => Err(error),
        };
        self.unregister_in_flight(run_id)?;
        result?;
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    async fn on_capability_result(
        &self,
        run_id: RunId,
        result: CapabilityResult,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let mut run = self.take_run(run_id)?;
        let Some(expected) = run.pending_tools.front() else {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "unexpected_capability_result".to_owned(),
            });
        };
        let expected_id = expected.request_id;
        if expected.phase != PendingToolPhase::Dispatched {
            self.store_unless_terminal(run, &[])?;
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "capability_result_before_dispatch".to_owned(),
            });
        }
        if expected_id != result.request_id {
            self.store_unless_terminal(run, &[])?;
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "capability_result_mismatch".to_owned(),
            });
        }
        let mut pending = run
            .pending_tools
            .pop_front()
            .expect("pending capability was checked above");
        pending.phase = PendingToolPhase::Settled;
        let call = pending.call;
        let effect_known =
            result.dimensions().effect != kiana_domain::CapabilityEffectState::Unknown;
        run.driver
            .tool_result(effect_known)
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        if !effect_known {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "result_unknown:tool_effect_unknown".to_owned(),
            });
        }

        let observation = ToolObservation::from_result(call.id.clone(), &result)
            .map_err(KianaHarnessError::Failed)?;
        match observation.status {
            ToolObservationStatus::Denied => {
                emitter.emit_event(RunnerEvent::Failed {
                    run_id: run.run_id,
                    error: format!(
                        "tool_denied_no_retry:{}",
                        observation
                            .error_code
                            .map(|code| code.as_str())
                            .unwrap_or("denied")
                    ),
                })?;
                return Ok(());
            }
            ToolObservationStatus::CancelledNotStarted => {
                emitter.emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "tool_cancelled_not_started".to_owned(),
                })?;
                return Ok(());
            }
            ToolObservationStatus::Unknown => {
                emitter.emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "result_unknown:tool_observation_unknown".to_owned(),
                })?;
                return Ok(());
            }
            ToolObservationStatus::Succeeded
            | ToolObservationStatus::FailedKnown
            | ToolObservationStatus::Pending => {}
        }

        run.messages.push(ModelMessage::tool(
            call.id,
            observation
                .model_text()
                .map_err(KianaHarnessError::Failed)?,
        ));

        if let Some(next_call) = run
            .pending_tools
            .front()
            .map(|pending| pending.call.clone())
        {
            run.driver
                .model_output(run.pending_tools.len() as u32, false)
                .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
            let cancellation = run.cancellation.clone();
            let state = cancellation.lock()?;
            if let Some(error) = state.as_ref() {
                let error = error.clone();
                drop(state);
                emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                return Ok(());
            }
            match self.emit_tool_request(&mut run, &next_call) {
                Ok(event) => emitter.emit_event(event)?,
                Err(error) => emitter.emit_event(RunnerEvent::Failed { run_id, error })?,
            }
        } else {
            self.model_step_in_flight(&mut run, emitter).await?;
        }
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    async fn continue_run(
        &self,
        run_id: RunId,
        prompt: String,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let prompt = safe_channel_text(SecretScanChannel::Prompt, &prompt);
        if prompt.trim().is_empty() {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "prompt_required".to_owned(),
            });
        }
        let mut run = match self.take_run(run_id) {
            Ok(run) => run,
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => {
                return emitter.emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "run_not_found".to_owned(),
                });
            }
            Err(error) => return Err(error),
        };
        if !run.pending_tools.is_empty() {
            let error = "run_busy".to_owned();
            emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: error.clone(),
            })?;
            self.store_unless_terminal(run, &[])?;
            return Ok(());
        }
        run.steps = 0;
        run.step_id = None;
        run.wall_time_started_at = Instant::now();
        run.last_text.clear();
        run.last_tool_call = None;
        run.driver
            .begin_turn()
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        run.driver
            .queue_input()
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        run.inbox
            .insert_for_run(run_id, InboxTarget::NextTurn, InboxMessage::user(prompt))
            .map_err(KianaHarnessError::Failed)?;
        for message in run.inbox.claim(InboxTarget::NextTurn) {
            run.messages.push(ModelMessage::user(message.text));
        }
        self.model_step_in_flight(&mut run, emitter).await?;
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    fn cancel(
        &self,
        run_id: RunId,
        reason: String,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let error = format!("cancelled:{reason}");
        let in_flight = self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .get(&run_id)
            .cloned();
        if let Some(cancellation) = in_flight {
            cancellation.request(error.clone())?;
            return emitter.emit_event(RunnerEvent::Failed { run_id, error });
        }
        match self.take_run(run_id) {
            Ok(mut run) => {
                run.driver
                    .request_cancel()
                    .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
                // The first queued call may already be executing at the control plane.
                // Only the remaining calls can be proven not to have been dispatched.
                let _in_flight = run.pending_tools.pop_front();
                for pending in run.pending_tools.drain(..) {
                    emitter.emit_event(RunnerEvent::ToolCancelled {
                        run_id,
                        request_id: pending.request_id,
                        call_id: pending.call.id,
                        result:json!({"error":"cancelled:queued","not_executed":true,"replay_safe":true}),
                    })?;
                }
                emitter.emit_event(RunnerEvent::Failed { run_id, error })
            }
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => emitter
                .emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "run_not_found".to_owned(),
                }),
            Err(error) => Err(error),
        }
    }

    async fn model_step(
        &self,
        run: &mut ActiveRun,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        const DRIVER_OWNER: &str = "model-driver";
        run.driver
            .claim(DRIVER_OWNER)
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        let result = loop {
            match self.model_step_once(run, emitter).await {
                Ok(StepProgress::Continue) => continue,
                Ok(StepProgress::Finished) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        let release = run
            .driver
            .release(DRIVER_OWNER)
            .map_err(|error| KianaHarnessError::Failed(error.to_string()));
        result.and(release)
    }

    async fn model_step_once(
        &self,
        run: &mut ActiveRun,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<StepProgress, KianaHarnessError> {
        if let Some(error) = run.cancellation.error()? {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error,
            })?;
            return Ok(StepProgress::Finished);
        }
        let checkpoint = emitter.events.len();
        if run.steps >= run.max_steps_per_turn {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error: "max_steps_per_turn".to_owned(),
            })?;
            return Ok(StepProgress::Finished);
        }
        if self
            .wall_time_budget
            .is_some_and(|budget| run.wall_time_started_at.elapsed() >= budget)
        {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error: RUN_BUDGET_EXCEEDED_WALL_TIME.to_owned(),
            })?;
            return Ok(StepProgress::Finished);
        }
        let budget = match self.effective_budget(run) {
            Ok(budget) => budget,
            Err(error) => {
                emitter.emit_event(RunnerEvent::Failed {
                    run_id: run.run_id,
                    error,
                })?;
                return Ok(StepProgress::Finished);
            }
        };
        let budget_scope = Self::budget_scope(run);
        if run.steps >= budget.max_model_steps_per_turn {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error: "budget_exceeded:model_steps".to_owned(),
            })?;
            return Ok(StepProgress::Finished);
        }
        if let Err(error) = self.budget_ledger.check_time(&budget_scope, budget) {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error,
            })?;
            return Ok(StepProgress::Finished);
        }
        if kiana_domain::tool_catalog_digest() != run.tool_catalog_digest {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error: "tool_catalog_changed".to_owned(),
            })?;
            return Ok(StepProgress::Finished);
        }
        let deferred = self
            .deferred_inputs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .remove(&run.run_id)
            .unwrap_or_default();
        for (target, message) in deferred {
            run.inbox
                .insert_for_run(run.run_id, target, message)
                .map_err(KianaHarnessError::Failed)?;
        }
        run.steps += 1;
        run.step_id = Some(StepId::new());
        run.driver
            .begin_step(run.step_id.expect("step ID was just assigned"), run.steps)
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;

        for message in run.inbox.claim(InboxTarget::NextStep) {
            if message
                .target_turn_id
                .is_some_and(|turn| Some(turn) != run.turn_id)
            {
                emitter.emit_event(RunnerEvent::Failed {
                    run_id: run.run_id,
                    error: "inbox_target_turn_mismatch".to_owned(),
                })?;
                return Ok(StepProgress::Finished);
            }
            run.messages.push(ModelMessage::user(message.text));
        }

        let compact = compact_if_needed(
            std::mem::take(&mut run.messages),
            self.compact_trigger_tokens,
            self.compact_user_message_max_tokens,
        );
        run.messages = compact.messages;

        if compact.applied {
            if let Err(error) = self.budget_ledger.reserve_compaction(&budget_scope, budget) {
                emitter.emit_event(RunnerEvent::Failed {
                    run_id: run.run_id,
                    error,
                })?;
                return Ok(StepProgress::Finished);
            }
            emitter.emit_event(RunnerEvent::Compacted {
                run_id: run.run_id,
                tokens_before: compact.tokens_before as u64,
                tokens_after: compact.tokens_after as u64,
                summary_present: compact.summary_present,
            })?;
        }

        let run_id = run.run_id;
        let request = ModelRequest {
            messages: run.messages.clone(),
            tools: tool_schemas(),
            sandbox: run.sandbox.clone(),
        };
        let cancellation = run.cancellation.clone();
        let output = self
            .invoke_model(
                run,
                request,
                kiana_domain::ModelPurpose::Task,
                kiana_domain::ModelResponseFormat::Text,
                emitter,
            )
            .await;
        let emitted_delta = true;
        if let Some(error) = cancellation.error()? {
            emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
            return Ok(StepProgress::Finished);
        }
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                return Ok(StepProgress::Finished);
            }
        };
        if matches!(
            output.normalized_stop_reason(),
            kiana_domain::ModelStopReason::Unknown
                | kiana_domain::ModelStopReason::Length
                | kiana_domain::ModelStopReason::Refusal
                | kiana_domain::ModelStopReason::Pause
                | kiana_domain::ModelStopReason::Incomplete
        ) {
            let reason = match output.normalized_stop_reason() {
                kiana_domain::ModelStopReason::Length => "model_output_truncated",
                kiana_domain::ModelStopReason::Refusal => "model_refused",
                kiana_domain::ModelStopReason::Pause => "model_pause_requires_explicit_continue",
                kiana_domain::ModelStopReason::Incomplete => "model_transport_incomplete",
                _ => "model_stop_reason_unknown",
            };
            emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: reason.to_owned(),
            })?;
            return Ok(StepProgress::Finished);
        }
        if !output.text.is_empty() {
            let redacted_text = safe_channel_text(SecretScanChannel::Transcript, &output.text);
            run.last_text = redacted_text.clone();
            if !emitted_delta {
                emitter.emit_event(RunnerEvent::Delta {
                    run_id,
                    text: redacted_text,
                })?;
            }
        }
        if !output.text.is_empty() || !output.tool_calls.is_empty() {
            run.messages.push(ModelMessage::assistant_with_tools(
                safe_channel_text(SecretScanChannel::Transcript, &output.text),
                output.tool_calls.clone(),
            ));
        }

        if output.tool_calls.is_empty() {
            // DeepSeek: a text-only step ends the turn only when next-step is empty.
            if run.inbox.next_step.is_empty() {
                let ModelOutput {
                    usage,
                    stop_reason,
                    model_id,
                    structured,
                    ..
                } = output;
                let mut completed_output = json!({
                    "schema": HARNESS_RESULT_SCHEMA,
                    "source": HARNESS_ID,
                    "text": run.last_text,
                    "steps": run.steps,
                    "sandbox": run.sandbox,
                });
                let completed = completed_output
                    .as_object_mut()
                    .expect("harness result is a JSON object");
                if let Some(usage) = usage {
                    completed.insert("usage".to_owned(), json!(usage));
                }
                if let Some(stop_reason) = stop_reason {
                    completed.insert("stop_reason".to_owned(), json!(stop_reason));
                }
                if let Some(model_id) = model_id {
                    completed.insert("model_id".to_owned(), json!(model_id));
                }
                if let Some(structured) = structured {
                    completed.insert("structured".to_owned(), structured);
                }
                run.driver
                    .model_output(0, true)
                    .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
                let state = cancellation.lock()?;
                if let Some(error) = state.as_ref() {
                    let error = error.clone();
                    drop(state);
                    emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                    return Ok(StepProgress::Finished);
                }
                emitter.emit_event(RunnerEvent::Completed {
                    run_id: run.run_id,
                    output: completed_output,
                })?;
                return Ok(StepProgress::Finished);
            }
            run.driver
                .model_output(0, false)
                .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
            return Ok(StepProgress::Continue);
        }

        let state = cancellation.lock()?;
        if let Some(error) = state.as_ref() {
            let error = error.clone();
            drop(state);
            emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
            return Ok(StepProgress::Finished);
        }
        run.pending_tools.clear();
        kiana_domain::validate_model_calls(&output.tool_calls)
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
        let _batch_plan =
            kiana_domain::plan_tool_batch(&output.tool_calls).map_err(KianaHarnessError::Failed)?;
        let mapped = output
            .tool_calls
            .into_iter()
            .enumerate()
            .map(|(ordinal, call)| {
                let request_id = stable_invocation_request_id(run, &call, ordinal);
                capability_for_tool_with_request_id(
                    &call,
                    request_id,
                    &run.sandbox,
                    &run.project_root,
                )
                .map(|request| PendingTool {
                    request_id: request.request_id,
                    call,
                    phase: PendingToolPhase::Queued,
                })
            })
            .collect::<Result<Vec<_>, _>>();
        let mapped = match mapped {
            Ok(mapped) => mapped,
            Err(error) => {
                emitter.replace_event_since(
                    checkpoint,
                    RunnerEvent::Failed {
                        run_id: run.run_id,
                        error,
                    },
                )?;
                return Ok(StepProgress::Finished);
            }
        };
        run.pending_tools.extend(mapped);
        run.driver
            .model_output(run.pending_tools.len() as u32, false)
            .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;

        match run.pending_tools.front().cloned() {
            Some(pending) => match self.emit_tool_request(run, &pending.call) {
                Ok(event) => emitter.emit_event(event)?,
                Err(error) => emitter.replace_event_since(
                    checkpoint,
                    RunnerEvent::Failed {
                        run_id: run.run_id,
                        error,
                    },
                )?,
            },
            None => emitter.replace_event_since(
                checkpoint,
                RunnerEvent::Failed {
                    run_id: run.run_id,
                    error: "tool_queue_empty".to_owned(),
                },
            )?,
        }
        Ok(StepProgress::Finished)
    }

    /// A caller that elects to repair malformed structured output must call this entry point
    /// explicitly. The provider parser never loops internally; this purpose is budgeted as a
    /// separate model attempt and remains visible in the committed model-turn metadata.
    #[allow(dead_code)]
    async fn invoke_output_repair(
        &self,
        run: &mut ActiveRun,
        request: ModelRequest,
        format: kiana_domain::ModelResponseFormat,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<ModelOutput, String> {
        self.invoke_model(
            run,
            request,
            kiana_domain::ModelPurpose::OutputRepair,
            format,
            emitter,
        )
        .await
    }

    async fn invoke_model(
        &self,
        run: &mut ActiveRun,
        request: ModelRequest,
        purpose: kiana_domain::ModelPurpose,
        format: kiana_domain::ModelResponseFormat,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<ModelOutput, String> {
        use crate::retry::{classify_retry, is_safe_to_retry, retry_delay, MAX_PROVIDER_ATTEMPTS};
        use kiana_domain::{ModelCallSpec, ModelRetryClass, RequestId};
        let admission = self
            .model_budget
            .lock()
            .map_err(|_| "model_budget_lock_poisoned".to_owned())?
            .clone();
        let call_id = kiana_domain::derived_request_id(
            "model.call",
            &format!("{}:{}:{purpose:?}", run.run_id, run.steps),
        );
        let max_time = self
            .wall_time_budget
            .or_else(|| {
                run.model_assignment
                    .as_ref()
                    .map(|a| Duration::from_millis(a.max_wall_time_ms))
            })
            .unwrap_or(Duration::from_secs(300));
        let remaining = max_time
            .checked_sub(run.wall_time_started_at.elapsed())
            .ok_or_else(|| RUN_BUDGET_EXCEEDED_WALL_TIME.to_owned())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "model_clock_untrusted".to_owned())?
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let deadline = now.saturating_add(remaining.as_millis().min(u128::from(u64::MAX)) as u64);
        let started = Instant::now();
        let step_id = run
            .step_id
            .ok_or_else(|| "step_identity_missing".to_owned())?;
        let turn_id = run
            .turn_id
            .unwrap_or_else(|| TurnId::from_uuid(run.run_id.as_uuid()));
        let step_identity = StepIdentity::new(run.run_id, turn_id, step_id, run.steps)
            .map_err(|error| format!("step_identity_invalid:{error}"))?;
        let budget_limits = self.effective_budget(run)?;
        let budget_scope = Self::budget_scope(run);
        for attempt in 0..MAX_PROVIDER_ATTEMPTS {
            if let Some(error) = run.cancellation.error().map_err(|e| e.to_string())? {
                return Err(error);
            }
            let cancellation = run.cancellation.clone();
            let cancellation_signal = cancellation.subscribe();
            let attempt_id = RequestId::new();
            let model_attempt_id = ModelAttemptId::new();
            let attempt_identity = ModelAttemptIdentity::new(
                run.run_id,
                turn_id,
                step_id,
                model_attempt_id,
                call_id,
                attempt + 1,
            )
            .map_err(|error| format!("model_attempt_identity_invalid:{error}"))?;
            let spec = ModelCallSpec {
                call_id,
                attempt_id,
                model_attempt_id: Some(model_attempt_id),
                step_id: Some(step_id),
                step: run.steps,
                purpose,
                assignment: run.model_assignment.clone(),
                response_format: format.clone(),
                replay: Vec::new(),
                deadline_unix_ms: deadline,
            };
            let prepared = self
                .model
                .prepare_call(request.clone(), spec)
                .map_err(|error| error.to_string())?;
            if run
                .model_route
                .as_ref()
                .is_some_and(|route| route != &prepared.route)
            {
                return Err("model_route_changed_during_run".to_owned());
            }
            run.model_route = Some(prepared.route.clone());
            let audit = prepared.audit();
            let route = prepared.route.clone();
            let budget = prepared.budget.clone();
            let reservation =
                self.budget_ledger
                    .reserve_attempt(&budget_scope, budget_limits, budget.total)?;
            let permit = if let Some(guard) = &admission {
                let admission_remaining = remaining.saturating_sub(started.elapsed());
                if admission_remaining.is_zero() {
                    self.budget_ledger.release_attempt(reservation)?;
                    return Err("model_attempt_deadline".to_owned());
                }
                let reserve = tokio::select! {
                    biased;
                    error = cancellation.cancelled() => {
                        self.budget_ledger.release_attempt(reservation)?;
                        return Err(error);
                    }
                    result = tokio::time::timeout(
                        admission_remaining,
                        guard.reserve_prepared(&prepared),
                    ) => result,
                };
                match reserve {
                    Ok(Ok(permit)) => Some(permit),
                    Ok(Err(error)) => {
                        self.budget_ledger.release_attempt(reservation)?;
                        return Err(format!("model_admission_denied:{error}"));
                    }
                    Err(_) => {
                        self.budget_ledger.release_attempt(reservation)?;
                        return Err("model_attempt_deadline".to_owned());
                    }
                }
            } else {
                None
            };
            let admission_lease_digest = permit.as_ref().map(|permit| {
                json_digest(&json!({
                    "permit_id": permit.permit_id,
                    "attempt_id": permit.attempt_id,
                    "request_hash": permit.request_hash,
                }))
            });
            let mut attempt_reservation = RetryAttemptReservation::new(
                QuotaReservationId::new(),
                run.run_id,
                AttemptId::from_uuid(attempt_id.as_uuid()),
                attempt + 1,
                budget.total,
                deadline,
                admission_lease_digest,
            )?;
            // The reservation is the per-attempt fence.  It is marked before entering the
            // existing ModelClient boundary and can only reach one terminal state below.
            attempt_reservation.dispatch()?;
            let mut redactor = StreamingRedactor::new();
            let mut stream = ModelStreamAccumulator::new(model_attempt_id);
            let run_id = run.run_id;
            let mut observed_delta = false;
            let mut callback = |delta: ModelDelta| -> Result<(), String> {
                if let Some(error) = cancellation.error().map_err(|e| e.to_string())? {
                    return Err(error);
                }
                observed_delta = true;
                let text = match &delta {
                    ModelDelta::Text { text } => text.clone(),
                    _ => String::new(),
                };
                stream.push(delta)?;
                if text.is_empty() {
                    return Ok(());
                }
                let text = redactor.push(&text);
                if text.is_empty() {
                    return Ok(());
                }
                emitter.emit(RunnerEvent::Delta { run_id, text })
            };
            let future = async {
                if let (Some(guard), Some(permit)) = (&admission, permit) {
                    self.model
                        .complete_admitted_cancellable(
                            prepared,
                            permit,
                            guard.as_ref(),
                            &mut callback,
                            cancellation_signal,
                        )
                        .await
                } else {
                    self.model
                        .complete_prepared_cancellable(prepared, &mut callback, cancellation_signal)
                        .await
                }
            };
            let result = tokio::select! {
                result=tokio::time::timeout(remaining.saturating_sub(started.elapsed()),future)=>result.unwrap_or_else(|_|Err(kiana_domain::ModelError::transport("model_attempt_deadline",ModelRetryClass::Never,true))),
                error=cancellation.cancelled()=>Err(kiana_domain::ModelError::invalid(error)),
            };
            drop(callback);
            let measured = match result
                .as_ref()
                .ok()
                .and_then(|reply| reply.output.usage.as_ref())
            {
                Some(usage) => Some(
                    usage
                        .input_tokens
                        .checked_add(usage.output_tokens)
                        .ok_or_else(|| "model_usage_overflow".to_owned())?,
                ),
                None => None,
            };
            let cancellation_requested = cancellation.error().map_err(|e| e.to_string())?.is_some();
            if cancellation_requested
                || result.as_ref().err().is_some_and(|error| {
                    error.request_sent
                        || error.side_effect_state == kiana_domain::ModelSideEffectState::Unknown
                })
            {
                attempt_reservation.mark_unknown(measured)?;
            } else {
                attempt_reservation.settle(measured)?;
            }
            self.budget_ledger.settle_attempt(reservation, measured)?;
            if let Some(guard) = &admission {
                guard
                    .settle(run.run_id, attempt_id, measured)
                    .await
                    .map_err(|error| format!("model_usage_settlement_failed:{error}"))?;
            }
            let usage = result
                .as_ref()
                .ok()
                .and_then(|reply| reply.output.usage.as_ref());
            let retry_reservation = attempt_reservation.clone();
            let retry_decision = result
                .as_ref()
                .err()
                .map(|error| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| "model_clock_untrusted".to_owned())?
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64;
                    classify_retry(
                        error,
                        observed_delta,
                        attempt + 1,
                        attempt + 1,
                        now,
                        deadline,
                        error.side_effect_state == kiana_domain::ModelSideEffectState::None
                            && !observed_delta,
                    )
                })
                .transpose()?;
            let budget_usage = self.budget_ledger.snapshot(&budget_scope)?;
            // Built as its own value: nesting 16 more keys inside the enclosing
            // literal pushes `json!` past its default macro recursion limit.
            let harness_budget = json!({"schema":crate::budget::HARNESS_BUDGET_SCHEMA,"scope":budget_scope,"source":budget_limits.source.as_str(),"max_model_steps_per_turn":budget_limits.max_model_steps_per_turn,"max_attempts_per_task":budget_limits.max_attempts_per_task,"max_tool_calls_per_task":budget_limits.max_tool_calls_per_task,"max_repairs_per_task":budget_limits.max_repairs_per_task,"max_compactions_per_task":budget_limits.max_compactions_per_task,"max_tokens_per_task":budget_limits.max_tokens_per_task,"model_attempts":budget_usage.model_attempts,"tool_calls":budget_usage.tool_calls,"repairs":budget_usage.repairs,"compactions":budget_usage.compactions,"reserved_tokens":budget_usage.reserved_tokens,"charged_tokens":budget_usage.charged_tokens,"unknown_attempts":budget_usage.unknown_attempts});
            emitter.emit(RunnerEvent::ModelTurn {run_id,step:run.steps,metadata:json!({
                "schema":"kiana.model-turn.v2","model_call_id":call_id,"model_request_id":attempt_id,"model_attempt_id":model_attempt_id,
                "turn_id":run.turn_id,"step_id":step_id,"step_identity":step_identity.clone(),"attempt_identity":attempt_identity,"attempt":attempt+1,
                "provider_id":route.provider_id,"model_id":result.as_ref().ok().and_then(|reply|reply.output.model_id.as_ref()).unwrap_or(&route.model_id),
                "prepared":audit.clone(),"route_digest":audit["route_digest"],"prompt_version":audit["prompt_version"],"tool_catalog_digest":run.tool_catalog_digest,
                "configuration_revision":route.configuration_revision,
                "provider_request_id":result.as_ref().ok().and_then(|reply|reply.provider_request_id.clone()),
                "provider_response_id":result.as_ref().ok().and_then(|reply|reply.provider_response_id.clone()),
                "provider_request_id_status":if result.as_ref().ok().and_then(|reply|reply.provider_request_id.as_ref()).is_some() {"known"} else {"unknown"},
                "provider_response_id_status":if result.as_ref().ok().and_then(|reply|reply.provider_response_id.as_ref()).is_some() {"known"} else {"unknown"},
                "streaming":route.streaming,"budget":budget,"reserved_tokens":budget.total,"prompt_sources":run.prompt_sources,
                "retry_reservation":retry_reservation,
                "harness_budget":harness_budget,
                "usage":usage,"usage_complete":usage.is_some(),"attempted":true,"purpose":purpose,
                "elapsed_ms":started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                "finish":result.as_ref().ok().map(|reply|reply.finish),
                "stop_reason_normalized":result.as_ref().ok().map(|reply|kiana_domain::ModelStopReason::from(reply.finish)).or_else(||result.as_ref().err().map(|error|error.outcome().stop_reason)),
                "outcome":result.as_ref().ok().map(|reply|reply.outcome()).or_else(||result.as_ref().err().map(|error|error.outcome())),
                "retry_class":result.as_ref().err().map(|error|error.retry_class),
                "assistant":result.as_ref().ok().map(|reply|kiana_domain::redact_value(&json!(reply.output))),
                "provider_timing":result.as_ref().ok().and_then(|reply|reply.provider_timing.as_ref()),
                "error":result.as_ref().err(),
            })})?;
            match result {
                Ok(reply) => {
                    if let Some(error) = cancellation.error().map_err(|e| e.to_string())? {
                        stream.cancel();
                        return Err(error);
                    }
                    let output = stream
                        .finish(reply.output)
                        .map_err(|error| format!("model_stream_normalization_failed:{error}"))?;
                    let tail = redactor.finish();
                    if !tail.is_empty() {
                        emitter.emit(RunnerEvent::Delta { run_id, text: tail })?;
                    }
                    return Ok(output);
                }
                Err(error)
                    if attempt + 1 < MAX_PROVIDER_ATTEMPTS
                        && retry_decision
                            .as_ref()
                            .is_some_and(|decision| decision.retry)
                        && is_safe_to_retry(&error, observed_delta) =>
                {
                    let delay = retry_delay(&error, attempt, attempt_id);
                    if delay >= remaining.saturating_sub(started.elapsed()) {
                        return Err("model_retry_deadline_exceeded".to_owned());
                    }
                    tokio::select! {_=tokio::time::sleep(delay)=>{},error=cancellation.cancelled()=>return Err(error)}
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("model_attempt_limit".to_owned())
    }

    fn take_run(&self, run_id: RunId) -> Result<ActiveRun, KianaHarnessError> {
        self.runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .remove(&run_id)
            .ok_or_else(|| KianaHarnessError::Failed("run_not_found".to_owned()))
    }

    fn has_run(&self, run_id: RunId) -> Result<bool, KianaHarnessError> {
        Ok(self
            .runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .contains_key(&run_id))
    }

    fn has_in_flight(&self, run_id: RunId) -> Result<bool, KianaHarnessError> {
        Ok(self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .contains_key(&run_id))
    }

    fn register_in_flight(
        &self,
        run_id: RunId,
        cancellation: Arc<RunCancellation>,
    ) -> Result<(), KianaHarnessError> {
        let mut in_flight = self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        if in_flight.contains_key(&run_id) {
            return Err(KianaHarnessError::Failed("run_already_exists".to_owned()));
        }
        in_flight.insert(run_id, cancellation);
        Ok(())
    }

    fn unregister_in_flight(&self, run_id: RunId) -> Result<(), KianaHarnessError> {
        self.in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .remove(&run_id);
        Ok(())
    }

    async fn model_step_in_flight(
        &self,
        run: &mut ActiveRun,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        self.register_in_flight(run.run_id, run.cancellation.clone())?;
        let result = self.model_step(run, emitter).await;
        self.unregister_in_flight(run.run_id)?;
        result
    }

    fn store_unless_terminal(
        &self,
        run: ActiveRun,
        events: &[RunnerEvent],
    ) -> Result<(), KianaHarnessError> {
        // A wall-time budget failure ends only the current turn. Continue
        // starts a fresh budget, so keep the run for that explicit command.
        let terminal_failure = events.iter().any(|event| {
            matches!(
                event,
                RunnerEvent::Failed { error, .. } if error != RUN_BUDGET_EXCEEDED_WALL_TIME
            )
        });
        if terminal_failure {
            return Ok(());
        }
        self.runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .insert(run.run_id, run);
        Ok(())
    }

    fn emit_tool_request(
        &self,
        run: &mut ActiveRun,
        call: &ModelToolCall,
    ) -> Result<RunnerEvent, String> {
        if let Some(error) = self.record_tool_call(run, call) {
            return Err(error);
        }
        let pending_id = run
            .pending_tools
            .front()
            .map(|pending| pending.request_id)
            .ok_or_else(|| "tool_queue_empty".to_owned())?;
        let mut request =
            capability_for_tool_with_request_id(call, pending_id, &run.sandbox, &run.project_root)?;
        let turn_id = run
            .turn_id
            .unwrap_or_else(|| TurnId::from_uuid(run.run_id.as_uuid()));
        let step_id = run
            .step_id
            .ok_or_else(|| "step_identity_missing".to_owned())?;
        let pending_batch = run
            .pending_tools
            .iter()
            .map(|pending| json!({"request_id":pending.request_id,"call_id":pending.call.id,"phase":pending.phase}))
            .collect::<Vec<_>>();
        request.arguments["turn_id"] = json!(turn_id);
        request.arguments["step_id"] = json!(step_id);
        request.arguments["pending_batch_digest"] =
            json!(kiana_domain::json_digest(&json!(pending_batch)));
        let limits = self.effective_budget(run)?;
        self.budget_ledger
            .reserve_tool_call(&Self::budget_scope(run), limits)?;
        // Extension provenance comes from the immutable system bundle loaded by daemon.
        // Model-provided fields are overwritten even when the trusted scope is empty.
        let scopes = match run
            .messages
            .iter()
            .find(|message| message.role == crate::model::ModelRole::System)
        {
            Some(message) if message.text.trim_start().starts_with('{') => {
                PromptBundle::decode(&message.text)?.extensions
            }
            _ => Vec::new(),
        };
        request.arguments["_extension_scopes"] = json!(scopes);
        if let Some(pending) = run.pending_tools.front_mut() {
            if pending.request_id != request.request_id {
                return Err("capability_request_identity_changed".to_owned());
            }
            if pending.phase != PendingToolPhase::Queued {
                return Err("capability_request_already_dispatched".to_owned());
            }
            pending.phase = PendingToolPhase::Dispatched;
        }
        Ok(RunnerEvent::CapabilityRequested {
            run_id: run.run_id,
            request,
        })
    }

    fn record_tool_call(&self, run: &mut ActiveRun, call: &ModelToolCall) -> Option<String> {
        let canonical_arguments = canonical_json(&call.arguments);
        let is_same_as_last = run.last_tool_call.as_ref().is_some_and(|last| {
            last.name == call.name && last.canonical_arguments == canonical_arguments
        });
        let count = if is_same_as_last {
            let last = run
                .last_tool_call
                .as_mut()
                .expect("the previous tool call was just observed");
            last.count = last.count.saturating_add(1);
            last.count
        } else {
            run.last_tool_call = Some(RepeatedToolCall {
                name: call.name.clone(),
                canonical_arguments,
                count: 1,
            });
            1
        };
        (count >= self.repeated_tool_call_threshold)
            .then(|| format!("repeated_tool_call:{}", call.name))
    }
}

fn safe_channel_text(channel: SecretScanChannel, text: &str) -> String {
    let redacted = redact_text(text);
    if scan_secret_sentinels(channel, &redacted).is_ok() {
        redacted
    } else {
        "[REDACTED]".to_owned()
    }
}

impl From<KianaHarnessError> for PortError {
    fn from(error: KianaHarnessError) -> Self {
        match error {
            KianaHarnessError::Unavailable(message) => PortError::Unavailable(message),
            KianaHarnessError::Failed(message) => PortError::Failed(message),
        }
    }
}

#[async_trait]
impl RunnerPort for KianaHarness {
    fn bind_model_assignment(
        &self,
        run_id: RunId,
        assignment: kiana_domain::ModelAssignment,
    ) -> Result<(), PortError> {
        assignment
            .validate()
            .map_err(|e| PortError::Failed(e.to_string()))?;
        if assignment.run_id != run_id {
            return Err(PortError::Failed(
                "model_assignment_run_mismatch".to_owned(),
            ));
        }
        let mut assignments = self
            .assignments
            .lock()
            .map_err(|_| PortError::Failed("harness_assignment_lock_poisoned".to_owned()))?;
        if assignments
            .get(&run_id)
            .is_some_and(|current| current != &assignment)
            || self.has_run(run_id)?
            || self.has_in_flight(run_id)?
        {
            return Err(PortError::Conflict(
                "model_assignment_already_bound".to_owned(),
            ));
        }
        assignments.insert(run_id, assignment);
        Ok(())
    }

    fn bind_model_history(
        &self,
        run_id: RunId,
        history: Vec<ModelMessage>,
    ) -> Result<(), PortError> {
        kiana_domain::validate_model_history(&history)
            .map_err(|e| PortError::Failed(e.to_string()))?;
        if history
            .iter()
            .any(|message| message.role == crate::model::ModelRole::System)
        {
            return Err(PortError::Failed(
                "history_cannot_change_system_authority".to_owned(),
            ));
        }
        let mut histories = self
            .histories
            .lock()
            .map_err(|_| PortError::Failed("harness_history_lock_poisoned".to_owned()))?;
        if histories.get(&run_id).is_some_and(|old| old != &history)
            || self.has_run(run_id)?
            || self.has_in_flight(run_id)?
        {
            return Err(PortError::Conflict(
                "model_history_already_bound".to_owned(),
            ));
        }
        histories.insert(run_id, history);
        Ok(())
    }

    fn install_model_budget(
        &self,
        budget: Arc<dyn kiana_ports::ModelBudgetPort>,
    ) -> Result<(), PortError> {
        let mut slot = self
            .model_budget
            .lock()
            .map_err(|_| PortError::Failed("model_budget_lock_poisoned".to_owned()))?;
        if slot.is_some() {
            return Err(PortError::Conflict(
                "model_budget_already_installed".to_owned(),
            ));
        }
        *slot = Some(budget);
        Ok(())
    }

    async fn checkpoint(&self, run_id: RunId) -> Result<serde_json::Value, PortError> {
        if self.has_in_flight(run_id)? {
            return Err(PortError::Conflict("runner_checkpoint_busy".to_owned()));
        }
        let runs = self
            .runs
            .lock()
            .map_err(|_| PortError::Failed("harness_lock_poisoned".to_owned()))?;
        let run = runs
            .get(&run_id)
            .ok_or_else(|| PortError::Unavailable("run_not_found".to_owned()))?;
        if run.cancellation.error()?.is_some() {
            return Err(PortError::Conflict(
                "runner_checkpoint_cancelled".to_owned(),
            ));
        }
        run.progress_tracker.validate().map_err(PortError::Failed)?;
        serde_json::to_value(HarnessCheckpoint {
            schema: "kiana.harness-checkpoint.v1".to_owned(),
            run_id,
            turn_id: run.turn_id,
            step_id: run.step_id,
            driver: Some(run.driver.clone()),
            progress_tracker: Some(run.progress_tracker.clone()),
            pending_clarification: run.pending_clarification.clone(),
            sandbox: run.sandbox.clone(),
            project_root: run.project_root.clone(),
            inbox: run.inbox.clone(),
            messages: run.messages.clone(),
            prompt_sources: run.prompt_sources.clone(),
            model_assignment: run.model_assignment.clone(),
            model_route: run.model_route.clone(),
            pending_tools: run.pending_tools.clone(),
            last_tool_call: run.last_tool_call.clone(),
            steps: run.steps,
            max_steps_per_turn: run.max_steps_per_turn,
            tool_catalog_digest: run.tool_catalog_digest.clone(),
            wall_time_elapsed_ms: run
                .wall_time_started_at
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            last_text: run.last_text.clone(),
        })
        .map_err(|error| PortError::Failed(format!("runner_checkpoint_invalid:{error}")))
    }

    async fn restore(&self, run_id: RunId, checkpoint: serde_json::Value) -> Result<(), PortError> {
        let checkpoint: HarnessCheckpoint = serde_json::from_value(checkpoint)
            .map_err(|error| PortError::Failed(format!("runner_checkpoint_invalid:{error}")))?;
        if checkpoint.schema != "kiana.harness-checkpoint.v1"
            || checkpoint.run_id != run_id
            || checkpoint.max_steps_per_turn == 0
            || checkpoint.steps > checkpoint.max_steps_per_turn
            || checkpoint
                .turn_id
                .zip(checkpoint.model_assignment.as_ref().map(|a| a.turn_id))
                .is_some_and(|(turn_id, assignment_turn)| turn_id != assignment_turn)
        {
            return Err(PortError::Failed("runner_checkpoint_invalid".to_owned()));
        }
        checkpoint.inbox.validate().map_err(PortError::Failed)?;
        let current_tool_catalog = kiana_domain::current_tool_catalog();
        current_tool_catalog.validate().map_err(PortError::Failed)?;
        if !checkpoint.tool_catalog_digest.trim().is_empty()
            && checkpoint.tool_catalog_digest != current_tool_catalog.digest
        {
            return Err(PortError::Failed(
                "runner_checkpoint_tool_catalog_changed".to_owned(),
            ));
        }
        let driver = checkpoint
            .driver
            .clone()
            .unwrap_or_else(|| RunDriver::new(run_id, checkpoint.turn_id));
        driver
            .validate()
            .map_err(|error| PortError::Failed(error.to_string()))?;
        let progress_tracker = checkpoint.progress_tracker.unwrap_or_default();
        progress_tracker.validate().map_err(PortError::Failed)?;
        if driver.frame.run_id != run_id || driver.frame.turn.turn_id != checkpoint.turn_id {
            return Err(PortError::Failed("runner_checkpoint_invalid".to_owned()));
        }
        let sandbox = normalize_sandbox(&checkpoint.sandbox)?;
        let mut ids = std::collections::HashSet::new();
        for (ordinal, pending) in checkpoint.pending_tools.iter().enumerate() {
            let request_id = pending.request_id;
            let call = &pending.call;
            if !ids.insert(&call.id) || call.id.trim().is_empty() {
                return Err(PortError::Failed(
                    "runner_checkpoint_duplicate_tool".to_owned(),
                ));
            }
            let turn_id = checkpoint
                .turn_id
                .unwrap_or_else(|| TurnId::from_uuid(run_id.as_uuid()));
            let step_id = checkpoint
                .step_id
                .ok_or_else(|| PortError::Failed("runner_checkpoint_step_missing".to_owned()))?;
            if request_id
                != stable_invocation_request_id_parts(run_id, turn_id, step_id, &call.id, ordinal)
            {
                return Err(PortError::Failed(
                    "runner_checkpoint_invocation_identity_mismatch".to_owned(),
                ));
            }
            capability_for_tool_with_request_id(
                call,
                request_id,
                sandbox,
                &checkpoint.project_root,
            )
            .map_err(PortError::Failed)?;
            if pending.phase == PendingToolPhase::Settled {
                return Err(PortError::Failed(
                    "runner_checkpoint_settled_tool_pending".to_owned(),
                ));
            }
        }
        let elapsed = Duration::from_millis(checkpoint.wall_time_elapsed_ms);
        if self
            .wall_time_budget
            .is_some_and(|budget| elapsed >= budget)
        {
            return Err(PortError::Failed(RUN_BUDGET_EXCEEDED_WALL_TIME.to_owned()));
        }
        if self.has_in_flight(run_id)? {
            return Err(PortError::Conflict("run_already_exists".to_owned()));
        }
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| PortError::Failed("harness_lock_poisoned".to_owned()))?;
        if runs.contains_key(&run_id) {
            return Err(PortError::Conflict("run_already_exists".to_owned()));
        }
        let started = Instant::now()
            .checked_sub(elapsed)
            .ok_or_else(|| PortError::Failed("runner_checkpoint_clock_invalid".to_owned()))?;
        runs.insert(
            run_id,
            ActiveRun {
                run_id,
                turn_id: checkpoint.turn_id,
                step_id: checkpoint.step_id,
                driver,
                progress_tracker,
                pending_clarification: checkpoint.pending_clarification,
                sandbox: checkpoint.sandbox,
                project_root: checkpoint.project_root,
                inbox: checkpoint.inbox,
                messages: checkpoint.messages,
                prompt_sources: checkpoint.prompt_sources,
                model_assignment: checkpoint.model_assignment,
                model_route: checkpoint.model_route,
                pending_tools: checkpoint.pending_tools,
                last_tool_call: checkpoint.last_tool_call,
                steps: checkpoint.steps,
                max_steps_per_turn: checkpoint.max_steps_per_turn.min(self.max_steps_per_turn),
                tool_catalog_digest: current_tool_catalog.digest,
                wall_time_started_at: started,
                last_text: checkpoint.last_text,
                cancellation: Arc::new(RunCancellation::default()),
            },
        );
        Ok(())
    }

    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.dispatch(command, None).await.map_err(PortError::from)
    }

    async fn send_with_events(
        &self,
        command: RunnerCommand,
        on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, PortError> {
        self.dispatch(command, Some(on_event))
            .await
            .map_err(PortError::from)
    }
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            let mut canonical = String::from("{");
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    canonical.push(',');
                }
                canonical.push_str(
                    &serde_json::to_string(key).expect("JSON object key serialization cannot fail"),
                );
                canonical.push(':');
                canonical.push_str(&canonical_json(value));
            }
            canonical.push('}');
            canonical
        }
        serde_json::Value::Array(values) => {
            let mut canonical = String::from("[");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    canonical.push(',');
                }
                canonical.push_str(&canonical_json(value));
            }
            canonical.push(']');
            canonical
        }
        _ => serde_json::to_string(value).expect("JSON scalar serialization cannot fail"),
    }
}

fn normalize_sandbox(sandbox: &str) -> Result<&str, KianaHarnessError> {
    match sandbox.trim() {
        "" | DEFAULT_HARNESS_SANDBOX => Ok(DEFAULT_HARNESS_SANDBOX),
        HARNESS_SANDBOX_WORKSPACE_WRITE => Ok(HARNESS_SANDBOX_WORKSPACE_WRITE),
        other => Err(KianaHarnessError::Failed(format!(
            "sandbox_unsupported:{other}"
        ))),
    }
}

fn stable_invocation_request_id(
    run: &ActiveRun,
    call: &ModelToolCall,
    ordinal: usize,
) -> RequestId {
    let turn_id = run
        .turn_id
        .unwrap_or_else(|| TurnId::from_uuid(run.run_id.as_uuid()));
    let step_id = run
        .step_id
        .unwrap_or_else(|| StepId::from_uuid(run.run_id.as_uuid()));
    stable_invocation_request_id_parts(run.run_id, turn_id, step_id, &call.id, ordinal)
}

fn stable_invocation_request_id_parts(
    run_id: RunId,
    turn_id: TurnId,
    step_id: StepId,
    assistant_item_id: &str,
    ordinal: usize,
) -> RequestId {
    derived_request_id(
        "harness.invocation",
        &format!(
            "run={}:turn={}:step={}:assistant_item={}:ordinal={ordinal}",
            run_id, turn_id, step_id, assistant_item_id
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::CapabilityKind;
    use kiana_runner_protocol::RunnerCommand;
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn scripted(outputs: Value) -> KianaHarness {
        KianaHarness::new(Arc::new(ScriptedModel::from_json(&outputs).unwrap()))
    }

    fn capability_request_id(events: &[RunnerEvent]) -> kiana_domain::RequestId {
        events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
                _ => None,
            })
            .expect("expected a capability request")
    }

    async fn send_capability_success(
        harness: &KianaHarness,
        run_id: RunId,
        request_id: kiana_domain::RequestId,
    ) -> Vec<RunnerEvent> {
        harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request_id, json!({"ok": true})),
            })
            .await
            .unwrap()
    }

    fn backdate_wall_time_start(harness: &KianaHarness, run_id: RunId, elapsed: Duration) {
        let mut runs = harness.runs.lock().unwrap();
        let run = runs.get_mut(&run_id).expect("run must be stored");
        run.wall_time_started_at = Instant::now()
            .checked_sub(elapsed)
            .expect("test clock underflow");
    }

    #[test]
    fn runtime_config_defaults_repeated_tool_call_threshold_to_three() {
        assert_eq!(RuntimeConfig::default().repeated_tool_call_threshold, 3);
    }

    #[test]
    fn runtime_config_defaults_wall_time_budget_to_none() {
        assert_eq!(RuntimeConfig::default().wall_time_budget, None);
    }

    #[tokio::test]
    async fn sufficient_wall_time_budget_allows_normal_completion() {
        let harness =
            scripted(json!([{"text": "done"}])).with_wall_time_budget(Duration::from_secs(5));
        let run_id = RunId::new();

        let events = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();

        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn wall_time_budget_exceeded_between_model_steps_fails_closed() {
        let harness = scripted(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "must not run"}
        ]))
        .with_wall_time_budget(Duration::from_secs(1));
        let run_id = RunId::new();

        let started = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        backdate_wall_time_start(&harness, run_id, Duration::from_secs(2));
        let events =
            send_capability_success(&harness, run_id, capability_request_id(&started)).await;

        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "run_budget_exceeded:wall_time".to_owned(),
            })
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!events.iter().any(|event| matches!(
            event,
            RunnerEvent::Delta { text, .. } if text == "must not run"
        )));
    }

    #[tokio::test]
    async fn continue_run_resets_wall_time_budget_after_timeout() {
        let harness = scripted(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "continued"}
        ]))
        .with_wall_time_budget(Duration::from_secs(1));
        let run_id = RunId::new();

        let started = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        backdate_wall_time_start(&harness, run_id, Duration::from_secs(2));
        let timed_out =
            send_capability_success(&harness, run_id, capability_request_id(&started)).await;
        assert_eq!(
            timed_out.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "run_budget_exceeded:wall_time".to_owned(),
            })
        );

        let continued = harness
            .send(RunnerCommand::continue_run(run_id, "keep going"))
            .await
            .unwrap();

        assert!(continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "continued")));
        assert!(continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn configured_repeated_tool_call_threshold_fails_at_that_count() {
        let harness = KianaHarness::with_config(
            Arc::new(
                ScriptedModel::from_json(&json!([
                    {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
                    {"text": "second", "tool_calls": [{"id": "c2", "name": "shell", "arguments": {"command": "ls"}}]},
                    {"text": "done"}
                ]))
                .unwrap(),
            ),
            RuntimeConfig {
                repeated_tool_call_threshold: 2,
                ..RuntimeConfig::default()
            },
        );
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;

        assert_eq!(
            second.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "repeated_tool_call:shell".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn start_command_max_steps_limits_that_run() {
        let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&json!([
            {"text": "first", "tool_calls": [{"id": "c1", "name": "memory.search", "arguments": {"query": "step", "collection": "role"}}]},
            {"text": "second", "tool_calls": [{"id": "c2", "name": "memory.search", "arguments": {"query": "step-1", "collection": "role"}}]},
            {"text": "third", "tool_calls": [{"id": "c3", "name": "memory.search", "arguments": {"query": "step-2", "collection": "role"}}]},
            {"text": "must not run"}
        ])).unwrap()))
        .with_max_steps(8);
        let run_id = RunId::new();

        let started = harness
            .send(RunnerCommand::start_in_with_history(
                run_id,
                "go",
                Vec::new(),
                String::new(),
                DEFAULT_HARNESS_SANDBOX,
                String::new(),
                false,
                3,
            ))
            .await
            .unwrap();
        let second =
            send_capability_success(&harness, run_id, capability_request_id(&started)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;
        let fourth = send_capability_success(&harness, run_id, capability_request_id(&third)).await;

        assert_eq!(
            started
                .iter()
                .filter(|event| matches!(event, RunnerEvent::ModelTurn { .. }))
                .count(),
            1
        );
        assert_eq!(
            second
                .iter()
                .filter(|event| matches!(event, RunnerEvent::ModelTurn { .. }))
                .count(),
            1
        );
        assert_eq!(
            third
                .iter()
                .filter(|event| matches!(event, RunnerEvent::ModelTurn { .. }))
                .count(),
            1
        );
        assert_eq!(
            fourth
                .iter()
                .filter(|event| matches!(event, RunnerEvent::ModelTurn { .. }))
                .count(),
            0
        );

        assert_eq!(
            fourth.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "max_steps_per_turn".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn third_consecutive_identical_tool_call_fails_closed() {
        let harness = scripted(json!([
            {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "second", "tool_calls": [{"id": "c2", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "third", "tool_calls": [{"id": "c3", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;

        assert_eq!(
            third.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "repeated_tool_call:shell".to_owned(),
            })
        );
        assert!(!third.iter().any(|event| matches!(
            event,
            RunnerEvent::CapabilityRequested { .. } | RunnerEvent::Completed { .. }
        )));
    }

    #[tokio::test]
    async fn consecutive_same_tool_with_different_arguments_is_not_blocked() {
        let harness = scripted(json!([
            {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "second", "tool_calls": [{"id": "c2", "name": "shell", "arguments": {"command": "ls src"}}]},
            {"text": "third", "tool_calls": [{"id": "c3", "name": "shell", "arguments": {"command": "ls tests"}}]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;
        let completed =
            send_capability_success(&harness, run_id, capability_request_id(&third)).await;

        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn alternating_different_tools_reset_repetition_count() {
        let harness = scripted(json!([
            {"text": "shell", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "patch", "tool_calls": [{"id": "c2", "name": "apply_patch", "arguments": {"patch": "one"}}]},
            {"text": "shell", "tool_calls": [{"id": "c3", "name": "shell", "arguments": {"command": "pwd"}}]},
            {"text": "patch", "tool_calls": [{"id": "c4", "name": "apply_patch", "arguments": {"patch": "two"}}]},
            {"text": "shell", "tool_calls": [{"id": "c5", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();
        let mut events = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();

        for _ in 0..4 {
            assert!(!events
                .iter()
                .any(|event| matches!(event, RunnerEvent::Failed { .. })));
            events =
                send_capability_success(&harness, run_id, capability_request_id(&events)).await;
        }
        let completed =
            send_capability_success(&harness, run_id, capability_request_id(&events)).await;

        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn repeated_tool_call_detection_ignores_json_object_key_order() {
        let harness = scripted(json!([
            {
                "text": "first",
                "tool_calls": [{
                    "id": "c1",
                    "name": "mcp",
                    "arguments": {"server": "local", "tool": "read", "arguments": {"beta": 2, "alpha": 1}}
                }]
            },
            {
                "text": "second",
                "tool_calls": [{
                    "id": "c2",
                    "name": "mcp",
                    "arguments": {"arguments": {"alpha": 1, "beta": 2}, "tool": "read", "server": "local"}
                }]
            },
            {
                "text": "third",
                "tool_calls": [{
                    "id": "c3",
                    "name": "mcp",
                    "arguments": {"server": "local", "tool": "read", "arguments": {"beta": 2, "alpha": 1}}
                }]
            },
            {"text": "done"}
        ]));
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;

        assert_eq!(
            third.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "repeated_tool_call:mcp".to_owned(),
            })
        );
    }

    #[derive(Debug)]
    struct ChunkedModel {
        chunks: Vec<&'static str>,
    }

    #[async_trait]
    impl ModelClient for ChunkedModel {
        async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
            Err("chunked_model_complete_must_not_be_called".to_owned())
        }

        async fn complete_streaming(
            &self,
            _request: ModelRequest,
            on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        ) -> Result<ModelOutput, String> {
            let mut text = String::new();
            for chunk in &self.chunks {
                text.push_str(chunk);
                on_delta(ModelDelta::Text {
                    text: (*chunk).to_owned(),
                })?;
            }
            Ok(ModelOutput::text(text))
        }
    }

    #[derive(Default)]
    struct CapturingModel {
        seen: Mutex<Vec<Vec<ModelMessage>>>,
    }

    #[async_trait]
    impl ModelClient for CapturingModel {
        async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
            self.seen.lock().unwrap().push(request.messages);
            Ok(ModelOutput::text("compacted"))
        }
    }

    #[tokio::test]
    async fn start_instructions_become_the_first_system_message() {
        let model = Arc::new(CapturingModel::default());
        let harness = KianaHarness::new(model.clone());
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start_in_with_instructions(
                run_id,
                "map it",
                "/repo",
                DEFAULT_HARNESS_SANDBOX,
                "Available skills:\n\n## code03-harness-demo\n",
                true,
            ))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        let first = seen.first().expect("model saw a request");
        assert_eq!(
            first.first().map(|message| message.role.clone()),
            Some(crate::model::ModelRole::System)
        );
        assert!(
            first
                .first()
                .is_some_and(|message| message.text.contains("code03-harness-demo")),
            "{first:?}"
        );
    }

    #[tokio::test]
    async fn text_only_turn_completes_without_a_tool_request() {
        let harness = scripted(json!([{"text": "architecture mapped"}]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();
        assert_eq!(events.first(), Some(&RunnerEvent::Started { run_id }));
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "architecture mapped")));
        let RunnerEvent::Completed { output, .. } = events.last().unwrap() else {
            panic!("expected completed");
        };
        assert_eq!(output["schema"], HARNESS_RESULT_SCHEMA);
        assert_eq!(output["source"], HARNESS_ID);
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
    }

    #[tokio::test]
    async fn sink_receives_each_streamed_delta_in_order() {
        let harness = KianaHarness::new(Arc::new(ChunkedModel {
            chunks: vec!["alpha", " beta", " gamma"],
        }));
        let run_id = RunId::new();
        let mut delivered = Vec::new();

        let events = harness
            .send_with_events(RunnerCommand::start(run_id, "stream it"), &mut |event| {
                delivered.push(event);
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(delivered, events);
        let deltas = events
            .iter()
            .filter_map(|event| match event {
                RunnerEvent::Delta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(deltas, vec!["alpha", " beta", " gamma"]);
        assert!(matches!(events.last(), Some(RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn sink_is_called_before_the_model_step_finishes() {
        #[derive(Debug)]
        struct ObservingModel {
            observed: Arc<AtomicBool>,
        }

        #[async_trait]
        impl ModelClient for ObservingModel {
            async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
                Err("observing_model_complete_must_not_be_called".to_owned())
            }

            async fn complete_streaming(
                &self,
                _request: ModelRequest,
                on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
            ) -> Result<ModelOutput, String> {
                on_delta(ModelDelta::Text {
                    text: "first".to_owned(),
                })?;
                assert!(
                    self.observed.load(Ordering::SeqCst),
                    "the sink must observe the delta before the model step continues"
                );
                on_delta(ModelDelta::Text {
                    text: " second".to_owned(),
                })?;
                Ok(ModelOutput::text("first second"))
            }
        }

        let observed = Arc::new(AtomicBool::new(false));
        let harness = KianaHarness::new(Arc::new(ObservingModel {
            observed: Arc::clone(&observed),
        }));
        let mut delivered = Vec::new();

        let events = harness
            .send_with_events(
                RunnerCommand::start(RunId::new(), "stream it"),
                &mut |event| {
                    if matches!(event, RunnerEvent::Delta { .. }) {
                        observed.store(true, Ordering::SeqCst);
                    }
                    delivered.push(event);
                    Ok(())
                },
            )
            .await
            .unwrap();

        assert!(observed.load(Ordering::SeqCst));
        assert_eq!(delivered, events);
    }

    #[tokio::test]
    async fn cancelling_an_in_flight_stream_rejects_late_deltas_and_completion() {
        #[derive(Default)]
        struct HoldingModel {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            late_delta_emitted: Arc<AtomicBool>,
        }

        #[async_trait]
        impl ModelClient for HoldingModel {
            async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
                Err("holding_model_complete_must_not_be_called".to_owned())
            }

            async fn complete_streaming(
                &self,
                _request: ModelRequest,
                on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
            ) -> Result<ModelOutput, String> {
                on_delta(ModelDelta::Text {
                    text: "before".to_owned(),
                })?;
                self.entered.notify_one();
                self.release.notified().await;
                on_delta(ModelDelta::Text {
                    text: "after".to_owned(),
                })?;
                self.late_delta_emitted.store(true, Ordering::SeqCst);
                Ok(ModelOutput::text("beforeafter"))
            }
        }

        let model = Arc::new(HoldingModel::default());
        let harness = Arc::new(KianaHarness::new(model.clone()));
        let run_id = RunId::new();
        let entered = model.entered.notified();
        let start_task = {
            let harness = harness.clone();
            tokio::spawn(async move { harness.send(RunnerCommand::start(run_id, "hold")).await })
        };
        entered.await;

        let cancel = harness
            .send(RunnerCommand::Cancel {
                run_id,
                reason: "user".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(
            cancel.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "cancelled:user".to_owned(),
            })
        );

        model.release.notify_one();
        let events = start_task.await.unwrap().unwrap();
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!events.iter().any(|event| matches!(
            event,
            RunnerEvent::Delta { text, .. } if text == "after"
        )));
        assert!(!model.late_delta_emitted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn sink_error_stops_the_run_without_completed() {
        let harness = KianaHarness::new(Arc::new(ChunkedModel {
            chunks: vec!["first", "second"],
        }));
        let run_id = RunId::new();
        let mut delivered = Vec::new();

        let error = harness
            .send_with_events(RunnerCommand::start(run_id, "stream it"), &mut |event| {
                let is_delta = matches!(event, RunnerEvent::Delta { .. });
                delivered.push(event);
                if is_delta {
                    Err("backpressure".to_owned())
                } else {
                    Ok(())
                }
            })
            .await
            .unwrap_err();

        match error {
            KianaHarnessError::Failed(message) => {
                assert_eq!(message, "runner_event_sink_failed:backpressure");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            delivered
                .iter()
                .filter(|event| matches!(event, RunnerEvent::Delta { .. }))
                .count(),
            1
        );
        assert!(!delivered
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn completed_event_records_optional_model_metadata() {
        let harness = scripted(json!([{
            "text": "architecture mapped",
            "usage": {"input_tokens": 12, "output_tokens": 4},
            "stop_reason": "end_turn",
            "model_id": "test-model"
        }]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();

        let RunnerEvent::Completed { output, .. } = events.last().unwrap() else {
            panic!("expected completed");
        };
        assert_eq!(output["usage"]["input_tokens"], 12);
        assert_eq!(output["usage"]["output_tokens"], 4);
        assert_eq!(output["stop_reason"], "end_turn");
        assert_eq!(output["model_id"], "test-model");
    }

    #[tokio::test]
    async fn over_budget_history_is_compacted_before_the_model_step() {
        let model = Arc::new(CapturingModel::default());
        let harness = KianaHarness::new(model.clone()).with_compact_budget(8, 4);
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "x".repeat(80)))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        let first = seen.first().expect("model saw a request");
        assert!(
            first
                .iter()
                .any(|message| crate::compact::is_summary_message(&message.text)),
            "{first:?}"
        );
        assert!(
            first
                .iter()
                .all(|message| message.role == crate::model::ModelRole::User),
            "{first:?}"
        );
        assert!(
            events.iter().any(|event| matches!(
                event,
                RunnerEvent::Compacted {
                    summary_present: true,
                    ..
                }
            )),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn shell_call_pauses_for_brokered_capability_result() {
        let harness = scripted(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "architecture mapped"}
        ]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Started { .. })));
        let request = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        assert_eq!(request.capability, CapabilityKind::Process);
        assert_eq!(request.operation, "shell.exec");
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));

        let completed = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request.request_id, json!({"stdout": "listed"})),
            })
            .await
            .unwrap();
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "architecture mapped")));
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn invalid_tool_arguments_fail_before_any_capability_request() {
        let harness = scripted(json!([
            {"text": "bad shell call", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": 42}}]}
        ]));
        let run_id = RunId::new();

        let events = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();

        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "invalid_arguments:shell:command".to_owned(),
            })
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
    }

    #[tokio::test]
    async fn serial_tools_in_one_model_step_request_one_capability_at_a_time() {
        let harness = scripted(json!([
            {"text": "two tools", "tool_calls": [
                {"id": "c1", "name": "shell", "arguments": {"command": "ls"}},
                {"id": "c2", "name": "shell", "arguments": {"command": "pwd"}}
            ]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();
        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let first_request = first
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            first
                .iter()
                .filter(|event| matches!(event, RunnerEvent::CapabilityRequested { .. }))
                .count(),
            1
        );

        let second = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(first_request.request_id, json!({"ok": 1})),
            })
            .await
            .unwrap();
        let second_request = second
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .unwrap();
        assert_ne!(first_request.request_id, second_request.request_id);
        assert!(!second
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));

        let done = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(second_request.request_id, json!({"ok": 2})),
            })
            .await
            .unwrap();
        assert!(done
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn project_root_is_copied_into_shell_capability() {
        let harness = scripted(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]}
        ]));
        let events = harness
            .send(RunnerCommand::start_in(
                RunId::new(),
                "map it",
                "/tmp/kiana-project",
                "read-only",
            ))
            .await
            .unwrap();
        let request = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        assert_eq!(request.arguments["project_root"], "/tmp/kiana-project");
        assert_eq!(request.operation, "shell.exec");
    }

    #[tokio::test]
    async fn missing_model_fails_closed() {
        let harness = KianaHarness::new(Arc::new(UnavailableModel::default()));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "hello"))
            .await
            .unwrap();
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "model_unavailable".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn next_step_inject_is_claimed_on_the_following_model_step() {
        #[derive(Debug)]
        struct CaptureModel {
            outputs: std::sync::Mutex<VecDeque<ModelOutput>>,
            seen: std::sync::Mutex<Vec<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl ModelClient for CaptureModel {
            async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
                let texts = request
                    .messages
                    .iter()
                    .map(|message| message.text.clone())
                    .collect();
                self.seen
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .push(texts);
                self.outputs
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .pop_front()
                    .ok_or_else(|| "harness_script_exhausted".to_owned())
            }
        }

        let model = Arc::new(CaptureModel {
            outputs: std::sync::Mutex::new(VecDeque::from([
                ModelOutput {
                    text: "running ls".to_owned(),
                    tool_calls: vec![ModelToolCall {
                        id: "c1".to_owned(),
                        name: "shell".to_owned(),
                        arguments: json!({ "command": "ls" }),
                    }],
                    ..ModelOutput::default()
                },
                ModelOutput::text("steered"),
            ])),
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let harness = KianaHarness::new(model.clone());
        let run_id = RunId::new();
        let started = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();
        let request = started
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        harness
            .inject(run_id, "steer: stay in the workspace")
            .unwrap();
        let completed = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request.request_id, json!({"stdout": "listed"})),
            })
            .await
            .unwrap();
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "steered")));
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[1]
                .iter()
                .any(|text| text == "steer: stay in the workspace"),
            "next-step inject must be claimed before the follow-up model call: {seen:?}"
        );
    }

    #[tokio::test]
    async fn shell_timeout_ms_is_copied_into_the_capability_request() {
        let harness = scripted(json!([{
            "text": "sleeping",
            "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "sleep 8", "timeout_ms": 250}}]
        }]));
        let events = harness
            .send(RunnerCommand::start(RunId::new(), "wait"))
            .await
            .unwrap();
        let request = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        assert_eq!(request.operation, "shell.exec");
        assert_eq!(request.arguments["timeout_ms"], 250);
        assert_eq!(request.arguments["command"], "sleep 8");
    }

    #[tokio::test]
    async fn danger_full_access_is_rejected() {
        let harness = scripted(json!([{"text": "nope"}]));
        let error = harness
            .send(RunnerCommand::start_in(
                RunId::new(),
                "hello",
                "/repo",
                "danger-full-access",
            ))
            .await
            .unwrap_err();
        match error {
            KianaHarnessError::Failed(message) => {
                assert_eq!(message, "sandbox_unsupported:danger-full-access");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn continue_appends_to_the_same_run_messages() {
        #[derive(Debug)]
        struct CaptureModel {
            outputs: std::sync::Mutex<VecDeque<ModelOutput>>,
            seen: std::sync::Mutex<Vec<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl ModelClient for CaptureModel {
            async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
                let texts = request
                    .messages
                    .iter()
                    .map(|message| message.text.clone())
                    .collect();
                self.seen
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .push(texts);
                self.outputs
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .pop_front()
                    .ok_or_else(|| "harness_script_exhausted".to_owned())
            }
        }

        let model = Arc::new(CaptureModel {
            outputs: std::sync::Mutex::new(VecDeque::from([
                ModelOutput::text("first turn"),
                ModelOutput::text("continued"),
            ])),
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let harness = KianaHarness::new(model.clone());
        let run_id = RunId::new();
        let started = harness
            .send(RunnerCommand::start(run_id, "hello"))
            .await
            .unwrap();
        assert!(started
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let continued = harness
            .send(RunnerCommand::continue_run(run_id, "keep going"))
            .await
            .unwrap();
        assert!(continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "continued")));
        assert!(continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].iter().any(|text| text == "hello"));
        assert!(seen[1].iter().any(|text| text == "hello"));
        assert!(seen[1].iter().any(|text| text == "keep going"));
    }

    #[tokio::test]
    async fn continue_unknown_run_fails_without_starting() {
        let harness = scripted(json!([{"text": "should not run"}]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::continue_run(run_id, "keep going"))
            .await
            .unwrap();
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn cancel_unknown_run_fails_closed() {
        let harness = scripted(json!([{"text": "should not run"}]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::Cancel {
                run_id,
                reason: "user".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            })
        );
    }
}
