//! Query 状态机的纯状态转移 reducer。
//!
//! 【文件职责与系统位置】
//! 本文件定义一份局部 Query 状态、一组已发生事件，以及 reducer 要求外层执行的 effect。
//! 它的工作只是根据“当前状态 + 新事件”计算“新状态 + 下一步意图”；它不调用模型、不运行
//! 工具、不执行停止钩子，也不把状态写入 EventLog。
//!
//! 预期调用链：
//! QueryState + QueryEvent
//!     ↓ `transition_query_state()` 校验当前 phase 是否接受该事件
//! 新 QueryState + 有序 QueryEffect 列表
//!     ↓ 外层适配器执行被允许的 effect
//! 已观察到的结果再包装成下一个 QueryEvent
//!
//! 【调用关系证据】仓内对 `transition_query_state()` 的直接调用目前仅在本文件测试；crate 根
//! 对外重导出函数不等于生产路径已经接线。不要把这套局部 reducer 当作 Kiana 的授权权威、
//! EventLog 事实源或主执行循环；所有真实副作用仍须由产品主路径的受控执行器完成。
//!
//! serde 派生由 serde 宏提供 Rust 值与外部数据格式之间的转换能力。事件和 effect 使用 tagged
//! enum，在序列化时附加类型标记，便于调用边界还原变体；序列化本身不会执行 effect。

use crate::config::QueryConfig;
use crate::token_budget::TokenBudgetDecision;
use kiana_types::ids::SessionId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// reducer 当前持有的一次查询状态快照。
///
/// 【作用】记录这份局部状态机所在阶段、会话、模型轮次、尚待返回的工具结果、token 计数和
/// 结束信息。reducer 每次都消费一个值并返回更新后的新值。
/// 【权威边界】它只是 Query reducer 的计算输入/输出，不是 EventLog 的替代品，也不证明模型、
/// 工具或 hook 的副作用已经发生；执行者必须先真实执行并观察结果，再提交对应事件。
/// `Serialize` / `Deserialize` 只提供数据转换，不会自动持久化它。
pub struct QueryState {
    /// 状态所属的会话 ID；Start 事件会用当前 `QueryConfig` 中的 ID 更新此字段。
    pub session_id: SessionId,
    /// 当前等待哪个外部步骤或是否已经结束；允许的事件由 reducer 中的 phase/event 表决定。
    pub phase: QueryPhase,
    /// 当前/下一次请求模型使用的轮次编号。首个请求使用 0；工具结果、hook 修复或预算续跑
    /// 路径会递增它，且使用饱和加法避免整数溢出。
    pub turn_index: u32,
    /// 最近一次模型响应声明的待完成工具调用数量；后续事件只提交数量，不带调用 ID，
    /// 因此外层必须保证计数对应当前响应中的新完成结果。
    pub pending_tool_uses: usize,
    /// reducer 收到的输入 token 增量累计值；使用饱和加法，达到 u64 上限后不再精确增长。
    pub input_tokens: u64,
    /// reducer 收到的输出 token 增量累计值；同样使用饱和加法，不能作为账单级精确计费凭证。
    pub output_tokens: u64,
    /// 最近预算决定报告的续跑次数；这是统计字段，不负责发起模型请求。
    pub budget_continuations: u32,
    /// 已结束时的停止原因；未结束时为 None。该字段供上层投影/展示，不表示外部系统已记录收据。
    pub stop_reason: Option<QueryStopReason>,
    /// Fail/Abort 事件携带的最近错误或原因；Abort 可没有原因。它是展示信息，不是授权拒绝证据。
    pub last_error: Option<String>,
}

impl QueryState {
    /// 根据显式配置创建尚未启动的初始状态。
    ///
    /// 【调用者】状态机创建方；当前仓内直接调用见本文件测试。
    /// 【输入】`QueryConfig` 提供会话 ID；四个 gate 当前不会复制进状态逻辑。
    /// 【输出】phase 为 Idle、轮次/计数为 0、没有停止原因或错误的 QueryState。
    /// 【副作用/失败】无 I/O，不返回错误；创建状态不会请求模型。
    /// 【下一步】只有 Idle 状态收到 `QueryEvent::Start` 后，reducer 才返回首个
    /// `RequestModel { turn_index: 0 }` effect。
    pub fn new(config: &QueryConfig) -> Self {
        Self {
            session_id: config.session_id.clone(),
            phase: QueryPhase::Idle,
            turn_index: 0,
            pending_tool_uses: 0,
            input_tokens: 0,
            output_tokens: 0,
            budget_continuations: 0,
            stop_reason: None,
            last_error: None,
        }
    }

    /// 判断是否处于 Completed、Failed 或 Aborted 这三种终态之一。
    /// 终态收到任意迟到事件时，reducer 会原样保留状态并返回 `Ignored`。
    pub fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 当前局部查询状态机所在阶段。
///
/// 事件只有在指定阶段才推进流程；Complete、Fail 和 Abort 可从任一非终态进入结束分支。
/// 这些阶段是 reducer 的控制标签，不是持久生命周期事实。
pub enum QueryPhase {
    /// 状态已创建，但外层尚未提交启动事件。
    Idle,
    /// 外层已收到 RequestModel 意图并发起请求，等待模型结果事件。
    WaitingForModel,
    /// reducer 已要求外层处理工具，等待工具结果数量归零后再请求下一轮模型。
    WaitingForTools,
    /// 外层负责执行停止钩子并把阻断/禁止继续结果送回。
    RunningStopHooks,
    /// 外层负责检查预算；reducer 等待预算决定事件，不会自行读取 token tracker。
    CheckingBudget,
    /// 正常完成，终态。
    Completed,
    /// 发生明确错误，终态。
    Failed,
    /// 被取消或中止，终态。
    Aborted,
}

impl QueryPhase {
    /// 判断阶段是否为终态；终态收到迟到事件时 reducer 会返回 `Ignored`。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            QueryPhase::Completed | QueryPhase::Failed | QueryPhase::Aborted
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// 外层执行器向 reducer 提交的已观察结果或生命周期信号。
///
/// `type` 是 serde 为 tagged enum 写入的变体标记；`rename_all` 把 Rust 变体名编码为
/// snake_case。事件只描述输入事实，不直接授权副作用。工具和模型的真实调用必须在外层受控
/// 执行后再提交结果；仅构造一个事件不会证明相应操作实际发生。
pub enum QueryEvent {
    /// 从 Idle 启动状态机；reducer 返回首轮模型请求 effect。
    Start,
    /// 外层已获得模型响应，并汇报工具数量和本次 token 增量。
    /// reducer 累计 token；有工具时转入 WaitingForTools，无工具时先转入停止钩子阶段。
    ModelResponseReceived {
        /// 本次模型响应中的工具调用数量；调用方必须只报告这份响应的调用。
        tool_use_count: usize,
        /// 本次响应新增的输入 token 数，累加到状态中的总值。
        input_tokens: u64,
        /// 本次响应新增的输出 token 数，累加到状态中的总值。
        output_tokens: u64,
    },
    /// 外层已完成并提交一批工具结果；该事件用数量表示进度，而不是逐项结果身份。
    ToolResultsSubmitted {
        /// 本批新完成结果的数量。若同一结果被重复计数或传入超额数量，本 reducer 无法凭 ID 去重。
        tool_result_count: usize,
    },
    /// 外层停止钩子执行结束后报告其反馈。
    StopHooksFinished {
        /// 非空时优先作为消息反馈给模型，即使 `prevent_continuation` 同时为 true 也是如此。
        blocking_errors: Vec<String>,
        /// 没有阻断错误时，true 会让 reducer 直接完成并记录 StopHookPrevented。
        prevent_continuation: bool,
    },
    /// 外层已经算出预算决定；reducer 只消费结果并选择继续或完成。
    TokenBudgetChecked {
        /// 预算模块给出的下一步决定。
        decision: TokenBudgetDecision,
    },
    /// 外层报告本状态机可以关闭；提供的 reason 会直接写入状态并形成 Finish effect。
    /// reducer 不校验该 reason 是否与 Completed 阶段语义一致。
    Complete {
        /// 结束原因。
        stop_reason: QueryStopReason,
    },
    /// 外层报告失败；错误文本进入状态的 last_error，停止原因为 Failed。
    Fail {
        /// 供状态/展示使用的稳定或已清理错误文本；不应放入未清理秘密。
        error: String,
    },
    /// 外层请求取消或中止；终态状态码固定为 Aborted。
    Abort {
        /// 可选展示原因；None 表示没有原因文本，不会阻止进入 Aborted。
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 查询状态机最终结束时使用的停止原因分类。
///
/// 这是稳定的枚举标签，不包含完整错误详情；其值会写入本地 QueryState，并由调用方决定是否
/// 投影到其他记录。枚举本身不写 EventLog 或收据。
pub enum QueryStopReason {
    /// 业务正常完成，或预算 Stop 没有附带 completion event 时的默认映射；后一种情况不必然
    /// 证明模型已完成任务，只表示输入决定没有可保存的预算摘要。
    Complete,
    /// 预算 decision 附带完成统计时，reducer 以此表示预算线或收益递减停止。
    TokenBudget,
    /// 停止钩子在没有阻断错误待反馈时禁止继续。
    StopHookPrevented,
    /// 收到 Abort 事件。
    Aborted,
    /// 收到 Fail 事件。
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// reducer 返回给外层的副作用意图。
///
/// 这些值是数据，不是执行记录。外层必须解释并按顺序处理；特别是请求模型、执行工具、运行
/// hook、追加消息都不能在序列化/展示层直接执行，也不能绕过产品主路径的授权控制。
pub enum QueryEffect {
    /// 请求外层适配器发送指定模型轮次；序号由 reducer 计算，不表示该请求已成功。
    RequestModel { turn_index: u32 },
    /// 要求外层处理当前响应报告的工具调用数；具体调用身份与权限不在 effect 中。
    RunTools { tool_use_count: usize },
    /// 要求外层执行停止钩子并返回结果事件。
    RunStopHooks,
    /// 要求外层运行预算 checker 并把决定作为事件送回。
    CheckTokenBudget,
    /// 要求外层把提示文本加入模型上下文；必须先于同一列表中的下一模型请求执行。
    AppendUserMessage { content: String },
    /// 要求外层把 hook 的阻断错误整理成模型可读反馈。
    EmitBlockingErrors { errors: Vec<String> },
    /// 要求外层完成查询并记录停止原因；该 effect 本身不是持久记录。
    Finish { stop_reason: QueryStopReason },
    /// 本次没有产生推进流程的执行意图。注意，某些部分进度事件会先更新计数，再返回 Ignored。
    Ignored { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// 一次纯转移计算的完整返回值。
pub struct QueryTransition {
    /// reducer 处理输入后的局部状态；调用方应使用此状态处理下一事件。
    pub state: QueryState,
    /// 外层要消费的意图列表；同一列表里的先后次序是状态机契约的一部分。
    pub effects: Vec<QueryEffect>,
}

impl QueryTransition {
    /// 汇总新状态和 effect 列表；不检查 effect 是否最终执行。
    fn new(state: QueryState, effects: Vec<QueryEffect>) -> Self {
        Self { state, effects }
    }
}

/// 将一个外层事件应用到状态快照，并计算下一状态与待执行 effect。
///
/// 【作用】此函数是本模块状态机的唯一转移入口：先检查终态和阶段，再处理对应事件分支。
/// 【调用者】设计上由 Query 外层循环在观察到模型/工具/hook/预算结果后调用；当前仓内直接
/// 调用仅见本文件单元测试，crate 根导出不证明产品入口已接线。
/// 【输入】当前状态（按值传入）、一个已发生/已计算的事件、显式配置。配置当前只用于 Start
/// 时复制 session ID，四个 QueryGates 不参与分支；调用方应确保配置与该查询状态属于同一会话，
/// 因为 reducer 不会拒绝 Start 时传入不同的 session ID。
/// 【输出】新的 `QueryState` 和需要外层处理的有序 `QueryEffect` 列表。
/// 【副作用】无 I/O、无模型/工具调用、无存储写入；不会自动重试。外层必须真实执行 effect，
/// 并把观察结果封装成下一个事件后再调用。
/// 【失败/忽略】函数不返回 Result。终态中的所有事件、或非终态下不匹配当前 phase 的普通
/// 事件，会返回 `Ignored`。注意 `ToolResultsSubmitted` 的部分结果分支先减少 pending 数，再
/// 返回 Ignored，因此 Ignored effect 不总意味着状态完全未变。
/// 【容易误改】终态检查先于事件匹配；Complete/Fail/Abort 可从任何非终态到达。hook 阻断错误
/// 优先于 prevent flag；预算 Continue 的 AppendUserMessage 必须排在 RequestModel 前面。
pub fn transition_query_state(
    mut state: QueryState,
    event: QueryEvent,
    config: &QueryConfig,
) -> QueryTransition {
    // 终态是不可逆边界：即使迟到事件本身是 Complete/Fail/Abort，也不允许覆盖既有结束状态。
    if state.is_terminal() {
        return QueryTransition::new(
            state,
            vec![QueryEffect::Ignored {
                reason: "query is already terminal".to_string(),
            }],
        );
    }

    // 普通事件只在其专属阶段生效；生命周期结束事件则被明确允许从任一非终态到达。
    // 这里先做白名单门控，避免例如模型响应在等待工具时覆盖 pending 计数。
    let event_allowed = matches!(
        (&state.phase, &event),
        (QueryPhase::Idle, QueryEvent::Start)
            | (
                QueryPhase::WaitingForModel,
                QueryEvent::ModelResponseReceived { .. }
            )
            | (
                QueryPhase::WaitingForTools,
                QueryEvent::ToolResultsSubmitted { .. }
            )
            | (
                QueryPhase::RunningStopHooks,
                QueryEvent::StopHooksFinished { .. }
            )
            | (
                QueryPhase::CheckingBudget,
                QueryEvent::TokenBudgetChecked { .. }
            )
            | (_, QueryEvent::Complete { .. })
            | (_, QueryEvent::Fail { .. })
            | (_, QueryEvent::Abort { .. })
    );
    if !event_allowed {
        // 阶段不匹配时，状态保持原样。调用方收到的 Ignored 只说明 reducer 未接受该事件，
        // 不会自动重新投递，也不会产生模型/工具 effect。
        return QueryTransition::new(
            state,
            vec![QueryEffect::Ignored {
                reason: "query event does not match current phase".to_owned(),
            }],
        );
    }

    match event {
        QueryEvent::Start => {
            // 状态创建时也可带 session ID；Start 再用当前配置明确绑定一次会话身份。
            state.session_id = config.session_id.clone();
            state.phase = QueryPhase::WaitingForModel;
            QueryTransition::new(state, vec![QueryEffect::RequestModel { turn_index: 0 }])
        }
        QueryEvent::ModelResponseReceived {
            tool_use_count,
            input_tokens,
            output_tokens,
        } => {
            // 输入/输出 token 是本次增量，累加为 query 范围的计数；saturating_add 达上限后会
            // 保持 u64::MAX，因此此状态统计不适合充当精确账单金额或账本事实。
            state.input_tokens = state.input_tokens.saturating_add(input_tokens);
            state.output_tokens = state.output_tokens.saturating_add(output_tokens);
            state.pending_tool_uses = tool_use_count;

            // 结束当前模型响应后，如果没有工具，先检查停止钩子，再决定预算；如果有工具，
            // 外层必须执行它们并回报结果，模型循环不能先行推进。
            if tool_use_count == 0 {
                state.phase = QueryPhase::RunningStopHooks;
                QueryTransition::new(state, vec![QueryEffect::RunStopHooks])
            } else {
                state.phase = QueryPhase::WaitingForTools;
                QueryTransition::new(state, vec![QueryEffect::RunTools { tool_use_count }])
            }
        }
        QueryEvent::ToolResultsSubmitted { tool_result_count } => {
            // 该 API 只收数量，没有调用 ID：调用方需报告本批新完成的唯一结果数。饱和减法
            // 防止无符号下溢；但若数量大于 pending，missing 也会变成 0 并继续请求模型，
            // reducer 不会识别超额/重复结果。这是调用方必须守住的边界，也是潜在校验缺口。
            let missing = state.pending_tool_uses.saturating_sub(tool_result_count);
            state.pending_tool_uses = missing;

            if missing == 0 {
                // 计算后的 pending 数归零时推进模型轮次；在调用方按约束报告唯一结果的正常情况，
                // 这表示所有工具结果都已到齐。超额计数也会被饱和减法压成 0。
                state.turn_index = state.turn_index.saturating_add(1);
                state.phase = QueryPhase::WaitingForModel;
                let turn_index = state.turn_index;
                QueryTransition::new(state, vec![QueryEffect::RequestModel { turn_index }])
            } else {
                // 部分结果已经从 pending 中扣除，但这时不请求模型；Ignored 表示“等待余下结果”，
                // 不表示本事件对状态完全没有影响。
                QueryTransition::new(
                    state,
                    vec![QueryEffect::Ignored {
                        reason: format!("{missing} tool result(s) still pending"),
                    }],
                )
            }
        }
        QueryEvent::StopHooksFinished {
            blocking_errors,
            prevent_continuation,
        } => {
            // 阻断错误优先反馈给模型，让它有机会修复；即使 prevent_continuation 同时为真，
            // 当前实现仍以错误反馈分支为先。未来若要改变优先级，需同时检查状态机契约和测试。
            if !blocking_errors.is_empty() {
                state.turn_index = state.turn_index.saturating_add(1);
                state.phase = QueryPhase::WaitingForModel;
                let turn_index = state.turn_index;
                // 顺序重要：先把阻断原因交给模型，再开始它的修复轮次。
                return QueryTransition::new(
                    state,
                    vec![
                        QueryEffect::EmitBlockingErrors {
                            errors: blocking_errors,
                        },
                        QueryEffect::RequestModel { turn_index },
                    ],
                );
            }

            if prevent_continuation {
                // 只有没有阻断错误待反馈时才应用明确禁止继续的标志。
                state.phase = QueryPhase::Completed;
                state.stop_reason = Some(QueryStopReason::StopHookPrevented);
                return QueryTransition::new(
                    state,
                    vec![QueryEffect::Finish {
                        stop_reason: QueryStopReason::StopHookPrevented,
                    }],
                );
            }

            // hook 没有阻断也没有禁止继续时，预算检查是下一个外部步骤。
            state.phase = QueryPhase::CheckingBudget;
            QueryTransition::new(state, vec![QueryEffect::CheckTokenBudget])
        }
        // 预算决定来自外层 checker。继续时必须先追加 nudge 再请求模型；停止时只完成状态转移。
        QueryEvent::TokenBudgetChecked { decision } => match decision {
            TokenBudgetDecision::Continue {
                nudge_message,
                continuation_count,
                ..
            } => {
                // 当前 reducer 只保存续跑次数，不保存 pct/turn_tokens/budget 这些展示字段。
                state.budget_continuations = continuation_count;
                state.turn_index = state.turn_index.saturating_add(1);
                state.phase = QueryPhase::WaitingForModel;
                let turn_index = state.turn_index;
                QueryTransition::new(
                    state,
                    vec![
                        QueryEffect::AppendUserMessage {
                            content: nudge_message,
                        },
                        QueryEffect::RequestModel { turn_index },
                    ],
                )
            }
            TokenBudgetDecision::Stop { completion_event } => {
                // `completion_event` 仅用于判断停止原因；其中的统计字段没有复制到 QueryState
                // 或 QueryEffect::Finish。需要保留摘要时，调用方必须在提交事件前自行复制或投影，
                // 因为本转移结果不会再携带这份 completion_event。
                state.phase = QueryPhase::Completed;
                state.stop_reason = Some(if completion_event.is_some() {
                    QueryStopReason::TokenBudget
                } else {
                    QueryStopReason::Complete
                });
                QueryTransition::new(
                    state.clone(),
                    vec![QueryEffect::Finish {
                        stop_reason: state.stop_reason.clone().unwrap(),
                    }],
                )
            }
        },
        QueryEvent::Complete { stop_reason } => {
            // 本事件允许携带任一 QueryStopReason；此处不再校验 reason 与事件类型是否一致。
            state.phase = QueryPhase::Completed;
            state.stop_reason = Some(stop_reason.clone());
            QueryTransition::new(state, vec![QueryEffect::Finish { stop_reason }])
        }
        QueryEvent::Fail { error } => {
            // 失败信息保留在内存状态供上层展示；这个 reducer 不负责清理敏感内容或记录日志。
            state.phase = QueryPhase::Failed;
            state.last_error = Some(error);
            state.stop_reason = Some(QueryStopReason::Failed);
            QueryTransition::new(
                state,
                vec![QueryEffect::Finish {
                    stop_reason: QueryStopReason::Failed,
                }],
            )
        }
        QueryEvent::Abort { reason } => {
            // 取消也可没有文本原因；停止类别仍固定为 Aborted。
            state.phase = QueryPhase::Aborted;
            state.last_error = reason;
            state.stop_reason = Some(QueryStopReason::Aborted);
            QueryTransition::new(
                state,
                vec![QueryEffect::Finish {
                    stop_reason: QueryStopReason::Aborted,
                }],
            )
        }
    }
}

#[cfg(test)]
/// 纯 reducer 单元测试：直接构造状态和事件，核对返回值与 effect 顺序，不执行模型、工具或 hook。
mod tests {
    use super::*;
    use crate::token_budget::BudgetCompletionEvent;
    use kiana_types::ids::SessionId;

    /// 使用固定会话和测试用 gates，避免依赖进程环境；当前 reducer 只读取其中的 session ID。
    fn config() -> QueryConfig {
        QueryConfig {
            session_id: SessionId::new("session-1".to_string()),
            gates: crate::config::QueryGates {
                streaming_tool_execution: false,
                emit_tool_use_summaries: false,
                is_ant: false,
                fast_mode_enabled: true,
            },
        }
    }

    #[test]
    /// Idle + Start 将状态绑定到配置会话，并请求编号为 0 的首轮模型。
    fn start_requests_first_model_turn() {
        let config = config();
        let state = QueryState::new(&config);

        let transition = transition_query_state(state, QueryEvent::Start, &config);

        assert_eq!(transition.state.phase, QueryPhase::WaitingForModel);
        assert_eq!(
            transition.effects,
            vec![QueryEffect::RequestModel { turn_index: 0 }]
        );
    }

    #[test]
    /// 模型响应有两个工具调用时，累计 token 并等待外层提交工具结果。
    fn model_response_with_tools_requests_tool_execution() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::WaitingForModel;

        let transition = transition_query_state(
            state,
            QueryEvent::ModelResponseReceived {
                tool_use_count: 2,
                input_tokens: 100,
                output_tokens: 50,
            },
            &config,
        );

        assert_eq!(transition.state.phase, QueryPhase::WaitingForTools);
        assert_eq!(transition.state.pending_tool_uses, 2);
        assert_eq!(transition.state.input_tokens, 100);
        assert_eq!(
            transition.effects,
            vec![QueryEffect::RunTools { tool_use_count: 2 }]
        );
    }

    #[test]
    /// 两个工具结果全部提交后，pending 清零并请求下一轮模型。
    fn tool_results_loop_back_to_model() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::WaitingForTools;
        state.pending_tool_uses = 2;

        let transition = transition_query_state(
            state,
            QueryEvent::ToolResultsSubmitted {
                tool_result_count: 2,
            },
            &config,
        );

        assert_eq!(transition.state.phase, QueryPhase::WaitingForModel);
        assert_eq!(transition.state.turn_index, 1);
        assert_eq!(
            transition.effects,
            vec![QueryEffect::RequestModel { turn_index: 1 }]
        );
    }

    #[test]
    /// 无工具的模型响应先运行停止钩子；钩子允许继续时才请求预算检查。
    fn model_response_without_tools_runs_stop_hooks_then_budget_check() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::WaitingForModel;

        let hooks = transition_query_state(
            state,
            QueryEvent::ModelResponseReceived {
                tool_use_count: 0,
                input_tokens: 100,
                output_tokens: 50,
            },
            &config,
        );
        assert_eq!(hooks.state.phase, QueryPhase::RunningStopHooks);
        assert_eq!(hooks.effects, vec![QueryEffect::RunStopHooks]);

        let budget = transition_query_state(
            hooks.state,
            QueryEvent::StopHooksFinished {
                blocking_errors: vec![],
                prevent_continuation: false,
            },
            &config,
        );
        assert_eq!(budget.state.phase, QueryPhase::CheckingBudget);
        assert_eq!(budget.effects, vec![QueryEffect::CheckTokenBudget]);
    }

    #[test]
    /// 预算 Continue 返回 nudge 与模型请求两个 effect，并验证它们的执行次序。
    fn budget_continue_appends_nudge_and_requests_model() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::CheckingBudget;

        let transition = transition_query_state(
            state,
            QueryEvent::TokenBudgetChecked {
                decision: TokenBudgetDecision::Continue {
                    nudge_message: "keep going".to_string(),
                    continuation_count: 1,
                    pct: 50,
                    turn_tokens: 5_000,
                    budget: 10_000,
                },
            },
            &config,
        );

        assert_eq!(transition.state.phase, QueryPhase::WaitingForModel);
        assert_eq!(transition.state.budget_continuations, 1);
        assert_eq!(
            transition.effects,
            vec![
                QueryEffect::AppendUserMessage {
                    content: "keep going".to_string(),
                },
                QueryEffect::RequestModel { turn_index: 1 },
            ]
        );
    }

    #[test]
    /// 带完成摘要的预算 Stop 会进入 TokenBudget 终态；本例不验证摘要统计被保留。
    fn budget_stop_completes_with_token_budget_reason() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::CheckingBudget;

        let transition = transition_query_state(
            state,
            QueryEvent::TokenBudgetChecked {
                decision: TokenBudgetDecision::Stop {
                    completion_event: Some(BudgetCompletionEvent {
                        continuation_count: 1,
                        pct: 95,
                        turn_tokens: 9_500,
                        budget: 10_000,
                        diminishing_returns: false,
                        duration_ms: 10,
                    }),
                },
            },
            &config,
        );

        assert_eq!(transition.state.phase, QueryPhase::Completed);
        assert_eq!(
            transition.state.stop_reason,
            Some(QueryStopReason::TokenBudget)
        );
        assert_eq!(
            transition.effects,
            vec![QueryEffect::Finish {
                stop_reason: QueryStopReason::TokenBudget,
            }]
        );
    }

    #[test]
    /// hook 返回阻断错误时，先把错误反馈给模型，再请求修复轮次。
    fn blocking_stop_hook_errors_are_fed_back_to_model() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::RunningStopHooks;

        let transition = transition_query_state(
            state,
            QueryEvent::StopHooksFinished {
                blocking_errors: vec!["fix formatting".to_string()],
                prevent_continuation: false,
            },
            &config,
        );

        assert_eq!(transition.state.phase, QueryPhase::WaitingForModel);
        assert_eq!(
            transition.effects,
            vec![
                QueryEffect::EmitBlockingErrors {
                    errors: vec!["fix formatting".to_string()],
                },
                QueryEffect::RequestModel { turn_index: 1 },
            ]
        );
    }

    #[test]
    /// Completed 状态收到迟到模型响应时不被重新打开，事件只产生 Ignored effect。
    fn terminal_state_ignores_late_events() {
        let config = config();
        let mut state = QueryState::new(&config);
        state.phase = QueryPhase::Completed;
        state.stop_reason = Some(QueryStopReason::Complete);

        let transition = transition_query_state(
            state,
            QueryEvent::ModelResponseReceived {
                tool_use_count: 1,
                input_tokens: 0,
                output_tokens: 0,
            },
            &config,
        );

        assert_eq!(transition.state.phase, QueryPhase::Completed);
        assert!(matches!(
            transition.effects.as_slice(),
            [QueryEffect::Ignored { .. }]
        ));
    }

    #[test]
    /// 将多种 phase/event 不匹配组合放在同一表中，确认它们不推进状态、不发出执行 effect。
    fn wrong_phase_events_are_ignored_without_mutating_state() {
        // 错误阶段的模型、工具、钩子和预算事件都不能修改状态或产生执行副作用。
        let config = config();
        let cases = [
            (
                QueryPhase::Idle,
                QueryEvent::ModelResponseReceived {
                    tool_use_count: 1,
                    input_tokens: 7,
                    output_tokens: 9,
                },
            ),
            (
                QueryPhase::WaitingForModel,
                QueryEvent::ToolResultsSubmitted {
                    tool_result_count: 1,
                },
            ),
            (
                QueryPhase::WaitingForTools,
                QueryEvent::StopHooksFinished {
                    blocking_errors: Vec::new(),
                    prevent_continuation: false,
                },
            ),
            (
                QueryPhase::RunningStopHooks,
                QueryEvent::TokenBudgetChecked {
                    decision: TokenBudgetDecision::Stop {
                        completion_event: None,
                    },
                },
            ),
            (
                QueryPhase::CheckingBudget,
                QueryEvent::ModelResponseReceived {
                    tool_use_count: 0,
                    input_tokens: 1,
                    output_tokens: 1,
                },
            ),
        ];

        for (phase, event) in cases {
            let mut state = QueryState::new(&config);
            state.phase = phase;
            state.turn_index = 3;
            state.pending_tool_uses = 2;
            state.input_tokens = 11;
            state.output_tokens = 13;
            let before = state.clone();
            let transition = transition_query_state(state, event, &config);
            assert_eq!(transition.state, before);
            assert_eq!(
                transition.effects,
                vec![QueryEffect::Ignored {
                    reason: "query event does not match current phase".to_owned(),
                }]
            );
        }
    }
}
