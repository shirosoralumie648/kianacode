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
///
/// 【设计理由】
/// 工作流队列不存储「工作本体」，只存储「两个纯数据对象的配对」：
///   1. 不变量（claim）：工作包的完整规格（item_id、依赖、并发限制等），一旦入队就不变；
///   2. 易变量（lease）：当前谁拿着它、什么时候过期、拿着期间发生了什么。
///
/// 【为什么分离而不是合并】这样做的好处是：claim 可复用（多个人可能在不同时刻来领同一个工作），
/// 而 lease 是「瞬时状态」。分离后可以不失一致性地回答各种查询：
///   * "谁在做"？ -> 看 `lease.owner_id`
///   * "做了多久"？ -> 看 `lease.expires_at_unix_ms` 和 `now`
///   * "有没有人做"？ -> `lease.is_none()` 就是 ready
#[derive(Clone, Debug)]
struct QueueEntry {
    /// 工作包契约（claim contract）：`item_id`、各类 scope/budget/path-lock 摘要、
    /// 依赖是否已解、`active_claim_count` / `max_parallel_claims` 并发上限等。
    /// 它描述「这项工作**是什么**」，本身不含「谁在做」。
    ///
    /// 【生命周期】从入队到永久：claim 不变（本适配器不提供删除操作）。
    /// 【重要约束】claim 的 `claim_digest` 是这个 QueueEntry 的「签名」；
    /// 若同一 item_id 来了不同 digest 的 claim，属于误用。
    claim: WorkflowQueueClaimContract,

    /// 当前租约。`None` = 还没人领（`ready()` 就是按这个字段筛的）。
    /// `Some(..)` = 已有人领，锁在该租约的 `status` / `effect_state` 上。
    ///
    /// 【租约的完整生命周期】
    ///   None (初始)
    ///       ↓ claim() 成功
    ///   Some(LeaseStatus::Active, EffectState::NotStarted)
    ///       ├─ heartbeat() → 续租（owner/fence/epoch 不变，to期延长）
    ///       ├─ record_effect(Running) → 工作已启动
    ///       ├─ record_effect(Succeeded/Failed/ResultUnknown) → 工作完结（终态）
    ///       ├─ fence() → 主动围栏（所有者失效，租约变 Fenced）
    ///       └─ reclaim() → 回收（新所有者+新 fence_token，租约回到 Active）
    ///
    /// 【终态三状态】Succeeded/Failed/ResultUnknown 到达后，后续只能
    ///   * heartbeat() → 都会被拒（已终止）
    ///   * record_effect() → 都会被拒（已终止）
    ///   * fence() → 只有 ResultUnknown 可以 fence（需要恢复）；已知终态加 fence 报错
    ///   * reclaim() → 允许从 Succeeded/Failed 回收；ResultUnknown 必须先恢复
    lease: Option<WorkflowQueueLease>,
}

/// 整个队列的内存态。放在一个 `Mutex` 后面，让所有读改写天然串行 —— 这就是
/// 「语义原子性（semantic atomicity）」的最小示范：一次 claim 的「查旧租约 + 写新租约」
/// 不会与另一次 reclaim 交叉撕裂。
///
/// 【为什么用 BTreeMap 不用 HashMap】
///   * 迭代顺序确定（按 key 排序），在 CI 里可复现测试结果；
///   * `ready()` 返回的待办项列表顺序因此稳定，便于断言；
///   * 付出的代价是查询 O(log n) 而非 O(1)，在本适配器规模（CI 最多几十项）下可接受。
#[derive(Clone, Debug, Default)]
struct QueueState {
    /// 键 = `item_id`（队列项主键）。用 `BTreeMap` 而非 `HashMap`：
    /// 迭代顺序确定，`ready()` 的返回顺序因此可复现，CI 断言才能稳定。
    ///
    /// 【队列 vs 真实 HashMap 的对比】
    /// 这里「队列」这个名字有点骗人：本结构不是 FIFO，而是**键值表**。
    /// 「队列」的含义是「待办项池」，每项独立有 ID，你可以按 ID 领取。
    ///   * 一个项可能一开始无人领（None lease）—— 处于 ready 状态；
    ///   * 被某人领走（Some lease）后就进入 in-flight；
    ///   * 工作完结后要么被另一个人回收，要么就一直停在终态，等外部系统清理。
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
///
/// 【并发模型】
/// 所有写操作都必须经过 `self.state.lock().await`。这产生一个**全局串行化点**，
/// 但这是有意的：
///   * 简单性：一次只有一个任务能改动队列；
///   * 安全性：不存在"查-改-写"竞争窗口；
///   * 成本：每次操作最多几微秒的锁耗时，在 CI 规模下可忽略不计。
///
/// 【与持久化实现的对接】一旦迁移到 EventLog 或数据库，本结构的角色会改变：
/// `Arc<Mutex<_>>` 会被 `Arc<dyn WorkflowQueueStore>`（trait 对象）替代，
/// 真实实现会转向**每个操作一条 log entry**，而非内存结构。
/// 这就是为什么文件头要特意强调「deliberately does not claim fsync」——
/// 它是一份设计边界声明，不是一个"实现可能会改"的句子。
#[derive(Clone, Debug, Default)]
pub struct MemoryWorkflowQueueStore {
    state: Arc<Mutex<QueueState>>,
}

impl MemoryWorkflowQueueStore {
    /// 构造一个空队列。`Default` 已实现，此处仅为可读性保留显式构造。
    pub fn new() -> Self {
        Self::default()
    }

    /// 把一个**已校验**的 `Ready` 项放进队列，但**不**领它。
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
    /// 【前置条件】
    ///   * `now > 0`：当前时刻（Unix 毫秒级时间戳）；0 是哨兵值，表示调用方没填时间；
    ///   * `ttl_ms > 0`：租约期限（毫秒）；0 表示配置错误。
    ///   * `ttl_ms ≤ 300_000`：租约不超过 5 分钟。
    ///
    /// 【为什么要 5 分钟上限】租约是「有人在干活」的乐观凭证。如果 5 分钟都不续期，
    /// 可以推断持有者已失联（崩溃、网络断开等）。上限再放大，一个坏的 worker
    /// 就能把整个队列锁死很长时间。
    ///
    /// 【魔数说明】
    ///   * `WORKFLOW_QUEUE_LEASE_TTL_MAX_MS = 300_000`：这个常数定义在
    ///     `kiana-domain/src/workflow_queue_lease.rs:11`；
    ///   * `now == 0`：Unix 纪元 0 不是有效的观察时刻。作为哨兵值可以检测
    ///     「调用方忘了填时间」；放行会让所有 `is_expired` 判定失真；
    ///   * `ttl_ms == 0`：0 租约等于「一领就过期」，属于配置错误。
    ///
    /// 【失败情况】全部 `PortError::Failed`：
    ///   * `workflow_queue_lease_ttl_invalid`：上述任一输入非法（now == 0，
    ///     或 ttl_ms == 0，或 ttl_ms > 300_000）；
    ///   * `workflow_queue_lease_expiry_overflow`：`now + ttl_ms` 超出 `u64::MAX`。
    ///     使用 `checked_add` 而非 `+` 是为了避免 Rust 的 `u64` 加法溢出在
    ///     debug 配置下触发 panic。
    ///
    /// 【调用示例】
    /// ```ignore
    /// let expires_at = MemoryWorkflowQueueStore::expiry(1_694_000_000_000, 60_000)?;
    /// // -> 1_694_000_060_000 （现在 + 1 分钟）
    /// ```
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
    ///
    /// 【补充】
    /// 虽然 BTreeMap 在内存里没问题，但一旦考虑序列化/持久化，字符串键里的
    /// `\0` / `\n` / `\r` 就成了"字段分隔符"的灾难。这是**纵深防御**：
    /// 领域层不知道或不在乎这些，存储层必须自己把关。
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
    ///
    /// 【魔数 256】与 `item_id` 同上限；同样排掉 `\0` / `\n` / `\r`。
    ///
    /// 【owner_id 的类型】虽然代码里是 `&str`，但语义上代表一个身份：
    ///   * 在本地模式下可能是 `"builder-session-abc123"` 之类的会话 ID；
    ///   * 在分布式场景下可能是 `"worker-pool-2-slot-5"` 这样的 worker 标识；
    ///   * 核心约束是「**同一租约的续约/围栏/记副作用只能由同一个 owner 发起**」。
    /// 这就是「fencing」的本质：通过所有权校验来防止「分裂脑」（split brain）。
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
    ///
    /// 【错误分类哲学】
    ///   * `Failed` = 调用方 bug / 输入违反不变量 / 无法通过重试恢复；
    ///   * `Conflict` = 队列里其他参与者改了状态 / 过期了 / 被别人抢了，重试可能成功。
    ///
    /// 【具体例子】
    ///   * 「owner 不符」→ Conflict，因为"会不会符"取决于别人有没有写；
    ///   * 「fence_token 错了」→ Conflict，因为可能是因为别人围栏了该租约；
    ///   * 「claim contract 自校验失败」→ Failed，因为这是调用方的问题。
    ///
    /// 这个映射是**结构化错误码**纪律的体现：调用方据此能区分
    /// 「重试有意义」（Conflict）和「重试也没用」（Failed）。
    fn lease_error(reason: String) -> PortError {
        PortError::Conflict(reason)
    }

    /// 为一次 claim 请求解析出队列项，必要时**隐式入队**。
    ///
    /// 【作用】claim 路径的前半段：纯校验 + 查表，尚未产生任何租约。
    /// 【调用者】仅本文件的 `WorkflowQueueStore::claim`。
    ///
    /// 【核心流程（按顺序）】
    ///   1. 校验工作包契约自洽：`WorkflowQueueClaimContract::validate` 会拒绝
    ///      依赖未解、存在环、scope/budget 不是父的子集、`active_claim_count`
    ///      超过 `max_parallel_claims` 等情况。这些错误都属于 Failed；
    ///   2. 校验 `item_id` / `owner_id` / `fence_token` / `authority_epoch` / TTL
    ///      是否满足存储侧约束（长度、禁字符、非零等）；
    ///   3. 要求 `claim.status == Ready`（只有 Ready 的项才能被领取）；
    ///   4. 加锁进入 `state`；
    ///   5. 查表：若命中则比对 `claim_digest`（如下一段说明）；
    ///   6. 未命中则**隐式入队**（以请求里自带的 claim 建一个新条目）。
    ///
    /// 【为什么必须比 `claim_digest`】
    /// 同一 `item_id` 若摘要不同，说明有人用同一个 key 塞了不同的工作包。
    /// 若放行，后领者会"拿着 A 的包、抢到 B 的租约"，导致不一致。
    /// 队列项与租约**必须**由同一次契约派生（签名必须匹配），故报
    /// `workflow_queue_claim_digest_conflict`。
    ///
    /// 【为什么 `fence_token == 0` 和 `authority_epoch == 0` 都算非法】
    /// 这两个是围栏凭证（fencing token）。0 是「没有围栏」的哨兵值 ——
    /// 允许 0 等于允许「不受世代约束的写入者」，那正是围栏机制要防的事。
    /// （`authority_epoch` 是权威世代号，`fence_token` 是逐次发放的令牌；
    /// 两者都为 0 时无法判断写入者属于哪一代。）
    ///
    /// 【自愈式注册的风险与收益】
    /// 这是有意的**自愈式注册**：`claim` 是自描述的（自带完整契约），
    /// 所以一个此前没 `enqueue()` 过的项也能被正确认领，而不必要求调用方
    /// 严格先入队再领取。
    ///   * 收益：灵活性，某个 claim 到了就能领，不用额外的编排；
    ///   * 代价：队列里出现的项未必都经过 `enqueue()`，"入队"与"领取"不再可区分。
    /// 在本适配器只用于 CI 语义的前提下可以接受，但换成持久实现时应当去掉。
    ///
    /// 【失败情况】
    ///   * `PortError::Failed` —— 契约不满足或输入非法；
    ///   * `PortError::Conflict` —— 同 key 不同摘要。
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
    /// 工作流队列的**核心操作**：领取一个待处理的工作包。
    ///
    /// 【数据流】
    /// ```text
    ///   WorkflowQueueClaimRequest { item_id, claim, owner_id, fence_token, authority_epoch, ttl }
    ///         │
    ///         ├─① 通过 entry_for_claim() 查到或隐式注册队列项
    ///         │
    ///         ├─② 计算到期时刻 = min(now + ttl, claim.claim_expires_at_unix_ms)
    ///         │   （租约不能超过工作包本身的 deadline）
    ///         │
    ///         ├─③ 校验该项还没被任何人领（lease == None）
    ///         │
    ///         ├─④ 若已被领，检查理由（过期？未解锁？仍在运行？）
    ///         │   → 不同理由返回不同 Conflict 错误码，caller 据此决定重试策略
    ///         │
    ///         └─⑤ 通过 WorkflowQueueLease::issue() 签发新租约（涉及 fence_token 生成）
    ///
    ///   返回 WorkflowQueueLease
    /// ```
    ///
    /// 【并发场景】
    ///   场景 A：两个 owner 同时 claim 同一项（正常竞争）
    ///     → 第一个 claim() 成功，设 lease = Some(...)；
    ///     → 第二个 claim() 看到 lease.is_some()，返回 Conflict("workflow_queue_claim_contended")；
    ///     → 第二个 owner 此后有三条路：
    ///        1) 等租约过期，然后 reclaim()；
    ///        2) 请求第一个 owner 主动 reclaim()；
    ///        3) 请求外部权威（ControlPlane）发起 fence()。
    ///
    ///   场景 B：claim 成功后，在 record_effect() 之前包崩了
    ///     → 租约维持，但不会 heartbeat，逐渐接近 expires_at；
    ///     → 超期后，别人可 reclaim() 或触发恢复流程。
    ///
    /// 【租约的语义生命周期】
    ///   None (初始，"没人领"状态，从 ready() 会看到这项)
    ///       │
    ///       └─ claim() 成功
    ///            │
    ///            ▼
    ///   Some { status: Active, effect_state: NotStarted }  ("领了，还没开始做")
    ///       │
    ///       ├─ heartbeat() → 续租 (owner/fence/epoch 必须全匹配)
    ///       │   |
    ///       │   └─ 回到该状态 (只是 expires_at 向后推)
    ///       │
    ///       ├─ record_effect(Running)
    ///       │   └─ Some { status: Active, effect_state: Running }  ("开始做了")
    ///       │       │
    ///       │       └─ record_effect(Succeeded/Failed/ResultUnknown)
    ///       │           └─ Some { status: Active, effect_state: Succeeded/Failed/ResultUnknown }
    ///       │               └─ 终态！heartbeat/record_effect 此后都被拒
    ///       │
    ///       ├─ fence()
    ///       │   └─ Some { status: Fenced, ... }  ("被围栏了，owner 失效")
    ///       │       └─ 所有者已失权，其它操作都被拒
    ///       │
    ///       └─ reclaim()
    ///           └─ Some { status: Active, effect_state: NotStarted, owner: new_owner_id, ... }
    ///               └─ 新所有者从头接手这个项
    ///
    /// 【关键约束】
    ///   * 租约必须由同一 owner 续约 / 记副作用，不能被别人"接管"（除非 reclaim）；
    ///   * 租约的到期时刻不能超过 claim 里声明的 deadline；
    ///   * 若发现 lease.status == ResultUnknown 或 effect_state == ResultUnknown，
    ///     必须先恢复（由 recover 相关逻辑处理）。
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

    /// 续约：延长现有租约的到期时间。
    ///
    /// 【作用】工作仍在进行中，定期"心跳"来告诉系统"我还活着"。
    /// heartbeat 的本质是**延长租约过期时刻**，而不是改变状态或所有权。
    ///
    /// 【先决条件】
    ///   * `item_id` 存在于队列；
    ///   * 该项已被领（lease != None）；
    ///   * 租约的 owner_id / fence_token / authority_epoch 必须**完全匹配**当前值。
    ///
    /// 【输入参数】
    ///   * `owner_id`：声称自己是当前持有者；
    ///   * `fence_token`：当前的围栏令牌（与签发租约时的相同）；
    ///   * `authority_epoch`：权威的世代号（与签发租约时的相同）；
    ///   * `observed_at_unix_ms`：当前时刻；
    ///   * `lease_ttl_ms`：新的租约期限。
    ///
    /// 【为什么三元组都要匹配】这就是**围栏机制**（fencing）：
    ///   * `owner_id` 确保"只有领取者能续约"，防止"我以为没人领，抢过来续约";
    ///   * `fence_token` 是"世代号"，防止"崩溃后重启的旧进程还能续约"；
    ///   * `authority_epoch` 是"权威确认号"，防止"鬼魂租约"（已被外部系统意图回收）。
    /// 三个都对，说明持有者身份真实有效；哪怕错一个都拒绝。
    ///
    /// 【失败情况】
    ///   * 项不存在 → `Conflict("workflow_queue_item_not_found")`；
    ///   * 项未被领 → `Conflict("workflow_queue_lease_not_found")`；
    ///   * owner / fence_token / authority_epoch 不匹配 → 领域层拒绝，映射为 Conflict；
    ///   * 租约已终止（Succeeded/Failed 等）→ Conflict；
    ///   * TTL 参数非法 → Failed。
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

    /// 记录工作的执行结果状态：未开始 / 执行中 / 成功 / 失败 / 结果未知。
    ///
    /// 【作用】工作流引擎通过该方法向队列汇报"这项工作现在什么状态"。
    /// 这是一个**事实汇报通道**，记录的状态会影响后续的 reclaim / 围栏 / 恢复决策。
    ///
    /// 【状态转移规则】
    ///   NotStarted  (初始，claim 成功后)
    ///       │
    ///       ├─ record_effect(Running)
    ///       │   └─ Running  ("工作已启动，还没出结果")
    ///       │       │
    ///       │       └─ record_effect(Succeeded/Failed/ResultUnknown)
    ///       │           └─ 终态：从此任何 record_effect 都被拒
    ///       │
    ///       └─ record_effect(Succeeded/Failed/ResultUnknown)  (跳过 Running 的快路径)
    ///           └─ 终态
    ///
    /// 【先决条件】
    ///   * 租约存在且未过期；
    ///   * owner_id / fence_token / authority_epoch 匹配；
    ///   * 租约尚未到达终态（Succeeded/Failed/ResultUnknown）。
    ///
    /// 【三种终态的语义】
    ///   * Succeeded      : 工作正常完成，结果可信。后续可以安全 reclaim；
    ///   * Failed         : 工作失败（可能是业务错误或系统错误），结果已定。后续可以 reclaim；
    ///   * ResultUnknown  : 不确定发生了什么（如持有者崩溃后不知道有没有跑完），
    ///                      需要进入恢复流程（由外部系统调 fence() + 恢复逻辑处理）。
    ///
    /// 【与事件日志的区别】
    ///   本方法只**更新队列内的状态机**，不向 EventLog 写任何事件。
    ///   EventLog 的写入由 ControlPlane 或 Runner 另行负责。
    ///   这个方法的职责就是"持有者定期汇报自己看到的状态"。
    ///
    /// 【失败情况】
    ///   * 租约不存在或已终止 → Conflict；
    ///   * owner 不匹配 → Conflict；
    ///   * 非法的状态转移（如尝试从 Succeeded 转为 Running）→ Conflict。
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

    /// 主动围栏：将当前持有者的权限立刻失效。
    ///
    /// 【作用】由外部权威（通常是 ControlPlane）发起，用于**强制中止**一个失联/可疑的持有者。
    /// 围栏后，旧持有者的所有后续操作（heartbeat / record_effect）都会被拒。
    /// 这是**分布式系统对付"鬼魂"持有者的标准手段**。
    ///
    /// 【"围栏"的比喻】想象一个羊圈（lease），里面的羊（owner）想逃脱：
    ///   1. 羊试图爬过栅栏（heartbeat）→ 栅栏挡住了（fence 已标记，heartbeat 被拒）；
    ///   2. 羊试图从里面改变圈的状态（record_effect）→ 没权限了（状态已改为 Fenced）；
    ///   3. 新的牧羊人可以打开栅栏，安全地收拾羊圈（reclaim / 恢复）。
    ///
    /// 【先决条件】
    ///   * 项和租约都存在；
    ///   * `owner_id` / `fence_token` / `authority_epoch` 匹配（确保权威身份）。
    ///
    /// 【围栏后的状态】
    ///   lease.status = Fenced  → 持有者的所有权失效
    ///   lease.effect_state     → 保持不变（记录的工作状态保留，便于恢复查询）
    ///
    /// 【何时调用】
    ///   * 持有者心跳超期（5 分钟不续租）—— 自动触发；
    ///   * ControlPlane 检测到"异常提交"（如声称 Succeeded 但数据不一致）—— 主动围栏；
    ///   * 权威裁定需要恢复（如 Reviewer 认为结果可疑）—— 主动围栏。
    ///
    /// 【失败情况】
    ///   * 租约不存在 → Conflict；
    ///   * owner 不匹配 → Conflict；
    ///   * fence_token / authority_epoch 不匹配 → Conflict；
    ///   * 已被终止的租约（Succeeded/Failed）不允许 fence（因为已经有结果了）→ Conflict；
    ///   * ResultUnknown 状态可以被 fence（进入恢复流程）。
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

    /// 回收：从旧持有者手中拿走租约，交给新持有者。
    ///
    /// 【作用】在旧持有者的租约过期后，新持有者可以接手这项工作。
    /// 回收会**删除旧租约，创建新租约**，所有权彻底转移。
    /// 这是**队列中最"暴力"的操作**，也是防止队列饿死的关键机制。
    ///
    /// 【为什么需要 reclaim】
    /// 场景：持有者 A 领了任务，但崩了，5 分钟内还没人发现，任务就"卡住"了。
    ///   →无 reclaim：需要靠人工干预或外部系统主动 fence；
    ///   →有 reclaim：等租约过期后，持有者 B 就能直接 reclaim，任务继续。
    /// 这样队列自动从"失效状态"恢复，无需人工。
    ///
    /// 【先决条件】
    ///   * 项存在于队列；
    ///   * 租约已过期（新持有者无权回收未过期的租约）；
    ///   * 租约非 ResultUnknown 状态（ResultUnknown 需要恢复流程介入）；
    ///   * 租约非 Running 状态（工作仍在进行，不能抢）。
    ///
    /// 【回收后的状态变化】
    ///   旧: Some { owner: old_owner, fence_token: T1, status: Active/Fenced, ... }
    ///   新: Some { owner: new_owner, fence_token: T2, status: Active, effect_state: NotStarted, ... }
    ///   → 新持有者获得新的 fence_token，旧 token 失效。
    ///   → effect_state 重置为 NotStarted，新持有者从零开始。
    ///
    /// 【为什么重置 effect_state】
    ///   当旧持有者的结果是 Succeeded / Failed 时，我们认为"该工作的结果已知"，
    ///   新持有者接手是"重新做一遍"。所以 effect_state 要清空。
    ///   如果结果是 ResultUnknown（未知），reclaim() 通常会被拒；需要先 fence 再恢复。
    ///
    /// 【失败情况】
    ///   * 项不存在 → Conflict；
    ///   * 租约不存在 → Conflict；
    ///   * 租约未过期 → Conflict("workflow_queue_lease_not_expired")；
    ///   * 租约正在 Running → Conflict("workflow_queue_reclaim_effect_in_flight")；
    ///   * 租约是 ResultUnknown → Conflict("workflow_queue_recovery_required")；
    ///   * new_fence_token 或 authority_epoch 非法 → Failed。
    ///
    /// 【与 fence 的区别】
    ///   * fence()  : 权威"否决"了当前持有者，但不创建新租约；
    ///   * reclaim(): 新持有者"接手"，直接创建新租约并转移所有权。
    /// fence 的目的是"中止"，reclaim 的目的是"继续服务"。
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

    /// 列出还没有人领取的队列项（处于 ready 状态）。
    ///
    /// 【作用】查询"现在有哪些工作可以做"。这是调度器（Scheduler）的入口查询。
    /// 调度器定期调用 ready()，看有没有新的待办项，然后决定分派给谁。
    ///
    /// 【什么叫"ready"】
    ///   一项工作是 ready，当且仅当：
    ///   1. 项存在于队列中（已被 enqueue 或隐式注册）；
    ///   2. 租约是 None（还没被任何人 claim）。
    ///   仅此两条。我们**不检查**"依赖是否都完成"——那是 ControlPlane 的活。
    ///
    /// 【入参】`limit` —— 本次最多返回多少项。
    ///   * 范围限制 1 ~ 256；
    ///   * 原因：避免一次性列出数千项导致消息过大，也防止恶意大量请求；
    ///   * 调用方可以分页查询。
    ///
    /// 【返回】一个 Vec，包含最多 `limit` 个 WorkflowQueueClaimContract。
    ///   * 顺序：BTreeMap 的遍历顺序（按 item_id 排序），确定可复现；
    ///   * 每项都是最新的 claim（从 QueueEntry 提取）；
    ///   * 取 first `limit` 项后即返回（不保证取完，可能有更多）。
    ///
    /// 【与 project_workflow_queue_ready 的区别】
    ///   项目根有一个 `kiana-core/src/workflow_queue.rs:project_workflow_queue_ready()`，
    ///   它是在 WorkPacket board 层面**计算出**哪些项满足依赖、budget 等，然后生成
    ///   "理论上应该 ready"的列表。而本 ready() 方法是"实际上正在等待领取的项"。
    ///   两个是两个系统。
    ///
    /// 【失败情况】
    ///   * `limit == 0` 或 `limit > 256` → Failed("workflow_queue_ready_limit_invalid")。
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
