//! Query 循环中的 token 预算提示与停止建议。
//!
//! 【文件职责与系统位置】
//! 本模块把调用方提供的会话累计 token 数、预算值和 `BudgetTracker` 合并成一个纯决定：
//! 继续并生成提示，或停止并可选附带统计。预期链路为：
//!
//! 模型响应累计用量
//!     ↓ 外层调用 `check_token_budget()`
//! `TokenBudgetDecision`
//!     ↓ 调用方把决定作为 `QueryEvent::TokenBudgetChecked` 交给 transitions reducer
//! `QueryEffect::AppendUserMessage` → `QueryEffect::RequestModel`，或 `Finish`
//!
//! 调用关系核对：仓内直接调用 `check_token_budget()` 目前只出现在本文件单元测试；
//! `transitions` 消费决定，但不负责计算决定。没有找到产品执行器调用它，因此这里的算法和
//! `get_budget_continuation_message()` 仅能说明已实现的局部行为，不能证明线上循环启用。
//!
//! 【重要边界】
//! 本模块只统计和建议，不会限制模型真实消耗，也不会自行发消息、调用模型或终止进程。硬
//! 配额必须由实际执行模型/工具的 Runner、ControlPlane 或供应商 adapter 强制执行。
//! 低于 90% 且没有连续低增长时会继续；至少续跑 3 次且最近两次各自增长都低于 500 token
//! 时视为收益递减并停止。阈值是当前代码事实，不代表所有入口都已接入。
use serde::{Deserialize, Serialize};
use std::time::Instant;

// 使用浮点比例检查预算线；0.9 表示累计用量必须严格低于 90% 才能继续。
const COMPLETION_THRESHOLD: f64 = 0.9;
// 最近单次增长低于 500 token 才算低增长；不是每个模型的固定输出上限。
const DIMINISHING_THRESHOLD: u64 = 500;

/// 在多次预算检查之间保留的循环状态。
///
/// 外层把同一个 tracker 传给每次 `check_token_budget()`，以便比较当前累计值和上次累计值。
/// 只把它用于同一个会话；混入另一会话的累计 token 会让差值和收益递减判断失真。
/// `Serialize` / `Deserialize` 是 serde 派生能力：Rust 字段可以转换到外部数据格式并还原；
/// 它们不自动持久化状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetTracker {
    /// 预算检查返回 Continue 后已经要求额外模型轮次的次数；不统计初始请求。
    pub continuation_count: u32,
    /// 最近一次 Continue 时，相对上次累计值增加了多少 token，用于下一次收益递减判断。
    pub last_delta_tokens: u64,
    /// 最近一次 Continue 时的会话累计 token 总数；不是单个响应的 token 数。
    pub last_global_turn_tokens: u64,
    /// 创建 tracker 的单调时钟起点，用于统计这轮预算跟踪耗时。
    /// 不序列化：恢复出来的 `Instant` 不可能代表原进程的时间点，缺省后耗时按 0 处理。
    #[serde(skip)]
    pub started_at: Option<Instant>,
}

impl BudgetTracker {
    /// 在查询开始时创建没有续跑历史的 tracker。
    ///
    /// 【调用者】执行预算检查的外层循环；当前仓内只在单元测试创建。
    /// 【输出】续跑数、最近差值和累计值均为 0，并从此刻启动单调计时。
    /// 【副作用】只读取单调时钟并构造内存值；不读取模型状态或环境变量。
    pub fn new() -> Self {
        BudgetTracker {
            continuation_count: 0,
            last_delta_tokens: 0,
            last_global_turn_tokens: 0,
            started_at: Some(Instant::now()),
        }
    }

    /// 返回从 tracker 创建以来的单调经过时间，单位为毫秒。
    ///
    /// `Instant` 测量经过时间，不受系统日期被校准影响。反序列化会跳过起点，因此恢复后的
    /// tracker 返回 0；这里不拿墙上时钟补猜一个看似精确的持续时间。
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
}

impl Default for BudgetTracker {
    /// 默认值与显式 `new()` 一样，会创建新的计时起点。
    fn default() -> Self {
        Self::new()
    }
}

/// 停止时可附带的预算循环摘要。
///
/// 它只在已经续跑过或因收益递减而停止时创建；首次就达到预算线、无预算或子代理快速停止
/// 不一定有该值。它是 Query 层统计，不是计费账单或持久收据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCompletionEvent {
    /// 本次循环中追加模型请求的次数，不含初始请求。
    pub continuation_count: u32,
    /// 已消耗比例四舍五入成的整数百分比；累计用量超过预算时可以大于 100。
    pub pct: u32,
    /// 做出停止决定时的会话累计 token 总数。
    pub turn_tokens: u64,
    /// 做出决定时采用的预算值；只有正数预算才进入普通检查路径。
    pub budget: u64,
    /// true 表示满足多次续跑且最近两次增长均低于阈值；false 常表示达到预算线。
    pub diminishing_returns: bool,
    /// 从 tracker 创建到停止决定的单调耗时，单位毫秒；恢复 tracker 可能为 0。
    pub duration_ms: u64,
}

/// 预算 checker 返回给外层执行器的两种决定。
///
/// `serde(tag = "action", rename_all = "snake_case")` 让序列化表示额外带一个 `action` 字段，
/// 值为 `continue` 或 `stop`；接收方据此区分后面的字段形状。这只定义数据编码，不执行决定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TokenBudgetDecision {
    /// 建议继续查询循环；外层应先追加 `nudge_message`，再请求下一轮模型。
    /// reducer 会把这两个动作作为有顺序的 effect 返回，但不会亲自执行。
    Continue {
        /// 用于提醒模型在剩余预算内完成工作的消息。
        nudge_message: String,
        /// 包含本次新增续跑后的次数。
        continuation_count: u32,
        /// 当前预算百分比。
        pct: u32,
        /// 当前累计 token 数。
        turn_tokens: u64,
        /// 本轮预算。
        budget: u64,
    },
    /// 建议停止循环；是否附带摘要取决于此前是否已续跑或是否命中收益递减。
    Stop {
        /// 无预算或子代理快速停止时为 `None`。
        completion_event: Option<BudgetCompletionEvent>,
    },
}

/// 根据累计 token、预算和最近增长情况给出继续/停止决定。
///
/// 【调用者】设计上由外层模型循环在响应结束后调用，再把决定送入
/// `transition_query_state()`；当前仓内只有本文件测试直接调用，transition reducer 本身只
/// 消费结果，不调用本函数。
/// 【输入】可变 tracker；可选 agent ID（`Some` 表示子代理调用）；可选预算；同一会话累计 token。
/// 【输出】`Continue` 携带给模型的提示、次数和展示用统计，或 `Stop` 携带可选完成摘要。
/// 【状态副作用】只有 Continue 分支会递增次数并更新最近 token 差值/累计数；所有 Stop 分支
/// 都保持 tracker 不变。函数不会执行模型调用、消息追加或硬配额拦截。
/// 【提前停止】预算缺失或为 0 时，以及 `agent_id` 为 Some 时，直接返回没有统计事件的 Stop。
/// 【继续条件】主会话预算为正、最近两次低增长条件不成立，且累计用量严格小于 90% 时继续。
/// 等于或超过 90% 不继续。`pct` 是四舍五入后的展示数字，决定分支使用原始浮点比例；因此
/// 展示为 90% 的值仍可能略低于 90% 而继续。
/// 【收益递减】至少已经续跑 3 次，且本次增量和 tracker 保存的上次增量都小于 500 token 时
/// 停止。需要两次低增长，避免只因某一轮短响应就过早终止。
/// 【失败情况】没有 `Result`；累计用量建议来自同一会话，否则 `saturating_sub` 会把倒退值
/// 当作 0 增长，可能误判收益递减。百分比仅用于提示/统计，不是精确计费值。
pub fn check_token_budget(
    tracker: &mut BudgetTracker,
    agent_id: Option<&str>,
    budget: Option<u64>,
    global_turn_tokens: u64,
) -> TokenBudgetDecision {
    // 无预算或零预算没有可计算的继续额度；不更新 tracker，也不生成“完成统计”，因为这条
    // 快速拒绝路径没有经过正常预算跟踪流程。
    let Some(budget) = budget.filter(|&b| b > 0) else {
        return TokenBudgetDecision::Stop {
            completion_event: None,
        };
    };

    // 子代理不在这份主会话续跑策略里自行循环，避免层层代理都延长模型调用。
    if agent_id.is_some() {
        return TokenBudgetDecision::Stop {
            completion_event: None,
        };
    }

    let turn_tokens = global_turn_tokens;
    // pct 只供提示和摘要显示；下面是否继续会直接比较浮点累计值与 90% 阈值。
    let pct = ((turn_tokens as f64 / budget as f64) * 100.0).round() as u32;
    // 用饱和减法避免计数器倒退时发生无符号下溢；倒退被解释成 0 新增，调用方仍应提供
    // 单调累计值，不能把不同会话或不同统计范围混在一起。
    let delta_since_last = global_turn_tokens.saturating_sub(tracker.last_global_turn_tokens);

    // 至少已续跑三次，并且连续两个被观察到的增量都不足 500 token，才认为继续产出很少。
    let is_diminishing = tracker.continuation_count >= 3
        && delta_since_last < DIMINISHING_THRESHOLD
        && tracker.last_delta_tokens < DIMINISHING_THRESHOLD;

    if !is_diminishing && (turn_tokens as f64) < (budget as f64) * COMPLETION_THRESHOLD {
        // 计数和差值只在真的安排下一轮时更新；之后再次检查时，“上次增量”才代表上一次续跑。
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
        // 若此前已经续跑，或当前因收益递减停止，附上摘要；第一次就达到预算线时保持 None。
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

/// 生成交给模型的预算提醒文本。
///
/// 【调用者】`check_token_budget()` 的 Continue 分支；外层再把字符串作为用户消息 effect 执行。
/// 【输入】展示百分比、累计 token 和正预算。
/// 【输出】包含剩余 token 的提示字符串。剩余量采用 `budget.saturating_sub(turn_tokens)`，
/// 已超预算时显示 0，不会生成负数。
/// 【边界】这是发送给模型的文本，不是硬限制、权限规则或停止信号；真正是否停止由 checker
/// 返回的 `TokenBudgetDecision` 决定。
pub fn get_budget_continuation_message(pct: u32, turn_tokens: u64, budget: u64) -> String {
    let remaining = budget.saturating_sub(turn_tokens);
    format!(
        "<budget_remaining>{remaining} tokens remaining ({pct}% used of {budget} budget). \
         Continue working toward completion.</budget_remaining>"
    )
}

#[cfg(test)]
/// 以下单元测试用直接构造的 tracker 和数字输入检查纯决策分支，不调用真实模型，也不证明
/// 外层生产循环已经接入 checker。
mod tests {
    use super::*;

    #[test]
    /// 子代理标识存在时立即停止，且不附加主会话预算统计。
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
    /// 缺失预算和 token 数值无关，都会走无统计的快速停止路径。
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
    /// 初次检查远低于 90% 且增长未达到收益递减条件时，安排一次续跑并更新计数器。
    fn continues_when_below_threshold() {
        let mut t = BudgetTracker::new();
        // 这里累计使用 50%，明显低于继续线。
        let d = check_token_budget(&mut t, None, Some(10_000), 5_000);
        assert!(matches!(d, TokenBudgetDecision::Continue { .. }));
        assert_eq!(t.continuation_count, 1);
    }

    #[test]
    /// 首次检查已经达到预算线时停止；由于此前没有续跑，不生成完成摘要。
    fn stops_when_above_threshold() {
        let mut t = BudgetTracker::new();
        // 这里累计使用 95%，高于 90% 继续线。
        let d = check_token_budget(&mut t, None, Some(10_000), 9_500);
        assert!(matches!(
            d,
            TokenBudgetDecision::Stop {
                completion_event: None
            }
        ));
    }

    #[test]
    /// 已续跑三次且前后两次增长都低于 500 token 时，以收益递减原因停止。
    fn stops_on_diminishing_returns() {
        let mut t = BudgetTracker {
            continuation_count: 3,
            last_delta_tokens: 100, // 上一次增量也低于 DIMINISHING_THRESHOLD。
            last_global_turn_tokens: 4_800,
            started_at: Some(Instant::now()),
        };
        // 本次增量为 4900 - 4800 = 100；前一增量和本次都低于阈值。
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
