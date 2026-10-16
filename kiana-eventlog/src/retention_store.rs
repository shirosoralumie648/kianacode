//! 事件保留策略存储：定义哪些事件该保留多长时间、哪些归档。
//!
//! # 什么是「保留策略」
//!
//! 事件账本可能包含敏感信息、临时数据、审计日志等，它们的保存期限各不相同。
//! 这个文件的职责是**存储和查询这些保留计划**：
//! - 法律保留（legal hold）：某类信息必须保留直到诉讼/审计结束
//! - 定期扫描（retention scan）：定期检查哪些事件该删除、哪些继续保留
//! - 删除证明（deletion tombstone）：记录某数据何时被删除、被谁删除
//! - 传播收据（propagation receipt）：确认删除命令已传到所有数据副本
//!
//! # 这是「策略存储」不是「执行引擎」
//!
//! 这个文件**不执行删除**。它只存储「删除计划」和「删除已完成」的证明。
//! 真正的删除由 SC-23（容量管理能力）来执行；这里存的是策略声明和审计日志。
//! 这样设计的好处是：事件账本永远不被这一层改变，账本的事实性得到保护。
//!
//! # 为什么要非持久化
//!
//! 这是一个**CI 测试用的适配器**，不面向生产。它在内存里存一切，进程重启就消失。
//! 真正的持久化由外层的持久化引擎（比如 JSONL 文件存储）来做。
//! 这样可以让 CI 快速运行、不依赖文件系统。
//!
//! 【与 `jsonl.rs` 的分界】
//! `jsonl.rs` 说「这个事件已经持久化了」，靠的是完整帧+校验和+fsync。
//! 这个文件说「根据策略，这个事件下周应该被删」，靠的是内存树，关键是**策略的表达**。
//! 不要把两个混在一起。

use async_trait::async_trait;
use kiana_domain::{
    DataPropagationPlan, DataPropagationReceipt, DeletionManifest, DeletionTombstone, EventCursor,
    LegalHoldReceipt, RequestId, RetentionScan, StoreIdentityId,
};
use kiana_ports::{PortError, RetentionStorePort};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 保留策略的内存状态。这是一个非持久化的快照，进程重启会丢失。
///
/// # 字段含义
///
/// - `scans`：定期扫描的结果。键是 (数据存储 ID, 光标位置)。
///   一次「扫描」是对某个时刻的审视：哪些事件该保留、哪些过期。
///
/// - `holds`：法律保留（诉讼冻结）。键是 (数据存储 ID, 保留 ID)。
///   某些数据被法律冻结，不能删除。这里存的是冻结证明。
///
/// - `tombstones`：删除证明。键是 (数据存储 ID, 删除 ID)。
///   某条数据被删除了，这里记录「什么时候被谁删的」。
///
/// - `manifests`：删除清单。键是 (数据存储 ID, 请求 ID)。
///   一次「请求删除 100 条数据」会产生一份清单，列出所有被删的 tombstone ID。
///   清单是「这次删除行动包含了哪些数据」的完整记录。
///
/// - `propagation`：传播收据。键是 (数据存储 ID, 目标, 数据纪元)。
///   删除命令传到了外部系统（比如备份存储），这里存的是确认收据。
///   「删除」不等于「彻底消失」；必须确认所有副本都删了。
///
/// - `deletion_epochs`：最新的删除纪元。键是数据存储 ID。
///   防止时间倒流：新的删除纪元必须 >= 旧的，否则拒绝。
///
/// 【这些集合都是「只能追加」的】
/// 一旦一条 tombstone 被记录，它不能被改。新的删除必须产生新的 tombstone ID。
/// 这样审计链才是清晰的。
#[derive(Debug, Default)]
struct RetentionState {
    scans: BTreeMap<(StoreIdentityId, EventCursor), RetentionScan>,
    holds: BTreeMap<(StoreIdentityId, String), LegalHoldReceipt>,
    tombstones: BTreeMap<(StoreIdentityId, String), DeletionTombstone>,
    manifests: BTreeMap<(StoreIdentityId, RequestId), DeletionManifest>,
    propagation: BTreeMap<(StoreIdentityId, String, u64), DataPropagationReceipt>,
    deletion_epochs: BTreeMap<StoreIdentityId, u64>,
}

/// 非持久化的保留策略边界，用于 CI 测试和本地组装。
///
/// # 设计决策：为什么用 Arc<Mutex<RetentionState>>
///
/// - `Arc`：多个 clone 可以指向同一个内存状态，适合在异步任务间共享。
/// - `Mutex`：同一时刻只有一个任务能修改状态，防止竞态条件。
/// - `Mutex`（不是 `RwLock`）：因为修改很频繁，写多于读，`Mutex` 更简单。
///
/// # 非持久化的含义
///
/// 进程重启、崩溃、或内存耗尽，所有的保留计划都会丢失。
/// 这对 CI 没问题，因为测试是短期的；真正的生产环境会用持久化的实现。
///
/// 【与 EventLog 的关系】
/// EventLog 存的是事件本身（append-only，不能改）。
/// RetentionStore 存的是「这些事件什么时候应该删」的策略。
/// 两者分离，确保事件账本的纯净性：删除决策改变，账本不变。
#[derive(Clone, Debug, Default)]
pub struct MemoryRetentionStore {
    state: Arc<Mutex<RetentionState>>,
}

impl MemoryRetentionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次保留策略扫描。
    ///
    /// # 扫描是什么
    ///
    /// 扫描是对「现在应该删哪些数据」的一次决策。它包含：
    /// - 扫描时间点（光标）
    /// - 政策版本号（如果政策改了，对旧数据的保留期限不同）
    /// - 数据纪元（同一时间点的多个版本数据的代号）
    /// - 扫描结果摘要（哈希值，用来检测冲突）
    /// - 所有被冻结的数据（法律保留列表）
    ///
    /// # 单调性检查：光标不能倒流
    ///
    /// 如果上一次扫描在光标 100，这次不能在光标 50。
    /// 这防止了「我们已经看过光标 100 的数据，现在又重新扫描光标 50」这样的矛盾。
    ///
    /// # 摘要冲突检查
    ///
    /// 如果已经有一条「光标 100，摘要 ABC」的扫描记录，
    /// 再来一条「光标 100，摘要 XYZ」的扫描，说明两次扫描结果不同，
    /// 这是不允许的。同一个光标的扫描结果必须一致。
    ///
    /// 【为什么要检查摘要而不是完整比较】
    /// 扫描数据可能很大（几千条法律保留项），完整存储很浪费。
    /// 只存摘要，节省空间，同时保留了「检测冲突」的能力。
    ///
    /// # 法律保留的合并
    ///
    /// 扫描结果里包含了所有活跃的法律保留。新扫描会把这些保留记录下来。
    /// 这样，即使扫描时间间隔很长，法律保留链也是完整的。
    pub async fn append_scan(
        &self,
        store_id: StoreIdentityId,
        scan: RetentionScan,
    ) -> Result<(), PortError> {
        scan.validate()
            .map_err(|error| PortError::Failed(format!("retention_scan:{error}")))?;
        let mut state = self.state.lock().await;

        // 【单调性检查】光标不能向后跳。
        if let Some((_, latest)) = state
            .scans
            .iter()
            .filter(|((id, _), _)| *id == store_id)
            .max_by_key(|((_, cursor), _)| *cursor)
        {
            if latest.source_cursor > scan.source_cursor {
                return Err(PortError::Conflict(
                    "retention_scan_cursor_regressed".to_owned(),
                ));
            }
        }

        // 【摘要冲突检查】同一光标的扫描结果必须一致。
        let key = (store_id, scan.source_cursor);
        if let Some(existing) = state.scans.get(&key) {
            if existing.scan_digest == scan.scan_digest {
                return Ok(()); // 幂等：同样的扫描重复提交是可以接受的
            }
            return Err(PortError::Conflict(
                "retention_scan_digest_conflict".to_owned(),
            ));
        }

        // 【法律保留的验证】扫描里包含的所有保留必须有效。
        for receipt in &scan.hold_receipts {
            Self::check_hold(&state, store_id, receipt)?;
        }

        // 【记录保留】把这次扫描带来的所有保留都存下来。
        for receipt in &scan.hold_receipts {
            state
                .holds
                .insert((store_id, receipt.hold_id.clone()), receipt.clone());
        }
        state.scans.insert(key, scan);
        Ok(())
    }

    /// 添加一条法律保留（诉讼冻结）。
    ///
    /// # 法律保留是什么
    ///
    /// 当有诉讼或监管审查时，某些数据必须保留不删。即使保留期满了，
    /// 只要诉讼还在进行，那些数据就不能碰。这条方法用来记录这样的冻结命令。
    ///
    /// # 冲突检查
    ///
    /// 同一个保留 ID 不能被添加两次。如果意外重复添加，我们检查内容是否一致。
    /// 如果一致，幂等返回成功；如果不一致，说明有人试图用同一个 ID 记录不同的保留，拒绝。
    pub async fn append_hold_receipt(
        &self,
        store_id: StoreIdentityId,
        receipt: LegalHoldReceipt,
    ) -> Result<(), PortError> {
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("legal_hold_receipt:{error}")))?;
        let mut state = self.state.lock().await;
        Self::check_hold(&state, store_id, &receipt)?;
        state
            .holds
            .insert((store_id, receipt.hold_id.clone()), receipt);
        Ok(())
    }

    /// 记录一条删除证明（tombstone）。
    ///
    /// # Tombstone 是什么
    ///
    /// 当某条数据被删除时，我们不会从账本里真的把它抹掉（账本是 append-only）。
    /// 取而代之的是，记录一份「这条数据在这个时间被这个请求删掉了」的证明。
    /// 这叫做 tombstone（墓碑）——一个标记，说明这个数据已经「死了」，不应该被使用。
    ///
    /// # 数据纪元防护
    ///
    /// 删除必须按时间顺序进行。不能今天删除纪元 3 的数据，明天又说删除纪元 2 的。
    /// 这是一个防护机制，防止删除命令乱序或被重放。
    ///
    /// 【deletion_epochs 的作用】
    /// 每个数据存储都有一个「最新删除纪元」的记录。
    /// 新的 tombstone 的 data_epoch 必须 >= 这个记录的值。
    /// 如果不是，说明有人试图提交一个「过时的」删除，拒绝。
    ///
    /// # 摘要冲突检查
    ///
    /// 同一个 tombstone ID，摘要必须一致。这用来检测删除命令是否被篡改。
    pub async fn append_deletion_tombstone(
        &self,
        store_id: StoreIdentityId,
        tombstone: DeletionTombstone,
    ) -> Result<(), PortError> {
        tombstone
            .validate()
            .map_err(|error| PortError::Failed(format!("deletion_tombstone:{error}")))?;
        let mut state = self.state.lock().await;

        // 【摘要一致性】同一个 tombstone ID 的内容必须不变。
        let key = (store_id, tombstone.tombstone_id.clone());
        if let Some(existing) = state.tombstones.get(&key) {
            if existing.tombstone_digest == tombstone.tombstone_digest {
                return Ok(()); // 幂等
            }
            return Err(PortError::Conflict(
                "deletion_tombstone_digest_conflict".to_owned(),
            ));
        }

        // 【纪元防护】删除不能时间倒流。
        if state
            .deletion_epochs
            .get(&store_id)
            .is_some_and(|epoch| tombstone.data_epoch < *epoch)
        {
            return Err(PortError::Conflict(
                "deletion_data_epoch_regressed".to_owned(),
            ));
        }

        // 【更新纪元记录】这个数据存储的最新删除纪元向前推进。
        state
            .deletion_epochs
            .entry(store_id)
            .and_modify(|epoch| *epoch = (*epoch).max(tombstone.data_epoch))
            .or_insert(tombstone.data_epoch);
        state.tombstones.insert(key, tombstone);
        Ok(())
    }

    /// 记录一份删除清单（deletion manifest）。
    ///
    /// # 清单的作用
    ///
    /// 一次删除操作可能涉及成百上千的 tombstone。
    /// 清单是「这一次删除行动包含了哪些 tombstone ID」的完整记录。
    /// 它是一个**原子单位**：要么所有 tombstone 都被认可为同一个请求的产物，
    /// 要么都不认可。
    ///
    /// # 边界一致性检查
    ///
    /// 清单里的每一个 tombstone ID，都必须指向一条真实存在的 tombstone 记录。
    /// 而且，这条记录的所有关键字段必须与清单一致：
    /// - request_id：都是同一个请求
    /// - project_ref：都涉及同一个项目
    /// - policy_revision：都基于同一版本的政策
    /// - data_epoch：都在同一个数据纪元
    /// - source_cursor：都从同一个光标开始扫描
    ///
    /// 【为什么要这么严格】
    /// 如果某个 tombstone 的关键字段与清单不一致，说明数据被篡改或损坏了。
    /// 不能冒着数据完整性的代价去让「几乎一致」的情况过关。
    pub async fn append_deletion_manifest(
        &self,
        store_id: StoreIdentityId,
        manifest: DeletionManifest,
    ) -> Result<(), PortError> {
        manifest
            .validate()
            .map_err(|error| PortError::Failed(format!("deletion_manifest:{error}")))?;
        let mut state = self.state.lock().await;

        // 【清单唯一性】同一个请求的清单只能有一份。
        let key = (store_id, manifest.request_id);
        if let Some(existing) = state.manifests.get(&key) {
            if existing.manifest_digest == manifest.manifest_digest {
                return Ok(()); // 幂等
            }
            return Err(PortError::Conflict(
                "deletion_manifest_digest_conflict".to_owned(),
            ));
        }

        // 【tombstone 边界检查】清单里的每个 ID 都必须指向一条真实的、一致的 tombstone。
        for tombstone_id in &manifest.tombstone_ids {
            let tombstone = state
                .tombstones
                .get(&(store_id, tombstone_id.clone()))
                .ok_or_else(|| PortError::Unavailable("deletion_tombstone_missing".to_owned()))?;

            // 【字段一致性检查】这条 tombstone 的关键字段必须与清单匹配。
            if tombstone.request_id != manifest.request_id
                || tombstone.project_ref != manifest.project_ref
                || tombstone.policy_revision != manifest.policy_revision
                || tombstone.data_epoch != manifest.data_epoch
                || tombstone.source_cursor != manifest.source_cursor
            {
                return Err(PortError::Conflict(
                    "deletion_manifest_tombstone_boundary_mismatch".to_owned(),
                ));
            }
        }
        state.manifests.insert(key, manifest);
        Ok(())
    }

    /// 查询最新的保留策略扫描结果（在给定光标之前）。
    async fn latest_scan(
        &self,
        store_id: StoreIdentityId,
        before_cursor: EventCursor,
    ) -> Option<RetentionScan> {
        self.state
            .lock()
            .await
            .scans
            .iter()
            .filter(|((id, cursor), _)| *id == store_id && *cursor <= before_cursor)
            .max_by_key(|((_, cursor), _)| *cursor)
            .map(|(_, scan)| scan.clone())
    }

    /// 记录一份传播收据（propagation receipt）。
    ///
    /// # 传播是什么
    ///
    /// 删除命令不只在主系统执行。主系统有备份、镜像、归档存储等多个副本。
    /// 一条数据的完整删除，需要在所有这些地方都执行。
    /// 传播收据就是「某副本确认收到并执行了删除」的证明。
    ///
    /// # 前置条件：必须先有删除清单
    ///
    /// 一个传播收据不能凭空出现。它必须对应一份真实的删除清单。
    /// 这确保了所有传播都是针对已知、已审查过的删除行动。
    ///
    /// # 边界一致性检查
    ///
    /// 传播收据的所有关键字段必须与对应的传播计划和删除清单一致：
    /// - project_ref：同一个项目
    /// - previous_epoch / data_epoch：数据纪元连续
    /// - source_cursor：同一个扫描光标
    /// - tombstone_digest：删除摘要不变
    /// - target：传播目标在计划里
    ///
    /// 如果有任何不一致，说明有人试图混淆「哪个删除传到了哪里」，拒绝。
    pub async fn append_propagation_receipt(
        &self,
        store_id: StoreIdentityId,
        plan: DataPropagationPlan,
        receipt: DataPropagationReceipt,
    ) -> Result<(), PortError> {
        plan.validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_plan:{error}")))?;
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_receipt:{error}")))?;

        // 【边界一致性】收据必须与计划的所有关键字段匹配。
        if receipt.project_ref != plan.project_ref
            || receipt.previous_epoch != plan.previous_epoch
            || receipt.data_epoch != plan.data_epoch
            || receipt.source_cursor != plan.source_cursor
            || receipt.tombstone_digest != plan.tombstone_digest
            || !plan
                .targets
                .iter()
                .any(|target| target.target == receipt.target)
        {
            return Err(PortError::Conflict(
                "data_propagation_receipt_boundary_mismatch".to_owned(),
            ));
        }

        let mut state = self.state.lock().await;

        // 【删除清单存在性】必须先有一份这个项目+纪元+光标组合的删除清单。
        let manifest = state
            .manifests
            .values()
            .find(|manifest| {
                manifest.project_ref == plan.project_ref
                    && manifest.data_epoch == plan.data_epoch
                    && manifest.source_cursor == plan.source_cursor
            })
            .ok_or_else(|| PortError::Unavailable("deletion_manifest_missing".to_owned()))?;

        // 【tombstone 列表非空】清单里至少要有一个 tombstone，否则这个删除操作是空的。
        if manifest.tombstone_ids.is_empty() {
            return Err(PortError::Unavailable(
                "deletion_manifest_tombstone_missing".to_owned(),
            ));
        }

        // 【收据唯一性】同一个传播目标不能有两份不同的收据。
        let key = (
            store_id,
            receipt.target.as_str().to_owned(),
            receipt.data_epoch,
        );
        if let Some(existing) = state.propagation.get(&key) {
            if existing == &receipt {
                return Ok(()); // 幂等
            }
            return Err(PortError::Conflict(
                "data_propagation_receipt_conflict".to_owned(),
            ));
        }
        state.propagation.insert(key, receipt);
        Ok(())
    }

    /// 查询某个数据纪元的所有传播收据。
    pub async fn propagation_receipts(
        &self,
        store_id: StoreIdentityId,
        data_epoch: u64,
    ) -> Vec<DataPropagationReceipt> {
        self.state
            .lock()
            .await
            .propagation
            .iter()
            .filter(|((id, _, epoch), _)| *id == store_id && *epoch == data_epoch)
            .map(|(_, receipt)| receipt.clone())
            .collect()
    }

    /// 检查法律保留记录的一致性。
    ///
    /// # 作用
    ///
    /// 如果相同的 hold_id 已经被记录过，新记录的摘要必须与旧的一致。
    /// 这防止了同一个 hold ID 被用来记录两条不同的保留（要么是数据篡改，要么是逻辑错误）。
    fn check_hold(
        state: &RetentionState,
        store_id: StoreIdentityId,
        receipt: &LegalHoldReceipt,
    ) -> Result<(), PortError> {
        if let Some(existing) = state.holds.get(&(store_id, receipt.hold_id.clone())) {
            if existing.receipt_digest == receipt.receipt_digest {
                return Ok(()); // 幂等
            }
            return Err(PortError::Conflict(
                "legal_hold_receipt_digest_conflict".to_owned(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl RetentionStorePort for MemoryRetentionStore {
    /// 根据最新的扫描结果生成保留计划。
    ///
    /// # 输出内容
    ///
    /// 返回一个 JSON，包含：
    /// - 这个数据存储要采用哪个政策版本
    /// - 哪些事件应该保留
    /// - 哪些已经过期但被法律保留冻结
    /// - 扫描摘要（用来验证完整性）
    /// - 完整的扫描对象（给消费者检查细节）
    ///
    /// # 如果没有扫描
    ///
    /// 如果我们从未对这个存储进行过保留策略扫描，返回 Unavailable 错误。
    /// 这不是一个失败状态，只是「我们还不知道应该删什么」。
    async fn plan_retention(
        &self,
        store_id: StoreIdentityId,
        before_cursor: EventCursor,
    ) -> Result<serde_json::Value, PortError> {
        let scan = self
            .latest_scan(store_id, before_cursor)
            .await
            .ok_or_else(|| PortError::Unavailable("retention_scan_missing".to_owned()))?;
        Ok(json!({
            "schema": "kiana.retention-plan.v1",
            "store_id": store_id,
            "project_ref": scan.project_ref.clone(),
            "policy_revision": scan.policy_revision,
            "data_epoch": scan.data_epoch,
            "source_cursor": scan.source_cursor,
            "projection_cursor": scan.projection_cursor,
            "scan_digest": scan.scan_digest.clone(),
            "scan": scan,
        }))
    }

    /// 【弃用】墓碑操作已交由 SC-23 处理。
    ///
    /// 这个方法的存在是为了保持接口兼容性，但不实际做任何事。
    /// 真正的删除证明应该通过 `append_deletion_tombstone` 来提交。
    async fn append_tombstone(
        &self,
        _store_id: StoreIdentityId,
        _object_ref: &str,
        _reason: &str,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "retention_tombstone_deferred_to_sc23".to_owned(),
        ))
    }

    /// 添加删除墓碑——委托给实例方法。
    async fn append_deletion_tombstone(
        &self,
        store_id: StoreIdentityId,
        tombstone: DeletionTombstone,
    ) -> Result<(), PortError> {
        MemoryRetentionStore::append_deletion_tombstone(self, store_id, tombstone).await
    }

    /// 添加删除清单——委托给实例方法。
    async fn append_deletion_manifest(
        &self,
        store_id: StoreIdentityId,
        manifest: DeletionManifest,
    ) -> Result<(), PortError> {
        MemoryRetentionStore::append_deletion_manifest(self, store_id, manifest).await
    }

    /// 添加传播收据——委托给实例方法。
    async fn append_propagation_receipt(
        &self,
        store_id: StoreIdentityId,
        plan: DataPropagationPlan,
        receipt: DataPropagationReceipt,
    ) -> Result<(), PortError> {
        MemoryRetentionStore::append_propagation_receipt(self, store_id, plan, receipt).await
    }

    /// 【未实现】物理清理操作。
    ///
    /// 这个方法本应执行「删除所有已标记为删除的数据」。但在这个内存实现里，
    /// 我们不管理实际的数据存储，只是跟踪删除状态。真正的清理由外层的持久化存储实现。
    ///
    /// 【为什么接口要求这个方法】
    /// 为了让 SC-23（容量管理）能够在统一的接口下工作。
    /// 每种存储实现都可能有不同的清理策略（是立即删除、还是异步清理、还是留给 GC）。
    async fn purge_tombstoned(
        &self,
        _store_id: StoreIdentityId,
        _expected_tombstone_revision: u64,
    ) -> Result<u64, PortError> {
        Err(PortError::Unavailable(
            "retention_purge_deferred_to_sc23".to_owned(),
        ))
    }
}
