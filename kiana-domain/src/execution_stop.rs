//! Run 级取消的 execution 停止集合。
//!
//! 本文件属于 Domain 层，只定义「一个 Run 下，有哪些 execution/hook 必须被停止、它们各自的
//! 停止状态」，以及把迟到结果挡在终态之外的确定性状态机。它不发送取消信号、不等待进程、
//! 不写 EventLog，也不执行对账；这些属于 Core/Daemon 的调用方。
//!
//! # 为什么是「集合」而不是一个布尔量
//!
//! 一次 Run 在一个时刻可能同时有多个执行中的能力调用，还可能并行运行受控 hook。若只保留
//! 一个布尔「已停止」标记，后登记的执行会覆盖先登记的执行，`run.cancelled` 就可能在前一个
//! 执行仍未确认停止时被写出。因此每一项都要有自己的状态、epoch 和（可用时）[`StopReport`]；
//! 集合只有在所有仍欠停止的项都到达 `Stopped`，或在取消请求前已 `Completed` 时才算 resolved。
//!
//! # 关键不变量
//!
//! - 取消请求只会把 `Inflight`/`Stopping` 项标记为 `Stopping`，绝不直接把任何项变成 `Stopped`；
//!   `Stopped` 只来自确认的停止证据。
//! - 取消请求之后到达的完成是迟到结果，`mark_completed` 会拒绝；迟到事实只能追加为诊断。
//! - 「阻塞提交任务被丢弃」（future 被 drop）不等于目标进程已停止，`note_blocking_commit_dropped`
//!   只记录诊断，不会让集合变成 resolved。
//! - 可写 scope 只有在 `Stopped` 或（取消请求前的）`Completed` 时才能释放。
//!
//! [StopReport]: crate::StopReport

use crate::{ExecutionId, RunId, StopReport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 单个执行项在停止集合中的状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStopState {
    /// 已登记、仍在执行；取消尚未对该项请求停止，也不欠一次停止证据。
    Inflight,
    /// 已请求停止，但尚无确认的停止证据。
    Stopping,
    /// 已有确认的停止证据（`StopReport.confirmed == true` 或 Core 侧等价证据）。
    Stopped,
    /// 在取消请求之前自行到达终态；不再欠一次停止。
    Completed,
    /// 已明确无法确认停止；集合必须保持 Unknown，不能当作 resolved。
    StopUnconfirmed,
}

/// 执行类别：普通 capability 与受控 hook 都必须被同一个取消集合覆盖。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStopKind {
    /// 一次能力调用。
    Capability,
    /// 一次受控 hook 子进程执行。
    Hook,
}

/// 停止集合中的一项：绑定稳定执行身份、登记时的取消 epoch、状态与可选停止回执。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionStopEntry {
    /// 被跟踪的执行身份。
    pub execution_id: ExecutionId,
    /// 执行类别（能力或 hook）。
    pub kind: ExecutionStopKind,
    /// 登记时所属的 Run 取消 epoch；用于把迟到报告与当前取消轮次区分开。
    pub epoch: u64,
    /// 当前停止状态。
    pub state: ExecutionStopState,
    /// 是否已经对该项请求停止。
    pub stop_requested: bool,
    /// 确认停止时记录的回执；`None` 表示 Core 侧只记录了等价布尔证据，或尚未确认。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_report: Option<StopReport>,
    /// 诊断：该执行对应的阻塞提交任务被丢弃过。它不能被读作已确认停止。
    pub blocking_commit_dropped: bool,
}

/// 对整个停止集合的收敛判断。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopSetResolution {
    /// 仍有未解决项（登记中、停止中，或等待确认）。
    Pending,
    /// 所有项都已解决：`Stopped` 或取消请求前的 `Completed`。
    Confirmed,
    /// 至少一项明确无法确认停止；调用方必须按 `result_unknown` 处理并保持 fencing。
    Unconfirmed,
}

/// 一个 Run 下的 execution 停止集合。
#[derive(Clone, Debug)]
pub struct RunExecutionStopSet {
    run_id: RunId,
    epoch: u64,
    stop_requested: bool,
    entries: BTreeMap<ExecutionId, ExecutionStopEntry>,
}

impl RunExecutionStopSet {
    /// 为一个 Run 创建空集合；epoch 从 0 开始，尚未请求停止。
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            epoch: 0,
            stop_requested: false,
            entries: BTreeMap::new(),
        }
    }

    /// 被跟踪的 Run。
    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    /// 当前取消 epoch；每次首次请求停止时前进一格。
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 是否已经对集合请求停止。
    pub fn stop_requested(&self) -> bool {
        self.stop_requested
    }

    /// 集合中的执行项数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 集合是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按执行身份读取登记项。
    pub fn entry(&self, execution_id: &ExecutionId) -> Option<&ExecutionStopEntry> {
        self.entries.get(execution_id)
    }

    /// 按执行身份读取登记项的稳定身份列表（升序）。
    pub fn execution_ids(&self) -> Vec<ExecutionId> {
        self.entries.keys().copied().collect()
    }

    /// 登记一个即将执行的能力或 hook。
    ///
    /// 【输出】新项成功登记返回 `Ok(())`；同一身份重复登记且类别一致是幂等操作；nil 身份或
    /// 同一身份被改成另一类别返回稳定错误。
    ///
    /// 【边界】取消已经请求后再登记不会被当成放行：`dispatch` 前仍会被取消检查拒绝，这里把新项
    /// 直接记为 `Stopping`，保证它在集合里仍欠一次停止确认。本函数不启动执行、不发送信号。
    pub fn register(
        &mut self,
        execution_id: ExecutionId,
        kind: ExecutionStopKind,
    ) -> Result<(), String> {
        if execution_id.as_uuid().is_nil() {
            return Err("execution_stop_id_invalid".to_owned());
        }
        if let Some(existing) = self.entries.get(&execution_id) {
            if existing.kind == kind {
                return Ok(());
            }
            return Err("execution_stop_kind_conflict".to_owned());
        }
        let state = if self.stop_requested {
            ExecutionStopState::Stopping
        } else {
            ExecutionStopState::Inflight
        };
        self.entries.insert(
            execution_id,
            ExecutionStopEntry {
                execution_id,
                kind,
                epoch: self.epoch,
                state,
                stop_requested: self.stop_requested,
                stop_report: None,
                blocking_commit_dropped: false,
            },
        );
        Ok(())
    }

    /// 请求停止集合内所有仍未解决的执行项，并返回需要停止的目标。
    ///
    /// 首次调用把 Run 取消 epoch 前进一步；重复调用不会再次前进（幂等取消）。已完成或已确认
    /// 停止的项不在返回值里。本函数只改变内存状态，不发送信号、不等待停止。
    pub fn begin_stop(&mut self) -> Vec<ExecutionId> {
        if !self.stop_requested {
            self.stop_requested = true;
            self.epoch = self.epoch.wrapping_add(1);
        }
        let mut targets = Vec::new();
        for entry in self.entries.values_mut() {
            match entry.state {
                ExecutionStopState::Inflight | ExecutionStopState::Stopping => {
                    entry.state = ExecutionStopState::Stopping;
                    entry.stop_requested = true;
                    targets.push(entry.execution_id);
                }
                ExecutionStopState::Stopped
                | ExecutionStopState::Completed
                | ExecutionStopState::StopUnconfirmed => {}
            }
        }
        targets
    }

    /// 用确认的 [`StopReport`] 把对应执行标记为 `Stopped`。
    ///
    /// 【关键不变量】回执必须自洽且 `confirmed == true`，其 `execution_id` 必须能解析并匹配集合中
    /// 已登记项。取消后到达的迟到回执如果对应已 `Completed` 的项会被拒绝；重复确认是幂等的。
    pub fn confirm_stop(&mut self, report: StopReport) -> Result<(), String> {
        report.validate()?;
        if !report.confirmed {
            return Err("stop_report_not_confirmed".to_owned());
        }
        let execution_id = ExecutionId::parse_str(&report.execution_id)
            .ok_or_else(|| "stop_report_execution_id_invalid".to_owned())?;
        let entry = self
            .entries
            .get_mut(&execution_id)
            .ok_or_else(|| "stop_report_unknown_execution".to_owned())?;
        match entry.state {
            ExecutionStopState::Completed => {
                return Err("late_stop_report_after_completion".to_owned());
            }
            ExecutionStopState::Stopped => return Ok(()),
            ExecutionStopState::Inflight
            | ExecutionStopState::Stopping
            | ExecutionStopState::StopUnconfirmed => {
                entry.state = ExecutionStopState::Stopped;
                entry.stop_requested = true;
                entry.stop_report = Some(report);
                Ok(())
            }
        }
    }

    /// 记录 Core 侧「已确认停止」的等价布尔证据，不伪造 [`StopReport`]。
    ///
    /// Core 的停止证明来自已校验的 capability/runner 结果；daemon 侧有真实回执时应使用
    /// [`RunExecutionStopSet::confirm_stop`]。已 `Completed` 的项拒绝迟到停止；重复确认幂等。
    pub fn confirm_stop_without_report(&mut self, execution_id: ExecutionId) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&execution_id)
            .ok_or_else(|| "stop_unknown_execution".to_owned())?;
        match entry.state {
            ExecutionStopState::Completed => Err("late_stop_after_completion".to_owned()),
            ExecutionStopState::Stopped => Ok(()),
            ExecutionStopState::Inflight
            | ExecutionStopState::Stopping
            | ExecutionStopState::StopUnconfirmed => {
                entry.state = ExecutionStopState::Stopped;
                entry.stop_requested = true;
                Ok(())
            }
        }
    }

    /// 明确标记某项「停止无法确认」。集合随后必须报告 `Unconfirmed`，调用方按 Unknown 处理。
    pub fn mark_stop_unconfirmed(&mut self, execution_id: ExecutionId) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&execution_id)
            .ok_or_else(|| "stop_unknown_execution".to_owned())?;
        match entry.state {
            ExecutionStopState::Stopped | ExecutionStopState::Completed => Ok(()),
            ExecutionStopState::Inflight
            | ExecutionStopState::Stopping
            | ExecutionStopState::StopUnconfirmed => {
                entry.state = ExecutionStopState::StopUnconfirmed;
                entry.stop_requested = true;
                Ok(())
            }
        }
    }

    /// 记录某项在取消请求前自行完成。
    ///
    /// 【先拒绝】取消请求之后才到达的完成是迟到结果，只能追加诊断事实，不能把集合改成已解决，
    /// 因此对 `Stopping`/`StopUnconfirmed` 返回错误；对已 `Stopped` 的项也拒绝。
    pub fn mark_completed(&mut self, execution_id: ExecutionId) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&execution_id)
            .ok_or_else(|| "stop_unknown_execution".to_owned())?;
        match entry.state {
            ExecutionStopState::Stopping | ExecutionStopState::StopUnconfirmed => {
                Err("late_completion_after_cancel_requires_reconciliation".to_owned())
            }
            ExecutionStopState::Stopped => Err("completion_after_confirmed_stop".to_owned()),
            ExecutionStopState::Inflight | ExecutionStopState::Completed => {
                entry.state = ExecutionStopState::Completed;
                Ok(())
            }
        }
    }

    /// 记录「该执行的阻塞提交任务被丢弃」这一诊断事实。
    ///
    /// 丢弃一个 future 不等于目标进程或外部副作用已经停止，因此它不改变停止状态，也不会让
    /// [`RunExecutionStopSet::resolution`] 变成 `Confirmed`。这一诊断只用于事后对账。
    pub fn note_blocking_commit_dropped(
        &mut self,
        execution_id: ExecutionId,
    ) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&execution_id)
            .ok_or_else(|| "stop_unknown_execution".to_owned())?;
        entry.blocking_commit_dropped = true;
        Ok(())
    }

    /// 释放该项持有的可写 scope（路径锁/写集租约）前的检查。
    ///
    /// 只有已 `Stopped`，或在取消请求**之前**已 `Completed` 的项才允许释放；`Inflight`、`Stopping`
    /// 和 `StopUnconfirmed` 都必须继续保留 fencing。该函数只返回判断，不实际释放锁。
    pub fn release_writable_scope(&self, execution_id: ExecutionId) -> Result<(), String> {
        let entry = self
            .entries
            .get(&execution_id)
            .ok_or_else(|| "stop_unknown_execution".to_owned())?;
        match entry.state {
            ExecutionStopState::Stopped => Ok(()),
            ExecutionStopState::Completed if !entry.stop_requested => Ok(()),
            _ => Err("writable_scope_release_requires_confirmed_stop".to_owned()),
        }
    }

    /// 仍欠停止确认的执行身份（`Stopping` 或 `StopUnconfirmed`）。
    pub fn unconfirmed(&self) -> Vec<ExecutionId> {
        self.entries
            .values()
            .filter(|entry| {
                matches!(
                    entry.state,
                    ExecutionStopState::Stopping | ExecutionStopState::StopUnconfirmed
                )
            })
            .map(|entry| entry.execution_id)
            .collect()
    }

    /// 是否所有项都已解决（`Stopped` 或取消请求前的 `Completed`）。空集合视为已解决。
    pub fn all_resolved(&self) -> bool {
        self.entries.values().all(|entry| {
            matches!(
                entry.state,
                ExecutionStopState::Stopped | ExecutionStopState::Completed
            )
        })
    }

    /// 对整个集合的收敛判断，供 Core 决定 `run.cancelled` 还是 `run.result_unknown`。
    pub fn resolution(&self) -> StopSetResolution {
        if self
            .entries
            .values()
            .any(|entry| entry.state == ExecutionStopState::StopUnconfirmed)
        {
            StopSetResolution::Unconfirmed
        } else if self.all_resolved() {
            StopSetResolution::Confirmed
        } else {
            StopSetResolution::Pending
        }
    }
}
