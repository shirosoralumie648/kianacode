//! Workflow queue 的 lease（租约）、心跳、fence（栅栏）和恢复合同。
//!
//! Claim 说明某个 WorkPacket 当前符合队列准入条件；本文件的 Lease 才把某个 item 暂时绑定到 worker、fence
//! token 和 authority epoch。Domain 只计算每一步是否合法，不保存记录。WorkflowQueueStore 要把这些纯状态
//! 转换放进一个原子存储操作；Daemon 的 `WorkflowQueueService` 通过有界 Tokio channel 转发请求；Core 在把项
//! 交给既有命令路径前再核对 owner/fence/epoch/expiry。未知 effect（外部动作可能已发生但回执未知）不能被当成
//! “没执行”后重新发起：
//!
//! ```text
//! Ready Claim -> Active/NotStarted Lease -> Running -> Succeeded | Failed | ResultUnknown
//!                    | heartbeat                 | fence/reclaim 由 Store 原子仲裁
//!                    └─ expiry + NotStarted -> 新 owner + 更大的 fence 才可 reclaim
//! ```
//!
//! 序列化的 digest 只串联字段，不是带密钥的身份认证；跨 worker 的唯一所有权来自 Store 的原子状态变更。
//! Serde 的序列化/反序列化 derive 用于 wire/storage 读写；`snake_case` 固定状态标签，`deny_unknown_fields`
//! 拒绝未识别字段。`previous_lease_digest` 的默认 None 只适用于 genesis，后续版本仍须由 validate 检查序号关系。

use crate::{json_digest, WorkflowQueueClaimContract, WorkflowQueueClaimStatus};
use serde::{Deserialize, Serialize};

/// AUT-08 Lease wire 结构版本。
pub const WORKFLOW_QUEUE_LEASE_SCHEMA: &str = "kiana.workflow-queue-lease.v1";
/// 心跳到期时刻与当前观察时刻最多相差 300,000 毫秒（5 分钟）。
/// 这是单次租约 TTL 上限，不是 worker 自动续约周期。
pub const WORKFLOW_QUEUE_LEASE_TTL_MAX_MS: u64 = 300_000;

/// 租约本身的生命周期状态；Completed/Fenced 是终态，ResultUnknown 要进入恢复流程。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQueueLeaseStatus {
    /// 当前 owner 持有有效租约，仍可做允许的队列状态转换。
    Active,
    /// effect 已明确成功或失败，不能再 heartbeat/fence/reclaim。
    Completed,
    /// 已撤销原租约，且当前没有需要追查的进行中/未知 effect。
    Fenced,
    /// effect 可能已发生但结果无法确认；不得重新调度，必须先恢复/核对。
    ResultUnknown,
}

/// 业务 effect 的执行观察状态，与租约是否仍 Active 是两个不同维度。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQueueEffectState {
    /// 尚无 effect 开始的证据，因此过期后才可能安全转给新 owner。
    NotStarted,
    /// 已记录开始，但还没有最终结果。
    Running,
    /// 已确认 effect 成功。
    Succeeded,
    /// 已确认 effect 失败；仍属于已结束尝试。
    Failed,
    /// 结果无法确认；重试可能造成外部动作重复。
    ResultUnknown,
}

/// 一条 worker 对队列项的有界租约状态快照。
///
/// 新快照通过上一条 `lease_digest` 串联，`sequence` 每次状态变化递增。该链在这里仅以 Option/摘要形状
/// 表示；要证明历史连续性，仍须由 Store 读取并核对实际前一版本。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueLease {
    /// wire schema 版本。
    pub schema: String,
    /// 对应的 WorkPacket/queue item ID。
    pub item_id: String,
    /// 产生此 Lease 的 Claim 内容摘要。
    pub claim_digest: String,
    /// 当前持有此租约的 worker/协调者标签。
    pub owner_id: String,
    /// 此 item 租约状态的单调版本号；初始值为 1，每个 transition 加 1。
    pub sequence: u64,
    /// 旧 worker 的栅栏代次；安全 reclaim 必须提供更大的 token。
    pub fence_token: u64,
    /// 产生或持有该租约时的授权配置代次，动作时要求与当前值完全一致。
    pub authority_epoch: u64,
    /// 本代租约的签发时间，Unix 毫秒。
    pub issued_at_unix_ms: u64,
    /// 最近一次有效 heartbeat/effect/fence 的观察时间，Unix 毫秒。
    pub heartbeat_at_unix_ms: u64,
    /// 本代租约到期时间，Unix 毫秒；到达该毫秒即视为 expired。
    pub expires_at_unix_ms: u64,
    /// effect 的业务执行观察状态。
    pub effect_state: WorkflowQueueEffectState,
    /// Lease 的租约生命周期状态。
    pub status: WorkflowQueueLeaseStatus,
    /// 上一代 lease digest；None 只允许出现在初始 sequence=1 的 genesis lease。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_lease_digest: Option<String>,
    /// 对本 Lease 字段的无密钥摘要，不等同于签名或 Store 提交证明。
    pub lease_digest: String,
}

impl WorkflowQueueLease {
    /// 从仍为 Ready 的 Claim 签发第一代 Lease。
    ///
    /// 【调用者】WorkflowQueueStore 在确认该 item 尚未被其他 worker 原子占用后。
    /// 【输入】Claim、owner、非零 fence token/authority epoch、当前观察时刻和过期时刻。
    /// 【输出】sequence=1、effect=NotStarted、status=Active 的首条租约快照。
    /// 【失败】Claim 无效/非 Ready，或 Lease 的 ID、时间、TTL、digest 合同不成立时返回错误。
    /// 【副作用】仅构造内存值；这里没有写入 Store，也不能阻止另一个并发调用同时构造相同 Lease。
    /// 【重要】唯一 claim 必须由 Store 在同一原子临界区中完成，不能把本函数成功当成资源已被独占。
    pub fn issue(
        claim: &WorkflowQueueClaimContract,
        owner_id: impl Into<String>,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        claim.validate()?;
        if claim.status != WorkflowQueueClaimStatus::Ready {
            return Err("workflow_queue_claim_not_ready".to_owned());
        }
        let mut lease = Self {
            schema: WORKFLOW_QUEUE_LEASE_SCHEMA.to_owned(),
            item_id: claim.item_id.clone(),
            claim_digest: claim.claim_digest.clone(),
            owner_id: owner_id.into(),
            sequence: 1,
            fence_token,
            authority_epoch,
            issued_at_unix_ms: observed_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms,
            effect_state: WorkflowQueueEffectState::NotStarted,
            status: WorkflowQueueLeaseStatus::Active,
            previous_lease_digest: None,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    /// 判断给定服务端观察时刻是否已达到 expires_at；等于到期时刻就算过期。
    pub fn is_expired(&self, now_unix_ms: u64) -> bool {
        now_unix_ms >= self.expires_at_unix_ms
    }

    /// 检查某次队列交接当前是否仍可开始 effect。
    ///
    /// 先核对 Lease 自身、owner、fence token、authority epoch，再要求租约 Active、effect 尚未开始且未到期。
    /// 这里接收的 `now_unix_ms` 来自调用方，不读取可信 ClockPort；调用方必须确保它是当前可信观察值。
    pub fn is_dispatchable(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_active".to_owned());
        }
        if self.effect_state != WorkflowQueueEffectState::NotStarted {
            return Err("workflow_queue_effect_already_started".to_owned());
        }
        if self.is_expired(now_unix_ms) {
            return Err("workflow_queue_lease_expired".to_owned());
        }
        Ok(())
    }

    /// 当前 owner 在旧租约过期前更新 heartbeat/expiry，并生成带前序摘要的新版本。
    ///
    /// 【失败】owner/fence/epoch 不匹配、租约非 Active/已过期、heartbeat 时间回退、expiry 不在未来或
    /// 新 TTL 超过 5 分钟时拒绝；sequence 溢出也拒绝。
    /// 【时间语义】expiry 必须仍在观察时刻之后且 TTL 不超过上限，但实现允许新 expiry 早于旧 expiry；
    /// 这里不保证每次调用都延长租约。
    /// 【为什么不原地改字段】新快照携带 `previous_lease_digest` 和递增 sequence，供 Store 进行 CAS/审计。
    /// 【副作用】纯函数式地返回新值；Store 仍需原子提交，失败不应覆盖旧租约。
    pub fn renew(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_active".to_owned());
        }
        if self.is_expired(observed_at_unix_ms) {
            return Err("workflow_queue_lease_expired".to_owned());
        }
        if observed_at_unix_ms < self.heartbeat_at_unix_ms {
            return Err("workflow_queue_heartbeat_rollback".to_owned());
        }
        if expires_at_unix_ms <= observed_at_unix_ms
            || expires_at_unix_ms <= self.heartbeat_at_unix_ms
            || expires_at_unix_ms.saturating_sub(observed_at_unix_ms)
                > WORKFLOW_QUEUE_LEASE_TTL_MAX_MS
        {
            return Err("workflow_queue_lease_expiry_invalid".to_owned());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: self.owner_id.clone(),
            sequence,
            fence_token: self.fence_token,
            authority_epoch: self.authority_epoch,
            issued_at_unix_ms: self.issued_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms,
            effect_state: self.effect_state,
            status: self.status,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    /// 记录 worker 对外部 effect 的一次执行观察，并根据结果更新 Lease status。
    ///
    /// 【调用者】effect adapter 或调度器在开始、完成、失败、无法确认时，通过 Store 转发。
    /// 【状态映射】Running 保持 Active；Succeeded/Failed 变成 Completed；ResultUnknown 变成同名恢复状态。
    /// 【失败】actor 不匹配、Lease 非 Active/已过期、状态边不允许或 sequence 溢出时拒绝。
    /// 【关键边界】这里记录观察，不执行 effect，也不证明外部世界确实按该值完成。
    /// 当前代码没有像 `renew()` 一样比较新 `observed_at` 与上一 heartbeat；只保证不早于 issued 时间，
    /// 因而调用方应提供单调时间，维护者不应假定本方法自身拒绝所有时间回退。
    pub fn record_effect(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        effect_state: WorkflowQueueEffectState,
    ) -> Result<Self, String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_active".to_owned());
        }
        if self.is_expired(observed_at_unix_ms) {
            return Err("workflow_queue_lease_expired".to_owned());
        }
        if !effect_transition_allowed(self.effect_state, effect_state) {
            return Err("workflow_queue_effect_transition_invalid".to_owned());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let status = match effect_state {
            WorkflowQueueEffectState::Succeeded | WorkflowQueueEffectState::Failed => {
                WorkflowQueueLeaseStatus::Completed
            }
            WorkflowQueueEffectState::ResultUnknown => WorkflowQueueLeaseStatus::ResultUnknown,
            WorkflowQueueEffectState::NotStarted | WorkflowQueueEffectState::Running => {
                WorkflowQueueLeaseStatus::Active
            }
        };
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: self.owner_id.clone(),
            sequence,
            fence_token: self.fence_token,
            authority_epoch: self.authority_epoch,
            issued_at_unix_ms: self.issued_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            effect_state,
            status,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    /// 撤销当前 Lease 的可调度资格；进行中或未知 effect 必须转为 recovery-required。
    ///
    /// 【失败】actor 不匹配、已经 Completed/Fenced 或新快照不满足 Lease 合同。
    /// 【状态】NotStarted/已知非运行状态会变为 Fenced；Running/ResultUnknown 会被保守地标成
    /// ResultUnknown，防止把可能已经发生的 effect 当作安全重试。
    /// 【容易误改】该函数保留原 `expires_at`，却把 heartbeat 推进到 `max(观察时刻, 旧 heartbeat)`；若观察时刻
    /// 到达或超过 expiry，新快照会因 heartbeat 不早于 expiry 而校验失败。调用者不能假设过期后 fence 总成功。
    pub fn fence(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if matches!(
            self.status,
            WorkflowQueueLeaseStatus::Completed | WorkflowQueueLeaseStatus::Fenced
        ) {
            return Err("workflow_queue_lease_already_terminal".to_owned());
        }
        let effect_state = if matches!(
            self.effect_state,
            WorkflowQueueEffectState::Running | WorkflowQueueEffectState::ResultUnknown
        ) {
            WorkflowQueueEffectState::ResultUnknown
        } else {
            self.effect_state
        };
        let status = if effect_state == WorkflowQueueEffectState::ResultUnknown {
            WorkflowQueueLeaseStatus::ResultUnknown
        } else {
            WorkflowQueueLeaseStatus::Fenced
        };
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: self.owner_id.clone(),
            sequence,
            fence_token: self.fence_token,
            authority_epoch: self.authority_epoch,
            issued_at_unix_ms: self.issued_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms.max(self.heartbeat_at_unix_ms),
            expires_at_unix_ms: self.expires_at_unix_ms,
            effect_state,
            status,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    /// 只有已过期且 effect 明确 NotStarted 的 Active Lease 才能安全转给新 owner。
    ///
    /// 【输入】新 owner、新 fence token、新 authority epoch、观察时间和新 expiry。
    /// 【成功】递增 sequence、保存旧 digest、新 fence 必须严格变大、authority epoch 不得回退，并重新从
    /// NotStarted/Active 开始；新 owner 不能复用旧 owner。
    /// 【失败】租约尚未到期、状态非 Active、effect 正在执行/结果未知、fence 非单调、epoch 回退、owner 重复，
    /// 或新 TTL/摘要无效时拒绝。
    /// 【并发边界】此处只生成下一值；必须由 Store 用原子比较交换提交，才可阻止两个 reclaimer 同时成功。
    pub fn reclaim(
        &self,
        new_owner_id: impl Into<String>,
        new_fence_token: u64,
        new_authority_epoch: u64,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate()?;
        if !self.is_expired(observed_at_unix_ms) {
            return Err("workflow_queue_lease_not_expired".to_owned());
        }
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_reclaimable".to_owned());
        }
        if self.effect_state != WorkflowQueueEffectState::NotStarted {
            return Err("workflow_queue_reclaim_effect_in_flight".to_owned());
        }
        if new_fence_token <= self.fence_token {
            return Err("workflow_queue_fence_not_monotonic".to_owned());
        }
        if new_authority_epoch < self.authority_epoch {
            return Err("workflow_queue_authority_epoch_rollback".to_owned());
        }
        let new_owner_id = new_owner_id.into();
        bounded(&new_owner_id, "workflow_queue_owner", 256)?;
        if new_owner_id == self.owner_id {
            return Err("workflow_queue_reclaim_owner_reused".to_owned());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: new_owner_id,
            sequence,
            fence_token: new_fence_token,
            authority_epoch: new_authority_epoch,
            issued_at_unix_ms: observed_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms,
            effect_state: WorkflowQueueEffectState::NotStarted,
            status: WorkflowQueueLeaseStatus::Active,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    /// 校验租约自洽并要求 owner/fence/authority epoch 与调用者当前快照完全一致。
    /// 小于记录 epoch 返回 rollback，大于记录 epoch 返回 stale；两个方向都拒绝继续旧租约操作。
    pub fn validate_actor(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.owner_id != owner_id {
            return Err("workflow_queue_owner_mismatch".to_owned());
        }
        if self.fence_token != fence_token {
            return Err("workflow_queue_fence_mismatch".to_owned());
        }
        if authority_epoch != self.authority_epoch {
            return Err(if authority_epoch < self.authority_epoch {
                "workflow_queue_authority_epoch_rollback"
            } else {
                "workflow_queue_authority_epoch_stale"
            }
            .to_owned());
        }
        Ok(())
    }

    /// 检查 Lease 版本、非零计数器、时间/TTL、前序链形状、状态组合与自身摘要。
    ///
    /// 【数值边界】item/owner 最长 256 字节；expiry 必须晚于 heartbeat，且两者最多相差 300,000 毫秒。
    /// sequence=1 必须没有 previous digest；sequence>1 必须带一个格式有效的前序 digest。
    /// 【失败】时间、状态和摘要不一致时返回稳定错误，Store 应拒绝迁移。
    /// 【边界】这里不会读取前一条 lease 来证明 digest 链连续，也不会认证时钟、owner 身份或 digest 来源。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_QUEUE_LEASE_SCHEMA
            || self.sequence == 0
            || self.fence_token == 0
            || self.authority_epoch == 0
            || self.issued_at_unix_ms == 0
            || self.heartbeat_at_unix_ms < self.issued_at_unix_ms
            || self.expires_at_unix_ms <= self.heartbeat_at_unix_ms
            || self
                .expires_at_unix_ms
                .saturating_sub(self.heartbeat_at_unix_ms)
                > WORKFLOW_QUEUE_LEASE_TTL_MAX_MS
        {
            return Err("workflow_queue_lease_header_invalid".to_owned());
        }
        bounded(&self.item_id, "workflow_queue_item_id", 256)?;
        bounded(&self.owner_id, "workflow_queue_owner", 256)?;
        digest(&self.claim_digest, "workflow_queue_claim_digest")?;
        if let Some(previous) = &self.previous_lease_digest {
            digest(previous, "workflow_queue_previous_lease_digest")?;
            if self.sequence == 1 {
                return Err("workflow_queue_genesis_parent_unexpected".to_owned());
            }
        } else if self.sequence != 1 {
            return Err("workflow_queue_lease_parent_required".to_owned());
        }
        if self.effect_state == WorkflowQueueEffectState::ResultUnknown
            && self.status != WorkflowQueueLeaseStatus::ResultUnknown
        {
            return Err("workflow_queue_unknown_effect_not_fenced".to_owned());
        }
        if self.status == WorkflowQueueLeaseStatus::ResultUnknown
            && self.effect_state != WorkflowQueueEffectState::ResultUnknown
        {
            return Err("workflow_queue_unknown_status_without_effect".to_owned());
        }
        if self.status == WorkflowQueueLeaseStatus::Completed
            && !matches!(
                self.effect_state,
                WorkflowQueueEffectState::Succeeded | WorkflowQueueEffectState::Failed
            )
        {
            return Err("workflow_queue_completed_effect_invalid".to_owned());
        }
        if self.status == WorkflowQueueLeaseStatus::Fenced
            && matches!(
                self.effect_state,
                WorkflowQueueEffectState::Running | WorkflowQueueEffectState::ResultUnknown
            )
        {
            return Err("workflow_queue_fenced_effect_unknown".to_owned());
        }
        if !valid_digest(&self.lease_digest) || self.lease_digest != self.digest() {
            return Err("workflow_queue_lease_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对除 `lease_digest` 本身以外的全部状态字段生成内容摘要，包含 `previous_lease_digest`。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "item_id": self.item_id,
            "claim_digest": self.claim_digest,
            "owner_id": self.owner_id,
            "sequence": self.sequence,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "heartbeat_at_unix_ms": self.heartbeat_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "effect_state": self.effect_state,
            "status": self.status,
            "previous_lease_digest": self.previous_lease_digest,
        }))
    }
}

/// 首次认领请求；Store 必须原子检查 Claim 仍有效且 item 尚无有效租约。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueClaimRequest {
    /// Core/readiness 投影生成的候选 Claim。
    pub claim: WorkflowQueueClaimContract,
    /// 这次租约的 worker 身份标签。
    pub owner_id: String,
    /// 由调度协调者提供的非零 fencing generation。
    pub fence_token: u64,
    /// 当前授权配置代次；旧代次不能继续操作新状态。
    pub authority_epoch: u64,
    /// 服务端观察时间，Unix 毫秒。
    pub observed_at_unix_ms: u64,
    /// 请求的 TTL；Store 会收紧到合同上限和 Claim deadline 之内。
    pub lease_ttl_ms: u64,
}

/// 当前 owner 更新 heartbeat/expiry 的请求；不改变 owner/fence/authority，只推进版本和时间观察值。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueHeartbeatRequest {
    /// 要续租的队列项。
    pub item_id: String,
    /// 当前 owner，必须与租约记录一致。
    pub owner_id: String,
    /// 当前 fencing generation，必须精确匹配。
    pub fence_token: u64,
    /// 当前授权代次，必须精确匹配。
    pub authority_epoch: u64,
    /// 本次心跳观察时刻，Unix 毫秒。
    pub observed_at_unix_ms: u64,
    /// 新 expiry 相对观察时刻的 TTL，Store/Domain 会限制最大值；合同不强制它晚于旧 expiry。
    pub lease_ttl_ms: u64,
}

/// 记录 worker 对 effect 状态的观察；它不是执行命令本身。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueEffectRequest {
    /// 正在处理的队列项。
    pub item_id: String,
    /// 当前 owner。
    pub owner_id: String,
    /// 当前 fencing generation。
    pub fence_token: u64,
    /// 当前授权代次。
    pub authority_epoch: u64,
    /// effect 状态发生或被观察到的 Unix 毫秒时间。
    pub observed_at_unix_ms: u64,
    /// 新观察到的状态；Store 仍会检查允许的状态迁移。
    pub effect_state: WorkflowQueueEffectState,
}

/// 收尾时撤销 Lease 可调度资格的请求；进行中的 effect 会保守转入 Unknown。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueFenceRequest {
    /// 要 fence 的队列项。
    pub item_id: String,
    /// 租约当前 owner。
    pub owner_id: String,
    /// 租约当前 fencing generation。
    pub fence_token: u64,
    /// 租约当前授权代次。
    pub authority_epoch: u64,
    /// fence 操作的 Unix 毫秒观察时间。
    pub observed_at_unix_ms: u64,
}

/// 过期且未开始 effect 的 Claim 转交给新 owner 的请求。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueReclaimRequest {
    /// 要重新认领的队列项。
    pub item_id: String,
    /// 与旧 owner 不同的新 worker 标签。
    pub new_owner_id: String,
    /// 必须严格大于旧 Lease 的 fence token。
    pub new_fence_token: u64,
    /// 新 Lease 使用的授权代次，不得低于旧 Lease。
    pub authority_epoch: u64,
    /// reclaim 的 Unix 毫秒观察时间，必须已到旧 expiry。
    pub observed_at_unix_ms: u64,
    /// 新租约时长，Store 会应用上限。
    pub lease_ttl_ms: u64,
}

/// 限定 effect 状态机允许的边；只接受未开始/运行中的观察进入合法后继状态。
///
/// 允许 `NotStarted -> NotStarted/Running/Succeeded/Failed/ResultUnknown`，以及
/// `Running -> Running/Succeeded/Failed/ResultUnknown`；Completed/Unknown 后继不在此处放行。
fn effect_transition_allowed(
    current: WorkflowQueueEffectState,
    next: WorkflowQueueEffectState,
) -> bool {
    matches!(
        (current, next),
        (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::Running
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::Succeeded
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::Failed
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::ResultUnknown
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::Succeeded
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::Failed
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::ResultUnknown
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::Running
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::NotStarted
        )
    )
}

/// 验证 owner/item 等文本的非空、字节长度与控制字符边界。
fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 给带字段名的错误信息包装 digest 形状校验。
fn digest(value: &str, field: &str) -> Result<(), String> {
    if !valid_digest(value) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 只检查 `sha256:` 加 64 位十六进制格式，不重算摘要内容。
fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
