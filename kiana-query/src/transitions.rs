//! Pure query-loop state transitions.
//!
//! This module owns the small reducer that keeps model calls, tool execution,
//! stop hooks, budget continuation, and terminal states explicit. The reducer
//! is intentionally side-effect free: callers inspect returned `QueryEffect`s
//! and perform I/O in the outer query loop.

use crate::config::QueryConfig;
use crate::token_budget::TokenBudgetDecision;
use kiana_types::ids::SessionId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryState {
    pub session_id: SessionId,
    pub phase: QueryPhase,
    pub turn_index: u32,
    pub pending_tool_uses: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub budget_continuations: u32,
    pub stop_reason: Option<QueryStopReason>,
    pub last_error: Option<String>,
}

impl QueryState {
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

    pub fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryPhase {
    Idle,
    WaitingForModel,
    WaitingForTools,
    RunningStopHooks,
    CheckingBudget,
    Completed,
    Failed,
    Aborted,
}

impl QueryPhase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            QueryPhase::Completed | QueryPhase::Failed | QueryPhase::Aborted
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryEvent {
    Start,
    ModelResponseReceived {
        tool_use_count: usize,
        input_tokens: u64,
        output_tokens: u64,
    },
    ToolResultsSubmitted {
        tool_result_count: usize,
    },
    StopHooksFinished {
        blocking_errors: Vec<String>,
        prevent_continuation: bool,
    },
    TokenBudgetChecked {
        decision: TokenBudgetDecision,
    },
    Complete {
        stop_reason: QueryStopReason,
    },
    Fail {
        error: String,
    },
    Abort {
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryStopReason {
    Complete,
    TokenBudget,
    StopHookPrevented,
    Aborted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryEffect {
    RequestModel { turn_index: u32 },
    RunTools { tool_use_count: usize },
    RunStopHooks,
    CheckTokenBudget,
    AppendUserMessage { content: String },
    EmitBlockingErrors { errors: Vec<String> },
    Finish { stop_reason: QueryStopReason },
    Ignored { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryTransition {
    pub state: QueryState,
    pub effects: Vec<QueryEffect>,
}

impl QueryTransition {
    fn new(state: QueryState, effects: Vec<QueryEffect>) -> Self {
        Self { state, effects }
    }
}

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

            if tool_use_count == 0 {
                state.phase = QueryPhase::RunningStopHooks;
                QueryTransition::new(state, vec![QueryEffect::RunStopHooks])
            } else {
                state.phase = QueryPhase::WaitingForTools;
                QueryTransition::new(state, vec![QueryEffect::RunTools { tool_use_count }])
            }
        }
        QueryEvent::ToolResultsSubmitted { tool_result_count } => {
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
}
