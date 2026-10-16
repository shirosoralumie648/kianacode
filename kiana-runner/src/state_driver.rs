//! Pure Harness lifecycle driver.
//!
//! 驱动器只管理有界状态转移，不调用模型、Broker、EventLog 或文件系统。
//! 步骤/交互 ID 与效果观察通过 `DriverInput` 传入；时间和 deadline 检查留在 harness。
//! 调用方执行 I/O，并通过 ControlPlane 持久化相应的 RunnerEvent 事实。
//!
//! ===========================================================================
//! 【下面这一段是给「刚接手本 crate」的读者做的导读】
//!
//! ## 本文件在系统里的位置
//!
//! `state_driver` 是 `kiana-runner` 内部**最底层的构件**：它把「一次运行现在处于哪个阶段」
//! 这件事变成一张纯数据状态机。它自己什么都不做——不发模型请求、不写事件、不碰磁盘。
//!
//! * 上游（谁调用我）：`kiana-runner/src/harness.rs`（`use crate::state_driver::RunDriver`）。
//!   harness 在开始一轮、开一个 step、收到模型输出、收到工具结果、被取消时
//!   把这些输入交给 RunDriver 更新帧。便捷方法通常丢弃意图列表，实际 I/O 顺序
//!   由 harness 的控制流安排，并不是统一的 intent 执行器。
//! * 下游（我输出给谁）：只输出 `DriverIntent` 列表 + 新的 `RunFrame`。
//!   真正的动作由 harness 执行。
//! * 外部测试：crate 外的 `kiana-core/tests/{p0_b01_state_machine_guard,er22_cancel_recovery_guard,
//!   sc15_cancel_unknown_guard}.rs` 用 `include_str!` 读本文件源码做守卫断言。
//!
//! ## 数据流
//!
//! ```text
//!   现实世界的观察（模型回了、工具结了、用户取消了……）
//!        │  由 harness 打包成 DriverInput
//!        ▼
//!   transition(&frame, input)  ←── 纯函数：只读旧帧 + 输入，返回新帧 + 意图
//!        │
//!        ├──► Vec<DriverIntent>  ──▶ 描述阶段意图；调用方可读取，便捷方法会丢弃
//!        └──► RunFrame（新的）    ──▶ RunDriver 保存此帧，可随 checkpoint 序列化
//! ```
//!
//! ## 为什么核心是「纯函数」
//!
//! `transition` 是自由函数：签名是 `fn(&RunFrame, DriverInput) -> Result<DriverTransition, DriverError>`，
//! **既不接收 `&mut self`，也不做任何 I/O、时钟读取或 ID 分配**。
//! 这带来三个直接好处：
//!
//! 1. **可确定性测试**：同样的帧 + 同样的输入，永远得到同样的结果帧。
//!    `kiana-runner/tests/h03_state_driver.rs` 里的
//!    `stream_and_buffered_calls_share_transitions` 就是断言
//!    「走增量方法的 `RunDriver`」和「直接连续调 `transition` 自由函数」最终得到**完全相等**的帧。
//! 2. **状态机不依赖 I/O**：因此不可能在迁移过程里偷偷发一次网络请求或写一条事件。
//!    「副作用全部由 harness 在状态机之外执行」是本设计最核心的边界。
//! 3. **可以被别的层复用**：daemon / 恢复流程可以拿一个反序列化的 `RunFrame` 直接重放。
//!
//! `RunDriver::transition` 在成功时保存新帧并返回意图；begin_turn 等便捷方法只返回
//! 成功/错误，另有 validate 方法检查帧。错误不替换旧帧，但这不代表外部 I/O 会回滚。
//!
//! ## 三个必须分清的概念
//!
//! * `HarnessPhase` —— **运行到哪儿了**（阶段，可能继续推进）。
//! * `DriverTerminal` —— **怎么结束的**（终态：正常结束/失败/取消/未知）。
//! * `DriverError`   —— **这次迁移被拒绝了**（异常：状态机拒绝了你要求的迁移）。
//!   终态和错误是两回事：RequestCancel 仅进入 Cancelling，收到 StopConfirmed 后才会
//!   写入 Cancelled。DriverError 拒绝的是本次迁移，调用方要传播或处理它，不能把它
//!   自动视为已经写入了 Failed 终态。

use kiana_domain::{InteractionId, RunId, StepId, TurnId};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// schema 版本串（Serialized schema version）
// ---------------------------------------------------------------------------
//
// `RunFrame` / `TurnFrame` 会被 `KianaHarness::checkpoint()` 序列化成 JSON 存到 checkpoint 里
// （见 harness 的 checkpoint/restore 实现），也可能在重建运行时被反序列化回来。
// 一旦线上已经存在按旧格式写下的 checkpoint，后来代码改了字段，
// 就必须能**认出「这份数据是哪一版写的」**，否则会把新字段用默认值静默填成看起来合法的帧，
// 造成状态错乱却查不出来。
//
// 所以帧里带一个 `schema` 字符串，格式是 `kiana.<名字>.v<主版本>`：
// 破坏性变更升主版本号（v1 → v2），然后由 `RunDriver::validate()` 和
// `harness::restore()` 明确拒绝不匹配的版本，而不是猜测。
pub const RUN_FRAME_SCHEMA: &str = "kiana.harness-run-frame.v1";
pub const TURN_FRAME_SCHEMA: &str = "kiana.harness-turn-frame.v1";

// 输入计数上限，不是保存消息正文的 Inbox 容量。
// QueueInput 在 accepted_inputs >= mailbox_capacity 时返回 MailboxFull；BeginTurn 清零。
// 它只约束此帧的接收计数，不度量文本字节，也不证明真实队列的最坏内存占用。
pub const DEFAULT_MAILBOX_CAPACITY: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessPhase {
    Idle,
    Queued,
    Preparing,
    ModelPending,
    ToolPending,
    AwaitingApproval,
    AwaitingInput,
    Compacting,
    Cancelling,
    TurnFinished,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverTerminal {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnFrame {
    pub schema: String,
    pub turn_id: Option<TurnId>,
    pub run_id: RunId,
    pub phase: HarnessPhase,
    pub terminal: Option<DriverTerminal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunFrame {
    pub schema: String,
    pub run_id: RunId,
    pub turn: TurnFrame,
    pub phase: HarnessPhase,
    pub step_id: Option<StepId>,
    pub step: u32,
    pub pending_tools: u32,
    /// A pending question is bound to one interaction and is never an approval handle.
    #[serde(default)]
    pub pending_interaction_id: Option<InteractionId>,
    pub driver_owner: Option<String>,
    pub mailbox_capacity: u32,
    pub accepted_inputs: u64,
    pub terminal: Option<DriverTerminal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DriverInput {
    QueueInput,
    BeginTurn,
    ClaimDriver { owner: String },
    ReleaseDriver { owner: String },
    BeginStep { step_id: StepId, step: u32 },
    ModelOutput { tool_count: u32, complete: bool },
    ToolResult { effect_known: bool },
    AwaitApproval,
    AwaitClarification { interaction_id: InteractionId },
    AnswerClarification { interaction_id: InteractionId },
    RequestCancel,
    StopConfirmed,
    RecoverUnknown,
    Terminal { outcome: DriverTerminal },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverIntent {
    AcknowledgeInput,
    StartModel,
    DispatchTools,
    WaitForToolResult,
    WaitForApproval,
    WaitForInput,
    CancelPendingTools,
    Reconcile,
    EmitTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DriverError {
    #[error("harness_driver_invalid_transition:{from:?}->{to:?}")]
    InvalidTransition {
        from: HarnessPhase,
        to: HarnessPhase,
    },
    #[error("harness_driver_second_owner")]
    SecondOwner,
    #[error("harness_driver_owner_mismatch")]
    OwnerMismatch,
    #[error("harness_driver_mailbox_full")]
    MailboxFull,
    #[error("harness_driver_terminal_conflict")]
    TerminalConflict,
    #[error("harness_driver_step_invalid")]
    StepInvalid,
    #[error("harness_driver_clarification_identity_mismatch")]
    ClarificationIdentityMismatch,
    #[error("harness_driver_clarification_not_pending")]
    ClarificationNotPending,
    #[error("harness_driver_frame_invalid")]
    InvalidFrame,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DriverTransition {
    pub frame: RunFrame,
    pub intents: Vec<DriverIntent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunDriver {
    pub frame: RunFrame,
}

impl RunDriver {
    pub fn new(run_id: RunId, turn_id: Option<TurnId>) -> Self {
        let turn = TurnFrame {
            schema: TURN_FRAME_SCHEMA.to_owned(),
            turn_id,
            run_id,
            phase: HarnessPhase::Idle,
            terminal: None,
        };
        Self {
            frame: RunFrame {
                schema: RUN_FRAME_SCHEMA.to_owned(),
                run_id,
                turn,
                phase: HarnessPhase::Idle,
                step_id: None,
                step: 0,
                pending_tools: 0,
                pending_interaction_id: None,
                driver_owner: None,
                mailbox_capacity: DEFAULT_MAILBOX_CAPACITY,
                accepted_inputs: 0,
                terminal: None,
            },
        }
    }

    pub fn transition(&mut self, input: DriverInput) -> Result<Vec<DriverIntent>, DriverError> {
        let transition = transition(&self.frame, input)?;
        self.frame = transition.frame;
        Ok(transition.intents)
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        if self.frame.schema != RUN_FRAME_SCHEMA
            || self.frame.turn.schema != TURN_FRAME_SCHEMA
            || self.frame.run_id != self.frame.turn.run_id
            || self.frame.step == 0 && self.frame.step_id.is_some()
            || self.frame.pending_tools > self.frame.mailbox_capacity
            || (self.frame.phase == HarnessPhase::AwaitingInput)
                != self.frame.pending_interaction_id.is_some()
            || (self.frame.phase != HarnessPhase::AwaitingInput
                && self.frame.pending_interaction_id.is_some())
        {
            return Err(DriverError::InvalidFrame);
        }
        Ok(())
    }

    pub fn queue_input(&mut self) -> Result<(), DriverError> {
        self.transition(DriverInput::QueueInput).map(|_| ())
    }

    pub fn begin_turn(&mut self) -> Result<(), DriverError> {
        self.transition(DriverInput::BeginTurn).map(|_| ())
    }

    pub fn begin_step(&mut self, step_id: StepId, step: u32) -> Result<(), DriverError> {
        self.transition(DriverInput::BeginStep { step_id, step })
            .map(|_| ())
    }

    pub fn model_output(&mut self, tool_count: u32, complete: bool) -> Result<(), DriverError> {
        self.transition(DriverInput::ModelOutput {
            tool_count,
            complete,
        })
        .map(|_| ())
    }

    pub fn tool_result(&mut self, effect_known: bool) -> Result<(), DriverError> {
        self.transition(DriverInput::ToolResult { effect_known })
            .map(|_| ())
    }

    pub fn await_clarification(
        &mut self,
        interaction_id: InteractionId,
    ) -> Result<(), DriverError> {
        self.transition(DriverInput::AwaitClarification { interaction_id })
            .map(|_| ())
    }

    pub fn answer_clarification(
        &mut self,
        interaction_id: InteractionId,
    ) -> Result<(), DriverError> {
        self.transition(DriverInput::AnswerClarification { interaction_id })
            .map(|_| ())
    }

    pub fn claim(&mut self, owner: impl Into<String>) -> Result<(), DriverError> {
        self.transition(DriverInput::ClaimDriver {
            owner: owner.into(),
        })
        .map(|_| ())
    }

    pub fn release(&mut self, owner: impl Into<String>) -> Result<(), DriverError> {
        self.transition(DriverInput::ReleaseDriver {
            owner: owner.into(),
        })
        .map(|_| ())
    }

    pub fn request_cancel(&mut self) -> Result<(), DriverError> {
        self.transition(DriverInput::RequestCancel).map(|_| ())
    }
}

fn move_phase(
    frame: &mut RunFrame,
    phase: HarnessPhase,
    intents: &mut Vec<DriverIntent>,
) -> Result<(), DriverError> {
    if frame.phase == phase {
        return Ok(());
    }
    let allowed = matches!(
        (frame.phase, phase),
        (
            HarnessPhase::Idle,
            HarnessPhase::Queued | HarnessPhase::Preparing | HarnessPhase::Cancelling
        ) | (HarnessPhase::Queued, HarnessPhase::Preparing)
            | (HarnessPhase::Queued, HarnessPhase::Cancelling)
            | (HarnessPhase::Preparing, HarnessPhase::ModelPending)
            | (HarnessPhase::Preparing, HarnessPhase::Cancelling)
            | (HarnessPhase::ModelPending, HarnessPhase::ToolPending)
            | (HarnessPhase::ModelPending, HarnessPhase::TurnFinished)
            | (HarnessPhase::ToolPending, HarnessPhase::ModelPending)
            | (HarnessPhase::ToolPending, HarnessPhase::AwaitingApproval)
            | (HarnessPhase::ModelPending, HarnessPhase::AwaitingInput)
            | (HarnessPhase::AwaitingApproval, HarnessPhase::ModelPending)
            | (HarnessPhase::AwaitingInput, HarnessPhase::ModelPending)
            | (HarnessPhase::AwaitingApproval, HarnessPhase::Cancelling)
            | (HarnessPhase::AwaitingInput, HarnessPhase::Cancelling)
            | (HarnessPhase::ModelPending, HarnessPhase::Cancelling)
            | (HarnessPhase::ToolPending, HarnessPhase::Cancelling)
            | (HarnessPhase::Cancelling, HarnessPhase::TurnFinished)
            | (HarnessPhase::Cancelling, HarnessPhase::RecoveryRequired)
            | (HarnessPhase::TurnFinished, HarnessPhase::Preparing)
            | (HarnessPhase::ModelPending, HarnessPhase::RecoveryRequired)
            | (HarnessPhase::ToolPending, HarnessPhase::RecoveryRequired)
            | (HarnessPhase::Preparing, HarnessPhase::RecoveryRequired)
    );
    if !allowed {
        return Err(DriverError::InvalidTransition {
            from: frame.phase,
            to: phase,
        });
    }
    frame.phase = phase;
    frame.turn.phase = phase;
    intents.push(match phase {
        HarnessPhase::Preparing => DriverIntent::StartModel,
        HarnessPhase::ModelPending => DriverIntent::StartModel,
        HarnessPhase::ToolPending => DriverIntent::DispatchTools,
        HarnessPhase::AwaitingApproval => DriverIntent::WaitForApproval,
        HarnessPhase::AwaitingInput => DriverIntent::WaitForInput,
        HarnessPhase::Cancelling => DriverIntent::CancelPendingTools,
        HarnessPhase::RecoveryRequired => DriverIntent::Reconcile,
        HarnessPhase::TurnFinished => DriverIntent::EmitTerminal,
        _ => DriverIntent::AcknowledgeInput,
    });
    Ok(())
}

/// 纯状态转移：只读旧帧与输入，返回新帧及意图，不做 I/O、读时钟或分配 ID。
///
/// 在局部克隆上修改，Err 不改变传入帧；这不是授权、预算或外部副作用的事务。
/// 同一帧和输入产生相同结果，但恢复时仍须由外层核对现实执行状态。
/// RunDriver::transition 保存结果，便捷方法会丢弃意图列表；实际 I/O 由 harness 编排。
///
/// ## 主要路径
///
/// Idle 经 QueueInput 进入 Queued；BeginTurn 进入 Preparing，BeginStep 进入 ModelPending。
/// 有工具的 ModelOutput 进入 ToolPending；已知 ToolResult 会递减 pending_tools 后
/// 转到 ModelPending，并不在这里等待计数归零。未知效果转入 RecoveryRequired。
/// complete=true 且无工具时写入 Completed，阶段为 TurnFinished；此后仍可 BeginTurn。
/// 相同阶段的 move_phase 直接成功，不再生成阶段意图。
///
/// RequestCancel 与 StopConfirmed 是两个输入：前者请求停止，后者才写 Cancelled。
/// Terminal 输入可显式写终态，但不能把已有终态改成另一种结果。
///
/// ## 常见拒绝
///
/// | 错误 | 条件示例 |
/// |---|---|
/// | InvalidTransition | 阶段迁移不在允许表中，例如 Idle 收到 ToolResult |
/// | SecondOwner | owner 为空，或已有不同 owner |
/// | OwnerMismatch | release 的 owner 与当前记录不同 |
/// | MailboxFull | accepted_inputs 已达到 mailbox_capacity |
/// | StepInvalid | step 为零或不递增；此分支不校验 StepId 的值 |
/// | TerminalConflict | 已写 Completed，又收到 Terminal { outcome: Failed } |
pub fn transition(frame: &RunFrame, input: DriverInput) -> Result<DriverTransition, DriverError> {
    let mut next = frame.clone();
    let mut intents = Vec::new();
    match input {
        // ===== 输入队列相关 =====
        // QueueInput：一条消息入队（例如 steering 指令、澄清回答）。
        // 如果当前空闲，队列开始工作。
        DriverInput::QueueInput => {
            if next.accepted_inputs >= u64::from(next.mailbox_capacity) {
                return Err(DriverError::MailboxFull);
            }
            next.accepted_inputs += 1;
            if next.phase == HarnessPhase::Idle {
                move_phase(&mut next, HarnessPhase::Queued, &mut intents)?;
            }
            intents.push(DriverIntent::AcknowledgeInput);
        }

        // ===== Turn 生命周期 =====
        // BeginTurn：开始新一轮模型调用。重置各种计数器和临时状态。
        DriverInput::BeginTurn => {
            next.accepted_inputs = 0; // 清空本 turn 的输入计数
            next.pending_tools = 0; // 清空待执行工具数
            next.step_id = None;
            next.step = 0;
            next.pending_interaction_id = None; // 清空待澄清的问题
            next.terminal = None;
            next.turn.terminal = None;
            move_phase(&mut next, HarnessPhase::Preparing, &mut intents)?;
        }

        // ===== Driver 所有权（多方竞争的仲裁）=====
        // ClaimDriver：一个所有者声明「我要独占这个 driver 的执行权」。
        // 同一所有者多次 claim 是允许的（幂等），但不同所有者不行。
        DriverInput::ClaimDriver { owner } => {
            if owner.trim().is_empty() {
                return Err(DriverError::SecondOwner);
            }
            if let Some(current) = &next.driver_owner {
                if current != &owner {
                    return Err(DriverError::SecondOwner); // 已被他人 claim
                }
            } else {
                next.driver_owner = Some(owner); // 首次 claim
            }
        }

        // ReleaseDriver：所有者释放执行权。必须是真正的所有者。
        DriverInput::ReleaseDriver { owner } => {
            if next.driver_owner.as_deref() != Some(owner.as_str()) {
                return Err(DriverError::OwnerMismatch); // 释放者必须与记录的 owner 一致
            }
            next.driver_owner = None;
        }

        // ===== Step 执行 =====
        // BeginStep：记录传入的 StepId，只检查 step 数字为正且递增。
        DriverInput::BeginStep { step_id, step } => {
            if step == 0 || (next.step != 0 && step <= next.step) {
                return Err(DriverError::StepInvalid); // step number 必须递增
            }
            next.step_id = Some(step_id);
            next.step = step;
            move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
        }

        // ===== 模型输出 =====
        // ModelOutput：模型响应了。检查是否有工具调用、是否完成。
        DriverInput::ModelOutput {
            tool_count,
            complete,
        } => {
            // 不能同时「有工具调用」和「已完成」
            if complete && tool_count != 0 {
                return Err(DriverError::InvalidTransition {
                    from: next.phase,
                    to: HarnessPhase::TurnFinished,
                });
            }
            if tool_count > 0 {
                // 有工具要执行，等待工具结果
                next.pending_tools = tool_count;
                move_phase(&mut next, HarnessPhase::ToolPending, &mut intents)?;
            } else if complete {
                // 没有工具，且标记了完成 → turn 结束
                next.pending_tools = 0;
                next.terminal = Some(DriverTerminal::Completed);
                next.turn.terminal = next.terminal;
                move_phase(&mut next, HarnessPhase::TurnFinished, &mut intents)?;
            } else {
                // 没有工具、也没完成 → 保持/进入 ModelPending；同阶段不新增意图
                move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
            }
        }

        // ===== 工具结果 =====
        // ToolResult：一个工具执行完了。如果不知道副作用，进入恢复模式；
        // 否则递减计数器，继续问模型。
        DriverInput::ToolResult { effect_known } => {
            if !effect_known {
                // 不知道有没有执行 → 需要恢复
                move_phase(&mut next, HarnessPhase::RecoveryRequired, &mut intents)?;
            } else {
                // 副作用确认 → 递减待处理工具数
                next.pending_tools = next.pending_tools.saturating_sub(1);
                move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
            }
        }

        // ===== 流程控制：等待 =====
        // AwaitApproval：等待 ControlPlane 的核准（例如「允许我执行这个能力吗」）。
        DriverInput::AwaitApproval => {
            move_phase(&mut next, HarnessPhase::AwaitingApproval, &mut intents)?;
        }

        // AwaitClarification：模型有个问题要问用户。保存问题 ID。
        DriverInput::AwaitClarification { interaction_id } => {
            if interaction_id.as_uuid().is_nil()
                || next.pending_interaction_id.is_some()
                || !matches!(next.phase, HarnessPhase::ModelPending)
            {
                return Err(DriverError::ClarificationIdentityMismatch);
            }
            next.pending_interaction_id = Some(interaction_id);
            move_phase(&mut next, HarnessPhase::AwaitingInput, &mut intents)?;
        }
        DriverInput::AnswerClarification { interaction_id } => {
            if next.phase != HarnessPhase::AwaitingInput
                || next.pending_interaction_id != Some(interaction_id)
            {
                return Err(DriverError::ClarificationNotPending);
            }
            next.pending_interaction_id = None;
            move_phase(&mut next, HarnessPhase::ModelPending, &mut intents)?;
        }
        DriverInput::RequestCancel => {
            next.pending_interaction_id = None;
            move_phase(&mut next, HarnessPhase::Cancelling, &mut intents)?;
        }
        DriverInput::StopConfirmed => {
            if next.phase != HarnessPhase::Cancelling {
                return Err(DriverError::InvalidTransition {
                    from: next.phase,
                    to: HarnessPhase::TurnFinished,
                });
            }
            next.terminal = Some(DriverTerminal::Cancelled);
            next.turn.terminal = next.terminal;
            next.pending_interaction_id = None;
            move_phase(&mut next, HarnessPhase::TurnFinished, &mut intents)?;
        }
        DriverInput::RecoverUnknown => {
            move_phase(&mut next, HarnessPhase::RecoveryRequired, &mut intents)?;
        }
        DriverInput::Terminal { outcome } => {
            if next.terminal.is_some_and(|current| current != outcome) {
                return Err(DriverError::TerminalConflict);
            }
            next.terminal = Some(outcome);
            next.turn.terminal = Some(outcome);
            next.pending_interaction_id = None;
            if next.phase != HarnessPhase::TurnFinished {
                next.phase = HarnessPhase::TurnFinished;
                next.turn.phase = HarnessPhase::TurnFinished;
                intents.push(DriverIntent::EmitTerminal);
            }
        }
    }
    Ok(DriverTransition {
        frame: next,
        intents,
    })
}
