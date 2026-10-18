//! Core 侧的 Run→execution 停止集合封装。
//!
//! [`kiana_domain::RunExecutionStopSet`] 是确定性状态机；本模块把它与一个 `watch` 唤醒通道绑在
//! 一起，使 `await_capability_stop` 能「等所有仍在执行的项都确认停止」，而不是等一个会被后登记
//! 执行覆盖的布尔量。取消事实仍由 lifecycle 先写入 EventLog；这里只承载进程内的停止收敛。
//!
//! 本模块不发送取消信号、不停止子进程，也不写事件。它拒绝「用一个布尔量代表整个集合」的做法，
//! 因此 `resolve` 必须逐项记录，`run.cancelled` 只在 `resolution()` 返回 `Confirmed` 后才成立。

use super::*;
use kiana_domain::{ExecutionId, ExecutionStopKind, RunExecutionStopSet, StopSetResolution};

/// 一个 Run 的停止集合与唤醒通道。
///
/// 集合本身在 `Mutex` 内；`wake` 只承载一个单调递增的版本号，用于叫醒正在 `await_capability_stop`
/// 的等待者，让它们重新读取集合状态。它不携带「已停止」的结论——结论只由集合给出。
pub(crate) struct RunStopChannel {
    set: Mutex<RunExecutionStopSet>,
    wake: watch::Sender<u64>,
}

impl RunStopChannel {
    /// 为一个 Run 创建空集合与唤醒通道。
    pub(crate) fn new(run_id: RunId) -> Self {
        let (wake, _receiver) = watch::channel(0u64);
        Self {
            set: Mutex::new(RunExecutionStopSet::new(run_id)),
            wake,
        }
    }

    fn lock_set(&self) -> std::sync::MutexGuard<'_, RunExecutionStopSet> {
        self.set.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn notify(&self) {
        self.wake
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    /// 订阅唤醒通道；等待者据此在集合变化时重新读取状态。
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.wake.subscribe()
    }

    /// 登记一个即将执行的能力或 hook。
    ///
    /// 【turn 边界】同一个 Run 会跨多个 turn 复用 run_id。若旧集合已经完全收敛（上一轮全部
    /// `Stopped`/`Completed`）且本轮尚未请求停止，这里在登记新执行前把它重置，避免新的 turn
    /// 读到上一轮的停止项；只要已请求停止（`stop_requested`）或仍有 `Pending`/`Unconfirmed`
    /// 项（取消正在进行）就绝不重置——取消后登记的项必须是 `Stopping`，不能借重置变回 `Inflight`。
    pub(crate) fn register(&self, execution_id: ExecutionId, kind: ExecutionStopKind) {
        {
            let mut set = self.lock_set();
            if !set.stop_requested() && set.resolution() == StopSetResolution::Confirmed {
                let run_id = set.run_id();
                *set = RunExecutionStopSet::new(run_id);
            }
            let _ = set.register(execution_id, kind);
        }
        self.notify();
    }

    /// 请求停止集合内所有仍未解决的项；幂等。
    pub(crate) fn begin_stop(&self) {
        self.lock_set().begin_stop();
        self.notify();
    }

    /// 记录一次执行的收敛结果。
    ///
    /// 【分支】取消未作用于该项时记为 `Completed`；取消已请求且拿到停止证据时记为 `Stopped`；
    /// 取消已请求但证据缺失时记为 `StopUnconfirmed`。取消请求之后到达的完成会被集合拒绝
    /// （迟到结果只能追加诊断事实），这里忽略该拒绝并保留原状态。
    pub(crate) fn resolve(&self, execution_id: ExecutionId, stop_requested: bool, confirmed: bool) {
        {
            let mut set = self.lock_set();
            if !stop_requested {
                let _ = set.mark_completed(execution_id);
            } else if confirmed {
                let _ = set.confirm_stop_without_report(execution_id);
            } else {
                let _ = set.mark_stop_unconfirmed(execution_id);
            }
        }
        self.notify();
    }

    /// 当前集合的整体收敛判断。
    pub(crate) fn resolution(&self) -> StopSetResolution {
        self.lock_set().resolution()
    }

    /// 当前集合快照，供清理计划读取每项的停止状态。
    ///
    /// 这是只读副本：调用方据此决定资源应记为 `Released` 还是 `IsolatedUnknown`，
    /// 不会因读取而改变任何执行状态。
    pub(crate) fn snapshot(&self) -> RunExecutionStopSet {
        self.lock_set().clone()
    }
}

impl ControlPlane {
    /// 登记一个 run 下的执行项到停止集合，必要时创建通道。
    pub(crate) fn register_execution_stop(
        &self,
        run_id: RunId,
        execution_id: ExecutionId,
        kind: ExecutionStopKind,
    ) {
        let channel = self.run_stop_channel(run_id);
        channel.register(execution_id, kind);
    }

    /// 记录执行项的收敛结果；没有登记项时忽略（幂等取消可能先于登记）。
    pub(crate) fn resolve_execution_stop(
        &self,
        run_id: RunId,
        execution_id: ExecutionId,
        stop_requested: bool,
        confirmed: bool,
    ) {
        let channel = self
            .capability_stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run_id)
            .cloned();
        if let Some(channel) = channel {
            channel.resolve(execution_id, stop_requested, confirmed);
        }
    }

    /// 读取一个 Run 的停止集合快照；没有登记过执行时返回空集合。
    ///
    /// 空集合表示「本进程没有已知在飞执行」，它**不是**外部副作用的停止证明；清理路径必须
    /// 结合资源自身的 `requires_stop_confirmation` 判断。
    pub(crate) fn execution_stop_snapshot(&self, run_id: RunId) -> RunExecutionStopSet {
        self.capability_stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run_id)
            .map(|channel| channel.snapshot())
            .unwrap_or_else(|| RunExecutionStopSet::new(run_id))
    }

    fn run_stop_channel(&self, run_id: RunId) -> Arc<RunStopChannel> {
        let mut channels = self
            .capability_stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        channels
            .entry(run_id)
            .or_insert_with(|| Arc::new(RunStopChannel::new(run_id)))
            .clone()
    }
}
