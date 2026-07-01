/// Token budget tracking and continuation decisions.
///
/// Corresponds to `tokenBudget.ts`. The query loop calls `check_token_budget`
/// after each model response to decide whether to continue nudging the model
/// toward completion or to stop.
///
/// Thresholds:
/// - `COMPLETION_THRESHOLD` (0.9): if token usage is below 90% of budget and
///   not diminishing, we may continue.
/// - `DIMINISHING_THRESHOLD` (500): if the last two delta measurements are both
///   below 500 tokens and we've already continued ≥3 times, we stop (diminishing
///   returns guard).
use serde::{Deserialize, Serialize};
use std::time::Instant;

const COMPLETION_THRESHOLD: f64 = 0.9;
const DIMINISHING_THRESHOLD: u64 = 500;

/// Mutable state updated each time `check_token_budget` is called.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetTracker {
    /// Number of times we have continued beyond the initial response.
    pub continuation_count: u32,
    /// Token delta observed in the last budget check.
    pub last_delta_tokens: u64,
    /// Global turn token count at the last budget check.
    pub last_global_turn_tokens: u64,
    /// Monotonic start time used for `duration_ms` in completion events.
    #[serde(skip)]
    pub started_at: Option<Instant>,
}

impl BudgetTracker {
    /// Create a fresh tracker at query start.
    pub fn new() -> Self {
        BudgetTracker {
            continuation_count: 0,
            last_delta_tokens: 0,
            last_global_turn_tokens: 0,
            started_at: Some(Instant::now()),
        }
    }

    /// Elapsed milliseconds since tracker creation (0 if clock unavailable).
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Data attached to a `stop` decision when the budget loop has been active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCompletionEvent {
    pub continuation_count: u32,
    /// Percentage of budget consumed (0–100).
    pub pct: u32,
    pub turn_tokens: u64,
    pub budget: u64,
    pub diminishing_returns: bool,
    pub duration_ms: u64,
}

/// Decision returned by `check_token_budget`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TokenBudgetDecision {
    /// Continue the query loop; include `nudge_message` as a user message.
    Continue {
        nudge_message: String,
        continuation_count: u32,
        pct: u32,
        turn_tokens: u64,
        budget: u64,
    },
    /// Stop the query loop; optionally record completion metrics.
    Stop {
        completion_event: Option<BudgetCompletionEvent>,
    },
}

/// Decide whether to continue the query loop based on token consumption.
///
/// Returns `Stop { completion_event: None }` immediately when:
/// - running as a subagent (`agent_id.is_some()`), or
/// - no budget is configured (`budget` is `None` or zero).
pub fn check_token_budget(
    tracker: &mut BudgetTracker,
    agent_id: Option<&str>,
    budget: Option<u64>,
    global_turn_tokens: u64,
) -> TokenBudgetDecision {
    let Some(budget) = budget.filter(|&b| b > 0) else {
        return TokenBudgetDecision::Stop {
            completion_event: None,
        };
    };

    if agent_id.is_some() {
        return TokenBudgetDecision::Stop {
            completion_event: None,
        };
    }

    let turn_tokens = global_turn_tokens;
    let pct = ((turn_tokens as f64 / budget as f64) * 100.0).round() as u32;
    let delta_since_last = global_turn_tokens.saturating_sub(tracker.last_global_turn_tokens);

    let is_diminishing = tracker.continuation_count >= 3
        && delta_since_last < DIMINISHING_THRESHOLD
        && tracker.last_delta_tokens < DIMINISHING_THRESHOLD;

    if !is_diminishing && (turn_tokens as f64) < (budget as f64) * COMPLETION_THRESHOLD {
        tracker.continuation_count += 1;
        tracker.last_delta_tokens = delta_since_last;
        tracker.last_global_turn_tokens = global_turn_tokens;

        return TokenBudgetDecision::Continue {
            nudge_message: get_budget_continuation_message(pct, turn_tokens, budget),
            continuation_count: tracker.continuation_count,
            pct,
            turn_tokens,
            budget,
        };
    }

    if is_diminishing || tracker.continuation_count > 0 {
        return TokenBudgetDecision::Stop {
            completion_event: Some(BudgetCompletionEvent {
                continuation_count: tracker.continuation_count,
                pct,
                turn_tokens,
                budget,
                diminishing_returns: is_diminishing,
                duration_ms: tracker.elapsed_ms(),
            }),
        };
    }

    TokenBudgetDecision::Stop {
        completion_event: None,
    }
}

/// Generate the nudge message inserted into the conversation to prompt the
/// model to keep working toward completion within the remaining budget.
pub fn get_budget_continuation_message(pct: u32, turn_tokens: u64, budget: u64) -> String {
    let remaining = budget.saturating_sub(turn_tokens);
    format!(
        "<budget_remaining>{remaining} tokens remaining ({pct}% used of {budget} budget). \
         Continue working toward completion.</budget_remaining>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stop_immediately_for_subagent() {
        let mut t = BudgetTracker::new();
        let d = check_token_budget(&mut t, Some("agent-1"), Some(10_000), 100);
        assert!(matches!(
            d,
            TokenBudgetDecision::Stop {
                completion_event: None
            }
        ));
    }

    #[test]
    fn returns_stop_immediately_with_no_budget() {
        let mut t = BudgetTracker::new();
        let d = check_token_budget(&mut t, None, None, 100);
        assert!(matches!(
            d,
            TokenBudgetDecision::Stop {
                completion_event: None
            }
        ));
    }

    #[test]
    fn continues_when_below_threshold() {
        let mut t = BudgetTracker::new();
        // 50% used — well below 90% threshold
        let d = check_token_budget(&mut t, None, Some(10_000), 5_000);
        assert!(matches!(d, TokenBudgetDecision::Continue { .. }));
        assert_eq!(t.continuation_count, 1);
    }

    #[test]
    fn stops_when_above_threshold() {
        let mut t = BudgetTracker::new();
        // 95% used — above 90% threshold with no prior continuations
        let d = check_token_budget(&mut t, None, Some(10_000), 9_500);
        assert!(matches!(
            d,
            TokenBudgetDecision::Stop {
                completion_event: None
            }
        ));
    }

    #[test]
    fn stops_on_diminishing_returns() {
        let mut t = BudgetTracker {
            continuation_count: 3,
            last_delta_tokens: 100, // below DIMINISHING_THRESHOLD
            last_global_turn_tokens: 4_800,
            started_at: Some(Instant::now()),
        };
        // delta = 4900 - 4800 = 100, below threshold → diminishing
        let d = check_token_budget(&mut t, None, Some(10_000), 4_900);
        assert!(matches!(
            d,
            TokenBudgetDecision::Stop {
                completion_event: Some(BudgetCompletionEvent {
                    diminishing_returns: true,
                    ..
                })
            }
        ));
    }
}
