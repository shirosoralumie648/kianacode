//! Bounded capability admission, separate from capability execution.
//!
//! The scheduler only decides when an already-authorized request may cross the Broker boundary.
//! It never invokes a handler, starts a process, or creates a second model loop. CellRegistry
//! remains the authority for Cell budgets and durable path locks; this gate supplies the missing
//! process-local FIFO fairness and conservative in-flight footprint isolation.
//!
//! 【它在主执行链中的位置】
//! `kiana-core::capabilities::dispatch_capability_action()` 在已完成请求准备和授权后，
//! 先向这里申请 `AdmissionLease`，再申请 Cell 额度，最后才调用 Core 的 Broker dispatcher：
//!
//! ```text
//! Runner / 直接能力入口
//!   -> ControlPlane 准备请求、完成策略与审批判断
//!   -> 本文件 acquire()：排队、限制并发、隔离冲突范围
//!   -> CellRegistry：占用该 Cell 的额度/并发约束
//!   -> ControlPlane dispatcher -> Capability Broker -> handler
//!   -> 统一 finalizer 记录结果、结算 Cell lease、释放本文件的 AdmissionLease
//! ```
//!
//! 这里的队列和活动槽只存在于当前进程内，不替代 EventStore、CellRegistry 的额度账本、
//! 跨进程路径锁或 Daemon 的操作系统资源限制。footprint（资源影响范围）只用于保守判断
//! 两个已准入请求是否可能互相冲突，不是授权范围，也不会证明底层文件一定被锁住。
//! `Unknown` 结果会把原 footprint 留在隔离集合中，避免尚未查清副作用时把同一范围交给
//! 后续请求；当前模块没有删除隔离项的接口，因此它会保留到该 scheduler 实例结束。

use kiana_domain::{CapabilityKind, CapabilityRequest, RiskLevel, ScopeDimension};
use kiana_ports::PortError;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use tokio::sync::{watch, Notify};

/// 同一 scheduler 中最多同时占用的准入槽；这限制等待 Broker 的并发数量，不代表 Cell 配额。
const MAX_ACTIVE_ADMISSIONS: usize = 8;
/// 队列中最多等待的请求数；达到上限后立即拒绝，避免积压无界增长。
const MAX_PENDING_ADMISSIONS: usize = 256;

/// 请求对共享资源的影响类型。`Unknown` 表示无法安全缩小其影响范围，调度时按独占处理。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FootprintMode {
    /// 只读请求；两个只读请求可并行，即使读取范围相同。
    Read,
    /// 有路径范围的本地写入；只有项目路径锁规则判为冲突时才互斥。
    Write,
    /// 外部副作用或无法可靠表示的范围；与任何活动/隔离请求冲突。
    Unknown,
}

/// 调度器对单个请求计算出的保守资源范围。
///
/// `paths` 用规范化后的项目相对路径参与冲突比较。这里不访问文件系统、不检查路径存在性，
/// 也不替代 ControlPlane/Broker 做权限判断；权限已由上游授权链负责。
#[derive(Clone, Debug)]
struct ResourceFootprint {
    /// 决定是否允许读读并行，以及范围不明时是否一律互斥。
    mode: FootprintMode,
    /// 本地影响路径；`*` 表示范围无法安全收窄或路径输入无法规范化。
    paths: Vec<String>,
}

impl ResourceFootprint {
    /// 从已经进入 Core 派发准备阶段的能力请求推导调度范围。
    ///
    /// 【输入】
    /// 请求的风险级别、能力/操作类别、`ExecutionScope` 中的授权路径与写入根目录。
    /// 此处读取这些值只为并发互斥判断，不会重新授予或收窄权限。
    ///
    /// 【输出】
    /// 一个用于后续 `conflicts()` 比较的 footprint。没有足够信息时使用 `*`，宁可降低
    /// 并发度，也不假定两个请求彼此独立。
    ///
    /// 【核心规则】
    /// 只读风险映射为 Read；本地写入映射为 Write；外部副作用/关键风险映射为 Unknown。
    /// 网络能力和列出的 Connector 操作无论风险字段为何都提升为 Unknown，因为远端副作用
    /// 不能用本地路径范围描述。Write 优先采用显式 `write_roots`，为空才回退到授权路径。
    ///
    /// 【易误改边界】
    /// scope 缺失、路径范围为不适用、或路径规范化失败都会退化为 `*`；不要把它们解释为
    /// “没有资源冲突”。这里刻意扩大互斥范围，不修改请求本身，也不代替上游的路径授权。
    fn from_request(request: &CapabilityRequest) -> Self {
        let mode = match request.risk {
            RiskLevel::ReadOnly => FootprintMode::Read,
            RiskLevel::LocalWrite => FootprintMode::Write,
            RiskLevel::ExternalSideEffect | RiskLevel::Critical => FootprintMode::Unknown,
        };
        let mode = if matches!(request.capability, CapabilityKind::Network)
            || matches!(
                request.operation.as_str(),
                "connector.invoke"
                    | "connector.manage"
                    | "connector.health"
                    | "connector.mcp_handshake"
            ) {
            FootprintMode::Unknown
        } else {
            mode
        };
        let paths = match mode {
            FootprintMode::Read => request
                .execution_scope
                .as_ref()
                .and_then(|scope| match &scope.permission_scope.paths {
                    ScopeDimension::Restricted(paths) => Some(paths.as_slice()),
                    ScopeDimension::NotApplicable => None,
                })
                .map(canonical_paths)
                .unwrap_or_else(|| vec!["*".to_owned()]),
            FootprintMode::Write => request
                .execution_scope
                .as_ref()
                .and_then(|scope| {
                    if scope.write_roots.is_empty() {
                        match &scope.permission_scope.paths {
                            ScopeDimension::Restricted(paths) => Some(paths.as_slice()),
                            ScopeDimension::NotApplicable => None,
                        }
                    } else {
                        Some(scope.write_roots.as_slice())
                    }
                })
                .map(canonical_paths)
                .unwrap_or_else(|| vec!["*".to_owned()]),
            // External effects have no trustworthy filesystem footprint. Treating them as
            // exclusive is the conservative rule; a future provider-specific footprint must be
            // explicitly modeled before it can narrow this lock.
            FootprintMode::Unknown => vec!["*".to_owned()],
        };
        Self { mode, paths }
    }

    /// 判断两个准入请求能否同时处于活动状态。
    ///
    /// 两个 Read 不冲突；只要任一方为 Unknown 就互斥；其余情况交给 Domain 的统一路径锁
    /// 比较规则。返回 `true` 只表示调度器必须串行化这两个请求，不代表底层资源锁已取得。
    fn conflicts(&self, other: &Self) -> bool {
        if self.mode == FootprintMode::Read && other.mode == FootprintMode::Read {
            return false;
        }
        if self.mode == FootprintMode::Unknown || other.mode == FootprintMode::Unknown {
            return true;
        }
        self.paths.iter().any(|left| {
            other
                .paths
                .iter()
                .any(|right| kiana_domain::path_locks_conflict(left, right))
        })
    }
}

/// 把请求中的路径转成稳定、可比较的集合。
///
/// 去除首尾空白并调用 Domain 路径规范化 helper 后排序/去重，确保同一范围不会因书写顺序
/// 或重复项不同而绕开冲突比较。非法路径、空集合或包含 `*` 时统一退化为全局范围；这是
/// 调度层的 fail-closed 互斥选择，不是输入校验错误，也不表示调用者获得了全盘访问权限。
fn canonical_paths(paths: &[String]) -> Vec<String> {
    let mut normalized = paths
        .iter()
        .map(|path| {
            let path = path.trim();
            if path == "*" {
                "*".to_owned()
            } else {
                kiana_domain::normalize_role_path(path).unwrap_or_else(|| "*".to_owned())
            }
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() || normalized.iter().any(|path| path == "*") {
        vec!["*".to_owned()]
    } else {
        normalized
    }
}

/// 一个尚未取得活动槽的排队项。
struct PendingAdmission {
    /// 进程内排队令牌，用于等待者定位自己并在取消时删除自己的排队项。
    ticket: u64,
    /// 上游请求推导出的冲突范围。
    footprint: ResourceFootprint,
}

/// 所有准入请求共用的短时状态快照。
struct SchedulerState {
    /// 下一个本地排队令牌。到达 `u64::MAX` 后当前实现会饱和，后续令牌可能重复；该极端
    /// 生命周期边界会破坏令牌唯一性，不能把它当作外部稳定 ID 或持久化序号。
    next_ticket: u64,
    /// 按进入顺序保存的等待项；只阻止与更早请求冲突的项，因此兼容请求可以并行通过。
    pending: VecDeque<PendingAdmission>,
    /// 已拿到准入槽、尚未完成最终结算的请求 footprint，键为其本地 ticket。
    active: HashMap<u64, ResourceFootprint>,
    /// 副作用结果未知的 footprint。它们继续挡住冲突请求，避免未知结果被误当作已释放。
    quarantined: Vec<ResourceFootprint>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            next_ticket: 1,
            pending: VecDeque::new(),
            active: HashMap::new(),
            quarantined: Vec::new(),
        }
    }
}

#[derive(Default)]
/// ControlPlane 共用的进程内并发准入器。
///
/// `std::sync::Mutex` 只保护很短的内存状态修改，所有加锁区间都在 `.await` 之前结束；
/// `Notify` 用于让排队任务等待状态变化，而不是轮询或创建新的执行循环。Mutex poison 时
/// 当前实现选择取回内部状态继续运行，并不校验或重建可能被 panic 留下的状态。
pub(crate) struct CapabilityAdmissionScheduler {
    state: Mutex<SchedulerState>,
    /// 状态变化时唤醒等待者；被唤醒后必须重新检查锁内条件，通知本身不是准入许可。
    notify: Notify,
}

/// 一次已占用的调度槽。
///
/// 该 lease 不是授权凭证或 Cell 额度 lease。调用者必须在结果已被确定分类后调用
/// `release()`；此类型没有 `Drop` 自动回收逻辑，若调用者提前丢弃且不释放，活动槽会留在
/// scheduler 状态中。
pub(crate) struct AdmissionLease {
    ticket: u64,
}

/// 调度器释放活动槽时对最终结果确定性的最小判断。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionOutcome {
    /// 已知请求完成、失败、取消或确定未执行，可以移除活动 footprint。
    Known,
    /// 无法确认副作用是否发生，活动 footprint 要转入 quarantine 并继续阻止冲突请求。
    Unknown,
}

impl CapabilityAdmissionScheduler {
    /// 将已准备好的请求放入公平、有界的准入队列，并等待兼容的活动槽。
    ///
    /// 【调用者】
    /// `kiana-core/src/capabilities.rs::dispatch_capability_action()` 在调用 Broker dispatcher
    /// 前调用；成功后还会占用 CellRegistry lease。这里的顺序让并发准入与预算/权限检查
    /// 分属不同职责。
    ///
    /// 【输入】
    /// - `request`：Core 已完成准备阶段的 `CapabilityRequest`，用于计算 footprint；
    /// - `cancellation`：来自 Run/请求生命周期的 Tokio watch 接收端，表示等待期间是否取消。
    ///
    /// 【输出】
    /// 成功返回 `AdmissionLease`，表示请求进入 `active` 并持有一个并发槽；这仍不代表
    /// Broker 已执行，也不代表 handler 成功。等待期间取消或等待队列达到 256 项时返回
    /// `PortError::Conflict`，由 Core 作为派发前拒绝处理。
    ///
    /// 【副作用】
    /// 修改本进程内的 pending/active 集合并唤醒其他 waiter；不执行 handler、不写 EventLog、
    /// 不启动子进程，也不创建 Tokio task。
    ///
    /// 【核心流程】
    /// 1. 已取消则在入队前返回；否则计算保守 footprint；
    /// 2. 在短临界区检查队列上限、分配 ticket 并追加到队尾；
    /// 3. 每轮检查取消状态，再检查所有更早冲突项、活动项、隔离项和活动槽上限；
    /// 4. 条件满足时把自己的项从 pending 移入 active 并返回 lease；否则异步等待通知或取消；
    /// 5. 被唤醒后重新从共享状态计算条件，绝不把通知事件直接当成“轮到我”。
    ///
    /// 【公平性与并发原因】
    /// 较早且冲突的请求会阻止后来的同范围请求插队；互不冲突的请求无需等待队首，可并行
    /// 通过，避免一个长时间写操作阻塞完全无关的读取范围。活动上限与待处理上限分别控制
    /// handler 前的并发数量和排队内存/负载。
    pub(crate) async fn acquire(
        &self,
        request: &CapabilityRequest,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<AdmissionLease, PortError> {
        if *cancellation.borrow() {
            return Err(PortError::Conflict("cancelled:before_dispatch".to_owned()));
        }
        let footprint = ResourceFootprint::from_request(request);
        let ticket = {
            // Mutex 只包围同步状态更新；不在 guard 存活期间 await，避免阻塞 Tokio worker。
            // 若之前有 panic 导致 poison，这里按现有策略恢复 guard，但不重验其内部一致性。
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.pending.len() >= MAX_PENDING_ADMISSIONS {
                return Err(PortError::Conflict(
                    "capability_scheduler_queue_full".to_owned(),
                ));
            }
            let ticket = state.next_ticket;
            state.next_ticket = state.next_ticket.saturating_add(1).max(1);
            state
                .pending
                .push_back(PendingAdmission { ticket, footprint });
            ticket
        };
        // 入队也是队列状态变化。唤醒者会重新检查“更早冲突项”条件，不会仅因通知直接获准。
        self.notify.notify_waiters();

        loop {
            if *cancellation.borrow() {
                self.cancel_pending(ticket);
                return Err(PortError::Conflict("cancelled:before_dispatch".to_owned()));
            }
            // 在检查共享状态前创建通知 future，避免检查结束到真正 await 之间遗漏状态变化。
            // Notify 只负责唤醒；通知后仍需重新加锁、重算所有冲突条件。
            let notified = self.notify.notified();
            let admitted = {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(index) = state.pending.iter().position(|item| item.ticket == ticket) {
                    let candidate = state.pending[index].footprint.clone();
                    let earlier_conflict = state
                        .pending
                        .iter()
                        .take(index)
                        .any(|item| item.footprint.conflicts(&candidate));
                    let active_conflict = state
                        .active
                        .values()
                        .any(|active| active.conflicts(&candidate));
                    let quarantine_conflict = state
                        .quarantined
                        .iter()
                        .any(|quarantined| quarantined.conflicts(&candidate));
                    if !earlier_conflict
                        && !active_conflict
                        && !quarantine_conflict
                        && state.active.len() < MAX_ACTIVE_ADMISSIONS
                    {
                        let pending = state.pending.remove(index).expect("candidate was present");
                        state.active.insert(ticket, pending.footprint);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if admitted {
                return Ok(AdmissionLease { ticket });
            }
            tokio::select! {
                _ = notified => {}
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        self.cancel_pending(ticket);
                        return Err(PortError::Conflict("cancelled:before_dispatch".to_owned()));
                    }
                }
            }
        }
    }

    /// 结束一次已准入操作：确定结果则释放槽，结果未知则把范围隔离。
    ///
    /// 【调用者】Core 的统一 `finalize_capability_action()` 在写入能力终态事实、结算 Cell
    /// lease 后调用，保证调度层使用最终的 `ExecutionStatus` 决定释放方式。
    ///
    /// `Known` 会从 active 删除 footprint；`Unknown` 会删除 active 项并把 footprint 追加到
    /// quarantine。后续相同/冲突范围的请求会继续等待，即使并发槽已释放。当前模块没有
    /// 清除 quarantine 的方法，因此未知结果造成的隔离会持续到此 scheduler 实例被销毁。
    /// 若 `lease.ticket` 不在 active 中，当前实现不产生错误，只会唤醒等待者。
    pub(crate) fn release(&self, lease: AdmissionLease, outcome: AdmissionOutcome) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(footprint) = state.active.remove(&lease.ticket) {
            if outcome == AdmissionOutcome::Unknown {
                // Unknown is not a safe release: this in-process quarantine has no removal path in
                // the current module, so the conflicting footprint remains blocked for this
                // scheduler's lifetime. Do not treat it as a normal failed request.
                state.quarantined.push(footprint);
            }
        }
        drop(state);
        self.notify.notify_waiters();
    }

    /// 从等待队列移除某个被取消的请求，并通知其他等待者重新评估队列条件。
    ///
    /// 只会影响尚在 pending 的 ticket；已经进入 active 的请求必须由其调用方走
    /// `release()` 统一收尾。ticket 不存在时是幂等空操作。
    fn cancel_pending(&self, ticket: u64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.pending.retain(|item| item.ticket != ticket);
        drop(state);
        self.notify.notify_waiters();
    }
}
