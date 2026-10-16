//! Opaque SecretRef rotation/revoke adapter for CI and local composition.
//!
//! This adapter stores references, not secret material.  It is intentionally in-memory and must
//! not be used as durable credential storage; its purpose is to make generation CAS and revoke
//! fencing executable without creating a second provider or authorization path.

// ============================================================================
// 中文说明
// ============================================================================
//
// 【本文件负责什么】
// 实现 `CredentialRotationPort`（凭证轮转端口）的一个**进程内**适配器
// `MemoryCredentialRotationStore`。它管的是「凭证引用（SecretRef）的轮转和撤销」
// 这一组**领域状态转换**。不存储实际的密钥材料，只存储**不透明的凭证引用**，
// 每次轮转都会递增 generation 号，让持有者的旧许可证立即失效。
//
// 【关键纪律：这只是参考实现，绝不能用于生产密钥管理】
// 本适配器「deliberately does not claim durability」。翻译成工程含义：
//   * 没有 fsync             -> 进程被杀，轮转历史直接消失；
//   * 没有进程崩溃恢复       -> 重启后状态归零，无法审计过往轮转；
//   * 只存引用，不存密钥     -> 实际的密钥在外部系统（如 KMS）管理。
// 本文件的**唯一**用途是在 CI 和本地组合中让轮转/撤销的「栅栏语义」可执行化。
// 任何包含真实密钥的应用都必须配一个事件驱动的、持久化的轮转系统。
//
// 【架构位置】
// kiana-eventlog 的 crate 级职责是「事件事实的**存储适配器**」。
// 本文件属于该 crate 中的「端口适配器」一族（与 workflow_queue / artifact_store 等同列）。
// 依赖方向是单向的：
//
//     kiana-domain（值对象 + 轮转规则）  <-  kiana-ports（trait 契约）  <-  本文件
//     本文件（适配器实现）  ->  kiana-daemon（组合根装配）
//
// 【数据如何流过】
//
//   调用方 (e.g. ControlPlane 权限检查)
//        │  SecretRef { reference_digest, store, key, generation, ... }
//        ▼
//   rotate_credential()  ──①validate() 检查引用格式
//        │                  ②verify generation 无溢出
//        │                  ③从内存态查现有的这条引用
//        ▼
//   mutate()           ←── 核心转换逻辑
//        │  ④如果 generation 不匹配 → Conflict（别人已经轮过了）
//        │  ⑤计算新的 generation = old + 1
//        │  ⑥生成新的 SecretRef（digest 会改变）
//        │  ⑦删除旧 digest 的引用，写入新的
//        ▼
//   return SecretRef  ──> 返回新的不透明引用给调用方
//                         持有者只能用新引用，旧引用立即失效
//
// 【关键约束】
//   * generation 只能单调递增，溢出报错；
//   * reference_digest 是【幂等性密钥】——同一 (store, key, generation) 生成相同 digest；
//   * 轮转是【原子**语义**转换】——一次 mutate 的「读旧 + 写新」由 RwLock 保护；
//   * 任何两个引用的 digest 都不能重复（冲突检查在 new() 入队时执行）。

use async_trait::async_trait;
use kiana_domain::SecretRef;
use kiana_ports::{CredentialRotationPort, PortError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 内存中的凭证引用表。
///
/// 【架构角色】这是本适配器的**唯一**持久（进程内）数据结构。
/// `refs` 是一个「reference_digest -> SecretRef」的映射，用 RwLock 包装让轮转操作天然串行。
///
/// 【生命周期】
///   None (初始)
///       ↓ new() 入队初始引用
///   Some(SecretRef { generation=1, digest=X })
///       ├─ rotate_credential(generation=1) → 成功：生成新引用，generation=2, digest=Y，删除 X
///       ├─ rotate_credential(generation=1) → 失败：已不存在旧 digest，或 generation 已推进
///       ├─ rotate_credential(generation=2) → 成功：generation=3, digest=Z，删除 Y
///       └─ 循环往复...
///
/// 【重要约束】
///   * 任何时刻，内存中每个 reference_digest 最多只有一条记录（唯一性）；
///   * generation 溢出（已达 u64::MAX）会报 Failed；
///   * 即使轮转失败（Conflict/Unavailable），RwLock 也保证了状态一致性。
#[derive(Clone, Debug, Default)]
pub struct MemoryCredentialRotationStore {
    refs: Arc<RwLock<HashMap<String, SecretRef>>>,
}

impl MemoryCredentialRotationStore {
    /// 用初始引用列表构造存储。
    ///
    /// 【初始化逻辑】
    ///   ①校验每条引用的自身完整性（validate()）；
    ///   ②按 reference_digest 去重（同一 digest 重复入队报 Conflict）；
    ///   ③写入内存 HashMap。
    ///
    /// 【错误场景】
    ///   * credential_ref_invalid:{error}    -> 单条引用格式错误（如字段缺失、校验和不对）；
    ///   * credential_rotation_duplicate_reference -> 同一 digest 已在队列里，重复了。
    ///
    /// 【为什么要在初始化时检查去重】
    /// 内存 HashMap 要求 key 唯一，digest 作为 key 不能重复。如果重复允许通过，
    /// 后续 mutate() 调用会看到 HashMap::insert 覆盖旧值，导致状态混乱（丢失历史版本）。
    /// 在入口处主动拦这个错误，能让调用方尽早发现初始化配置错误。
    pub fn new(initial: impl IntoIterator<Item = SecretRef>) -> Result<Self, PortError> {
        let mut refs = HashMap::new();
        for secret_ref in initial {
            secret_ref
                .validate()
                .map_err(|error| PortError::Failed(format!("credential_ref_invalid:{error}")))?;
            if refs
                .insert(secret_ref.reference_digest.clone(), secret_ref)
                .is_some()
            {
                return Err(PortError::Conflict(
                    "credential_rotation_duplicate_reference".to_owned(),
                ));
            }
        }
        Ok(Self {
            refs: Arc::new(RwLock::new(refs)),
        })
    }

    /// 核心轮转/撤销逻辑（内部使用）。
    ///
    /// 这是 rotate_credential() 和 revoke_credential() 的共享实现。
    /// 两者都是「generation CAS + 签发新引用」的同一套流程。
    ///
    /// 【参数】
    ///   * secret_ref: 旧引用（包含 store、key、generation、audience 等）；
    ///   * observed_generation: 调用方观察到的【旧】generation 号。
    ///
    /// 【流程】
    ///   ①validate() 检查新旧引用格式；
    ///   ②如果 observed_generation == 0 -> 错误（generation 从 1 开始）；
    ///   ③获取写锁，按 reference_digest 查旧引用；
    ///   ④判断 generation CAS：现有的 generation 必须等于 observed_generation；
    ///   ⑤如果不匹配（代表别人已经轮过）-> Conflict；
    ///   ⑥生成新的 SecretRef，generation 递增 1（溢出报错）；
    ///   ⑦删除旧 digest，写入新 digest；
    ///   ⑧返回新引用。
    ///
    /// 【为什么 generation 是 u64】
    /// u64 = 足够大（不会在真实轮转场景溢出），yet 有溢出检查（overflow 报错）。
    /// 原则是：宁可让轮转失败（Err），也不能悄悄绕一圈回到旧 generation。
    ///
    /// 【为什么要 CAS】
    /// 场景：两个并发请求都说"我看到 generation=5，给我轮到 6"。
    /// CAS 确保只有第一个能成功（generation 变 6），第二个看到 generation 已是 6，报 Conflict。
    /// 这样应用就知道"轮转可能已被别人抢先做了"，可以重试或者退避。
    async fn mutate(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        secret_ref
            .validate()
            .map_err(|error| PortError::Failed(format!("credential_ref_invalid:{error}")))?;
        if observed_generation == 0 {
            return Err(PortError::Failed(
                "credential_rotation_generation_invalid".to_owned(),
            ));
        }
        let mut refs = self.refs.write().await;
        let current = refs.get(&secret_ref.reference_digest).ok_or_else(|| {
            PortError::Unavailable("credential_rotation_reference_missing".to_owned())
        })?;
        if current.generation != observed_generation {
            return Err(PortError::Conflict(
                "credential_rotation_generation_conflict".to_owned(),
            ));
        }
        let next = SecretRef::new(
            current.store.clone(),
            current.key.clone(),
            current.purpose.clone(),
            current.audience.clone(),
            observed_generation.checked_add(1).ok_or_else(|| {
                PortError::Failed("credential_rotation_generation_overflow".to_owned())
            })?,
        )
        .map_err(PortError::Failed)?;
        let old_digest = current.reference_digest.clone();
        refs.remove(&old_digest);
        refs.insert(next.reference_digest.clone(), next.clone());
        Ok(next)
    }
}

#[async_trait]
impl CredentialRotationPort for MemoryCredentialRotationStore {
    /// 轮转凭证：递增 generation，失效旧引用。
    ///
    /// 【调用方场景】
    /// ControlPlane 在每次重新准入前可以检查凭证是否需要轮转。
    /// 如果需要，调用本方法，得到新的不透明引用，然后用新引用重新做准入决策。
    ///
    /// 【语义】
    /// 这是一次「read-modify-write」的原子操作：
    ///   * 读当前 generation（== observed_generation 检查）；
    ///   * 递增 generation；
    ///   * 生成新 digest，删除旧 digest。
    /// 原子性由 RwLock 保证。
    ///
    /// 【失败原因】
    ///   * credential_ref_invalid:* -> 引用格式错误；
    ///   * credential_rotation_generation_invalid -> observed_generation 是 0；
    ///   * credential_rotation_reference_missing -> 这条引用不存在或已被删除；
    ///   * credential_rotation_generation_conflict -> generation 已被别人推进（乐观锁冲突）；
    ///   * credential_rotation_generation_overflow -> generation == u64::MAX，溢出。
    ///
    /// 【重试策略】
    /// 如果返回 Conflict，调用方应该重新查询当前引用，然后用新的 generation 重试。
    async fn rotate_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        self.mutate(secret_ref, observed_generation).await
    }

    /// 撤销凭证：生成新引用，让持有者的旧许可证立即失效。
    ///
    /// 【与 rotate_credential 的区别】
    /// 在**本适配器中**，两者的实现完全相同 —— 都是「generation CAS + 新引用」。
    /// 区别在于**语义**：
    ///   * rotate()  = 定期更新，主动轮转（安全最佳实践）；
    ///   * revoke()  = 紧急处置，某个租户的凭证被泄露，需要立即失效。
    /// 在真实的、事件驱动的轮转系统中，两者可能有不同的实现（如 revoke 可能还要标记
    /// 为「已撤销」、写审计日志等），但在这个 CI 版本中它们一样。
    ///
    /// 【关键承诺】
    /// 返回的新引用是**不透明的**（持有者无法伪造）。持有者的旧引用会立即无法用于
    /// 任何后续准入，因为内存中已删除了旧 digest —— `reference_missing` 会报错。
    /// 这样就形成了一个**代（generation）栅栏**：旧代的所有许可证同时失效。
    ///
    /// 【失败原因】
    /// 同 rotate_credential()。
    async fn revoke_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        // Revocation is represented by the generation fence.  The returned ref is opaque and
        // cannot be resolved by a stale lease; a later admission must prove a new generation.
        self.mutate(secret_ref, observed_generation).await
    }
}
