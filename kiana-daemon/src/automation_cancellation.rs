//! 由 `DaemonHost` 持有、显式启动的 AUT-17 取消编排服务。
//!
//! # 本文件负责什么
//!
//! 上游（组合根、调度器或测试）通过服务 handle 请求 cancel / fence / fetch；本文件把请求放进有界
//! Tokio channel，由**唯一** worker 顺序调用 `kiana-core` 的取消协调器：
//!
//! ```text
//! DaemonHost/service caller -> bounded mpsc -> run_worker
//!     -> AutomationDispatchStore::fetch_command        （权威命令，不信任调用方快照）
//!     -> AutomationCancellationStore::fetch_cancellation（当前取消事实，可为空）
//!     -> Domain begin_automation_cancellation()        （queued=NotExecuted / claimed=Requested）
//!     -> AutomationCancellationStore::commit_cancellation（先落库，再请求 stop）
//!     -> AutomationWorkerStopController::request_stop   （恰好一次）
//!     -> Domain fold_automation_stop_report()           （Stopped / NotExecuted / 未确认）
//!     -> AutomationCancellationStore::commit_cancellation（CAS 折叠结果）
//!     -> oneshot reply -> caller / existing ControlPlane route
//! ```
//!
//! 这是**协调 actor**（单任务按消息处理），不是定时器、不是模型循环，也不是第二个执行循环：它不
//! 执行 capability、不做授权判断、不写 EventLog；stop 只通过注入的 `AutomationWorkerStopController`
//! 边界发生，而该控制器仍应由现有 supervisor/provider 边界实现。`DaemonHost` 的构造只创建 service
//! 状态，需显式 start；当前仓库搜索只找到 Host 的公开启动包装与测试调用，没有找到 CLI 或其他生产
//! 入口调用该包装，因此不能据此说默认运行时已启动取消服务。
//!
//! # 关键纪律
//!
//! * queued（`Prepared`）取消判为 `NotExecuted`：不调用 stop 控制器，也不假装 effect 发生过。
//! * 已 claim 的 attempt 必须先落库取消事实再请求 stop；崩溃重启后同一代次的重放只会重发同一
//!   stop 请求（按代次幂等），不会重放 effect。
//! * 只有 stop 报告为 `Stopped` 且带证据时才返回 `Stopped`；控制器报错、报告绑定漂移或提交不确定
//!   一律计入 `recovery_required`。
//! * 迟到结果只能经 `record_automation_late_result` 隔离成 `Fenced`；该状态计入恢复，不能被说成
//!   stopped。
//! * channel 只限制排队消息数量，不是取消并发上限；worker 一次只处理一条请求。
//!
//! 当前唯一 `AutomationCancellationStore` 实现是 `MemoryAutomationCancellationStore`（CI 语义、
//! 进程内），因此本服务最多支撑 `local_behavior` 级别的证据，不构成 durable 或跨进程恢复证明。

use kiana_core::{
    cancel_automation_worker_command, record_automation_late_result, AutomationCancellationOutcome,
};
use kiana_domain::{
    AutomationCancelRequest, AutomationCancelState, AutomationCancellationCommit,
    AutomationCancellationFact, AutomationLateResult,
};
use kiana_ports::{
    AutomationCancellationStore, AutomationDispatchStore, AutomationWorkerStopController, PortError,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

/// 最多暂存 64 个尚待 worker 处理的取消请求；满时 `mpsc::Sender::send()` 等待空位形成 backpressure。
/// 这个数不表示可并行取消 64 个 execution，worker 一次只处理一条请求。
pub const AUTOMATION_CANCELLATION_CHANNEL_CAPACITY: usize = 64;

/// shutdown 时的汇总值；具体 execution/fact 不在这个计数报告里返回。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationCancellationShutdownReport {
    /// worker 成功记录 `Stopped`（已确认停止 + 证据）的取消数。
    pub stopped_count: usize,
    /// worker 成功记录 `NotExecuted`（确认无 effect）的取消数。
    pub not_executed_count: usize,
    /// 关闭时仍在途、结果未知或已隔离迟到结果、需要后续对账的取消数。
    pub recovery_required_count: usize,
}

/// 发给单 worker 的私有取消队列；每个请求变体都携带 oneshot 回信端。
enum CancellationServiceCommand {
    /// 重取命令与取消事实、按需请求一次 stop，并提交折叠结果。
    Cancel(
        String,
        AutomationCancelRequest,
        oneshot::Sender<Result<AutomationCancellationOutcome, PortError>>,
    ),
    /// 把一份迟到结果隔离成 `Fenced`；没有取消事实时稳定拒绝。
    Fence(
        AutomationLateResult,
        oneshot::Sender<Result<AutomationCancellationCommit, PortError>>,
    ),
    /// 纯读取当前取消事实；不执行 stop、不改状态。
    Fetch(
        String,
        u32,
        oneshot::Sender<Result<Option<AutomationCancellationFact>, PortError>>,
    ),
    /// 先处理队列中已接收的请求，再汇总仍需对账的取消事实。
    Shutdown(oneshot::Sender<Result<AutomationCancellationShutdownReport, PortError>>),
}

/// 一个已启动 worker 的生命周期句柄。
struct RunningService {
    /// 向 worker 投递请求的有界发送端。
    sender: mpsc::Sender<CancellationServiceCommand>,
    /// worker 退出通知；shutdown command 完成后发送 true。
    shutdown: watch::Sender<bool>,
    /// 用于等待 worker 退出并观察 Tokio join 错误。
    join: JoinHandle<()>,
}

/// 通过 `AutomationCancellationService` 的 async mutex 保护其可选运行实例。
#[derive(Default)]
struct ServiceState {
    /// None 表示尚未启动，或 shutdown 已取走运行句柄并正在异步收尾。
    running: Option<RunningService>,
}

/// 每个 DaemonHost 的有界取消编排 service handle；clone 会共享同一 lifecycle state。
#[derive(Clone, Default)]
pub struct AutomationCancellationService {
    /// Tokio mutex 保护 start/shutdown 与运行句柄替换；请求只在取 sender 时短暂持锁。
    state: Arc<AsyncMutex<ServiceState>>,
}

impl std::fmt::Debug for AutomationCancellationService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutomationCancellationService")
            .field(
                "channel_capacity",
                &AUTOMATION_CANCELLATION_CHANNEL_CAPACITY,
            )
            .finish_non_exhaustive()
    }
}

impl AutomationCancellationService {
    /// 创建尚未启动的 handle；不会 spawn worker，也不会自动取消任何命令。
    pub fn new() -> Self {
        Self::default()
    }

    /// 显式启动唯一 worker，并注入命令 store、取消 store 与 stop 控制器作为其三个边界。
    ///
    /// 【调用者】DaemonHost 装配层（`start_automation_cancellation_service`）或测试。
    /// 【输入】`store` 提供已提交的可重取命令；`cancellations` 保存取消事实与代次 CAS；`stop` 是
    /// 唯一的停止边界，只能返回 `AutomationStopReport`。
    /// 【输出】成功表示 worker 已 spawn；重复 start 返回 `PortError::Conflict`。
    /// 【副作用】创建容量为 64 的 mpsc channel、shutdown watch 和 Tokio task。
    /// 【设计边界】这里没有 scheduler timer；调用者必须显式发 `cancel()`。Service mutex 只阻止同时
    /// start，不负责跨进程 CAS 竞争，也不负责授权。
    pub async fn start(
        &self,
        store: Arc<dyn AutomationDispatchStore>,
        cancellations: Arc<dyn AutomationCancellationStore>,
        stop: Arc<dyn AutomationWorkerStopController>,
    ) -> Result<(), PortError> {
        let mut state = self.state.lock().await;
        if state.running.is_some() {
            return Err(PortError::Conflict(
                "automation_cancellation_service_already_started".to_owned(),
            ));
        }
        let (sender, receiver) = mpsc::channel(AUTOMATION_CANCELLATION_CHANNEL_CAPACITY);
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let join = tokio::spawn(run_worker(
            store,
            cancellations,
            stop,
            receiver,
            shutdown_receiver,
        ));
        state.running = Some(RunningService {
            sender,
            shutdown,
            join,
        });
        Ok(())
    }

    /// 取消一条已提交命令：queued → `NotExecuted`；claimed → 请求一次 stop 并折叠报告。
    ///
    /// 【失败】未启动返回 `Unavailable`；命令不存在、代次陈旧、epoch 漂移、终态拒绝或 CAS 冲突
    /// 返回稳定 `PortError`，且这些路径都不触碰 stop 控制器。
    pub async fn cancel(
        &self,
        command_id: &str,
        request: AutomationCancelRequest,
    ) -> Result<AutomationCancellationOutcome, PortError> {
        self.request(|reply| {
            CancellationServiceCommand::Cancel(command_id.to_owned(), request, reply)
        })
        .await
    }

    /// 记录一份迟到结果的隔离（`Fenced`）；绝不把执行改回任何“活着”的状态。
    pub async fn record_late_result(
        &self,
        late: AutomationLateResult,
    ) -> Result<AutomationCancellationCommit, PortError> {
        self.request(|reply| CancellationServiceCommand::Fence(late, reply))
            .await
    }

    /// 纯读取当前取消事实；用于对账与“提交不确定后的重取”。
    pub async fn fetch_cancellation(
        &self,
        execution_id: &str,
        attempt: u32,
    ) -> Result<Option<AutomationCancellationFact>, PortError> {
        self.request(|reply| {
            CancellationServiceCommand::Fetch(execution_id.to_owned(), attempt, reply)
        })
        .await
    }

    /// 复用公共 channel/reply 流程；业务状态变化实际发生在 worker 调用 store/stop 时。
    ///
    /// 【失败】service 未启动返回 Unavailable；worker 已关闭/丢失返回 Failed；sender 满时 await 空位。
    /// 【确认丢失】store 可能已完成修改而 oneshot 回信接收端已关闭，此时调用方收到 ack-lost 错误，
    /// 不能据此推断操作未发生；应查询/对账当前事实，直接重试同代次取消只会重发同一 stop 请求。
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, PortError>>) -> CancellationServiceCommand,
    ) -> Result<T, PortError> {
        let sender = {
            let state = self.state.lock().await;
            state
                .running
                .as_ref()
                .map(|running| running.sender.clone())
                .ok_or_else(|| {
                    PortError::Unavailable("automation_cancellation_service_not_started".to_owned())
                })?
        };
        let (reply, response) = oneshot::channel();
        sender.send(build(reply)).await.map_err(|_| {
            PortError::Failed("automation_cancellation_service_command_lost".to_owned())
        })?;
        response
            .await
            .map_err(|_| PortError::Failed("automation_cancellation_service_ack_lost".to_owned()))?
    }

    /// 停止 worker：先经同一 FIFO channel 投递 Shutdown，再等待其处理回执和 join。
    ///
    /// 已进入 channel 且排在 Shutdown 前的消息会先处理；随后仍然在途/未知/已隔离的取消被计入
    /// recovery-required，而不是被说成已完成。返回计数而不是逐项结果；没有启动的 service 返回三个零。
    pub async fn shutdown(&self) -> Result<AutomationCancellationShutdownReport, PortError> {
        let running = { self.state.lock().await.running.take() };
        let Some(running) = running else {
            return Ok(AutomationCancellationShutdownReport {
                stopped_count: 0,
                not_executed_count: 0,
                recovery_required_count: 0,
            });
        };
        let (reply, response) = oneshot::channel();
        running
            .sender
            .send(CancellationServiceCommand::Shutdown(reply))
            .await
            .map_err(|_| {
                PortError::Failed(
                    "automation_cancellation_service_shutdown_command_lost".to_owned(),
                )
            })?;
        let report = response.await.map_err(|_| {
            PortError::Failed("automation_cancellation_service_shutdown_ack_lost".to_owned())
        })??;
        let _ = running.shutdown.send(true);
        running.join.await.map_err(|_| {
            PortError::Failed("automation_cancellation_service_join_failed".to_owned())
        })?;
        Ok(report)
    }
}

/// 消费单个有界请求队列；`active` 只记录本 worker 见过的取消事实快照，不是 store 全量索引。
///
/// 所有消息串行调用 store/stop，避免本服务内的 handler 交错；跨 service/进程的原子性仍归 store。
/// worker restart 时 `active` 从空 Map 开始，持久/恢复状态只能由 store 的查询与拒绝行为提供。
async fn run_worker(
    store: Arc<dyn AutomationDispatchStore>,
    cancellations: Arc<dyn AutomationCancellationStore>,
    stop: Arc<dyn AutomationWorkerStopController>,
    mut receiver: mpsc::Receiver<CancellationServiceCommand>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut active = BTreeMap::<(String, u32), AutomationCancellationFact>::new();
    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else { break; };
                let stop_worker =
                    handle_command(&*store, &*cancellations, &*stop, command, &mut active).await;
                if stop_worker { break; }
            }
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() { break; }
            }
        }
    }
    receiver.close();
}

/// 对一个请求执行对应端口调用、更新本地 active 索引并回送结果。
///
/// 回信 send 失败不回滚已完成的 store 操作；`request()` 侧可能只看到 ack-lost，因此该边界可能
/// 需要对账。`Err` 路径不更新 active：调用方既不能推断 stop 已发出，也不能推断它未发出。
async fn handle_command(
    store: &dyn AutomationDispatchStore,
    cancellations: &dyn AutomationCancellationStore,
    stop: &dyn AutomationWorkerStopController,
    command: CancellationServiceCommand,
    active: &mut BTreeMap<(String, u32), AutomationCancellationFact>,
) -> bool {
    match command {
        CancellationServiceCommand::Cancel(command_id, request, reply) => {
            let result =
                cancel_automation_worker_command(store, cancellations, stop, &command_id, &request)
                    .await;
            if let Ok(outcome) = &result {
                let fact = match outcome {
                    AutomationCancellationOutcome::NotExecuted(fact)
                    | AutomationCancellationOutcome::Stopped(fact)
                    | AutomationCancellationOutcome::RecoveryRequired(fact)
                    | AutomationCancellationOutcome::Replayed(fact) => fact,
                };
                active.insert((fact.execution_id.clone(), fact.attempt), fact.clone());
            }
            let _ = reply.send(result);
            false
        }
        CancellationServiceCommand::Fence(late, reply) => {
            let result = record_automation_late_result(cancellations, &late).await;
            if let Ok(commit) = &result {
                active.insert(
                    (commit.fact.execution_id.clone(), commit.fact.attempt),
                    commit.fact.clone(),
                );
            }
            let _ = reply.send(result);
            false
        }
        CancellationServiceCommand::Fetch(execution_id, attempt, reply) => {
            let result = cancellations
                .fetch_cancellation(&execution_id, attempt)
                .await;
            if let Ok(Some(fact)) = &result {
                active.insert((fact.execution_id.clone(), fact.attempt), fact.clone());
            }
            let _ = reply.send(result);
            false
        }
        CancellationServiceCommand::Shutdown(reply) => {
            let mut stopped_count = 0;
            let mut not_executed_count = 0;
            let mut recovery_required_count = 0;
            for fact in active.values() {
                match fact.state {
                    AutomationCancelState::Stopped => stopped_count += 1,
                    AutomationCancelState::NotExecuted => not_executed_count += 1,
                    AutomationCancelState::Requested
                    | AutomationCancelState::Stopping
                    | AutomationCancelState::ResultUnknown
                    | AutomationCancelState::Fenced => recovery_required_count += 1,
                }
            }
            active.clear();
            let _ = reply.send(Ok(AutomationCancellationShutdownReport {
                stopped_count,
                not_executed_count,
                recovery_required_count,
            }));
            true
        }
    }
}
