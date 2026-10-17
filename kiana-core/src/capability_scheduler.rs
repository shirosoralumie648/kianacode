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

#[cfg(test)]
mod tests {
    //! CAP-23 行为夹具：直接驱动生产 `CapabilityAdmissionScheduler`，证明叠加写范围不会
    //! 并发执行、排队调用取消后不会启动、Unknown 结果继续隔离同范围、互不冲突的读/写可以
    //! 并行，以及释放/拒绝/队列上限的清理边界。
    //!
    //! 这些夹具只覆盖调度层的准入判定，不启动任何 handler、不写 EventLog、不申请 Cell 额度；
    //! Cell 预算/并发上限由 `cell_registry::tests` 的夹具覆盖。夹具里的 `ExecutionScope`
    //! 只为推导 footprint 而构造，未经过 Domain/Broker 的授权校验，因此不能当作授权证据。
    use super::*;
    use kiana_domain::{
        AuthenticatedPrincipalRef, ExecutionScope, ProjectIdentity, RequestId, ScopeLimit,
        ScopeSet, SessionId,
    };
    use std::sync::{Arc, PoisonError};
    use std::time::Duration;

    /// 测试专用视图：只读共享状态，用于断言某次准入确实没有发生，而不是只靠计时推断。
    impl CapabilityAdmissionScheduler {
        fn active_tickets(&self) -> Vec<u64> {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let mut tickets = state.active.keys().copied().collect::<Vec<_>>();
            tickets.sort_unstable();
            tickets
        }

        fn pending_tickets(&self) -> Vec<u64> {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.pending.iter().map(|item| item.ticket).collect()
        }

        fn quarantined_len(&self) -> usize {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.quarantined.len()
        }
    }

    fn lease_channel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
        watch::channel(false)
    }

    fn scope(permission_paths: &[&str], write_roots: &[&str]) -> ExecutionScope {
        let permission_scope = ScopeSet::new(
            ScopeDimension::NotApplicable,
            ScopeDimension::Restricted(
                permission_paths
                    .iter()
                    .map(|path| (*path).to_owned())
                    .collect(),
            ),
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )
        .expect("cap23 fixture scope set");
        let project = ProjectIdentity::new(
            "/tmp/cap23-project",
            "/tmp/cap23-project",
            None,
            None,
            "cap23-fixture-trust",
        )
        .expect("cap23 fixture project identity");
        ExecutionScope {
            schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
            version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
            principal: AuthenticatedPrincipalRef::local(),
            project,
            session_id: SessionId::new("cap23-fixture-session"),
            run_id: None,
            turn_id: None,
            cell_id: None,
            grant_refs: Vec::new(),
            budget_lease_id: None,
            work_packet_id: None,
            environment_id: "kiana-local".to_owned(),
            workspace_revision: None,
            permission_scope_digest: permission_scope.digest(),
            permission_scope,
            read_roots: Vec::new(),
            write_roots: write_roots.iter().map(|path| (*path).to_owned()).collect(),
            read_denies: Vec::new(),
            write_denies: Vec::new(),
            memory_scopes: Vec::new(),
            server_scopes: Vec::new(),
            network_policy: Vec::new(),
            authority_epoch: 1,
            trust_revision: "cap23-fixture".to_owned(),
            data_epoch: 1,
            cancellation_epoch: 1,
            deadline_unix_ms: u64::MAX,
            fencing_token: 1,
            catalog_digest: String::new(),
            action_digest: String::new(),
            scope_digest: String::new(),
        }
    }

    fn request(
        risk: RiskLevel,
        capability: CapabilityKind,
        operation: &str,
        permission_paths: &[&str],
        write_roots: &[&str],
    ) -> CapabilityRequest {
        let mut request = CapabilityRequest::new(
            RequestId::new(),
            capability,
            operation,
            serde_json::json!({"call_id": "cap23-call"}),
        )
        .with_risk(risk);
        request.execution_scope = Some(scope(permission_paths, write_roots));
        request
    }

    #[tokio::test]
    async fn overlapping_write_scopes_never_execute_concurrently() {
        let scheduler = Arc::new(CapabilityAdmissionScheduler::default());
        // shell.exec 只声明服务端授权写 scope；锁必须来自 `ExecutionScope.write_roots`，
        // 而不是从命令文本里猜文件清单。
        let shell = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Process,
            "shell.exec",
            &[],
            &["src"],
        );
        let (_shell_tx, shell_rx) = lease_channel();
        let shell_lease = scheduler
            .acquire(&shell, shell_rx)
            .await
            .expect("shell write admitted");
        assert_eq!(scheduler.active_tickets(), vec![shell_lease.ticket]);

        let patch = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src/lib.rs"],
        );
        let (_patch_tx, patch_rx) = lease_channel();
        let patch_scheduler = scheduler.clone();
        let mut patch_waiter =
            tokio::spawn(async move { patch_scheduler.acquire(&patch, patch_rx).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut patch_waiter)
                .await
                .is_err(),
            "an overlapping child write path must not enter the active set"
        );
        assert_eq!(
            scheduler.active_tickets(),
            vec![shell_lease.ticket],
            "only the shell write may hold the overlapping scope"
        );

        // 外部副作用按 Unknown 处理：即使本地写已经持有活动槽，也必须串行化。
        let external = request(
            RiskLevel::ExternalSideEffect,
            CapabilityKind::Network,
            "connector.invoke",
            &[],
            &[],
        );
        let (_external_tx, external_rx) = lease_channel();
        let external_scheduler = scheduler.clone();
        let mut external_waiter =
            tokio::spawn(async move { external_scheduler.acquire(&external, external_rx).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut external_waiter)
                .await
                .is_err(),
            "unknown external effects stay exclusive against an active local write"
        );

        scheduler.release(shell_lease, AdmissionOutcome::Known);
        let patch_lease = tokio::time::timeout(Duration::from_secs(5), patch_waiter)
            .await
            .expect("overlapping write admitted only after the holder released")
            .expect("patch waiter joined")
            .expect("patch lease");
        assert_eq!(scheduler.active_tickets(), vec![patch_lease.ticket]);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut external_waiter)
                .await
                .is_err(),
            "the unknown external effect still waits for the local write to finish"
        );

        scheduler.release(patch_lease, AdmissionOutcome::Known);
        let external_lease = tokio::time::timeout(Duration::from_secs(5), external_waiter)
            .await
            .expect("external effect admitted after every local write released")
            .expect("external waiter joined")
            .expect("external lease");
        assert_eq!(scheduler.active_tickets(), vec![external_lease.ticket]);
        scheduler.release(external_lease, AdmissionOutcome::Known);
        assert!(scheduler.active_tickets().is_empty());
    }

    #[tokio::test]
    async fn cancelled_queued_call_never_spawns() {
        let scheduler = Arc::new(CapabilityAdmissionScheduler::default());
        let holder = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src"],
        );
        let (_holder_tx, holder_rx) = lease_channel();
        let holder_lease = scheduler
            .acquire(&holder, holder_rx)
            .await
            .expect("holder admitted");

        let queued = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src/app.rs"],
        );
        let (queued_tx, queued_rx) = lease_channel();
        let queued_scheduler = scheduler.clone();
        let mut queued_waiter =
            tokio::spawn(async move { queued_scheduler.acquire(&queued, queued_rx).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut queued_waiter)
                .await
                .is_err(),
            "the queued call must wait behind the overlapping write"
        );
        assert_eq!(scheduler.pending_tickets().len(), 1);

        queued_tx.send(true).expect("cancel the queued call");
        let queued_result = tokio::time::timeout(Duration::from_secs(5), queued_waiter)
            .await
            .expect("cancelled waiter finished")
            .expect("queued waiter joined");
        match queued_result {
            Err(PortError::Conflict(reason)) => assert_eq!(reason, "cancelled:before_dispatch"),
            Err(other) => panic!("queued cancellation must fail closed, got {other:?}"),
            Ok(_) => panic!("a cancelled queued call must never be admitted"),
        }
        assert_eq!(
            scheduler.active_tickets(),
            vec![holder_lease.ticket],
            "the cancelled call must not hold a slot"
        );
        assert!(
            scheduler.pending_tickets().is_empty(),
            "cancellation removes the queued entry"
        );

        scheduler.release(holder_lease, AdmissionOutcome::Known);
        assert!(
            scheduler.active_tickets().is_empty(),
            "releasing the holder must not revive the cancelled queue entry"
        );
    }

    #[tokio::test]
    async fn pre_cancelled_requests_never_enter_the_queue_or_take_a_slot() {
        let scheduler = CapabilityAdmissionScheduler::default();
        let (cancel_tx, cancel_rx) = lease_channel();
        cancel_tx.send(true).expect("pre-cancel");
        let cancelled = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src"],
        );
        match scheduler.acquire(&cancelled, cancel_rx).await {
            Err(PortError::Conflict(reason)) => assert_eq!(reason, "cancelled:before_dispatch"),
            Err(other) => panic!("pre-cancelled request must fail closed, got {other:?}"),
            Ok(_) => panic!("a pre-cancelled request must not take a slot"),
        }
        assert!(scheduler.active_tickets().is_empty());
        assert!(scheduler.pending_tickets().is_empty());

        // 同一范围仍可由后续未被取消的请求正常使用。
        let (_later_tx, later_rx) = lease_channel();
        let lease = scheduler
            .acquire(&cancelled, later_rx)
            .await
            .expect("scope stays free after a pre-cancelled request");
        scheduler.release(lease, AdmissionOutcome::Known);
        assert!(scheduler.active_tickets().is_empty());
    }

    #[tokio::test]
    async fn read_only_requests_share_and_disjoint_writes_run_concurrently() {
        let scheduler = Arc::new(CapabilityAdmissionScheduler::default());
        let read_a = request(
            RiskLevel::ReadOnly,
            CapabilityKind::Filesystem,
            "context.search",
            &["src/lib.rs"],
            &[],
        );
        let (_read_a_tx, read_a_rx) = lease_channel();
        let read_a_lease = scheduler
            .acquire(&read_a, read_a_rx)
            .await
            .expect("read a admitted");
        let read_b = request(
            RiskLevel::ReadOnly,
            CapabilityKind::Filesystem,
            "context.search",
            &["src/lib.rs"],
            &[],
        );
        let (_read_b_tx, read_b_rx) = lease_channel();
        let read_b_lease = scheduler
            .acquire(&read_b, read_b_rx)
            .await
            .expect("a second read of the same path is admitted");
        let write_docs = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["docs"],
        );
        let (_write_docs_tx, write_docs_rx) = lease_channel();
        let write_docs_lease = scheduler
            .acquire(&write_docs, write_docs_rx)
            .await
            .expect("a disjoint write scope is admitted beside reads");
        assert_eq!(
            scheduler.active_tickets().len(),
            3,
            "reads and disjoint writes share the active set"
        );
        assert_eq!(scheduler.quarantined_len(), 0);

        // 与该读范围重叠的写必须等待：读子树被写范围覆盖时按冲突处理。
        let write_src = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src"],
        );
        let (_write_src_tx, write_src_rx) = lease_channel();
        let write_src_scheduler = scheduler.clone();
        let mut write_src_waiter =
            tokio::spawn(
                async move { write_src_scheduler.acquire(&write_src, write_src_rx).await },
            );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut write_src_waiter)
                .await
                .is_err(),
            "a write overlapping an active read subtree must not run concurrently"
        );

        scheduler.release(read_a_lease, AdmissionOutcome::Known);
        scheduler.release(read_b_lease, AdmissionOutcome::Known);
        scheduler.release(write_docs_lease, AdmissionOutcome::Known);
        let write_src_lease = tokio::time::timeout(Duration::from_secs(5), write_src_waiter)
            .await
            .expect("the overlapping write is admitted once the reads release")
            .expect("write waiter joined")
            .expect("write lease");
        scheduler.release(write_src_lease, AdmissionOutcome::Known);
        assert!(scheduler.active_tickets().is_empty());
    }

    #[tokio::test]
    async fn unknown_result_quarantines_its_scope_from_successors() {
        let scheduler = Arc::new(CapabilityAdmissionScheduler::default());
        let first = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src"],
        );
        let (_first_tx, first_rx) = lease_channel();
        let first_lease = scheduler
            .acquire(&first, first_rx)
            .await
            .expect("first admitted");
        scheduler.release(first_lease, AdmissionOutcome::Unknown);
        assert!(scheduler.active_tickets().is_empty());
        assert_eq!(scheduler.quarantined_len(), 1);

        let successor = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src/lib.rs"],
        );
        let (_successor_tx, successor_rx) = lease_channel();
        let successor_scheduler = scheduler.clone();
        let mut successor_waiter =
            tokio::spawn(
                async move { successor_scheduler.acquire(&successor, successor_rx).await },
            );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut successor_waiter)
                .await
                .is_err(),
            "an unresolved side effect must not hand its scope to a sibling request"
        );
        assert!(scheduler.active_tickets().is_empty());
        assert_eq!(scheduler.pending_tickets().len(), 1);

        // quarantine 只绑定该范围，不是全局停机：互不重叠的读仍然可以运行。
        let disjoint = request(
            RiskLevel::ReadOnly,
            CapabilityKind::Filesystem,
            "context.search",
            &["docs"],
            &[],
        );
        let (_disjoint_tx, disjoint_rx) = lease_channel();
        let disjoint_lease = scheduler
            .acquire(&disjoint, disjoint_rx)
            .await
            .expect("a disjoint read is admitted while the scope is quarantined");
        assert_eq!(scheduler.active_tickets(), vec![disjoint_lease.ticket]);
        scheduler.release(disjoint_lease, AdmissionOutcome::Known);

        successor_waiter.abort();
        let _ = successor_waiter.await;
    }

    #[tokio::test]
    async fn released_or_rejected_admissions_clean_up_their_slot_and_queue_is_bounded() {
        let scheduler = Arc::new(CapabilityAdmissionScheduler::default());
        let gate = request(
            RiskLevel::ExternalSideEffect,
            CapabilityKind::Network,
            "connector.invoke",
            &[],
            &[],
        );
        let (_gate_tx, gate_rx) = lease_channel();
        let gate_lease = scheduler
            .acquire(&gate, gate_rx)
            .await
            .expect("gate admitted");
        // 已知结果立即释放活动槽：同一范围可以再次准入。
        scheduler.release(gate_lease, AdmissionOutcome::Known);
        let (_gate_again_tx, gate_again_rx) = lease_channel();
        let gate_lease = scheduler
            .acquire(&gate, gate_again_rx)
            .await
            .expect("a known outcome releases the slot for the same scope");
        assert_eq!(scheduler.active_tickets(), vec![gate_lease.ticket]);

        // 排队有硬上限：达到上限后新请求必须被拒绝，而不是让队列无界增长。
        // 取消信号的 sender 必须活到断言之后：sender 被丢弃会让等待者走
        // `changed()` 失败分支并自行出队，那样就测不到队列上限。
        let mut waiters = Vec::new();
        let mut paddles = Vec::new();
        for _ in 0..MAX_PENDING_ADMISSIONS {
            let pending = request(
                RiskLevel::LocalWrite,
                CapabilityKind::Filesystem,
                "apply_patch",
                &[],
                &["src"],
            );
            let (pending_tx, pending_rx) = lease_channel();
            paddles.push(pending_tx);
            let pending_scheduler = scheduler.clone();
            waiters.push(tokio::spawn(async move {
                pending_scheduler.acquire(&pending, pending_rx).await
            }));
        }
        let mut attempts = 0;
        while scheduler.pending_tickets().len() < MAX_PENDING_ADMISSIONS && attempts < 10_000 {
            tokio::task::yield_now().await;
            attempts += 1;
        }
        assert_eq!(
            scheduler.pending_tickets().len(),
            MAX_PENDING_ADMISSIONS,
            "every spawned waiter must be queued before the bound is probed"
        );

        let overflowing = request(
            RiskLevel::LocalWrite,
            CapabilityKind::Filesystem,
            "apply_patch",
            &[],
            &["src"],
        );
        let (_overflow_tx, overflow_rx) = lease_channel();
        match scheduler.acquire(&overflowing, overflow_rx).await {
            Err(PortError::Conflict(reason)) => {
                assert_eq!(reason, "capability_scheduler_queue_full")
            }
            Err(other) => panic!("queue overflow must fail closed, got {other:?}"),
            Ok(_) => panic!("the admission queue must reject work beyond its bound"),
        }

        for waiter in waiters {
            waiter.abort();
        }
        scheduler.release(gate_lease, AdmissionOutcome::Known);
        assert!(scheduler.active_tickets().is_empty());
    }
}
