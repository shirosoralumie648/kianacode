//! WorkPacket 看板与 workflow queue 共用的 claim（认领资格）领域合同。
//!
//! 它位于“哪些 WorkPacket 可以进入队列”这一层：Core 用 `ready_packets()` 找到候选项，再把 packet 与父级
//! scope/budget/path-lock 摘要投影成 Claim；AUT-08 的 QueueStore 才会原子地创建 Lease，Daemon service 负责
//! 串行转发队列命令。Claim 本身只是可验证的数据结构，不保存队列、不锁定 worker，也不启动任务：
//!
//! ```text
//! WorkPacket 集合 -> readiness 投影 -> WorkflowQueueClaimContract
//!     -> WorkflowQueueStore 原子 claim -> WorkflowQueueLease
//!     -> Core 交接前核对 ready/packet/owner/fence/authority/deadline
//! ```
//!
//! packet 摘要由 `from_work_packet()` 根据已验证 WorkPacket 计算；依赖/活动数量等运行时观察值仍由调用者提供。
//! 本合同中的 SHA-256 字符串只做内容自洽检查，不是签名或持久化回执。
//! Serde 的 `Serialize`/`Deserialize` 把 Rust 值转成 JSON 等 wire 形式并读回来；`snake_case` 固定枚举标签，
//! `deny_unknown_fields` 则让带有未识别字段的输入显式失败，避免把拼错/版本不兼容的合同静默接受。

use crate::{json_digest, WorkPacket};
use serde::{Deserialize, Serialize};

/// AUT-07 Claim wire 结构版本；字段形状变化时需要与其生产者/消费者同步演进。
pub const WORKFLOW_QUEUE_CLAIM_SCHEMA: &str = "kiana.workflow-queue-claim.v1";

/// 一个候选项在队列视图中的分类；这不是后续租约的 `WorkflowQueueLeaseStatus`。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQueueClaimStatus {
    /// 满足当前 readiness 条件、尚未绑定 worker 的候选项。
    Ready,
    /// 已绑定 owner 的 Claim 描述；实际并发所有权仍由 AUT-08 Lease/Store 确认。
    Claimed,
    /// Claim 所给出的有效期已过。
    Expired,
    /// 由于依赖、策略或其他调用方观察状态暂不可认领。
    Blocked,
}

/// 将一个队列项与其 WorkPacket、父级范围和调度观察值绑定起来的可序列化合同。
///
/// `validate()` 检查字段形状、拒绝标志和自摘要；要证明这些摘要真与某个 packet 一致，调用者还必须执行
/// `validate_packet_binding()`。真正的唯一认领由 WorkflowQueueStore 的原子状态迁移完成。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueClaimContract {
    /// wire schema 版本。
    pub schema: String,
    /// 队列中的工作项 ID，正常由 WorkPacket ID 派生。
    pub item_id: String,
    /// 完整 WorkPacket 内容与 queue 相关字段的摘要。
    pub work_packet_digest: String,
    /// 父 WorkPacket 的 scope 摘要；当前 `validate()` 要求此 Option 必须为 Some。
    pub parent_scope_digest: Option<String>,
    /// 子/当前 WorkPacket 自身的 scope 摘要。
    pub item_scope_digest: String,
    /// 父 WorkPacket 的 budget 摘要；当前 `validate()` 要求此 Option 必须为 Some。
    pub parent_budget_digest: Option<String>,
    /// 子/当前 WorkPacket 自身的 budget 摘要。
    pub item_budget_digest: String,
    /// WorkPacket 路径锁集合的摘要，供下游确认写入范围未漂移。
    pub path_lock_digest: String,
    /// 调用者在这次快照中观察到的依赖是否全部完成。
    pub dependencies_resolved: bool,
    /// 调用者观察到的依赖图是否存在循环；true 会拒绝 Claim。
    pub dependency_cycle: bool,
    /// 子 packet scope 是否包含于父级允许范围；这是布尔观察，不替代 digest 重绑定。
    pub scope_intersection_valid: bool,
    /// 子预算是否受父级预算约束；这是布尔观察，不替代 digest 重绑定。
    pub budget_subset_valid: bool,
    /// 调用者观察到的当前活动 Claim 数；此字段本身不做并发占额。
    pub active_claim_count: u32,
    /// 本次调度允许的并发上限；必须大于零，且活动数等于上限时仍可通过本合同。
    pub max_parallel_claims: u32,
    /// 调用者观察到此项是否已被重复 Claim；true 会拒绝。
    pub duplicate_claim: bool,
    /// 已 Claim 项的 owner 标签；Ready 状态必须为 None，Claimed 状态必须非空。
    pub claim_owner: Option<String>,
    /// 此 Claim 候选的过期时刻，Unix 毫秒。
    pub claim_expires_at_unix_ms: u64,
    /// 形成上述调度观察值的时间，Unix 毫秒。
    pub observed_at_unix_ms: u64,
    /// 当前 Claim 投影状态。
    pub status: WorkflowQueueClaimStatus,
    /// 对上述字段的无密钥摘要，用于检测字段漂移，不认证生成者。
    pub claim_digest: String,
}

impl WorkflowQueueClaimContract {
    /// 从 packet 看板使用的同一 WorkPacket 合同派生 Claim 的不可变绑定字段。
    ///
    /// 【仓内调用】Core ready projector 与 Domain CP-24 fixture；当前 Rust 搜索未找到 QueueStore 调用本构造器。
    /// 【输入】当前 packet、父 packet、依赖/循环/活动数/并发上限/重复观察、owner、有效期、观察时间和目标状态。
    /// 【输出】packet/scope/budget/path-lock 摘要由 WorkPacket 重算，其余队列快照值按调用者提供的内容写入。
    /// 【失败】任一 packet 无效、父子 ID 关系不符或最终合同拒绝时返回稳定字符串错误。
    /// 【设计原因】不能相信请求里自带的 packet/scope/budget 摘要；这里从两份 WorkPacket 重新计算，防止 child
    /// 把更宽的路径或预算伪装成父级授权范围。
    /// 【边界】依赖完成、循环、活动计数与 duplicate 标志仍来自调用方快照；本函数不查询队列或做并发预留。
    #[allow(clippy::too_many_arguments)]
    pub fn from_work_packet(
        packet: &WorkPacket,
        parent: &WorkPacket,
        dependencies_resolved: bool,
        dependency_cycle: bool,
        active_claim_count: u32,
        max_parallel_claims: u32,
        duplicate_claim: bool,
        claim_owner: Option<String>,
        claim_expires_at_unix_ms: u64,
        observed_at_unix_ms: u64,
        status: WorkflowQueueClaimStatus,
    ) -> Result<Self, String> {
        packet.validate().map_err(str::to_owned)?;
        parent.validate().map_err(str::to_owned)?;
        if packet
            .parent_packet_id
            .as_deref()
            .is_some_and(|id| id != parent.id)
            || (packet.parent_packet_id.is_none() && packet.id != parent.id)
        {
            return Err("workflow_queue_parent_packet_mismatch".to_owned());
        }
        Self::new(
            packet.id.clone(),
            packet.workflow_queue_packet_digest(),
            Some(parent.workflow_queue_scope_digest()),
            packet.workflow_queue_scope_digest(),
            Some(parent.workflow_queue_budget_digest()),
            packet.workflow_queue_budget_digest(),
            packet.workflow_queue_path_lock_digest(),
            dependencies_resolved,
            dependency_cycle,
            packet.workflow_queue_scope_is_subset_of(parent),
            packet.workflow_queue_budget_is_subset_of(parent),
            active_claim_count,
            max_parallel_claims,
            duplicate_claim,
            claim_owner,
            claim_expires_at_unix_ms,
            observed_at_unix_ms,
            status,
        )
    }

    /// 判断本记录是否标为 Ready，供 Core ready view/transaction gate 区分可认领候选项。
    /// 这只读取当前合同的状态字段，不查询 WorkPacket，也不证明队列里没有竞争者。
    pub fn is_ready_packet(&self) -> bool {
        self.status == WorkflowQueueClaimStatus::Ready
    }

    /// 在把 Lease 交给 worker 前，用不可变 WorkPacket/父级快照重新核对 Claim 的内容绑定。
    ///
    /// 【仓内调用】Core 的 transaction gate 与 Domain CP-24 fixture；未找到产品路径直接调用 Core gate。
    /// QueueStore 实现当前没有直接调用本 helper。
    /// 【输入】合同中声明的当前 packet 与其父 packet 快照。
    /// 【成功】两份 packet 合法、父子关系与 scope/budget 收窄关系成立，且合同摘要和重新计算结果一致。
    /// 【失败】packet 无效、父级不匹配、scope/budget 扩大或任一摘要漂移时拒绝交接。
    /// 【容易误改】只比对调用方给出的 digest 会允许伪造过宽权限，所以必须从 packet 重新派生再比对。
    /// 当前实现没有单独比较 `self.item_id == packet.id`；Core 的 transaction gate 还会要求 Claim 与 ready view
    /// 中对应项完全相等，其他直接调用此 helper 的路径也必须自行保证 item ID 绑定。
    pub fn validate_packet_binding(
        &self,
        packet: &WorkPacket,
        parent: &WorkPacket,
    ) -> Result<(), String> {
        packet.validate().map_err(str::to_owned)?;
        parent.validate().map_err(str::to_owned)?;
        if packet
            .parent_packet_id
            .as_deref()
            .is_some_and(|id| id != parent.id)
            || (packet.parent_packet_id.is_none() && packet.id != parent.id)
        {
            return Err("workflow_queue_parent_packet_mismatch".to_owned());
        }
        if !packet.workflow_queue_scope_is_subset_of(parent) {
            return Err("workflow_queue_scope_intersection_invalid".to_owned());
        }
        if !packet.workflow_queue_budget_is_subset_of(parent) {
            return Err("workflow_queue_budget_subset_invalid".to_owned());
        }
        let packet_digest = packet.workflow_queue_packet_digest();
        if self.work_packet_digest != packet_digest {
            return Err("workflow_queue_packet_digest_mismatch".to_owned());
        }
        let parent_scope_digest = parent.workflow_queue_scope_digest();
        if self.parent_scope_digest.as_deref() != Some(parent_scope_digest.as_str()) {
            return Err("workflow_queue_parent_scope_digest_mismatch".to_owned());
        }
        let item_scope_digest = packet.workflow_queue_scope_digest();
        if self.item_scope_digest != item_scope_digest {
            return Err("workflow_queue_item_scope_digest_mismatch".to_owned());
        }
        let parent_budget_digest = parent.workflow_queue_budget_digest();
        if self.parent_budget_digest.as_deref() != Some(parent_budget_digest.as_str()) {
            return Err("workflow_queue_parent_budget_digest_mismatch".to_owned());
        }
        let item_budget_digest = packet.workflow_queue_budget_digest();
        if self.item_budget_digest != item_budget_digest {
            return Err("workflow_queue_item_budget_digest_mismatch".to_owned());
        }
        let path_lock_digest = packet.workflow_queue_path_lock_digest();
        if self.path_lock_digest != path_lock_digest {
            return Err("workflow_queue_path_lock_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 用显式字段构造 Claim 并立即计算/检查自摘要。
    ///
    /// 【输入】所有数据均由调用者传入，包括 digest 和调度观察布尔值；父 scope/budget 参数虽然是 Option，
    /// 但 `validate()` 当前要求两者都为 Some。
    /// 【输出】字段合同成立时返回 Claim；不成立时返回具体错误，不会产生外部副作用。
    /// 【安全边界】本构造函数只能证明字段格式和自摘要一致，不能证明 digest 对应真实 packet 或调用者有权调度。
    /// 安全交接应使用 `from_work_packet()` 并由队列 Store 原子仲裁。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        item_id: impl Into<String>,
        work_packet_digest: impl Into<String>,
        parent_scope_digest: Option<String>,
        item_scope_digest: impl Into<String>,
        parent_budget_digest: Option<String>,
        item_budget_digest: impl Into<String>,
        path_lock_digest: impl Into<String>,
        dependencies_resolved: bool,
        dependency_cycle: bool,
        scope_intersection_valid: bool,
        budget_subset_valid: bool,
        active_claim_count: u32,
        max_parallel_claims: u32,
        duplicate_claim: bool,
        claim_owner: Option<String>,
        claim_expires_at_unix_ms: u64,
        observed_at_unix_ms: u64,
        status: WorkflowQueueClaimStatus,
    ) -> Result<Self, String> {
        let mut contract = Self {
            schema: WORKFLOW_QUEUE_CLAIM_SCHEMA.to_owned(),
            item_id: item_id.into(),
            work_packet_digest: work_packet_digest.into(),
            parent_scope_digest,
            item_scope_digest: item_scope_digest.into(),
            parent_budget_digest,
            item_budget_digest: item_budget_digest.into(),
            path_lock_digest: path_lock_digest.into(),
            dependencies_resolved,
            dependency_cycle,
            scope_intersection_valid,
            budget_subset_valid,
            active_claim_count,
            max_parallel_claims,
            duplicate_claim,
            claim_owner,
            claim_expires_at_unix_ms,
            observed_at_unix_ms,
            status,
            claim_digest: String::new(),
        };
        contract.claim_digest = contract.digest();
        contract.validate()?;
        Ok(contract)
    }

    /// 校验版本、文本/digest 外形、调度拒绝标志、owner/status 关系、时间和自摘要。
    ///
    /// 【失败】分别拒绝缺父 scope/budget、并发上限为零或活动数超过上限、循环/未解决依赖、范围/预算扩大、
    /// duplicate、观察时间无效/Claim 已过期、owner/status 不一致及摘要漂移。
    /// 【语义边界】活动数等于 max 时允许通过；这不是原子 capacity reservation。Expired/Blocked 也没有比
    /// 通用字段合同更多的状态专属约束，真实可执行性由 readiness 投影和 Store 再判定。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_QUEUE_CLAIM_SCHEMA {
            return Err("workflow_queue_claim_schema_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.item_id, "workflow_queue_item_id", 256),
            (&self.claim_digest, "workflow_queue_claim_digest", 512),
        ] {
            bounded(value, field, max)?;
        }
        for (value, field) in [
            (&self.work_packet_digest, "workflow_queue_packet_digest"),
            (&self.item_scope_digest, "workflow_queue_item_scope_digest"),
            (
                &self.item_budget_digest,
                "workflow_queue_item_budget_digest",
            ),
            (&self.path_lock_digest, "workflow_queue_path_lock_digest"),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                &self.parent_scope_digest,
                "workflow_queue_parent_scope_digest",
            ),
            (
                &self.parent_budget_digest,
                "workflow_queue_parent_budget_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if self.parent_scope_digest.is_none() {
            return Err("workflow_queue_parent_scope_required".to_owned());
        }
        if self.parent_budget_digest.is_none() {
            return Err("workflow_queue_parent_budget_required".to_owned());
        }
        if self.max_parallel_claims == 0 || self.active_claim_count > self.max_parallel_claims {
            return Err("workflow_queue_parallel_limit_exceeded".to_owned());
        }
        if self.dependency_cycle {
            return Err("workflow_queue_dependency_cycle".to_owned());
        }
        if !self.dependencies_resolved {
            return Err("workflow_queue_dependency_unresolved".to_owned());
        }
        if !self.scope_intersection_valid {
            return Err("workflow_queue_scope_intersection_invalid".to_owned());
        }
        if !self.budget_subset_valid {
            return Err("workflow_queue_budget_subset_invalid".to_owned());
        }
        if self.duplicate_claim {
            return Err("workflow_queue_duplicate_claim".to_owned());
        }
        if self.observed_at_unix_ms == 0
            || self.claim_expires_at_unix_ms <= self.observed_at_unix_ms
        {
            return Err("workflow_queue_claim_expired".to_owned());
        }
        if self.status == WorkflowQueueClaimStatus::Claimed
            && self.claim_owner.as_deref().is_none_or(str::is_empty)
        {
            return Err("workflow_queue_claim_owner_required".to_owned());
        }
        if self.status == WorkflowQueueClaimStatus::Ready && self.claim_owner.is_some() {
            return Err("workflow_queue_ready_claim_owner_invalid".to_owned());
        }
        if self.claim_digest != self.digest() {
            return Err("workflow_queue_claim_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将除了 `claim_digest` 自身以外的 Claim 字段序列化成稳定内容摘要；摘要不是签名或 queue receipt。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "item_id": self.item_id,
            "work_packet_digest": self.work_packet_digest,
            "parent_scope_digest": self.parent_scope_digest,
            "item_scope_digest": self.item_scope_digest,
            "parent_budget_digest": self.parent_budget_digest,
            "item_budget_digest": self.item_budget_digest,
            "path_lock_digest": self.path_lock_digest,
            "dependencies_resolved": self.dependencies_resolved,
            "dependency_cycle": self.dependency_cycle,
            "scope_intersection_valid": self.scope_intersection_valid,
            "budget_subset_valid": self.budget_subset_valid,
            "active_claim_count": self.active_claim_count,
            "max_parallel_claims": self.max_parallel_claims,
            "duplicate_claim": self.duplicate_claim,
            "claim_owner": self.claim_owner,
            "claim_expires_at_unix_ms": self.claim_expires_at_unix_ms,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "status": self.status,
        }))
    }
}

/// 检查标识非空、UTF-8 字节数不超过调用方上限，并拒绝 NUL/换行。
fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验 `sha256:` 加 64 位十六进制文本的外形；不重算内容，也不认证来源。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
