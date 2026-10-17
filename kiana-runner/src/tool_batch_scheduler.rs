//! # 文件职责
//!
//! 本文件实现 H16 的有界并行工具组执行器：把一个 domain 的 `ToolBatchPlan` 按组顺序执行，
//! 只读组用有界 worker 池并发派发，独占组作为屏障先 drain 再单独执行；所有结果按源顺序
//! （source ordinal）结算为模型历史，而不是按完成先后。
//!
//! # 在系统中的位置
//!
//! ```text
//! 模型返回有序 ModelToolCall 列表
//!   -> Runner 校验调用形状并调用 domain::plan_tool_batch() 得到 ToolBatchPlan
//!   -> 本执行器按组顺序调用调用方提供的 admit()（派发前一刻重新检查授权）
//!   -> 允许的调用才进入有界 worker 池；独占组前后各 drain 一次
//!   -> 每个 execute() 结果按源 ordinal 回填，形成模型历史
//! ```
//!
//! ⚠ 这个执行器**不执行任何工具副作用**：它只调度调用方传入的闭包。`execute` 必须继续走
//! ControlPlane → Broker 的能力请求路径；这里不产生第二条执行循环，也不把批次级允许当成
//! 兄弟调用的授权。真正的产品循环接线、跨进程资源 CAS 与 external/live/physical proof
//! 仍留待后续组合步骤，当前 baseline 只提供并验证这个调度契约。
//!
//! # 权限边界
//!
//! 并行额度是 run、部署与角色的交集（见 `ToolBatchParallelismCap`），只用于决定工作组
//! 宽度，不扩张任何调用的权限。撤销、取消或预检查失败通过 `admit()` 返回
//! `ToolBatchAdmission::NotExecuted`，执行器据此**不再派发**同批尚未开始的调用
//! （deny/cancel 阻止尚未派发的工作）。已在 H13 落账的调用通过
//! `ToolBatchAdmission::AlreadyCommitted` 直接复用，不重新执行。

use kiana_domain::{ToolBatchParallelismCap, ToolBatchPlan, ToolSchedulingMode};
use std::thread;

/// 批次因兄弟调用被拒绝而停止时，剩余调用回填的稳定原因。
const REASON_BLOCKED: &str = "blocked_after_not_executed";
/// worker 线程 panic 时回填的稳定原因；出现它说明调度本身失败，不是工具的已知失败。
const REASON_WORKER_PANICKED: &str = "tool_batch_worker_panicked";

/// 派发前对单个调用的准入判定，由调用方在“临执行前”重新检查后给出。
///
/// 【为什么不是一个布尔】撤销（`NotExecuted`）与“已在 H13 落账、无需重跑”（`AlreadyCommitted`）
/// 都会阻止真正执行，但含义完全不同：前者要收口尚未派发的兄弟调用，后者是恢复语义。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolBatchAdmission {
    /// 允许派发该调用。
    Allow,
    /// 该调用的结果已提交，直接复用记录，不重新执行（crash 后不重跑已完成调用）。
    AlreadyCommitted,
    /// 不得派发（authority 撤销、取消、预检查失败）；批量调度据此停止派发剩余调用。
    NotExecuted(String),
}

/// 单个已派发调用的结果分类。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolBatchCallState {
    /// 已执行并取得已知结果。
    Completed,
    /// 已执行但得到已知失败；失败不会停止派发兄弟调用。
    Failed(String),
    /// 复用已提交结果，未重新执行。
    Reused,
    /// 未派发（准入拒绝、批次因兄弟调用被拒绝而停止，或 worker 失败）。
    NotExecuted(String),
}

/// 按源顺序记录的单调用结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolBatchCallOutcome {
    /// 源序号（assistant 输出顺序）；历史按它排序。
    pub ordinal: usize,
    /// 所属组号。
    pub group_id: u32,
    /// provider 调用 ID。
    pub call_id: String,
    /// 结果分类。
    pub state: ToolBatchCallState,
}

/// 一次批次执行的汇总。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolBatchRunReport {
    /// 按源序号（不是完成顺序）排列的历史。
    pub history: Vec<ToolBatchCallOutcome>,
    /// 实际派发并执行的调用数。
    pub dispatched: usize,
    /// 复用已提交结果的调用数。
    pub reused: usize,
    /// 未派发的调用数（准入拒绝或批次停止）。
    pub not_executed: usize,
    /// 调度器观察到的最大在途（已派发未回收）调用数；它是缓存上界的记账值，不是并发证明。
    pub max_in_flight: usize,
    /// 是否因为某个调用被拒绝而停止了后续派发。
    pub halted: bool,
}

/// 有界并行批次执行器。
///
/// 构造时校验计划；`run()` 才真正调度。执行器自身不持有权限，也不启动能力请求。
#[derive(Clone, Debug)]
pub struct ToolBatchScheduler {
    plan: ToolBatchPlan,
    cap: ToolBatchParallelismCap,
}

impl ToolBatchScheduler {
    /// 用一个已校验的 `ToolBatchPlan` 和并行额度上限构造执行器。
    ///
    /// 【失败情况】计划不满足 domain 合同（空组、组号断裂、call ID 重复、只读组超限等）时
    /// 返回稳定错误字符串；不构造部分可用的执行器。
    pub fn new(plan: ToolBatchPlan, cap: ToolBatchParallelismCap) -> Result<Self, String> {
        plan.validate()?;
        Ok(Self { plan, cap })
    }

    /// 只读访问底层计划，供调用方回显分组意图。
    pub fn plan(&self) -> &ToolBatchPlan {
        &self.plan
    }

    /// 生效的并行额度上限。
    pub fn cap(&self) -> ToolBatchParallelismCap {
        self.cap
    }

    /// 按组顺序执行整个批次，返回按源序号排列的历史。
    ///
    /// 【调用者】Runner 的工具批次派发路径；当前 baseline 由 focused fixtures 驱动。
    /// 【输入】`admit(ordinal, call_id)` 在派发前一刻重新检查授权；`execute(ordinal, call_id)`
    /// 代表真正的能力请求路径。两者都被执行器按需调用。
    /// 【核心流程】逐组处理：独占组先 drain 全部在途调用再单独执行（独占屏障）；只读组按
    /// `dispatch_width()` 限制同时派发数，超过宽度先回收最早的在途调用（因此已完成的乱序结果
    /// 至多缓存 `width` 条）。任一调用被 `admit()` 判为 `NotExecuted` 时，同批尚未派发的调用
    /// 全部记为未执行并停止派发。
    /// 【输出】`history` 恒按源 ordinal 排列，与完成顺序无关。
    /// 【副作用】只启动调用方提供的 `execute` 的 worker 线程；不访问文件、网络或 ControlPlane。
    pub fn run<A, E>(&self, admit: A, execute: E) -> ToolBatchRunReport
    where
        A: Fn(usize, &str) -> ToolBatchAdmission,
        E: Fn(usize, &str) -> ToolBatchCallState + Sync,
    {
        let units = self.plan.dispatch_units().expect("validated plan");
        let mut history: Vec<Option<ToolBatchCallOutcome>> = vec![None; units.len()];
        let mut dispatched = 0usize;
        let mut reused = 0usize;
        let mut not_executed = 0usize;
        let mut max_in_flight = 0usize;
        let mut halted = false;
        let execute = &execute;

        thread::scope(|scope| {
            let mut in_flight = Vec::new();
            let mut next = 0usize;
            for group in &self.plan.groups {
                let width = group.dispatch_width(self.cap).max(1) as usize;
                // 独占屏障的一侧：先结束前一只读组的所有在途调用，之后才启动独占调用。
                if group.mode == ToolSchedulingMode::Exclusive {
                    while !in_flight.is_empty() {
                        join_oldest(&mut in_flight, &mut history);
                    }
                }
                for _ in &group.call_ids {
                    let unit = &units[next];
                    next += 1;
                    let ordinal = unit.ordinal;
                    if halted {
                        history[ordinal] = Some(ToolBatchCallOutcome {
                            ordinal,
                            group_id: unit.group_id,
                            call_id: unit.call_id.clone(),
                            state: ToolBatchCallState::NotExecuted(REASON_BLOCKED.to_owned()),
                        });
                        not_executed += 1;
                        continue;
                    }
                    match admit(ordinal, &unit.call_id) {
                        ToolBatchAdmission::AlreadyCommitted => {
                            history[ordinal] = Some(ToolBatchCallOutcome {
                                ordinal,
                                group_id: unit.group_id,
                                call_id: unit.call_id.clone(),
                                state: ToolBatchCallState::Reused,
                            });
                            reused += 1;
                        }
                        ToolBatchAdmission::NotExecuted(reason) => {
                            history[ordinal] = Some(ToolBatchCallOutcome {
                                ordinal,
                                group_id: unit.group_id,
                                call_id: unit.call_id.clone(),
                                state: ToolBatchCallState::NotExecuted(reason),
                            });
                            not_executed += 1;
                            halted = true;
                        }
                        ToolBatchAdmission::Allow => {
                            // 有界 pool：达到宽度先回收最早的在途调用，限制乱序结果缓存。
                            while in_flight.len() >= width {
                                join_oldest(&mut in_flight, &mut history);
                            }
                            let group_id = unit.group_id;
                            let call_for_thread = unit.call_id.clone();
                            let handle = scope.spawn(move || {
                                let state = execute(ordinal, &call_for_thread);
                                ToolBatchCallOutcome {
                                    ordinal,
                                    group_id,
                                    call_id: call_for_thread,
                                    state,
                                }
                            });
                            in_flight.push(handle);
                            max_in_flight = max_in_flight.max(in_flight.len());
                            dispatched += 1;
                        }
                    }
                }
                // 独占屏障的另一侧：独占调用必须完全结束后，才允许下一组只读调用启动。
                if group.mode == ToolSchedulingMode::Exclusive {
                    while !in_flight.is_empty() {
                        join_oldest(&mut in_flight, &mut history);
                    }
                }
            }
            while !in_flight.is_empty() {
                join_oldest(&mut in_flight, &mut history);
            }
        });

        let mut outcomes = Vec::new();
        for (ordinal, unit) in units.iter().enumerate() {
            match history[ordinal].take() {
                Some(outcome) => outcomes.push(outcome),
                None => outcomes.push(ToolBatchCallOutcome {
                    ordinal,
                    group_id: unit.group_id,
                    call_id: unit.call_id.clone(),
                    state: ToolBatchCallState::NotExecuted(REASON_WORKER_PANICKED.to_owned()),
                }),
            }
        }

        ToolBatchRunReport {
            history: outcomes,
            dispatched,
            reused,
            not_executed,
            max_in_flight,
            halted,
        }
    }
}

/// 回收最早派发的在途调用，把结果写回历史（FIFO，保证历史按源顺序结算）。
///
/// 【为什么按 FIFO 而不是“谁先完成谁先记”】H16 要求历史按 ordinal 排列。按派发顺序回收，
/// 意味着较早的调用一定先落历史；同时最多只有 `width` 个结果处于“已完成但未结算”状态。
/// 【失败情况】worker panic 时 `join()` 返回错误，此处不写历史；调用方在收尾时用
/// `NotExecuted("tool_batch_worker_panicked")` 补齐该槽位。
fn join_oldest<'scope>(
    in_flight: &mut Vec<thread::ScopedJoinHandle<'scope, ToolBatchCallOutcome>>,
    history: &mut [Option<ToolBatchCallOutcome>],
) {
    if in_flight.is_empty() {
        return;
    }
    let handle = in_flight.remove(0);
    if let Ok(outcome) = handle.join() {
        history[outcome.ordinal] = Some(outcome);
    }
}
