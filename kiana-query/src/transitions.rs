//! 查询循环的纯状态转移 reducer。
//!
//! 本模块把模型调用、工具执行、停止钩子、预算续跑和终态都表示成显式事件与副作用描述。
//! reducer 本身不执行 I/O、不调用模型、不运行工具；调用方必须消费返回的
//! [`QueryEffect`]，在外层完成实际动作后再把结果作为下一个 [`QueryEvent`] 送回。这样
//! 可以单独测试每条状态边，并避免在 reducer 内偷偷形成第二条执行循环。

use crate::config::QueryConfig;
use crate::token_budget::TokenBudgetDecision;
use kiana_types::ids::SessionId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// 一次查询的可序列化运行时状态。
///
/// 字段是 reducer 的权威输入/输出快照，不是 EventLog 的替代品。`pending_tool_uses` 表示
/// 当前模型响应尚未收到结果的数量；累计 token 和停止原因则用于后续预算与收据投影。
pub struct QueryState {
    /// 状态所属会话。
    pub session_id: SessionId,
    /// 当前状态机阶段。
    pub phase: QueryPhase,
    /// 已进入的模型轮次索引，从 0 开始。
    pub turn_index: u32,
    /// 当前等待返回的工具调用数量。
    pub pending_tool_uses: usize,
    /// 累计输入 token 数。
    pub input_tokens: u64,
    /// 累计输出 token 数。
    pub output_tokens: u64,
    /// 预算 reducer 已追加的续跑次数。
    pub budget_continuations: u32,
    /// 若已结束，记录稳定的停止原因。
    pub stop_reason: Option<QueryStopReason>,
    /// 最近一次失败或取消的展示性错误；不是权限事实源。
    pub last_error: Option<String>,
}

impl QueryState {
    /// 根据配置创建处于 `Idle` 的初始状态。
    ///
    /// 初始状态不会自动请求模型，必须通过 [`QueryEvent::Start`] 产生第一条
    /// [`QueryEffect::RequestModel`]。
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

    /// 判断当前阶段是否已经没有合法后续事件。
    pub fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 查询 reducer 的阶段。
pub enum QueryPhase {
    /// 已创建但尚未收到 Start。
    Idle,
    /// 等待模型响应。
    WaitingForModel,
    /// 等待外层提交全部工具结果。
    WaitingForTools,
    /// 正在执行停止钩子。
    RunningStopHooks,
    /// 正在计算 token 预算的继续/停止决定。
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
/// 外层执行器反馈给 reducer 的事件。
///
/// 事件只描述已经观察到的事实，例如模型返回了几个工具调用或停止钩子是否阻断；它们
/// 不直接携带授权决定。工具和模型的真实调用必须在外层经过控制面后再提交结果。
pub enum QueryEvent {
    /// 启动查询并请求首轮模型。
    Start,
    /// 模型响应已收到，附带工具数和 token 统计。
    ModelResponseReceived {
        /// 响应中待执行的工具调用数。
        tool_use_count: usize,
        /// 本次输入 token 数。
        input_tokens: u64,
        /// 本次输出 token 数。
        output_tokens: u64,
    },
    /// 外层已提交部分或全部工具结果。
    ToolResultsSubmitted {
        /// 已提交的结果数。
        tool_result_count: usize,
    },
    /// 停止钩子执行结束。
    StopHooksFinished {
        /// 需要反馈给模型的阻断错误；非空时优先继续修复。
        blocking_errors: Vec<String>,
        /// 钩子明确禁止继续查询。
        prevent_continuation: bool,
    },
    /// 预算检查已经完成。
    TokenBudgetChecked {
        /// 预算模块给出的下一步决定。
        decision: TokenBudgetDecision,
    },
    /// 外层确认正常结束。
    Complete {
        /// 结束原因。
        stop_reason: QueryStopReason,
    },
    /// 外层观察到明确失败。
    Fail {
        /// 稳定或已清理的失败信息。
        error: String,
    },
    /// 外层请求中止查询。
    Abort {
        /// 可选的人类可读原因。
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 查询结束时写入状态和收据的原因分类。
pub enum QueryStopReason {
    /// 模型/钩子正常完成。
    Complete,
    /// 达到 token 预算或收益递减阈值。
    TokenBudget,
    /// 停止钩子明确阻止继续。
    StopHookPrevented,
    /// 用户或系统中止。
    Aborted,
    /// 发生明确错误。
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// reducer 要求外层执行的副作用描述。
///
/// 这些值是意图，不是已完成的执行证据。特别是 `RunTools` 和 `RequestModel` 只能触发
/// 相应的受控适配器调用，不能在序列化/展示层直接执行。
pub enum QueryEffect {
    /// 请求模型执行指定轮次。
    RequestModel { turn_index: u32 },
    /// 执行当前模型响应中的工具调用。
    RunTools { tool_use_count: usize },
    /// 运行停止钩子集合。
    RunStopHooks,
    /// 检查 token 预算。
    CheckTokenBudget,
    /// 向模型上下文追加一条用户消息。
    AppendUserMessage { content: String },
    /// 把阻断错误反馈给模型。
    EmitBlockingErrors { errors: Vec<String> },
    /// 结束查询并记录原因。
    Finish { stop_reason: QueryStopReason },
    /// 忽略不适用于当前阶段或终态的迟到事件。
    Ignored { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// 一次 reducer 调用产生的新状态和待执行效果。
pub struct QueryTransition {
    /// 事件应用后的状态快照。
    pub state: QueryState,
    /// 外层需要按顺序处理的副作用意图。
    pub effects: Vec<QueryEffect>,
}

impl QueryTransition {
    fn new(state: QueryState, effects: Vec<QueryEffect>) -> Self {
        Self { state, effects }
    }
}

/// 将一个事件应用到查询状态，返回纯数据转移结果。
///
/// 终态优先规则保证迟到的模型/工具结果不会重新打开已完成查询。工具结果不足时只返回
/// `Ignored`，不会把缺失结果当成成功；停止钩子阻断优先于预算检查；预算继续时先追加
/// nudge 再请求下一轮模型。调用方必须按返回效果顺序执行，不能自行重排导致状态与事实
/// 记录不一致。
pub fn transition_query_state(
    mut state: QueryState,
    event: QueryEvent,
    config: &QueryConfig,
) -> QueryTransition {
    if state.is_terminal() {
        return QueryTransition::new(
            state,
            vec![QueryEffect::Ignored {
                reason: "query is already terminal".to_string(),
            }],
        );
    }

    // 每个事件只允许在对应阶段生效，错误阶段到达时只能忽略，不能意外推进状态。
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
        return QueryTransition::new(
            state,
            vec![QueryEffect::Ignored {
                reason: "query event does not match current phase".to_owned(),
            }],
        );
    }

    match event {
        QueryEvent::Start => {
            state.session_id = config.session_id.clone();
            state.phase = QueryPhase::WaitingForModel;
            QueryTransition::new(state, vec![QueryEffect::RequestModel { turn_index: 0 }])
        }
        QueryEvent::ModelResponseReceived {
            tool_use_count,
            input_tokens,
            output_tokens,
        } => {
            state.input_tokens = state.input_tokens.saturating_add(input_tokens);
            state.output_tokens = state.output_tokens.saturating_add(output_tokens);
            state.pending_tool_uses = tool_use_count;

            // 没有工具调用时先跑 stop hooks；有工具调用则必须等所有结果齐全后再回模型。
            if tool_use_count == 0 {
                state.phase = QueryPhase::RunningStopHooks;
                QueryTransition::new(state, vec![QueryEffect::RunStopHooks])
            } else {
                state.phase = QueryPhase::WaitingForTools;
                QueryTransition::new(state, vec![QueryEffect::RunTools { tool_use_count }])
            }
        }
        QueryEvent::ToolResultsSubmitted { tool_result_count } => {
            // 饱和减法避免迟到/多报结果造成下溢；多出的结果不会伪造新的工具完成数。
            let missing = state.pending_tool_uses.saturating_sub(tool_result_count);
            state.pending_tool_uses = missing;

            if missing == 0 {
                state.turn_index = state.turn_index.saturating_add(1);
                state.phase = QueryPhase::WaitingForModel;
                let turn_index = state.turn_index;
                QueryTransition::new(state, vec![QueryEffect::RequestModel { turn_index }])
            } else {
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
            // 当前实现仍以错误反馈分支为先，这是既有行为，不能在注释之外擅自改变。
            if !blocking_errors.is_empty() {
                state.turn_index = state.turn_index.saturating_add(1);
                state.phase = QueryPhase::WaitingForModel;
                let turn_index = state.turn_index;
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
                state.phase = QueryPhase::Completed;
                state.stop_reason = Some(QueryStopReason::StopHookPrevented);
                return QueryTransition::new(
                    state,
                    vec![QueryEffect::Finish {
                        stop_reason: QueryStopReason::StopHookPrevented,
                    }],
                );
            }

            state.phase = QueryPhase::CheckingBudget;
            QueryTransition::new(state, vec![QueryEffect::CheckTokenBudget])
        }
        // 预算继续会产生两个有序效果：先更新对话，再请求下一模型轮次。
        QueryEvent::TokenBudgetChecked { decision } => match decision {
            TokenBudgetDecision::Continue {
                nudge_message,
                continuation_count,
                ..
            } => {
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
            state.phase = QueryPhase::Completed;
            state.stop_reason = Some(stop_reason.clone());
            QueryTransition::new(state, vec![QueryEffect::Finish { stop_reason }])
        }
        QueryEvent::Fail { error } => {
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
mod tests {
    use super::*;
    use crate::token_budget::BudgetCompletionEvent;
    use kiana_types::ids::SessionId;

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
