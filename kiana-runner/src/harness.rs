//! 进程内运行的 Agent harness（Agent 执行框架）。
//!
//! 循环与收件箱语义参考 DeepSeek Harness（MIT）的
//! `packages/core/agent-loop` 和 `packages/core/agent`；模型可见的工具名称
//! 与消息项形状参考 OpenAI Codex（Apache-2.0）的 `codex-rs/exec` 和
//! `codex-rs/protocol`（例如 `shell`、`apply_patch`）。这里的“参考”说明设计来源，
//! 不表示运行时会调用这些项目的代码。
//!
//! 当前实现属于 Kiana workspace；`reference/` 只作为审计材料，不是 workspace 成员。
//! 工具不会在本文件中执行：模型输出先转成 `CapabilityRequest`，交给控制面和
//! capability broker（能力代理）完成授权与执行。
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
//! ```text
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
//! * **上游调用者（已核实）**：生产环境由 `kiana-daemon::DaemonHost` 的装配代码
//!   （包括 `configured_env_harness()`）构造 `KianaHarness`；Core 和 Daemon 的测试
//!   也会直接构造它。`kiana-core::ControlPlane` 通过本文件的
//!   `impl RunnerPort for KianaHarness` 发送命令；启动、继续、注入和取消由
//!   `kiana-core::lifecycle` 驱动，能力结果由 `kiana-core::dispatch` 回灌，
//!   检查点保存与恢复由 `kiana-core::recovery` 驱动。
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
//! Core 使用 `RunnerPort::send` / `send_with_events` 进入 trait 实现，二者都转调私有
//! `dispatch`。`KianaHarness` 还提供同名的公开便捷方法，供进程内调用和测试使用。
//! `dispatch` 把 `RunnerCommand` 分派到五类处理函数：
//!
//! | RunnerCommand | 处理函数 | 语义 |
//! |---|---|---|
//! | `Start` | `start` | 建立新的 run 状态、整理上下文并运行首个模型步 |
//! | `CapabilityResult` | `on_capability_result` | 接收 Core/Broker 的工具结果并推进已挂起的 run |
//! | `Inject` | `enqueue_injected` | 把外部消息按目标轮次或步骤放进收件箱 |
//! | `Continue` | `continue_run` | 在已有 run 上开始一个新 turn |
//! | `Cancel` | `cancel` | 请求停止运行中的 run，或终止已挂起的 run |
//!
//! ---------------------------------------------------------------------------
//! 【数据如何流过】
//! ---------------------------------------------------------------------------
//! ```text
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
//! 因为执行权不在本文件。模型步发出 `CapabilityRequested` 后会停止本地循环；
//! 命令结束时由 `store_unless_terminal` 把仍可继续的 `ActiveRun` 放回内存 `runs` 表。
//! 能力请求事件只携带请求单，**不会包含整个 run 状态**。若需要崩溃恢复，Core 会另外
//! 通过 `RunnerPort::checkpoint` 获取快照并交给恢复路径管理。
//! ControlPlane 收到请求后进行授权并交给 Broker，再把 `CapabilityResult` 作为新命令送回。
//! 只有结果通过 `on_capability_result` 校验后，模型循环才继续。
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
    ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelRole, ModelToolCall,
    ScriptedModel, UnavailableModel,
};
use crate::progress::ProgressTracker;
use crate::state_driver::{DriverInput, DriverTerminal, RunDriver};
use crate::stream_normalizer::ModelStreamAccumulator;
use crate::tools::{capability_for_tool_with_request_id, tool_schemas};
use async_trait::async_trait;
use kiana_domain::{
    derived_request_id, json_digest, redact_text, scan_secret_sentinels, AttemptId,
    CapabilityResult, InputId, ModelAttemptId, ModelAttemptIdentity, PromptBundle,
    QuotaReservationId, RequestId, RetryAttemptReservation, RunId, SecretScanChannel, StepId,
    StepIdentity, StreamingRedactionSink, StreamingRedactor, TokenAccounting, ToolObservation,
    ToolObservationStatus, TurnId, WireBudget,
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
/// `store_unless_terminal` 就靠**精确字符串相等**判断
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
    /// 精确语义见 `record_tool_call`：只有工具名和规范化后的参数都与上一条相同，
    /// 才会增加连续次数；参数有变化就从 1 重新计数。
    pub repeated_tool_call_threshold: u32,
    /// 单个 run 的**墙钟预算**：从 `start` 起经过多久就强制停止。
    ///
    /// **为什么是 `Option<Duration>` 而不是 `Duration`**：
    /// `None` 表示不启用这个 harness 级显式上限；模型分配和统一预算仍可收紧时间，
    /// 且 `invoke_model` 在两者都未指定时仍有 300 秒的模型调用兜底。测试用的
    /// `ScriptedModel` 是本地回放，通常毫秒级完成，因此不必由测试重复设置此项；
    /// 生产 daemon 可以显式设置。
    /// 另外注意 `Duration` 是**相对时长**而不是绝对时间戳——
    /// `Instant` 本身不能稳定写进 checkpoint，因此 `HarnessCheckpoint` 只保存已经
    /// 经过的毫秒数；恢复时再以当前单调时钟重建一个等价起点。
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
/// Kiana 的 run 是**可挂起**的：跑到一半要等 Broker 回话时，模型步骤会返回，
/// `store_unless_terminal` 将仍可继续的 `ActiveRun` 留在 `KianaHarness.runs` 内存表中；
/// 上层把工具结果作为新命令送回来后，`take_run` 再取出同一份状态。需要跨进程恢复时，
/// `RunnerPort::checkpoint` 会把受支持状态另行编码为 `HarnessCheckpoint`，再由 Core 恢复流程使用。
/// 因此这个结构体必须**完整到足以自洽地暂停和恢复一个 run**——
/// 少一个字段，恢复出来的 run 就和崩溃前不是同一个 run。
///
/// 【为什么用 `Mutex<HashMap<RunId, ActiveRun>>` 而不是每 run 一个 task】
/// 因为 run 的生命周期**跨命令**。`Start` 命令结束时它可能还没跑完
/// （挂起等能力结果），此时没有 task 持有它了。
/// 用表来存，才能在下一条 `CapabilityResult` 命令里找回来。
/// 代价是**单个 run 的状态不能被两个命令同时改**——
/// `take_run` 就是「把整个 run 原子地从表里摘走」的动作，
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
    /// 由 `model_step_once` 在每一步开始时新建，
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
    /// 服务端拥有的待澄清问题会随同一个 run 跨越 checkpoint/restore 保存。
    /// 待澄清的问题。`Option` = 当前没有。
    ///
    /// 【为什么它能跨 checkpoint/restore 存活】
    /// 因为它在 `HarnessCheckpoint` 里，而且**权限属于服务端**：
    /// 模型既不能读它也不能改它，只能等上层把答案通过 inject 送进来。
    pending_clarification: Option<kiana_domain::ClarificationRequest>,
    /// 沙箱档位，只可能是 `"read-only"` 或 `"workspace-write"`。
    ///
    /// 经过 `normalize_sandbox` 归一化，未知值直接拒绝，
    /// 所以这里没有「脏值」。它**只是参数上下文，不是授权凭证**——
    /// ControlPlane 和 Broker 会独立再验一次。
    sandbox: String,
    /// 项目根目录。**同样只是参数上下文**，
    /// 会被抄进 `shell` 工具的 `CapabilityRequest` 里，
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
    /// 解码后的结构化 PromptBundle；只有 bundle 路径（instructions 是 JSON 编码的
    /// `PromptBundle`）才有。每个模型步用它编译一次不可变的 `ContextPlan` /
    /// `ResolvedStepContext`，使估算、发送与证据解释复用同一个对象；legacy 纯文本
    /// instructions 没有 bundle，runner 侧不编译上下文（见 `step_context` 模块文档）。
    prompt_bundle: Option<PromptBundle>,
    /// 控制面下发的模型分配（model assignment）：角色、配额、模型路由权限。
    ///
    /// **由 `bind_model_assignment` 在 run 启动前单独写入**，
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
    /// `model_route_changed_during_run`。
    model_route: Option<kiana_domain::ModelRoute>,
    /// 本次模型步要执行的工具队列（先进先出）。
    ///
    /// **一个模型步里的多个工具在这里排队，但只发一个**：
    /// `emit_tool_request` 永远只看 `front()`，
    /// 要等前一个的 `CapabilityResult` 回来（`on_capability_result`）
    /// 才发下一个。测试
    /// `serial_tools_in_one_model_step_request_one_capability_at_a_time` 这个测试
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
    /// 每次模型步都和当前摘要比对，不一致就报
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
    /// 要能各自拿到同一份（`cancel` 会从 `in_flight` 表取出它的 clone）。
    cancellation: Arc<RunCancellation>,
}

/// 一个待处理工具调用的阶段。
///
/// 【为什么是三态而不是布尔 `in_flight`】
/// 区分「还没发出去」和「发出去了但没回话」至关重要：
/// 前者可以安全地取消（证明没执行过），
/// 后者**不能**假设没执行 —— `cancel` 会把队首视作可能已经交给 ControlPlane/Broker 的调用。
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
    /// `restore` 会显式拒绝它。
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
    /// 快照结构版本。写死 `"kiana.harness-checkpoint.v1"`，由 `restore` 校验。
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
    /// `Instant::now().checked_sub(elapsed)` 把它还原成一个「等价的起点」。
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

/// Kiana 规范 Agent loop 的进程内适配器与运行状态所有者。
///
/// 【它在系统中的位置】
/// `ControlPlane` 通过 `RunnerPort` 把 `Start`、`Continue`、`Inject`、`Cancel`
/// 和 `CapabilityResult` 命令交给这里；这里调用 `ModelClient` 取得模型输出，
/// 把工具意图包装成 `RunnerEvent::CapabilityRequested`，再把事件交还 Core。
/// 具体工具权限判断和副作用执行位于 Core 与 Capability Broker，因此持有本类型
/// 并不等于获得文件、网络或其他工具的执行权限。
///
/// 【为什么状态按 run 保存】
/// 一条命令可能在工具请求发出后暂停，待 Core/Broker 回灌结果时再继续。
/// `runs` 保存这种跨命令的完整会话状态；`in_flight` 只暂存当前仍在执行中的
/// 取消句柄；`deferred_inputs` 收集模型步执行期间到达、尚不能安全并入消息列表的输入。
/// `assignments` 与 `histories` 是启动前由上层绑定的待消费数据，`start` 会按
/// `run_id` 取走一次，防止模型输出反过来伪造角色或历史。
///
/// 【主要下游】
/// `ModelClient` 负责模型调用；`crate::budget` 记录预算预留与结算；`crate::inbox`、
/// `crate::state_driver` 管理输入目标与合法状态转换；`crate::tools` 只构造能力请求；
/// Core 后续通过事件处理器把能力结果送回来。
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
    /// 使用默认运行参数创建 harness。
    ///
    /// 【调用者】生产装配通常由 `kiana-daemon::DaemonHost` 创建；测试也会注入
    /// `ScriptedModel` 等确定性模型。
    ///
    /// 【输入与输出】接收一个实现 `ModelClient` 的共享对象，返回保存该对象和运行状态的
    /// harness。默认参数来自 `RuntimeConfig::default()`。
    ///
    /// 【失败与副作用】构造过程本身不发网络请求或创建 run；后续 `send` 才会驱动模型。
    /// 配置若含无效预算，会在执行预算检查时以结构化错误拒绝，而不是在构造时 panic。
    pub fn new(model: Arc<dyn ModelClient>) -> Self {
        Self::with_config(model, RuntimeConfig::default())
    }

    /// 用显式运行参数创建 harness，并在后续执行时使用这些上限。
    ///
    /// 【输入】`model` 是模型调用边界；`config` 包含每轮步数、压缩阈值、重复工具调用
    /// 门槛、墙钟和统一预算。
    ///
    /// 【为什么保存预算校验错误】无效预算会被记录在 `budget_config_error`，
    /// 之后由 `effective_budget` 在 run 执行路径中转换为失败事件。这样构造 API 不需
    /// 改成可失败签名，同时错误也不会被忽略后继续无预算运行。
    ///
    /// 【并发状态】每个 run、启动前分配、历史、执行中的取消句柄和延迟输入分别放入
    /// 互斥保护的表中；同一个 run 的状态会先从 `runs` 表取出，再由单个命令独占修改。
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

    /// 返回当前已生效的 harness 级配置快照。
    ///
    /// 该快照不包含某个 run 的 `ModelAssignment` 限额；run 执行时，
    /// `effective_budget` 还会把 assignment 中更严格的额度与本配置取交集。
    /// 调用者得到的是按值复制的参数，不会拿到内部可变状态的引用。
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

    /// 将 harness 默认预算与当前 run 的模型分配限制合并成实际预算。
    ///
    /// 【调用者】每个模型步检查额度、模型请求预留和工具请求预留都会通过此方法取预算。
    ///
    /// 【输入与输出】输入是当前 run（可选带有服务端绑定的 `ModelAssignment`）；成功时
    /// 返回生效的 `HarnessBudgetConfig`。每个上限都取较小值，因此 run 只能进一步收紧
    /// harness 限制，不能通过 assignment 放宽运行时上限。
    ///
    /// 【失败情况】初始配置或 assignment 无效时返回稳定错误字符串；上层把它记录为
    /// `RunnerEvent::Failed`，并停止当前模型步。此处不向 provider 发请求。
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

    /// 为预算账本生成项目与角色维度的隔离键。
    ///
    /// 模型 token、调用次数等累计额度不能只按进程全局统计，否则一个项目或角色的使用
    /// 会错误消耗另一个项目或角色的预算。没有 assignment 时使用 `unassigned`，使该类
    /// run 仍能被单独统计。
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

    /// 从 harness 专用环境变量装配模型实现。
    ///
    /// 当 `ENV_HARNESS_SCRIPT` 指向有效 JSON cassette（录制输出回放文件）时，使用
    /// `ScriptedModel` 按文件内容回放；未设置、为空或无法读取时使用
    /// `UnavailableModel`。因此这个入口适合本地 harness 回放或显式测试配置，
    /// 不会因为环境变量缺失而悄悄创建一个真实 provider 连接。
    /// 若需要线上模型，生产装配由 `DaemonHost` 把已配置的 `ModelClient` 注入构造函数。
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

    /// 把本地 runner 输入排入该 run 的下一个模型步骤。
    ///
    /// 【调用者与数据流】这是进程内便捷 API；协议命令 `RunnerCommand::Inject` 经
    /// `dispatch` 后进入 `enqueue_injected`。调用本方法时，文本被包装为 runner 来源的
    /// `InboxMessage`，再交给 `insert_next_step` / `enqueue_injected` 放入收件箱。
    /// 它只确认输入已排队，不会绕过 ControlPlane，也不会直接唤醒空闲 run；run 下次
    /// 进入 `model_step_once` 时才会认领并并入模型消息。
    ///
    /// 【失败情况】空白文本、run 不存在、锁中毒或输入目标检查失败时返回
    /// `KianaHarnessError`。该方法不直接调用模型或执行工具。
    pub fn inject(&self, run_id: RunId, text: impl Into<String>) -> Result<(), KianaHarnessError> {
        self.insert_next_step(run_id, text)
    }

    /// 将引导文本排入与 `inject` 相同的下一步骤收件箱。
    ///
    /// 两个便捷方法当前共用 `insert_next_step`，不会创建额外的并行执行循环或
    /// 并发唤醒命令。如果 run 正在等待 capability result，文本会等待该结果回灌后
    /// 的下一次 `model_step`；这一顺序保证模型上下文只由当前 run 的驱动器串行修改。
    pub fn steer(&self, run_id: RunId, text: impl Into<String>) -> Result<(), KianaHarnessError> {
        self.insert_next_step(run_id, text)
    }

    /// 把带协议身份和目标的 Inject 命令写入正确的收件箱。
    ///
    /// `RunnerCommand::Inject` 的字段由上游 ControlPlane/协议适配器提供。仍在 `runs`
    /// 表中的 run 可以立即写入其 Inbox；若 run 暂时从表中取出、正在执行命令，则先放进
    /// `deferred_inputs`，由下一次模型步开始前归并。这样注入不会与正在修改的消息列表
    /// 并发竞争。run 既不在活动表也不在 `in_flight` 表时返回 `run_not_found`。
    /// 成功只表示排队，不表示模型已经看到这条消息。
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

    /// 为便捷 `inject` / `steer` API 生成下一步骤输入并放入收件箱。
    ///
    /// 当前有 turn 的已存储 run 会将消息绑定到该 turn；没有 turn 的旧式 run 或正处于
    /// `in_flight` 状态的 run 则通过 runner 来源与新 `InputId` 交给 `enqueue_injected`。
    /// 先检查 run 是否存在再决定写入位置，是为了不把拼错的 `run_id` 静默保存成永远
    /// 不会被消费的延迟输入。
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

    /// 处理一条 Runner 命令并收集本次产生的事件。
    ///
    /// 【调用者】生产路径由 `RunnerPort::send` trait 实现委托到这里；进程内调用和测试
    /// 也可使用该公开方法。
    ///
    /// 【输入与输出】接收版本化 RunnerCommand，返回处理期间收集到的 `RunnerEvent`。
    /// 事件可能包括 Started、Delta、CapabilityRequested、Completed 或 Failed；工具请求
    /// 只作为事件交回 Core，并不在这里执行。
    ///
    /// 【副作用】Start/Continue 会调用注入的 `ModelClient`，并更新内存中的 run 状态；
    /// 事件最终交给调用方。本方法本身不绕过 Core 去调用 capability handler。
    ///
    /// 【失败情况】业务拒绝通常表现为返回成功的事件列表，其中包含 `Failed`；锁中毒、
    /// 下游 sink 失败等 harness 基础设施错误则通过 `KianaHarnessError` 返回。调用方负责
    /// 将错误映射到 `PortError` 或协议响应。
    pub async fn send(
        &self,
        command: RunnerCommand,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        self.dispatch(command, None).await
    }

    /// 发送命令，并在每个协议事件产生时同步交付给 `sink`。
    ///
    /// 【调用者】需要边执行边向上游转发 Delta 等事件的 `RunnerPort::send_with_events`
    /// 会进入本方法；典型消费者是负责事件传输或 UI 呈现的 adapter。
    ///
    /// 【输入与输出】输入除了 RunnerCommand 以外还有同步事件回调 `sink`。成功时回调
    /// 收到的事件序列与返回的 `Vec<RunnerEvent>` 逐项一致；因此返回值仍保留完整事件，
    /// 回调则用于即时交付。
    ///
    /// 【副作用与失败】事件会先交付给 `sink`，之后再进入返回列表。`sink` 返回错误表示
    /// 下游取消或背压，harness 立即停止并以 `runner_event_sink_failed:*` 返回；已经交付
    /// 的事件无法撤回，之后不会再产生新事件。
    pub async fn send_with_events(
        &self,
        command: RunnerCommand,
        sink: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        self.dispatch(command, Some(sink)).await
    }

    /// 按命令变体选择处理函数，并用同一个 EventEmitter 收集或流式发送事件。
    ///
    /// 这是两个 `send` 入口的共同分派点，避免普通调用与流式调用产生不同业务流程。
    /// `Inject` 的 wire 字符串目标会先转换为内部 enum；非法目标生成 `Failed` 事件并
    /// 返回事件列表。合法目标再由 `enqueue_injected` 保存。其他命令分别进入启动、能力
    /// 结果回灌、继续或取消路径。
    ///
    /// `project_trusted` 字段不会由 harness 作为授权依据；项目可信度必须由 Daemon/Core
    /// 的服务端信任链确认。此处只整理 runner 所需的上下文参数。
    /// 业务失败多以 `RunnerEvent::Failed` 返回，真正的 emitter、锁或状态驱动错误则作为
    /// `KianaHarnessError` 传播。
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
            RunnerCommand::Resume { run_id } => self.resume(run_id, &mut emitter).await,
            RunnerCommand::Cancel { run_id, reason } => self.cancel(run_id, reason, &mut emitter),
        }?;
        Ok(emitter.into_events())
    }

    /// 根据 Start 命令建立新的 run，并开始执行第一个模型步骤。
    ///
    /// 【调用者】只能由 `dispatch` 在收到 `RunnerCommand::Start` 后调用；生产命令由
    /// `ControlPlane` 经 `RunnerPort` 发送。
    ///
    /// 【输入】`StartInput` 汇集 run/turn 身份、用户提示、可选历史、项目根路径、沙箱档位、
    /// system instructions 和本次 turn 的步数上限。这些字段是运行上下文，不是授权凭证。
    /// `ModelAssignment` 与精确历史若已由 Core 预先绑定，会按 `run_id` 从对应表中取走。
    ///
    /// 【核心流程与数据流】先脱敏用户提示并校验上限、身份唯一性和沙箱；再建立
    /// `ActiveRun` / `RunDriver`，装配 PromptBundle 来源、system/context、历史和用户消息；
    /// 最后注册取消句柄、发出 Started，并进入 `model_step`。之后模型结果变为 Delta、
    /// CapabilityRequested 或终态事件返回给 Core。
    ///
    /// 【副作用与失败】会调用 `ModelClient` 并更新 harness 内存状态，但不会直接执行工具。
    /// 参数或状态拒绝通常发出 `RunnerEvent::Failed`；锁、状态驱动器和事件 sink 等基础设施
    /// 失败会作为 `KianaHarnessError` 传播。若 Started 已经交付，后续错误也不会撤回该事件。
    ///
    /// 【设计原因】历史中的旧 Tool 项没有协议级调用身份，不能伪造成可重放的工具调用；
    /// 因而它们只作为标明“无重放授权”的不可信上下文传给模型。
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
            prompt_bundle: None,
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
                run.prompt_bundle = Some(bundle.clone());
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
                // 旧式进程内 instructions 只是提示文本，不携带角色、授权或资源写集。
                // 仍加入产品 system prompt 是为了保留旧调用方的基本运行约束；真正权限仍由 Core/Broker 判定。
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
            // Wire v1 历史没有 assistant tool-call 声明与对应 call id。
            // 将旧工具观测标记为不可信的上下文，不能为其伪造协议身份，否则模型可能把
            // 一段历史文字误当成当前可重放、可授权的真实工具结果。
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

    /// 消费 Core/Broker 回灌的工具结果，并决定继续同一批工具还是开始下一个模型步。
    ///
    /// 【调用者】Core 的 `deliver_capability_result` 在 capability 执行结果已被归一化并
    /// 记录后，通过 `RunnerCommand::CapabilityResult` 调用此路径。
    ///
    /// 【输入与身份校验】run 从 `runs` 表中取出；传入 `CapabilityResult.request_id` 必须
    /// 等于队首待处理请求，且该请求阶段必须是 `Dispatched`。这些检查确保迟到、重复或
    /// 错配的结果不会被贴到另一条工具调用上。
    ///
    /// 【核心流程】先将队首标记为 Settled 并检查效果是否已知；未知效果直接失败，因为
    /// 外部副作用可能已经发生，不能自动重试。已知结果经 `ToolObservation` 转成模型可读
    /// 文本并追加到消息历史；若还有排队调用就串行发出下一条能力请求，否则回到
    /// `model_step_in_flight` 继续模型循环。
    ///
    /// 【失败情况】无对应请求、阶段错误、request id 不一致都会产生 `Failed`；Denied、
    /// 未开始的取消、未知观测也会终止本次 run，不会向模型自动重试被拒绝的工具。
    /// 结构化结果转换、状态驱动和事件交付失败则通过 `KianaHarnessError` 传播。
    /// Core 根据这些事件及 EventStore 记录决定最终外部响应和 Receipt。
    async fn on_capability_result(
        &self,
        run_id: RunId,
        result: CapabilityResult,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let mut run = self.take_run(run_id)?;
        let Some(expected) = run.pending_tools.front() else {
            // 注意：`take_run` 已经先把状态从 runs 表移走；这个协议错误分支不会重新存回。
            // 正常 Core 流程只会对确有待处理请求的 run 回灌结果。若有其他调用方直接发送
            // 多余的 CapabilityResult，这里除返回 Failed 外还会丢弃该 run 的可继续状态。
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

    /// 在已存在的 run 上开始一个新的 turn，并保留之前累积的对话上下文。
    ///
    /// 【调用者】`dispatch` 收到 `RunnerCommand::Continue` 后调用；上游的 Core 生命周期
    /// 路径会先确认该 run 当前允许继续。
    ///
    /// 【输入与输出】输入是已有 `run_id` 和新的用户提示。提示先经过 prompt 通道脱敏，再
    /// 作为 `NextTurn` inbox 消息认领并追加到模型历史；返回事件通过同一个 emitter 回到 Core。
    ///
    /// 【状态变化】仅当 run 没有尚未结算的工具请求时才继续；新 turn 会重置本 turn 的步数、
    /// step id、墙钟起点、最后文本与重复工具调用指纹。预算账本仍以 project/role scope
    /// 累计，不能把“新 turn”理解成清空已经结算的模型或工具用量。
    ///
    /// 【失败情况】空提示、run 不存在或仍有待处理工具会产生 `Failed` 事件；状态驱动、
    /// inbox、锁或事件交付失败作为 `KianaHarnessError` 返回。它不会创建新的 run id。
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

    /// Continue a checkpoint that ControlPlane has explicitly restored and claimed.
    ///
    /// Resume deliberately has no prompt or authority-bearing fields. The restored active run
    /// is the sole source of model context; a run with a pending tool is refused because its
    /// external outcome requires reconciliation before another model step.
    async fn resume(
        &self,
        run_id: RunId,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let mut run = match self.take_run(run_id) {
            Ok(run) => run,
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => {
                return emitter.emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "result_unknown:resume_state_missing".to_owned(),
                });
            }
            Err(error) => return Err(error),
        };
        if !run.pending_tools.is_empty() {
            self.store_unless_terminal(run, &[])?;
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "result_unknown:resume_pending_tool".to_owned(),
            });
        }
        self.model_step_in_flight(&mut run, emitter).await?;
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    /// 请求停止当前 run；对已经挂起的 run 同时标明哪些工具可以证明尚未派发。
    ///
    /// 【调用者】Core 生命周期中的取消流程通过 `RunnerCommand::Cancel` 到达这里。Core
    /// 还会独立撤销审批、记录取消状态并收集停止证明，因此本事件本身不等于“外部副作用
    /// 已停止”的证明。
    ///
    /// 【两条路径】如果 run 仍在 `in_flight` 表，向共享 `RunCancellation` 发布取消信号，
    /// 让正在等待的异步模型调用尽快退出。若 run 已挂起在 `runs` 表，则从状态表取出，
    /// 将队首调用视为可能已交给 Core/Broker 的未知状态；只为其余 `Queued` 调用发出
    /// `ToolCancelled { not_executed: true }`，随后发出 Failed 取消事件。
    ///
    /// 【容易误改】不能把队首也报告为“未执行”：能力请求事件可能已经被 Core 消费，
    /// Broker 可能已产生副作用。若停止状态无法证明，Core 必须保留 `result_unknown` 语义。
    /// run 不存在时返回 `run_not_found` 事件；取消句柄、锁或状态驱动失败会传播为错误。
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
                // 队首调用可能已经由 ControlPlane 接收并开始派发。
                // 这里只能证明后续尚在本地队列中的调用还没有发出去。
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

    /// 以唯一的 `model-driver` 所有者身份串行驱动当前 run 的模型循环。
    ///
    /// 每次模型输出如果要求继续（例如收件箱在本步骤边界新增了输入），本方法会继续
    /// 调用 `model_step_once`；如果步骤完成、run 挂起等工具回灌或已经失败，则停止循环。
    /// `RunDriver::claim/release` 用来拒绝同一 run 被两个命令同时推进，尤其防止两个
    /// 异步任务同时消费同一条工具结果或同时修改消息历史。
    ///
    /// 即使某个模型步返回错误，也会尝试释放 driver 所有权。若步骤和释放都成功则返回成功；
    /// 若步骤失败，保留步骤错误供调用者记录；释放失败本身也会作为错误返回。
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

    /// 执行一个模型步骤：检查边界、准备上下文、调用模型并解释输出。
    ///
    /// 【调用者】由 `model_step` 串行循环调用；`Start`、`Continue` 和工具结果回灌都会
    /// 最终进入这条路径。
    ///
    /// 【核心流程】依次检查取消、turn 步数、墙钟、项目/角色预算和工具目录摘要；把执行中
    /// 延迟到达的输入并入 inbox；为本步骤分配 `StepId`；认领只投递到当前步骤的消息；按
    /// token 预算压缩历史；再将消息与模型工具 schema 交给 `invoke_model`。输出会被脱敏并
    /// 追加到历史，工具调用则变成 `CapabilityRequested` 事件交回 Core。
    ///
    /// 【返回语义】`Continue` 表示当前 run 可在本地立即再执行一个模型步；`Finished` 只
    /// 表示本轮本地驱动要停，可能是终态，也可能是已发出能力请求、等待 Core/Broker 回灌。
    /// 具体区别看发出的事件及 `store_unless_terminal` 是否保留 run。
    ///
    /// 【失败情况】超预算、工具目录在运行中改变、消息目标 turn 不一致、模型拒绝/截断/
    /// 不完整输出、工具形状非法或 sink 失败都会停止当前驱动。涉及预算或模型结果的不确定
    /// 性必须保留其错误类别，不能在这里改写成可安全重试的普通错误。
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
            run.driver
                .transition(DriverInput::Terminal {
                    outcome: DriverTerminal::Failed,
                })
                .map_err(|error| KianaHarnessError::Failed(error.to_string()))?;
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
        // 延迟输入在本步骤边界统一并入 inbox，避免模型客户端正在读取 messages 时，
        // 另一条 Inject 命令并发修改同一个 Vec。turn/step 目标校验和实际消费发生在下方。
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
        if let Some(error) = compact.error {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error,
            })?;
            return Ok(StepProgress::Finished);
        }

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
        // `invoke_model` 通过 ModelClient 的 complete_streaming 回调发送文本 Delta；trait 的
        // 默认实现会把非空完整文本作为一个 Delta，原生流式实现则发送分块。这里固定为 true，
        // 避免把最终 `output.text` 再发一次重复 Delta；实现 ModelClient 时必须遵守该回调契约。
        let emitted_delta = true;
        if let Err(error) = &output {
            if error.starts_with("runner_event_sink_failed:") {
                return Err(KianaHarnessError::Failed(error.clone()));
            }
        }
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
            // 只有本步没有工具调用、也没有刚到达的 NextStep 输入时，纯文本输出才结束当前 turn。
            // 若遗漏此检查，模型生成文本期间到达的用户指引会留在收件箱里，run 却先被标成完成，
            // 调用方只能另开命令才能处理这条本应紧接着生效的输入。
            let deferred = self
                .deferred_inputs
                .lock()
                .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
                .remove(&run_id)
                .unwrap_or_default();
            let mut deferred_next_step = false;
            for (target, message) in deferred {
                if target == InboxTarget::NextStep {
                    deferred_next_step = true;
                }
                run.inbox
                    .insert_for_run(run_id, target, message)
                    .map_err(KianaHarnessError::Failed)?;
            }
            if run.inbox.next_step.is_empty() && !deferred_next_step {
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
        if let Err(error) = kiana_domain::validate_model_calls(&output.tool_calls)
            .map_err(|error| error.to_string())
            .and_then(|()| kiana_domain::plan_tool_batch(&output.tool_calls).map(|_| ()))
        {
            emitter.replace_event_since(checkpoint, RunnerEvent::Failed { run_id, error })?;
            return Ok(StepProgress::Finished);
        }
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

    /// 为明确选择修复结构化输出的调用方执行单独的 OutputRepair 模型请求。
    ///
    /// 普通解析路径不会在 provider parser 内部偷偷循环修复。需要修复的上层必须显式调用
    /// 这个入口；修复请求作为独立 `ModelPurpose::OutputRepair` attempt 纳入预算，并写入
    /// ModelTurn 元数据，方便审计“任务回答”与“格式修复”各自花费了什么。
    /// 本入口目前没有普通主路径调用者，保留它不表示每次格式错误都会自动重试。
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

    /// 以预算、稳定身份和审计记录为边界调用模型；一次逻辑调用中可能包含受控重试。
    ///
    /// 【调用者与下游】任务请求由 `model_step_once` 发起；显式结构化输出修复则经
    /// `invoke_output_repair` 使用相同执行边界。`ModelClient::prepare_call` 解析并固定
    /// provider/model 路由，之后 `complete_prepared_cancellable`（或带准入许可的变体）
    /// 才真正调用模型。
    ///
    /// 【数据流】输入 `ModelRequest` 含整理后的消息、模型可见工具 schema 和沙箱上下文；
    /// 本函数生成 call/attempt 身份，检查项目/角色预算，预留 attempt 与可选准入许可，
    /// 然后将 `ModelDelta` 送入流累加器和秘密文本脱敏器。安全的文本增量发为 Runner Delta；
    /// 最终回复、用量、路由、重试判定和结果摘要进入 `RunnerEvent::ModelTurn`，成功时返回
    /// 规范化后的 `ModelOutput` 给步骤处理逻辑。
    ///
    /// 【为什么重试有条件】网络超时不能证明 provider 没收到请求。每次调用先登记独立
    /// reservation；若请求可能已送达或副作用状态未知，就把 attempt 记为 unknown。只有
    /// retry 模块认定错误可恢复、尚未向用户交付 Delta 且 deadline 仍有余量时，才等待退避
    /// 后重试。取消信号会参与准入等待、provider 等待和退避等待。
    ///
    /// 【失败与副作用】配置、锁、预算、时钟、路由、准入、provider、事件 sink、用量结算或
    /// stream normalization 失败都会返回错误给 `model_step_once`，由它转成失败事件。
    /// provider 可能已经收到请求，所以错误不总表示“什么都没发生”；unknown attempt 会保留
    /// 在审计与预算记录里。本函数只调用模型，不执行模型发出的工具。
    async fn invoke_model(
        &self,
        run: &mut ActiveRun,
        request: ModelRequest,
        purpose: kiana_domain::ModelPurpose,
        format: kiana_domain::ModelResponseFormat,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<ModelOutput, String> {
        use crate::retry::{
            classify_retry, is_safe_to_retry, retry_delay, unsupported_recovery_reason,
            MAX_PROVIDER_ATTEMPTS,
        };
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
        // 选择最严格的墙钟上限：优先显式 harness 配置，其次服务端下发的 model assignment；
        // 两者均未设置时以 300 秒（5 分钟）兜底，防止模型/重试路径无限等待。
        // `remaining` 会从 run 起点扣除已耗时，因此准入等待、模型调用和退避共用同一份余量。
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
            let prepared_route_digest = prepared.route.digest();
            // bundle 路径：用同一步身份、消息快照和 wire 预算编译不可变上下文，然后对照
            // 本 attempt 实际 route/catalog/workspace/data 复核。路由摘要取已钉住的 route
            // （首次即本 attempt 的 route）；不一致时在预留预算和发送之前拒绝，避免 route
            // 变化后仍把旧计划当作同一快照发出去。legacy/纯文本路径保持既有行为。
            let step_context_explanation = if let Some(bundle) = run.prompt_bundle.as_ref() {
                let wire_bytes = serde_json::to_vec(&prepared.wire_body)
                    .map_err(|_| "step_context_wire_invalid".to_owned())?
                    .len() as u64;
                let wire_budget = WireBudget::from_final_wire(
                    wire_bytes,
                    prepared.budget.reserved_output,
                    prepared.budget.limit,
                    prepared.budget.limit,
                    TokenAccounting::ConservativeUtf8 {
                        bytes_per_token: 1,
                        safety_margin_tokens: 1,
                    },
                )
                .map_err(|error| format!("step_context_wire_budget:{error}"))?;
                let resolved = crate::step_context::compile_step_context(
                    crate::step_context::StepContextInputs {
                        bundle,
                        messages: &prepared.request.messages,
                        sandbox: &prepared.request.sandbox,
                        step_identity: step_identity.clone(),
                        model_profile: bundle.model_profile.clone(),
                        route_digest: run
                            .model_route
                            .as_ref()
                            .map(|route| route.digest())
                            .unwrap_or_else(|| prepared_route_digest.clone()),
                        catalog_digest: prepared.tool_catalog_hash.clone(),
                        wire_budget,
                    },
                )
                .map_err(|error| format!("step_context_compile:{error}"))?;
                crate::step_context::recheck_step_context_bindings(
                    &resolved,
                    &prepared_route_digest,
                    &prepared.tool_catalog_hash,
                    &prepared.request.messages,
                    &prepared.request.sandbox,
                )?;
                Some(
                    serde_json::to_value(
                        crate::step_context::StepContextExplanation::from_resolved(&resolved),
                    )
                    .map_err(|_| "step_context_explanation_invalid".to_owned())?,
                )
            } else {
                None
            };
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
            // 这条 reservation 是单次模型 attempt 的边界记录。先标为已派发再进入 ModelClient；
            // 后续只能结算为已知用量或标记结果未知，避免超时后把可能已被 provider 执行的请求
            // 误记成“从未发送”。
            attempt_reservation.dispatch()?;
            // The transcript stream is a published signal boundary: bind the channel profile so its
            // digest travels with the run and a stream error cannot fall back to raw model text.
            let redactor = StreamingRedactor::new();
            let mut redactor = StreamingRedactionSink::from_redactor(
                redactor,
                kiana_domain::RedactionProfile::for_channel(SecretScanChannel::Transcript),
                SecretScanChannel::Transcript,
            )?;
            let mut stream = ModelStreamAccumulator::new(model_attempt_id);
            let run_id = run.run_id;
            let mut observed_delta = false;
            let mut sink_error: Option<String> = None;
            let mut callback = |delta: ModelDelta| -> Result<(), String> {
                if let Some(error) = sink_error.as_ref() {
                    return Err(error.clone());
                }
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
                let text = redactor.push(&text)?;
                if text.is_empty() {
                    return Ok(());
                }
                let delivered = emitter.emit(RunnerEvent::Delta { run_id, text });
                if let Err(error) = &delivered {
                    sink_error = Some(error.clone());
                }
                delivered
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
            if let Some(error) = sink_error {
                return Err(error);
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
            // 单独构造预算对象，避免再把 16 个键值内联进外层 `json!`。
            // `json!` 是 serde_json 的编译期宏，宏展开规模超过默认递归限制时编译会失败；
            // 拆开后字段与最终 JSON 内容不变，只是分开构造，控制单次宏展开的规模。
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
                "step_context":step_context_explanation,
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
                    let tail = redactor.finish()?;
                    if !tail.tail.is_empty() {
                        emitter.emit(RunnerEvent::Delta {
                            run_id,
                            text: tail.tail,
                        })?;
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
                Err(error) => {
                    if let Some(reason) = unsupported_recovery_reason(&error) {
                        return Err(reason.to_owned());
                    }
                    return Err(error.to_string());
                }
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

    /// 在 run 运行期间登记取消句柄，并在模型驱动结束后清除登记。
    ///
    /// `in_flight` 表使另一条 Cancel 命令能在不持有 run 可变借用的情况下拿到同一个
    /// `Arc<RunCancellation>` 并通知正在等待的异步任务。先登记再驱动、结束后再注销，
    /// 使取消请求能区分“仍在执行”与“已挂起保存在 runs 表”两种情况。
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

    /// 根据本次事件决定是否把 run 留在内存表中供后续命令继续。
    ///
    /// 任意非墙钟预算 `Failed` 都按终态处理，不再保存状态；墙钟预算超时只结束当前 turn，
    /// 所以仍保留 run，让上层之后显式发送 Continue。没有终态失败时（包括已发出
    /// CapabilityRequested、等待 Broker 的情况）把同一 `run_id` 的状态放回 `runs` 表。
    /// 这个规则是“超时可继续、其他明确失败不继续”的 runner 生命周期边界。
    fn store_unless_terminal(
        &self,
        run: ActiveRun,
        events: &[RunnerEvent],
    ) -> Result<(), KianaHarnessError> {
        // 墙钟预算错误只结束当前 turn；Continue 会为新 turn 重新计时，
        // 所以保留 run 供调用方显式继续。其他 Failed 都按终态处理并丢弃此内存状态。
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

    /// 把队列中的一个模型工具意图转换成发给 Core 的能力请求事件。
    ///
    /// 【调用者】模型输出被校验并排入 `pending_tools` 后调用；能力结果回灌后若队列仍有
    /// 下一项，也由 `on_capability_result` 串行调用。
    ///
    /// 【数据流】从队首取得稳定 request id，用 `crate::tools` 将模型 `ModelToolCall` 映射为
    /// `CapabilityRequest`；附加服务端 turn/step 身份、当前待处理批次摘要和可信 PromptBundle
    /// 中的扩展范围；预留一次工具调用预算；最后把队首阶段从 Queued 改为 Dispatched 并返回
    /// `RunnerEvent::CapabilityRequested`。Core 收到事件后才进行 policy/gate/approval，
    /// Broker 才可能执行 handler。
    ///
    /// 【失败情况】重复工具空转、空队列、缺少 step 身份、参数映射失败、预算不足、可信提示包
    /// 损坏或请求身份/阶段不一致时返回错误给调用者；调用者将错误变成 Failed 事件。
    /// 这里生成的是申请单，成功返回不代表已授权或执行成功。
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
        // 扩展来源范围只从 daemon 加载的不可变 system PromptBundle 读取。
        // 即使可信范围为空，也覆盖模型参数中同名字段，防止模型自行声明扩展来源。
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

    /// 检查模型是否连续重复请求完全相同的工具操作。
    ///
    /// 比较前会递归排序 JSON 对象键，避免 `{a:1,b:2}` 与 `{b:2,a:1}` 仅因字段顺序不同
    /// 而被误认为不同参数。只有工具名和规范化参数同时匹配上一条调用才增加计数；新操作
    /// 会重新从 1 计数。达到 `repeated_tool_call_threshold` 时返回稳定错误码，阻止下一次
    /// capability request，防止模型在没有推进工作的情况下持续消耗预算。
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

/// 按指定输出通道脱敏文本，并在秘密标记仍可识别时整体替换为固定占位文本。
///
/// Prompt、Transcript 等通道有不同扫描规则。先 `redact_text` 再扫描，可兼顾可读的局部脱敏和
/// 失败关闭：若脱敏后的内容仍命中秘密 sentinel，就不返回原始文本。调用方将结果用于模型
/// 上下文或事件；此 helper 不记录日志，也不改变调用方的错误路径。
fn safe_channel_text(channel: SecretScanChannel, text: &str) -> String {
    let redacted = redact_text(text);
    if scan_secret_sentinels(channel, &redacted).is_ok() {
        redacted
    } else {
        "[REDACTED]".to_owned()
    }
}

impl From<KianaHarnessError> for PortError {
    /// 将 runner 错误映射为 Core 依赖的端口错误类别。
    ///
    /// 错误消息保持原样，以便调用链上的日志、事件或协议适配器保留稳定错误码；这里只做
    /// 类型边界转换，不把执行结果重解释为成功或安全可重试。
    fn from(error: KianaHarnessError) -> Self {
        match error {
            KianaHarnessError::Unavailable(message) => PortError::Unavailable(message),
            KianaHarnessError::Failed(message) => PortError::Failed(message),
        }
    }
}

#[async_trait]
impl RunnerPort for KianaHarness {
    /// 在 run 启动前绑定由 ControlPlane 生成的服务端模型分配。
    ///
    /// 分配中包含 run/turn、角色、模型配置与运行预算；模型文本没有写入此表的路径。
    /// 绑定前会校验结构与 run id，并拒绝在 run 已开始或已有不同分配时覆盖它，避免一条
    /// run 在执行中更换权限或额度。`start` 按 run id 消费这份待绑定数据。
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

    /// 在 run 启动前绑定 Core 已验证的精确模型历史。
    ///
    /// 历史会按 run id 暂存，并在 `start` 时优先于兼容 wire v1 的简化历史使用。此接口
    /// 再次验证消息结构，并拒绝 system 消息，防止调用方通过“历史”覆写 system prompt 所代表的
    /// 服务端指令。已启动/挂起的 run 或不一致的重复绑定都会返回冲突错误。
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

    /// 安装唯一的模型准入与用量结算端口。
    ///
    /// `invoke_model` 在调用 provider 前可通过该端口申请 admission permit，并在每次 attempt
    /// 结束后结算用量。这里只接受一次安装，避免同一个 harness 在运行期间切换计费或准入权威。
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

    /// 将一个已挂起且当前不在执行的 run 编码成版本化 JSON checkpoint。
    ///
    /// 【调用者】Core 的恢复/审批持久化流程经 `RunnerPort` 请求快照；模型运行中的 run
    /// 不允许在并发修改状态时拍快照。
    ///
    /// 【核心流程】拒绝 in-flight 或已取消 run，验证进度状态；复制消息历史，对精确匹配
    /// `PromptBundle` typed encoder 输出的 system message 做规范化，然后将 run id、turn/step、
    /// driver、inbox、assignment、pending tools、预算步数和已用墙钟时间写入
    /// `HarnessCheckpoint`。规范化只发生在 checkpoint 副本上，实时消息不变。
    ///
    /// 【失败情况与安全边界】run 不存在、正在执行、已取消、内部状态无效或 JSON 编码失败时
    /// 返回 `PortError`。无法证明安全的提示包保持原样，让 Core 的脱敏/可恢复性检查继续拒绝；
    /// 返回 checkpoint 只表示状态可编码，不证明 provider 或现实副作用结果正确。
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
        let mut messages = run.messages.clone();
        for message in &mut messages {
            if message.role != crate::model::ModelRole::System
                || !message.content.is_empty()
                || !message.tool_calls.is_empty()
                || message.tool_call_id.is_some()
            {
                continue;
            }
            let Ok(bundle) = PromptBundle::decode(&message.text) else {
                continue;
            };
            // 只有经过类型解码后再次编码仍逐字相同的 PromptBundle，才允许规范化 JSON 格式。
            // 带未知/重复字段、结构损坏或只是普通 JSON 文本的内容必须原样留下，
            // 以便 Core 的脱敏一致性检查继续拒绝无法安全规范化的快照。
            if bundle.encode().ok().as_deref() != Some(message.text.as_str()) {
                continue;
            }
            // serde_json::Value 的序列化格式与通用 JSON 脱敏器采用的规范格式一致。
            // 这里只改 checkpoint 副本，不改当前运行中的 system message；PromptBundle 内
            // 如果仍含秘密，Core 对比安全副本与原始副本时仍会把快照标记为不可恢复。
            message.text = serde_json::to_value(bundle)
                .map_err(|error| {
                    PortError::Failed(format!("runner_checkpoint_prompt_invalid:{error}"))
                })?
                .to_string();
        }
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
            messages,
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

    /// 验证外部 checkpoint 并将其重新装载为尚未开始推进的 `ActiveRun`。
    ///
    /// checkpoint 来自恢复持久化边界，必须视为不可信输入：先检查 schema、run/turn/step
    /// 一致性、步数上限、inbox 和状态驱动器，再确认工具目录摘要仍匹配当前版本。所有待处理
    /// 工具都会重新校验唯一 call id、稳定 invocation request id、sandbox 与请求形状；已经
    /// Settled 的工具不允许仍留在 pending 队列中。
    ///
    /// 墙钟耗时会重新变成相对当前单调时钟的起点。只有全部检查通过且 harness 中不存在同 id
    /// 的 run/in-flight 执行时，才把状态放入 `runs` 表。此方法不调用模型，也不重新派发工具；
    /// 后续仍需由 Core 发起 Continue 或回灌 CapabilityResult。
    ///
    /// 任一身份、结构、工具目录、预算、sandbox 或重复 run 检查失败都会返回 `PortError`，
    /// 不会留下部分恢复的 run。
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
        let prompt_bundle = decode_prompt_bundle_from_messages(&checkpoint.messages);
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
                prompt_bundle,
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

/// 递归生成对象键排序后的 JSON 字符串，供重复调用检测比较参数语义。
///
/// JSON 对象字段不依赖顺序；直接序列化可能让同一组参数因为键顺序不同而产生不同指纹。
/// 此函数排序对象键并递归规范化其值，但保持数组元素顺序，因为数组顺序通常有业务含义。
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

/// 将外部 sandbox 标签归一到 harness 当前认识的档位，并拒绝未知标签。
///
/// 空字符串按默认只读档处理；其余只接受 `read-only` 和 `workspace-write`。返回的只是
/// `CapabilityRequest` 上下文，不是授权证明；Core 与 Broker 仍会按服务端 scope 再次核验。
fn normalize_sandbox(sandbox: &str) -> Result<&str, KianaHarnessError> {
    match sandbox.trim() {
        "" | DEFAULT_HARNESS_SANDBOX => Ok(DEFAULT_HARNESS_SANDBOX),
        HARNESS_SANDBOX_WORKSPACE_WRITE => Ok(HARNESS_SANDBOX_WORKSPACE_WRITE),
        other => Err(KianaHarnessError::Failed(format!(
            "sandbox_unsupported:{other}"
        ))),
    }
}

/// 从恢复的消息历史里重新解码 PromptBundle（只有 bundle 路径的 run 才有）。
///
/// 只信任消息本身携带的规范化系统消息：解码失败（legacy 纯文本产品提示、被篡改的 JSON）
/// 一律返回 `None`，不发明 bundle、也不从快照的其它字段推断上下文。
fn decode_prompt_bundle_from_messages(messages: &[ModelMessage]) -> Option<PromptBundle> {
    messages.iter().find_map(|message| {
        if message.role != ModelRole::System {
            return None;
        }
        PromptBundle::decode(&message.text).ok()
    })
}

/// 为模型输出中的一个工具调用构造可重复得到的 request id。
///
/// 同一 run/turn/step 中相同 assistant item 和 ordinal 必须得到相同身份，否则审批恢复或
/// checkpoint 重放可能把原请求变成新的 invocation。旧式缺失 turn/step 的 run 以 run id
/// 派生兼容身份；完整身份则由 `stable_invocation_request_id_parts` 统一编码。
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

/// 使用完整的 run/turn/step/tool-call 身份生成命名空间化 request id。
///
/// `ordinal` 区分同一模型消息中的多个工具调用；身份分量供 checkpoint 校验与 Core 的
/// invocation resume binding 对照。它是身份生成，不代表请求已通过授权。
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

    async fn checkpoint_prompt_fixture() -> (KianaHarness, RunId, String) {
        let harness = scripted(json!([{
            "text":"inspect",
            "tool_calls":[{"id":"checkpoint-call","name":"memory.search",
                "arguments":{"query":"checkpoint"}}]
        }]));
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        let role = kiana_domain::RoleSpec::builder();
        harness
            .bind_model_assignment(
                run_id,
                kiana_domain::ModelAssignment {
                    schema: "kiana.model-assignment.v1".to_owned(),
                    run_id,
                    turn_id,
                    role_id: role.role_id.clone(),
                    role_version: Some(role.version),
                    catalog_version: Some(kiana_domain::SchemaVersion::new(1, 0)),
                    prompt_hash: Some(role.prompt_hash.clone()),
                    input_schema: Some(role.input_schema.clone()),
                    output_schema: Some(role.output_schema.clone()),
                    profile: role.model_profile.clone(),
                    project_root: "/checkpoint-format-fixture".to_owned(),
                    project_trusted: true,
                    authority_revision: None,
                    max_wall_time_ms: 60_000,
                    runtime_budget: None,
                },
            )
            .unwrap();
        let encoded = PromptBundle::for_role(&role).encode().unwrap();
        let events = harness
            .send(RunnerCommand::start_in_with_history_and_turn(
                run_id,
                "pause",
                Vec::new(),
                "/checkpoint-format-fixture",
                "read-only",
                encoded.clone(),
                true,
                role.max_steps,
                Some(turn_id),
            ))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
        (harness, run_id, encoded)
    }

    #[tokio::test]
    async fn checkpoint_prompt_bundle_is_canonical_without_changing_live_input() {
        let (harness, run_id, encoded) = checkpoint_prompt_fixture().await;
        let canonical = serde_json::from_str::<Value>(&encoded).unwrap().to_string();
        assert_ne!(
            encoded, canonical,
            "fixture must exercise JSON field ordering"
        );
        let checkpoint = harness.checkpoint(run_id).await.unwrap();
        let text = checkpoint["messages"][0]["text"].as_str().unwrap();
        assert!(
            text == canonical,
            "checkpoint must serialize the same typed bundle canonically"
        );
        assert!(
            kiana_domain::redact_value(&checkpoint) == checkpoint,
            "nonsecret typed checkpoint must survive generic redaction unchanged"
        );
        PromptBundle::decode(text).unwrap();
        assert!(
            harness.runs.lock().unwrap().get(&run_id).unwrap().messages[0].text == encoded,
            "checkpoint formatting must leave the live model input intact"
        );
        let restored = scripted(json!([]));
        restored.restore(run_id, checkpoint).await.unwrap();
    }

    #[tokio::test]
    async fn checkpoint_prompt_normalization_preserves_secret_and_malformed_input_denials() {
        let (harness, run_id, encoded) = checkpoint_prompt_fixture().await;
        let mut nested_secret = PromptBundle::for_role(&kiana_domain::RoleSpec::builder());
        nested_secret.sections.push(kiana_domain::PromptSection {
            name: "fixture_context".to_owned(),
            order: 200,
            text: r#"{"api_key":"checkpoint-nested-private"}"#.to_owned(),
            source: "fixture:context".to_owned(),
            authority: kiana_domain::PromptAuthority::Context,
        });
        let duplicate = encoded.replacen('{', "{\"schema\":\"kiana.prompt-bundle.v1\",", 1);
        let wrong_case = encoded.replacen("\"schema\"", "\"SCHEMA\"", 1);
        let ordinary_secret = "ordinary API_KEY=checkpoint-uppercase-private".to_owned();
        let nested_encoded = nested_secret.encode().unwrap();
        for (label, input, valid_bundle) in [
            ("duplicate_key", duplicate, false),
            ("case_sensitive_field", wrong_case, false),
            ("ordinary_secret", ordinary_secret, false),
            ("nested_secret", nested_encoded.clone(), true),
        ] {
            harness
                .runs
                .lock()
                .unwrap()
                .get_mut(&run_id)
                .unwrap()
                .messages[0]
                .text = input.clone();
            let checkpoint = harness.checkpoint(run_id).await.unwrap();
            let text = checkpoint["messages"][0]["text"].as_str().unwrap();
            if valid_bundle {
                assert!(
                    serde_json::from_str::<Value>(text).unwrap()
                        == serde_json::from_str::<Value>(&input).unwrap(),
                    "{label}"
                );
            } else {
                assert!(
                    text == input,
                    "invalid or non-bundle input must not be reinterpreted: {label}"
                );
            }
            assert!(
                kiana_domain::redact_value(&checkpoint) != checkpoint,
                "Core must retain its non-resumable redaction denial: {label}"
            );
        }
        harness
            .runs
            .lock()
            .unwrap()
            .get_mut(&run_id)
            .unwrap()
            .messages[0]
            .text = encoded;
        let untrusted_json = "{\n  \"message\": \"same data\", \"other\": true\n}";
        harness
            .runs
            .lock()
            .unwrap()
            .get_mut(&run_id)
            .unwrap()
            .messages[1]
            .text = untrusted_json.to_owned();
        let checkpoint = harness.checkpoint(run_id).await.unwrap();
        assert!(checkpoint["messages"][1]["text"] == untrusted_json);
        assert!(
            kiana_domain::redact_value(&checkpoint) != checkpoint,
            "ordinary JSON text must not gain the typed prompt exception"
        );
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
    async fn legacy_start_instructions_remain_untrusted_user_context() {
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
        assert_eq!(first[0].text, kiana_domain::PRODUCT_SYSTEM_PROMPT);
        assert!(
            first
                .iter()
                .any(|message| message.role == crate::model::ModelRole::User
                    && message.text.contains("code03-harness-demo")),
            "{first:?}"
        );
        assert!(
            !first
                .iter()
                .any(|message| message.role == crate::model::ModelRole::System
                    && message.text.contains("code03-harness-demo")),
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
        assert_eq!(
            delivered.len(),
            2,
            "sink failure must stop all later delivery"
        );
        assert!(matches!(delivered[0], RunnerEvent::Started { .. }));
        assert!(matches!(delivered[1], RunnerEvent::Delta { .. }));
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
    async fn impossible_compaction_budget_stops_before_any_model_call() {
        let model = Arc::new(CapturingModel::default());
        let harness = KianaHarness::new(model.clone()).with_compact_budget(1, 0);
        let events = harness
            .send(RunnerCommand::start(RunId::new(), "long goal".repeat(100)))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { error, .. }
            if error == "compact_latest_group_exceeds_budget")));
        assert!(!events.iter().any(|event| matches!(
            event,
            RunnerEvent::CapabilityRequested { .. } | RunnerEvent::Completed { .. }
        )));
        assert!(model.seen.lock().unwrap().is_empty());
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
