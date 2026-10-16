//! 由 `DaemonHost` 持有、显式启动的 workflow queue 协调 worker。
//!
//! 上游通过 Host 暴露的 service handle 请求 claim/heartbeat/effect/fence/reclaim/tick；本文件把命令放进有界
//! Tokio channel，再由唯一 worker 顺序调用 `WorkflowQueueStore`。Store 负责队列状态的原子转换，Core 负责
//! dispatch tuple 的最终核对；lease 返回给 existing ControlPlane route（既有 ControlPlane 命令路径）；
//! 本服务不直接执行 effect，也不启动模型或 Provider 路径：
//!
//! ```text
//! DaemonHost/service caller -> bounded mpsc command -> run_worker
//!     -> WorkflowQueueStore -> oneshot reply -> caller / existing ControlPlane route
//! ```
//!
//! 这是协调 actor（单任务按消息处理），不是定时器：`tick(limit)` 只有被调用时才读取 ready 项。`DaemonHost` 的
//! 构造只创建 service 状态，需显式 start；当前仓库搜索只找到 Host 的公开启动包装定义，没有找到 CLI 或其他
//! 生产入口调用该包装，因此不能据此说默认运行时已启动 queue worker。channel 只限制排队消息数量，不是 effect 并发上限。
//! Tokio 的 `mpsc` 是异步多发送端/单接收端队列，满时让发送方等待；`oneshot` 为单条命令回传一次结果，
//! `watch` 广播停止信号，`spawn` 把 worker future 交给 Tokio runtime 调度。`Arc` 让克隆 handle 共享状态，
//! `AsyncMutex` 只保护运行句柄，不应跨网络/Store await 长期占锁。当前 workspace 中唯一的 `WorkflowQueueStore`
//! 实现是 `MemoryWorkflowQueueStore`，它只见于 fixture；尚无 durable queue adapter 或 service 产品调用链。

use kiana_domain::{
    WorkflowQueueClaimRequest, WorkflowQueueEffectRequest, WorkflowQueueFenceRequest,
    WorkflowQueueHeartbeatRequest, WorkflowQueueLease, WorkflowQueueLeaseStatus,
    WorkflowQueueReclaimRequest,
};
use kiana_ports::{PortError, WorkflowQueueStore};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

/// 最多暂存 64 个尚待 worker 处理的命令；满时 `mpsc::Sender::send()` 等待空位形成 backpressure。
/// 这个数不表示可并行运行 64 个 effect，worker 一次只处理一条命令。
pub const WORKFLOW_SERVICE_CHANNEL_CAPACITY: usize = 64;

/// shutdown 时的汇总值；具体 item/error 不在这个计数报告里返回。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowQueueShutdownReport {
    /// Store 成功 fence 且结果非 Unknown 的 lease 数。
    pub fenced_count: usize,
    /// fence 返回 Unknown 或发生任意 Store 错误的 lease 数；需要后续恢复/核对。
    pub recovery_required_count: usize,
}

/// 发给单 worker 的私有命令队列；每个请求变体都携带 oneshot 回信端。
enum WorkflowQueueCommand {
    /// 首次认领 Ready Claim。
    Claim(
        WorkflowQueueClaimRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    /// 延长当前 owner 的租约。
    Heartbeat(
        WorkflowQueueHeartbeatRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    /// 折叠一条 effect 状态观察。
    Effect(
        WorkflowQueueEffectRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    /// 收尾并 fence 当前租约。
    Fence(
        WorkflowQueueFenceRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    /// 把安全到期项转交给新 owner。
    Reclaim(
        WorkflowQueueReclaimRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    /// 按调用者给的 limit 读取 ready Claim；不是定时器 tick。
    Tick(
        usize,
        oneshot::Sender<Result<Vec<kiana_domain::WorkflowQueueClaimContract>, PortError>>,
    ),
    /// 先处理队列中已接收的命令，再 fence worker 当前记录的 active lease。
    Shutdown(
        Option<u64>,
        oneshot::Sender<Result<WorkflowQueueShutdownReport, PortError>>,
    ),
}

/// 一个已启动 worker 的生命周期句柄。
struct RunningService {
    /// 向 worker 投递命令的有界发送端。
    sender: mpsc::Sender<WorkflowQueueCommand>,
    /// worker 退出通知；shutdown command 完成后发送 true。
    shutdown: watch::Sender<bool>,
    /// 用于等待 worker 退出并观察 Tokio join 错误。
    join: JoinHandle<()>,
}

/// 通过 `WorkflowQueueService` 的 async mutex 保护其可选运行实例。
#[derive(Default)]
struct ServiceState {
    /// None 表示尚未启动，或 shutdown 已取走运行句柄并正在异步收尾。
    running: Option<RunningService>,
}

/// 每个 DaemonHost 的有界队列协调 service handle；clone 会共享同一 lifecycle state。
#[derive(Clone, Default)]
pub struct WorkflowQueueService {
    /// Tokio mutex 保护 start/shutdown 与运行句柄替换；请求只在取 sender 时短暂持锁。
    state: Arc<AsyncMutex<ServiceState>>,
}

impl std::fmt::Debug for WorkflowQueueService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowQueueService")
            .field("channel_capacity", &WORKFLOW_SERVICE_CHANNEL_CAPACITY)
            .finish_non_exhaustive()
    }
}

impl WorkflowQueueService {
    /// 创建尚未启动的 handle；不会 spawn worker，也不会自动定时读取队列。
    pub fn new() -> Self {
        Self::default()
    }

    /// 显式启动唯一 worker，并把传入 QueueStore 作为所有队列操作的状态后端。
    ///
    /// 【调用者】DaemonHost 装配层；当前仓库没有找到 CLI/其他生产入口调用该 Host 包装的代码。
    /// 【输入】共享的 `WorkflowQueueStore` trait object。
    /// 【输出】成功表示 worker 已 spawn；重复 start 返回 `PortError::Conflict`。
    /// 【副作用】创建容量为 64 的 mpsc channel、shutdown watch 和 Tokio task。
    /// 【设计边界】这里没有 scheduler timer；调用者必须显式发 `tick()`。Service mutex 只阻止同时 start，
    /// 不负责和跨进程 Store 竞争，也不负责 effect 执行。
    pub async fn start(&self, store: Arc<dyn WorkflowQueueStore>) -> Result<(), PortError> {
        let mut state = self.state.lock().await;
        if state.running.is_some() {
            return Err(PortError::Conflict(
                "workflow_service_already_started".to_owned(),
            ));
        }
        let (sender, receiver) = mpsc::channel(WORKFLOW_SERVICE_CHANNEL_CAPACITY);
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let join = tokio::spawn(run_worker(store, receiver, shutdown_receiver));
        state.running = Some(RunningService {
            sender,
            shutdown,
            join,
        });
        Ok(())
    }

    /// 将 Ready Claim 排入 worker，等待 Store 原子认领结果。
    pub async fn claim(
        &self,
        request: WorkflowQueueClaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Claim(request, reply))
            .await
    }

    /// 将 heartbeat 请求排入 worker；只有 Store 成功返回的新 lease 才更新 worker 的 active 快照。
    pub async fn heartbeat(
        &self,
        request: WorkflowQueueHeartbeatRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Heartbeat(request, reply))
            .await
    }

    /// 让 Store 按 lease 合同记录 effect 观察；终态/Unknown 返回后从本地 active 集合移除。
    pub async fn record_effect(
        &self,
        request: WorkflowQueueEffectRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Effect(request, reply))
            .await
    }

    /// 通过 Store 撤销一个 lease 的 dispatch 能力。
    pub async fn fence(
        &self,
        request: WorkflowQueueFenceRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Fence(request, reply))
            .await
    }

    /// 请求 Store 原子检查到期、未启动 effect 与新 fence 后转交新 owner。
    pub async fn reclaim(
        &self,
        request: WorkflowQueueReclaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Reclaim(request, reply))
            .await
    }

    /// 显式向 Store 请求最多 `limit` 条 ready Claim；不会等待定时器或自动执行 Claim。
    pub async fn tick(
        &self,
        limit: usize,
    ) -> Result<Vec<kiana_domain::WorkflowQueueClaimContract>, PortError> {
        self.request(|reply| WorkflowQueueCommand::Tick(limit, reply))
            .await
    }

    /// 复用公共 channel/reply 流程；业务状态变化实际发生在 worker 调用 Store 时。
    ///
    /// 【失败】service 未启动返回 Unavailable；worker 已关闭/丢失返回 Failed；sender 满时 await 空位。
    /// 【确认丢失】Store 可能已完成修改而 oneshot 回信接收端已关闭，此时调用方收到 ack-lost 错误，不能据此
    /// 推断操作未发生；应查询/对账 Store 当前状态，直接重试可能撞上已提交的租约状态。
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, PortError>>) -> WorkflowQueueCommand,
    ) -> Result<T, PortError> {
        let sender = {
            let state = self.state.lock().await;
            state
                .running
                .as_ref()
                .map(|running| running.sender.clone())
                .ok_or_else(|| PortError::Unavailable("workflow_service_not_started".to_owned()))?
        };
        let (reply, response) = oneshot::channel();
        sender
            .send(build(reply))
            .await
            .map_err(|_| PortError::Failed("workflow_service_command_lost".to_owned()))?;
        response
            .await
            .map_err(|_| PortError::Failed("workflow_service_ack_lost".to_owned()))?
    }

    /// 停止 worker：先经同一 FIFO channel 投递 Shutdown，再等待其处理回执和 join。
    ///
    /// 已进入 channel 且排在 Shutdown 前的消息会先处理；随后 active lease 尝试 fence，运行中/未知 effect
    /// 被计入 recovery-required。返回计数而不是逐项结果；没有启动的 service 返回两个零。
    pub async fn shutdown(&self) -> Result<WorkflowQueueShutdownReport, PortError> {
        self.shutdown_at(None).await
    }

    /// 以可选服务端观察时刻执行 shutdown；None 时每个 lease 使用其最近 heartbeat 时间。
    ///
    /// 【失败】shutdown command/reply 丢失或 Tokio join 失败时返回 PortError。由于状态会先从 `running` 取走，
    /// 错误后 service handle 已不再通过共享 state 可见；并发 shutdown 可能看到 None 并返回零报告，并发 start
    /// 也可能在旧 worker 完成 drain 前启动新 worker。
    /// 维护者若更改此顺序，需同时考虑启动/停止并发和消息 drain，不能只把 Option 赋值搬动。
    pub async fn shutdown_at(
        &self,
        observed_at_unix_ms: Option<u64>,
    ) -> Result<WorkflowQueueShutdownReport, PortError> {
        let running = { self.state.lock().await.running.take() };
        let Some(running) = running else {
            return Ok(WorkflowQueueShutdownReport {
                fenced_count: 0,
                recovery_required_count: 0,
            });
        };
        let (reply, response) = oneshot::channel();
        running
            .sender
            .send(WorkflowQueueCommand::Shutdown(observed_at_unix_ms, reply))
            .await
            .map_err(|_| PortError::Failed("workflow_service_shutdown_command_lost".to_owned()))?;
        let report = response
            .await
            .map_err(|_| PortError::Failed("workflow_service_shutdown_ack_lost".to_owned()))??;
        let _ = running.shutdown.send(true);
        running
            .join
            .await
            .map_err(|_| PortError::Failed("workflow_service_join_failed".to_owned()))?;
        Ok(report)
    }
}

/// 消费单个有界命令队列；`active` 只记录本 worker 成功操作过的 lease 快照，不是 Store 全量索引。
///
/// 所有消息串行调用 Store，避免本服务内的 command handler 交错；跨 service/进程的原子性仍归 Store。
/// worker restart 时 active 从空 Map 开始，持久/恢复状态只能由 Store 的查询与拒绝行为提供。
async fn run_worker(
    store: Arc<dyn WorkflowQueueStore>,
    mut receiver: mpsc::Receiver<WorkflowQueueCommand>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut active = BTreeMap::<String, WorkflowQueueLease>::new();
    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else { break; };
                let stop = handle_command(&*store, command, &mut active).await;
                if stop { break; }
            }
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() { break; }
            }
        }
    }
    receiver.close();
}

/// 对一个命令执行对应 Store API、更新本地 active 索引并回送结果。
///
/// 回信 send 失败不回滚已完成的 Store 操作；`request()` 侧可能只看到 ack-lost，因此该边界可能需要对账。
async fn handle_command(
    store: &dyn WorkflowQueueStore,
    command: WorkflowQueueCommand,
    active: &mut BTreeMap<String, WorkflowQueueLease>,
) -> bool {
    match command {
        WorkflowQueueCommand::Claim(request, reply) => {
            let result = store.claim(request).await;
            if let Ok(lease) = &result {
                active.insert(lease.item_id.clone(), lease.clone());
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Heartbeat(request, reply) => {
            let result = store.heartbeat(request).await;
            if let Ok(lease) = &result {
                active.insert(lease.item_id.clone(), lease.clone());
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Effect(request, reply) => {
            let result = store.record_effect(request).await;
            if let Ok(lease) = &result {
                if lease.status == WorkflowQueueLeaseStatus::Active {
                    active.insert(lease.item_id.clone(), lease.clone());
                } else {
                    active.remove(&lease.item_id);
                }
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Fence(request, reply) => {
            let result = store.fence(request).await;
            if let Ok(lease) = &result {
                active.remove(&lease.item_id);
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Reclaim(request, reply) => {
            let result = store.reclaim(request).await;
            if let Ok(lease) = &result {
                active.insert(lease.item_id.clone(), lease.clone());
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Tick(limit, reply) => {
            let _ = reply.send(store.ready(limit).await);
            false
        }
        WorkflowQueueCommand::Shutdown(observed_at_unix_ms, reply) => {
            let mut fenced_count = 0;
            let mut recovery_required_count = 0;
            let leases = active.values().cloned().collect::<Vec<_>>();
            for lease in leases {
                // shutdown 未提供时钟时使用最近 heartbeat，保持时间合同有效但不声称这是实际 wall clock。
                let observed_at = observed_at_unix_ms.unwrap_or(lease.heartbeat_at_unix_ms);
                let result = store
                    .fence(WorkflowQueueFenceRequest {
                        item_id: lease.item_id.clone(),
                        owner_id: lease.owner_id.clone(),
                        fence_token: lease.fence_token,
                        authority_epoch: lease.authority_epoch,
                        observed_at_unix_ms: observed_at,
                    })
                    .await;
                match result {
                    // Unknown 不能算普通成功 fence；无论 Store 返回 Unknown 还是失败，都必须留给恢复路径。
                    Ok(next) if next.status == WorkflowQueueLeaseStatus::ResultUnknown => {
                        recovery_required_count += 1;
                    }
                    Ok(_) => fenced_count += 1,
                    Err(_) => recovery_required_count += 1,
                }
            }
            active.clear();
            let _ = reply.send(Ok(WorkflowQueueShutdownReport {
                fenced_count,
                recovery_required_count,
            }));
            true
        }
    }
}
