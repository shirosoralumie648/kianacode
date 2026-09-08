//! 查询循环的 token 预算跟踪与继续/停止决定。
//!
//! 外层在每次模型响应后调用 [`check_token_budget`]，根据累计 token 数和最近增长量决定
//! 是否追加一条继续工作的用户消息。这个模块只做本地计数和纯决定，不负责限制模型实际
//! 消耗的硬配额；硬预算仍必须由 Runner、ControlPlane 或供应商适配器执行。
//!
//! 两个固定阈值共同防止无限续跑：低于 90% 且增长正常时最多继续；已经续跑至少三次、
//! 且最近两次增长都低于 500 token 时视为收益递减并停止。阈值是当前实现事实，不代表
//! 所有入口都已经接入该 reducer。
use serde::{Deserialize, Serialize};
use std::time::Instant;

const COMPLETION_THRESHOLD: f64 = 0.9;
const DIMINISHING_THRESHOLD: u64 = 500;

/// 每次预算检查都会更新的可序列化状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetTracker {
    /// 超过初始响应后已继续请求模型的次数。
    pub continuation_count: u32,
    /// 上一次检查观察到的 token 增量。
    pub last_delta_tokens: u64,
    /// 上一次检查记录的累计 turn token 数。
    pub last_global_turn_tokens: u64,
    /// 单调时钟起点，用于生成完成事件中的耗时；序列化时刻意跳过。
    #[serde(skip)]
    pub started_at: Option<Instant>,
}

impl BudgetTracker {
    /// 在查询开始时创建空跟踪器。
    pub fn new() -> Self {
        BudgetTracker {
            continuation_count: 0,
            last_delta_tokens: 0,
            last_global_turn_tokens: 0,
            started_at: Some(Instant::now()),
        }
    }

    /// 返回创建跟踪器以来经过的毫秒数。
    ///
    /// `Instant` 不可序列化；从恢复数据构造的跟踪器可能没有起点，此时返回 0，而不是
    /// 使用 wall-clock 猜测耗时。
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

/// 预算循环曾经续跑时附加在停止决定上的统计数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCompletionEvent {
    /// 已续跑次数。
    pub continuation_count: u32,
    /// 已消耗预算的四舍五入百分比（理论上可因累计超额大于 100）。
    pub pct: u32,
    /// 当前累计 turn token 数。
    pub turn_tokens: u64,
    /// 本轮采用的正预算值。
    pub budget: u64,
    /// 是否因连续低增量而判定为收益递减。
    pub diminishing_returns: bool,
    /// 从跟踪器创建到停止的单调耗时毫秒。
    pub duration_ms: u64,
}

/// [`check_token_budget`] 返回的继续/停止决定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TokenBudgetDecision {
    /// 继续查询循环，并把 `nudge_message` 作为用户消息追加给模型。
    Continue {
        /// 用于提醒模型在剩余预算内完成工作的消息。
        nudge_message: String,
        /// 决定作出后累计的续跑次数。
        continuation_count: u32,
        /// 当前预算百分比。
        pct: u32,
        /// 当前累计 token 数。
        turn_tokens: u64,
        /// 本轮预算。
        budget: u64,
    },
    /// 停止查询循环；若已经续跑过，可附带完成统计。
    Stop {
        /// 无预算或子代理快速停止时为 `None`。
        completion_event: Option<BudgetCompletionEvent>,
    },
}

/// 根据 token 消耗决定是否继续查询循环。
///
/// 预算为 `None`/零时，或调用方标记为子代理时，函数立即返回无统计的 `Stop`。正常主代理
/// 在低于 90% 且未触发收益递减时返回 `Continue`；一旦超过阈值或已出现收益递减则停止。
/// `global_turn_tokens` 应来自同一会话的累计计数，调用方必须保证不会把不同会话的计数
/// 混在一起，否则增量判断会失真。
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

/// 生成追加到对话中的预算提醒消息。
///
/// 该字符串是模型提示，不是权限或硬限制。`remaining` 使用饱和减法，预算已经超出时显示
/// 0；真正停止仍由 [`check_token_budget`] 的决定控制。
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
