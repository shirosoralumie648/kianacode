//! 由 `DaemonHost` 持有、显式启动的 AUT-15 worker dispatch/observation 协调服务。
//!
//! # 本文件负责什么
//!
//! 上游（组合根、调度器或测试）通过服务 handle 请求 commit / dispatch / fetch；本文件把命令放进
//! 有界 Tokio channel，由**唯一** worker 顺序调用 `kiana-core` 的
//! `dispatch_automation_worker_command()`。该协调器内部按固定顺序访问端口：
//!
//! ```text
//! DaemonHost/service caller -> bounded mpsc -> run_worker
//!     -> AutomationDispatchStore::fetch_command      （重取可重取命令，不信任调用方快照）
//!     -> Domain claim 判定 + Store::claim_command    （唯一 worker/attempt CAS）
//!     -> AutomationWorkerEffectRunner::run_dispatch_attempt（恰好一次 effect）
//!     -> Store::observe_command                      （结构化结果，Observed/Unknown）
//!     -> oneshot reply -> caller / existing ControlPlane route
//! ```
//!
//! 这是**协调 actor**（单任务按消息处理），不是定时器、不是模型循环，也不是第二个执行循环：
//! 它不执行 capability、不做授权判断、不写 EventLog；effect 只通过注入的 runner 边界发生，而
//! 该 runner 仍应由现有 ControlPlane/Broker 脊柱实现。`DaemonHost` 的构造只创建 service 状态，
//! 需显式 start；当前仓库搜索只找到 Host 的公开启动包装与测试调用，没有找到 CLI 或其他生产
//! 入口调用该包装，因此不能据此说默认运行时已启动 dispatch worker。
//!
//! # 关键纪律
//!
//! * 命令先进入 store（可重取事实），再由 worker 重取并 claim；崩溃重启后携带过期快照的 worker
//!   走 `Replayed` 路径，不会重复执行 effect。
//! * 只有 CAS claim 成功才调用 runner；拒绝、重放、并发失败都保持 0 次 effect。
//! * 结果提交不确定时保存 `Unknown` 状态并计入 `recovery_required`，绝不伪装成功。
//! * channel 只限制排队消息数量，不是 effect 并发上限；worker 一次只处理一条命令。
//!
//! 当前唯一 `AutomationDispatchStore` 实现是 `MemoryAutomationDispatchStore`（CI 语义、进程内），
//! 因此本服务最多支撑 `local_behavior` 级别的证据，不构成 durable 或跨进程恢复证明。

use kiana_core::{dispatch_automation_worker_command, AutomationWorkerDispatchOutcome};
use kiana_domain::{AutomationDispatchCommand, AutomationDispatchCommit};
use kiana_ports::{AutomationDispatchStore, AutomationWorkerEffectRunner, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

/// 最多暂存 64 个尚待 worker 处理的命令；满时 `mpsc::Sender::send()` 等待空位形成 backpressure。
/// 这个数不表示可并行运行 64 个 effect，worker 一次只处理一条命令。
pub const AUTOMATION_DISPATCH_CHANNEL_CAPACITY: usize = 64;

/// shutdown 时的汇总值；具体 command/error 不在这个计数报告里返回。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationDispatchShutdownReport {
    /// worker 成功记录确定结果（`Observed`）的命令数。
    pub observed_count: usize,
    /// 关闭时仍未终态或结果为 Unknown、需要后续对账的命令数。
    pub recovery_required_count: usize,
}

/// 发给单 worker 的私有命令队列；每个请求变体都携带 oneshot 回信端。
enum DispatchServiceCommand {
    /// 以 CAS 语义提交一条可重取命令事实。
    Commit(
        AutomationDispatchCommand,
        u64,
        oneshot::Sender<Result<AutomationDispatchCommit, PortError>>,
    ),
    /// 重取命令、取得唯一 attempt、执行一次 effect 并提交观察结果。
    Dispatch(
        String,
        String,
        u32,
        oneshot::Sender<Result<AutomationWorkerDispatchOutcome, PortError>>,
    ),
    /// 纯读取当前权威副本；不执行 effect、不改状态。
    Fetch(
        String,
        oneshot::Sender<Result<AutomationDispatchCommand, PortError>>,
    ),
    /// 先处理队列中已接收的命令，再汇总仍在对账中的命令。
    Shutdown(oneshot::Sender<Result<AutomationDispatchShutdownReport, PortError>>),
}

/// 一个已启动 worker 的生命周期句柄。
struct RunningService {
    /// 向 worker 投递命令的有界发送端。
    sender: mpsc::Sender<DispatchServiceCommand>,
    /// worker 退出通知；shutdown command 完成后发送 true。
    shutdown: watch::Sender<bool>,
    /// 用于等待 worker 退出并观察 Tokio join 错误。
    join: JoinHandle<()>,
}

/// 通过 `AutomationDispatchService` 的 async mutex 保护其可选运行实例。
#[derive(Default)]
struct ServiceState {
    /// None 表示尚未启动，或 shutdown 已取走运行句柄并正在异步收尾。
    running: Option<RunningService>,
}

/// 每个 DaemonHost 的有界 dispatch/observation service handle；clone 会共享同一 lifecycle state。
#[derive(Clone, Default)]
pub struct AutomationDispatchService {
    /// Tokio mutex 保护 start/shutdown 与运行句柄替换；请求只在取 sender 时短暂持锁。
    state: Arc<AsyncMutex<ServiceState>>,
}

impl std::fmt::Debug for AutomationDispatchService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutomationDispatchService")
            .field("channel_capacity", &AUTOMATION_DISPATCH_CHANNEL_CAPACITY)
            .finish_non_exhaustive()
    }
}

impl AutomationDispatchService {
    /// 创建尚未启动的 handle；不会 spawn worker，也不会自动派发任何命令。
    pub fn new() -> Self {
        Self::default()
    }

    /// 显式启动唯一 worker，并注入命令 store 与 effect runner 作为其两个边界。
    ///
    /// 【调用者】DaemonHost 装配层（`start_automation_dispatch_service`）或测试。
    /// 【输入】`store` 保存可重取命令事实与 CAS 转移；`runner` 是唯一的 effect 边界，只被允许
    /// 返回 outcome 与摘要。
    /// 【输出】成功表示 worker 已 spawn；重复 start 返回 `PortError::Conflict`。
    /// 【副作用】创建容量为 64 的 mpsc channel、shutdown watch 和 Tokio task。
    /// 【设计边界】这里没有 scheduler timer；调用者必须显式发 `dispatch()`。Service mutex 只阻止
    /// 同时 start，不负责和跨进程 store 竞争，也不负责授权。
    pub async fn start(
        &self,
        store: Arc<dyn AutomationDispatchStore>,
        runner: Arc<dyn AutomationWorkerEffectRunner>,
    ) -> Result<(), PortError> {
        let mut state = self.state.lock().await;
        if state.running.is_some() {
            return Err(PortError::Conflict(
                "automation_dispatch_service_already_started".to_owned(),
            ));
        }
        let (sender, receiver) = mpsc::channel(AUTOMATION_DISPATCH_CHANNEL_CAPACITY);
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let join = tokio::spawn(run_worker(store, runner, receiver, shutdown_receiver));
        state.running = Some(RunningService {
            sender,
            shutdown,
            join,
        });
        Ok(())
    }

    /// 提交一条已 `prepare` 的命令事实；同幂等键同摘要返回既有副本（`replayed=true`）。
    pub async fn commit_command(
        &self,
        command: AutomationDispatchCommand,
        expected_revision: u64,
    ) -> Result<AutomationDispatchCommit, PortError> {
        self.request(|reply| DispatchServiceCommand::Commit(command, expected_revision, reply))
            .await
    }

    /// 让 worker 重取命令、取得唯一 attempt、执行一次 effect 并提交观察结果。
    ///
    /// 【失败】未启动返回 `Unavailable`；命令不存在、worker/attempt 冲突、revision 过期或并发
    /// CAS 失败返回稳定 `PortError`，且这些路径都不触碰 runner。
    pub async fn dispatch(
        &self,
        command_id: &str,
        worker_id: &str,
        attempt: u32,
    ) -> Result<AutomationWorkerDispatchOutcome, PortError> {
        self.request(|reply| {
            DispatchServiceCommand::Dispatch(
                command_id.to_owned(),
                worker_id.to_owned(),
                attempt,
                reply,
            )
        })
        .await
    }

    /// 纯读取命令当前权威副本；用于对账与“提交不确定后的重取”。
    pub async fn fetch_command(
        &self,
        command_id: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        self.request(|reply| DispatchServiceCommand::Fetch(command_id.to_owned(), reply))
            .await
    }

    /// 复用公共 channel/reply 流程；业务状态变化实际发生在 worker 调用 store/runner 时。
    ///
    /// 【失败】service 未启动返回 Unavailable；worker 已关闭/丢失返回 Failed；sender 满时 await 空位。
    /// 【确认丢失】store 可能已完成修改而 oneshot 回信接收端已关闭，此时调用方收到 ack-lost 错误，
    /// 不能据此推断操作未发生；应查询/对账 store 当前状态，直接重试可能撞上已提交的 claim。
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, PortError>>) -> DispatchServiceCommand,
    ) -> Result<T, PortError> {
        let sender = {
            let state = self.state.lock().await;
            state
                .running
                .as_ref()
                .map(|running| running.sender.clone())
                .ok_or_else(|| {
                    PortError::Unavailable("automation_dispatch_service_not_started".to_owned())
                })?
        };
        let (reply, response) = oneshot::channel();
        sender.send(build(reply)).await.map_err(|_| {
            PortError::Failed("automation_dispatch_service_command_lost".to_owned())
        })?;
        response
            .await
            .map_err(|_| PortError::Failed("automation_dispatch_service_ack_lost".to_owned()))?
    }

    /// 停止 worker：先经同一 FIFO channel 投递 Shutdown，再等待其处理回执和 join。
    ///
    /// 已进入 channel 且排在 Shutdown 前的消息会先处理；随后仍然 `Claimed`/`Unknown` 的命令被计入
    /// recovery-required，而不是被说成已完成。返回计数而不是逐项结果；没有启动的 service 返回两个零。
    pub async fn shutdown(&self) -> Result<AutomationDispatchShutdownReport, PortError> {
        let running = { self.state.lock().await.running.take() };
        let Some(running) = running else {
            return Ok(AutomationDispatchShutdownReport {
                observed_count: 0,
                recovery_required_count: 0,
            });
        };
        let (reply, response) = oneshot::channel();
        running
            .sender
            .send(DispatchServiceCommand::Shutdown(reply))
            .await
            .map_err(|_| {
                PortError::Failed("automation_dispatch_service_shutdown_command_lost".to_owned())
            })?;
        let report = response.await.map_err(|_| {
            PortError::Failed("automation_dispatch_service_shutdown_ack_lost".to_owned())
        })??;
        let _ = running.shutdown.send(true);
        running
            .join
            .await
            .map_err(|_| PortError::Failed("automation_dispatch_service_join_failed".to_owned()))?;
        Ok(report)
    }
}

/// 消费单个有界命令队列；`active` 只记录本 worker 成功操作过的命令快照，不是 store 全量索引。
///
/// 所有消息串行调用 store/runner，避免本服务内的 handler 交错；跨 service/进程的原子性仍归 store。
/// worker restart 时 `active` 从空 Map 开始，持久/恢复状态只能由 store 的查询与拒绝行为提供。
async fn run_worker(
    store: Arc<dyn AutomationDispatchStore>,
    runner: Arc<dyn AutomationWorkerEffectRunner>,
    mut receiver: mpsc::Receiver<DispatchServiceCommand>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut active = BTreeMap::<String, AutomationDispatchCommand>::new();
    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else { break; };
                let stop = handle_command(&*store, &*runner, command, &mut active).await;
                if stop { break; }
            }
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() { break; }
            }
        }
    }
    receiver.close();
}

/// 对一个命令执行对应端口调用、更新本地 active 索引并回送结果。
///
/// 回信 send 失败不回滚已完成的 store 操作；`request()` 侧可能只看到 ack-lost，因此该边界可能
/// 需要对账。`Err` 路径不更新 active：调用方既不能推断 effect 未执行，也不能推断它执行过。
async fn handle_command(
    store: &dyn AutomationDispatchStore,
    runner: &dyn AutomationWorkerEffectRunner,
    command: DispatchServiceCommand,
    active: &mut BTreeMap<String, AutomationDispatchCommand>,
) -> bool {
    match command {
        DispatchServiceCommand::Commit(command, expected_revision, reply) => {
            // 只提交事实：未 dispatch 的命令没有尝试过 effect，因此不进入 active 恢复计数。
            let result = store.commit_command(command, expected_revision).await;
            let _ = reply.send(result);
            false
        }
        DispatchServiceCommand::Dispatch(command_id, worker_id, attempt, reply) => {
            let result =
                dispatch_automation_worker_command(store, runner, &command_id, &worker_id, attempt)
                    .await;
            if let Ok(outcome) = &result {
                let command = match outcome {
                    AutomationWorkerDispatchOutcome::Observed(command)
                    | AutomationWorkerDispatchOutcome::Unknown(command)
                    | AutomationWorkerDispatchOutcome::Replayed(command) => command,
                };
                active.insert(command.command_id.clone(), command.clone());
            }
            let _ = reply.send(result);
            false
        }
        DispatchServiceCommand::Fetch(command_id, reply) => {
            let _ = reply.send(store.fetch_command(&command_id).await);
            false
        }
        DispatchServiceCommand::Shutdown(reply) => {
            let mut observed_count = 0;
            let mut recovery_required_count = 0;
            for command in active.values() {
                if command.is_terminal()
                    && command.state() == kiana_domain::AutomationDispatchState::Observed
                {
                    observed_count += 1;
                } else {
                    recovery_required_count += 1;
                }
            }
            active.clear();
            let _ = reply.send(Ok(AutomationDispatchShutdownReport {
                observed_count,
                recovery_required_count,
            }));
            true
        }
    }
}
