//! CI-semantics workflow queue adapter.
//!
//! This adapter provides one atomic in-process state transition for queue claims.  It is useful
//! for exercising the deny/reclaim/Unknown contract in GitHub CI; it deliberately does not claim
//! fsync, process recovery or cross-process fencing.  A durable implementation must use the same
//! domain transitions behind an EventLog-backed store.

// ============================================================================
// 中文说明
// ============================================================================
//
// 【本文件负责什么】
// 实现 `WorkflowQueueStore`（工作流队列端口）的一个**进程内**适配器
// `MemoryWorkflowQueueStore`。它管的是「谁可以拿走队列里的哪个待办项、拿了多久、
// 做完了没有、能不能被别人抢回来」这一组**领域状态转换**（domain transition），
// 也就是租约（lease）的签发 / 续约 / 围栏 / 回收。
//
// 【属于哪个模块、上下游是谁】
// kiana-eventlog 的 crate 级职责是「事件事实（event facts）的**存储适配器**」。
// 本文件属于该 crate 中的「端口适配器」一族（与 artifact_store / retention_store
// 等同列），不是事件日志本体。依赖方向是单向的：
//
//     kiana-domain（值对象 + 领域规则）  <-  kiana-ports（trait 契约）  <-  本文件
//
//     本文件（适配器实现）  ->  kiana-core / kiana-daemon（组合根 DaemonHost 装配）
//
// 上游：调用方构造 `MemoryWorkflowQueueStore` 并交给 `kiana-daemon` 的
// `WorkflowQueueService`。已实测的唯一调用者是
// `kiana-daemon/tests/aut09_workflow_service.rs:44`（测试，非生产接线）。
// 下游：本文件不写事件、不调模型、不执行任何副作用；它只是把状态回答给调用方。
//
// 【关键纪律：主动声明自己不是持久的】
// 文件头原文已经写明本适配器「deliberately does not claim fsync, process recovery
// or cross-process fencing」。翻译成工程含义：
//   * 没有 fsync              -> 进程被杀，内存态直接消失，没有任何"已落盘"的证据；
//   * 没有进程崩溃恢复        -> 重启后租约归零，无法区分「从没租过」和「租过但崩了」；
//   * 没有跨进程围栏          -> 这里的 `fence_token` / `authority_epoch` 只在本进程内有意义。
// 一个**内存适配器必须主动声明自己不持久**，是因为本项目用
// `source < local_behavior < durable < live < physical` 这条证明等级阶梯。
// 默默享受「有 Mutex 所以是原子的」带来的高等级错觉，会让上层把
// 「local_behavior」当成「durable」来用——这正是这份注释要拦的事。
//
// 【数据如何流过】
//
//   调用方(WorkflowQueueService)
//        │  WorkflowQueueClaimRequest{ claim, owner_id, fence_token, authority_epoch, ... }
//        ▼
//   claim() ──①校验 claim 自身契约──► entry_for_claim()
//        │                              └─ 不在队列里就顺带入队（自愈式注册）
//        │ ②算到期时间 = min(now+ttl, claim.claim_expires_at_unix_ms)
//        │ ③看已有租约的状态：Unknown / Running / 过期 / 未过期 → 四种不同拒绝理由
//        │ ④WorkflowQueueLease::issue() 签发租约
//        ▼
//   state(Arc<Mutex<QueueState>>)   ← 唯一的语义原子性边界
//        │
//        ├──► heartbeat()  续约：owner/fence/epoch 三者必须全部对得上
//        ├──► record_effect() 记副作用状态：NotStarted/Running/Succeeded/Failed/ResultUnknown
//        ├──► fence()     主动围栏：把租约打死，让持有者立刻失效
//        ├──► reclaim()   回收：换新 owner + 新 fence_token + 新到期时间
//        └──► ready()     列出「还没被任何人租走」的项
//
// 【本文件绝不做什么】
// 队列租约与「能力执行」是两条独立的线。`kiana-ports` 的
// `WorkflowQueueStore` 文档明确：租约不授权任何能力、不分派任何 handler；
// 准入仍归 ControlPlane，副作用仍只由 Broker 产生。
// `kiana-core/tests/aut08_workflow_queue_guard.rs` 就是靠断言本文件里
// **不存在**能力执行调用来把这条边界钉死的。
//
// ============================================================================

use async_trait::async_trait;
use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimRequest, WorkflowQueueClaimStatus,
    WorkflowQueueEffectRequest, WorkflowQueueEffectState, WorkflowQueueFenceRequest,
    WorkflowQueueHeartbeatRequest, WorkflowQueueLease, WorkflowQueueLeaseStatus,
    WorkflowQueueReclaimRequest, WORKFLOW_QUEUE_LEASE_TTL_MAX_MS,
};
use kiana_ports::{PortError, WorkflowQueueStore};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 队列里的一项 = 「工作包契约 + 当前租约」。
///
/// 【架构角色】这是本适配器的**唯一**持久（进程内）数据结构，其它函数全部围绕它做状态转换。
#[derive(Clone, Debug)]
struct QueueEntry {
    /// 工作包契约（claim contract）：`item_id`、各类 scope/budget/path-lock 摘要、
    /// 依赖是否已解、`active_claim_count` / `max_parallel_claims` 并发上限等。
    /// 它描述「这项工作**是什么**」，本身不含「谁在做」。
    claim: WorkflowQueueClaimContract,
    /// 当前租约。`None` = 还没人领（`ready()` 就是按这个字段筛的）。
    /// `Some(..)` = 已有人领，锁在该租约的 `status` / `effect_state` 上。
    lease: Option<WorkflowQueueLease>,
}

/// 整个队列的内存态。放在一个 `Mutex` 后面，让所有读改写天然串行 —— 这就是
/// 「语义原子性（semantic atomicity）」的最小示范：一次 claim 的「查旧租约 + 写新租约」
/// 不会与另一次 reclaim 交叉撕裂。
#[derive(Clone, Debug, Default)]
struct QueueState {
    /// 键 = `item_id`（队列项主键）。用 `BTreeMap` 而非 `HashMap`：
    /// 迭代顺序确定，`ready()` 的返回顺序因此可复现，CI 断言才能稳定。
    entries: BTreeMap<String, QueueEntry>,
}

/// In-process queue adapter for CI and deterministic source behavior checks.
///
/// 【中文补充】进程内队列适配器，供 CI 与确定性源码行为检查使用。
///
/// 【为什么用 Mutex 而不是别的】`tokio::sync::Mutex` 在这里是**语义原子性**的载体：
/// 它保证「检查现有租约 → 写入新租约」这两步之间没有别的任务插进来。
/// 但它**只**在进程内有效，所以本类型只主张 `local_behavior` 等级，
/// 绝不主张 durable / live / physical（见文件头的中文说明）。
///
/// 【构造】`new()` 等价于 `default()`：一个空队列。多个 `clone()` 共享同一份状态
/// （内部是 `Arc`），所以克隆出来的句柄看到的是同一个队列。
#[derive(Clone, Debug, Default)]
pub struct MemoryWorkflowQueueStore {
    state: Arc<Mutex<QueueState>>,
}

impl MemoryWorkflowQueueStore {
    /// 构造一个空队列。`Default` 已实现，此处仅为可读性保留显式构造。
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a validated ready item in the queue without claiming it.
    ///
    /// 【中文补充】把一个**已校验**的 `Ready` 项放进队列，但**不**领它。
    ///
    /// 【作用】入队。这是「有项可领」的前提，但入队 ≠ 授权，调用方随后还得
    /// 走 `claim()` 才能拿到租约。
    /// 【调用者】`WorkflowQueueService`；已实测的生产侧未见接线，调用者仅为
    /// `kiana-daemon/tests/aut09_workflow_service.rs` 相关测试与源码守卫。
    /// 【输入】`claim` —— 必须自带完整且自洽的工作包契约。
    /// 【输出】`Ok(())` 表示入队成功；重复 `item_id` 视为冲突。
    /// 【副作用】在共享 `state` 里插入一条 `lease: None` 的 `QueueEntry`。
    /// 【失败情况】
    ///   * `PortError::Failed` —— 契约自校验不过（`workflow_queue_*` 前缀），或
    ///     状态不是 `Ready`（`workflow_queue_enqueue_requires_ready`）。
    ///     两者都是**调用方 bug / 契约不满足**，不是时序竞争。
    ///   * `PortError::Conflict` —— `item_id` 已存在（`workflow_queue_item_duplicate`）。
    ///     注意这里不比较摘要：同 key 不同内容也算重复，属于「入队必须幂等」的保守选择。
    /// 【核心流程】先校验后加锁 —— 顺序不能反，否则非法请求会先占住锁再失败。
    pub async fn enqueue(&self, claim: WorkflowQueueClaimContract) -> Result<(), PortError> {
        claim.validate().map_err(PortError::Failed)?;
        if claim.status != WorkflowQueueClaimStatus::Ready {
            return Err(PortError::Failed(
                "workflow_queue_enqueue_requires_ready".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        if state.entries.contains_key(&claim.item_id) {
            return Err(PortError::Conflict(
                "workflow_queue_item_duplicate".to_owned(),
            ));
        }
        state
            .entries
            .insert(claim.item_id.clone(), QueueEntry { claim, lease: None });
        Ok(())
    }

    /// 计算租约到期时刻 = `now + ttl_ms`，并顺手把非法输入挡掉。
    ///
    /// 【为什么单独抽出来】`claim` / `heartbeat` / `reclaim` 三条路径都要算到期时间。
    /// 集中在一处，才能保证「TTL 上限」这个魔数只在一个地方被强制。
    ///
    /// 【魔数说明】
    ///   * `WORKFLOW_QUEUE_LEASE_TTL_MAX_MS = 300_000`（5 分钟，定义在
    ///     `kiana-domain/src/workflow_queue_lease.rs:11`）：租约**不能**长到 5 分钟以上。
    ///     理由是租约是「有人在干活」的乐观凭证，5 分钟仍不续期就足以判定持有者已失联；
    ///     上限再放大，一个崩溃的 worker 就能把队列锁死很久。
    ///   * `now == 0`：Unix 纪元 0 不是有效的观察时刻。它是「调用方忘了填时间」的哨兵值，
    ///     放行会让所有 `is_expired` 判定失真。
    ///   * `ttl_ms == 0`：0 租约等于「一领就过期」，属于配置错误。
    /// 【失败情况】`PortError::Failed`：
    ///   `workflow_queue_lease_ttl_invalid`（上述任一输入非法）、
    ///   `workflow_queue_lease_expiry_overflow`（`now + ttl` 超出 `u64`，用
    ///   `checked_add` 而不是 `+`，因为 Rust 的 `u64` 加法溢出在 debug 下会 panic）。
    fn expiry(now: u64, ttl_ms: u64) -> Result<u64, PortError> {
        if now == 0 || ttl_ms == 0 || ttl_ms > WORKFLOW_QUEUE_LEASE_TTL_MAX_MS {
            return Err(PortError::Failed(
                "workflow_queue_lease_ttl_invalid".to_owned(),
            ));
        }
        now.checked_add(ttl_ms)
            .ok_or_else(|| PortError::Failed("workflow_queue_lease_expiry_overflow".to_owned()))
    }

    /// 队列项主键的合法性闸门。
    ///
    /// 【魔数 256】`item_id` 长度上限 256 字节。这与 `kiana-domain` 侧
    /// `WorkflowQueueClaimContract::validate`（`workflow_queue_claim.rs:204`）里的
    /// 上限一致 —— 两侧重复检查不是冗余：领域层保护结构体本身的完整性，
    /// 这里保护**存储键**（它会直接进 `BTreeMap` 的 key）。若只靠领域层，
    /// 未来若出现绕过 `validate()` 的构造路径，键就可能带进控制字符。
    ///
    /// 【为什么额外禁 `\0` / `\n` / `\r`】这些字符进入键之后，会在日志、错误信息、
    /// 后续可能的持久化文件里制造歧义（换行可伪造出「另一行记录」）。
    /// 属于边界上的输入卫生，不是洁癖。
    fn item_id_valid(item_id: &str) -> Result<(), PortError> {
        if item_id.trim().is_empty() || item_id.len() > 256 || item_id.contains(['\0', '\n', '\r'])
        {
            return Err(PortError::Failed(
                "workflow_queue_item_id_invalid".to_owned(),
            ));
        }
        Ok(())
    }

    /// 持有者标识（`owner_id` / `new_owner_id`）的合法性闸门。
    ///
    /// 【为什么要单独一份而不是复用 `item_id_valid`】语义不同：`item_id` 是**数据主键**，
    /// `owner_id` 是**主体标识**，将来若要给 owner 加不同的约束（长度、字符集），
    /// 分开的函数才改得动。规则本身目前刻意保持一致。
    /// 【魔数 256】与 `item_id` 同上限；同样排掉 `\0` / `\n` / `\r`。
    fn owner_valid(owner_id: &str) -> Result<(), PortError> {
        if owner_id.trim().is_empty()
            || owner_id.len() > 256
            || owner_id.contains(['\0', '\n', '\r'])
        {
            return Err(PortError::Failed("workflow_queue_owner_invalid".to_owned()));
        }
        Ok(())
    }

    /// 把「领域层算租约失败」的 `String` 理由升格为 `PortError::Conflict`。
    ///
    /// 【为什么统一映射成 Conflict 而不是原样 Failed】租约相关的所有领域拒绝
    /// （owner 不符、围栏不符、已围栏、已过期、状态不允许转换……）本质都是
    /// 「调用方拿的是一份过期的认知」，属于状态冲突而非契约破坏。
    /// 这个映射是**结构化错误码**纪律的体现：调用方据此能区分
    /// 「重试有意义」（Conflict）和「重试也没用」（Failed），
    /// 而不是把所有原因塌缩成一个通用 I/O 错误。
    fn lease_error(reason: String) -> PortError {
        PortError::Conflict(reason)
    }

    /// 为一次 claim 请求解析出队列项，必要时**隐式入队**。
    ///
    /// 【作用】claim 路径的前半段：纯校验 + 查表，尚未产生任何租约。
    /// 【调用者】仅本文件的 `WorkflowQueueStore::claim`。
    /// 【核心流程】
    ///   1. 校验工作包契约自洽（`WorkflowQueueClaimContract::validate` 会拒绝
    ///      依赖未解、存在环、scope/budget 不是父的子集、`active_claim_count`
    ///      超过 `max_parallel_claims` 等情况）；
    ///   2. 校验 `item_id` / `owner_id` / `fence_token` / `authority_epoch` / TTL；
    ///   3. 要求 `claim.status == Ready`；
    ///   4. 查表命中则比对 `claim_digest`，不一致就报冲突；
    ///   5. 未命中则以**请求里自带的 claim** 建一个新条目入队。
    ///
    /// 【为什么这里允许「没入队就自动入队」】这是有意的**自愈式注册**：
    /// `claim` 是自描述的（自带完整契约），所以一个此前没 `enqueue()` 过的项
    /// 也能被正确认领，而不必要求调用方严格先入队再领取。
    /// 代价是这条路径上「入队」与「领取」不再可区分 —— 队列里出现的项
    /// 未必都经过 `enqueue()`。在本适配器只用于 CI 语义的前提下可以接受，
    /// 但换成持久实现时应当去掉这个隐式入队。
    ///
    /// 【为什么必须比 `claim_digest`】同一 `item_id` 若摘要不同，说明有人用
    /// 同一个 key 塞了不同的工作包。若放行，后领者会拿着 A 的包、抢到 B 的租约。
    /// 队列项与租约**必须**由同一次契约派生，故报
    /// `workflow_queue_claim_digest_conflict`。
    ///
    /// 【为什么 `fence_token == 0` 和 `authority_epoch == 0` 都算非法】
    /// 这两个是围栏凭证（fencing token）。0 是「没有围栏」的哨兵值 ——
    /// 允许 0 等于允许「不受世代约束的写入者」，那正是围栏机制要防的事。
    /// （`authority_epoch` 是权威世代号，`fence_token` 是逐次发放的令牌；
    /// 两者都为 0 时无法判断写入者属于哪一代。）
    ///
    /// 【失败情况】全部为 `PortError::Failed`（契约/输入不满足）或
    /// `PortError::Conflict`（同 key 不同摘要）。
    async fn entry_for_claim(
        &self,
        request: &WorkflowQueueClaimRequest,
    ) -> Result<QueueEntry, PortError> {
        request.claim.validate().map_err(PortError::Failed)?;
        Self::item_id_valid(&request.claim.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        if request.fence_token == 0 || request.authority_epoch == 0 {
            return Err(PortError::Failed(
                "workflow_queue_authority_or_fence_invalid".to_owned(),
            ));
        }
        let _ = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?;
        if request.claim.status != WorkflowQueueClaimStatus::Ready {
            return Err(PortError::Failed(
                "workflow_queue_claim_not_ready".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        if let Some(entry) = state.entries.get(&request.claim.item_id) {
            if entry.claim.claim_digest != request.claim.claim_digest {
                return Err(PortError::Conflict(
                    "workflow_queue_claim_digest_conflict".to_owned(),
                ));
            }
            return Ok(entry.clone());
        }
        let entry = QueueEntry {
            claim: request.claim.clone(),
            lease: None,
        };
        state
            .entries
            .insert(request.claim.item_id.clone(), entry.clone());
        Ok(entry)
    }
}

#[async_trait]
impl WorkflowQueueStore for MemoryWorkflowQueueStore {
    async fn claim(
        &self,
        request: WorkflowQueueClaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        let entry = self.entry_for_claim(&request).await?;
        let expires_at = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?
            .min(request.claim.claim_expires_at_unix_ms);
        if expires_at <= request.observed_at_unix_ms {
            return Err(PortError::Conflict(
                "workflow_queue_claim_expired".to_owned(),
            ));
        }
        if let Some(lease) = &entry.lease {
            if lease.status == WorkflowQueueLeaseStatus::ResultUnknown
                || lease.effect_state == WorkflowQueueEffectState::ResultUnknown
            {
                return Err(PortError::Conflict(
                    "workflow_queue_recovery_required".to_owned(),
                ));
            }
            if lease.effect_state == WorkflowQueueEffectState::Running {
                return Err(PortError::Conflict(
                    "workflow_queue_reclaim_effect_in_flight".to_owned(),
                ));
            }
            return Err(PortError::Conflict(
                if lease.is_expired(request.observed_at_unix_ms) {
                    "workflow_queue_reclaim_required"
                } else {
                    "workflow_queue_claim_contended"
                }
                .to_owned(),
            ));
        }
        let lease = WorkflowQueueLease::issue(
            &entry.claim,
            request.owner_id,
            request.fence_token,
            request.authority_epoch,
            request.observed_at_unix_ms,
            expires_at,
        )
        .map_err(Self::lease_error)?;
        let mut state = self.state.lock().await;
        let current = state
            .entries
            .get_mut(&lease.item_id)
            .ok_or_else(|| PortError::Failed("workflow_queue_item_lost".to_owned()))?;
        if current.lease.is_some() {
            return Err(PortError::Conflict(
                "workflow_queue_claim_contended".to_owned(),
            ));
        }
        current.lease = Some(lease.clone());
        Ok(lease)
    }

    async fn heartbeat(
        &self,
        request: WorkflowQueueHeartbeatRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        let expires_at = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?;
        if request.fence_token == 0 || request.authority_epoch == 0 {
            return Err(PortError::Failed(
                "workflow_queue_authority_or_fence_invalid".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .renew(
                &request.owner_id,
                request.fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
                expires_at,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn record_effect(
        &self,
        request: WorkflowQueueEffectRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .record_effect(
                &request.owner_id,
                request.fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
                request.effect_state,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn fence(
        &self,
        request: WorkflowQueueFenceRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .fence(
                &request.owner_id,
                request.fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn reclaim(
        &self,
        request: WorkflowQueueReclaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.new_owner_id)?;
        let expires_at = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?;
        if request.new_fence_token == 0 || request.authority_epoch == 0 {
            return Err(PortError::Failed(
                "workflow_queue_authority_or_fence_invalid".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .reclaim(
                request.new_owner_id,
                request.new_fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
                expires_at,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn ready(&self, limit: usize) -> Result<Vec<WorkflowQueueClaimContract>, PortError> {
        if limit == 0 || limit > 256 {
            return Err(PortError::Failed(
                "workflow_queue_ready_limit_invalid".to_owned(),
            ));
        }
        let state = self.state.lock().await;
        Ok(state
            .entries
            .values()
            .filter(|entry| entry.lease.is_none())
            .take(limit)
            .map(|entry| entry.claim.clone())
            .collect())
    }
}
