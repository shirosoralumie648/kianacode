//! ControlPlane 侧的 workflow queue 就绪视图与交接前置校验。
//!
//! 上游把当前 WorkPacket 快照和服务端观察时间交给 `project_workflow_queue_ready()`；它复用 Domain 的
//! `ready_packets()`，生成确定顺序的 Ready Claim、blocked 原因和过期 claim 投影。AUT-08 QueueStore 单独负责
//! 原子建立/更新 lease；当调度器把 packet 交回既有 ControlPlane 命令路径时，transaction gate 再核对 ready
//! view、packet/parent、claim、owner、fence token、authority epoch、deadline 和 lease 时间：
//!
//! ```text
//! WorkPacket snapshot -> readiness -> ReadyView/Claim -> QueueStore lease
//!     -> validate_workflow_queue_transaction() -> existing ControlPlane route
//! ```
//!
//! 这里的函数都是纯 gate/projector：不写 EventLog、不占并发槽、不运行 Runner，也不直接产生 effect。ready
//! view 的 digest 是无密钥自摘要，调用者仍须保证输入来自可信快照；当前两个 projector/gate 只有源码定义和
//! fixture 调用，尚未发现生产命令路径实际执行该交接。Serde derive 负责该 view 与 JSON/wire 值之间的转换，
//! 未知字段会拒绝。

use kiana_domain::{
    json_digest, ready_packets, WorkPacket, WorkflowQueueClaimContract, WorkflowQueueClaimStatus,
    WorkflowQueueEffectState, WorkflowQueueLease, WorkflowQueueLeaseStatus,
    WORKFLOW_QUEUE_LEASE_TTL_MAX_MS,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// 可供 scheduler 读取的确定性队列快照；本结构不持有锁，也不预留任何 Claim。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueReadyView {
    /// 当前 readiness 谓词认定可运行的 packet Claim。
    pub claims: Vec<WorkflowQueueClaimContract>,
    /// 暂不可运行的 item ID 到阻塞原因映射。
    pub blocked: BTreeMap<String, String>,
    /// 已超过 packet claim 时间的 item ID；仍是投影，不表示本结构回收了租约。
    pub expired_claims: Vec<String>,
    /// 对 claims/blocked/expired 的无密钥摘要，用于发现字段漂移。
    pub ready_digest: String,
}

impl WorkflowQueueReadyView {
    /// 校验每条 claim 有效且标为 Ready、item 不重复，并重新计算 view 自摘要。
    /// 不验证底层 packet、当前时间或 view 是否从权威 Store 读取。
    pub fn validate(&self) -> Result<(), String> {
        let mut item_ids = BTreeSet::new();
        for claim in &self.claims {
            claim.validate()?;
            if !claim.is_ready_packet() || !item_ids.insert(claim.item_id.clone()) {
                return Err("workflow_queue_ready_duplicate_item".to_owned());
            }
        }
        if self.ready_digest != self.digest() {
            return Err("workflow_queue_ready_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 摘要不包含 `ready_digest` 自身，覆盖 ready claim、blocked reason 和 expired ID。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "claims": self.claims,
            "blocked": self.blocked,
            "expired_claims": self.expired_claims,
        }))
    }
}

/// 从 packet board 共用的 WorkPacket readiness 谓词生成一份只读 queue view。
///
/// 【预期用途】ControlPlane/UI/scheduler 读取当前可运行候选项；当前仓库只有该函数定义和测试调用，没有生产调用点。
/// 此函数不等同于 AUT-08 的实际 claim 操作。
/// 【输入】以 packet ID 索引的当前 WorkPacket 快照、非零 Unix 毫秒观察时刻、每个 Claim 的并发上限。
/// 【输出】含稳定排序 Ready Claim、blocked reason、expired ID 和自摘要的 view。
/// 【失败】时间/上限为零、依赖图无效、ready ID 找不到 packet、父子/scope/budget 不一致、expiry 加法溢出时拒绝。
/// 【核心流程】复用 `ready_packets()` 的 status/dependency/deadline/claim 规则；为每个 ready ID 找 packet 与
/// parent，重算各摘要，构造 Ready Claim，按 item ID 排序后校验 view。
/// 【为什么这样设计】看板与队列必须共享 readiness 规则，否则 UI 会显示可运行但 queue 拒绝的不同结果。
/// 【边界】当前每条 Claim 的 active count 固定为 0，故 max_parallel_claims 只是记录到合同，不在此处统计或
/// 原子限制实际并发；并发唯一性由 Store 决定。该投影也不认证输入快照来源。
pub fn project_workflow_queue_ready(
    packets: &BTreeMap<String, WorkPacket>,
    now_unix_ms: u64,
    max_parallel_claims: u32,
) -> Result<WorkflowQueueReadyView, String> {
    if now_unix_ms == 0 || max_parallel_claims == 0 {
        return Err("workflow_queue_ready_input_invalid".to_owned());
    }
    let readiness = ready_packets(packets, now_unix_ms).map_err(|error| error.to_string())?;
    let mut claims = Vec::new();
    for item_id in &readiness.ready {
        let packet = packets
            .get(item_id)
            .ok_or_else(|| "workflow_queue_ready_packet_missing".to_owned())?;
        let parent = packet
            .parent_packet_id
            .as_ref()
            .and_then(|parent_id| packets.get(parent_id))
            .unwrap_or(packet);
        let claim = WorkflowQueueClaimContract::from_work_packet(
            packet,
            parent,
            true,
            false,
            0,
            max_parallel_claims,
            false,
            None,
            now_unix_ms
                .checked_add(WORKFLOW_QUEUE_LEASE_TTL_MAX_MS)
                .ok_or_else(|| "workflow_queue_ready_expiry_overflow".to_owned())?,
            now_unix_ms,
            WorkflowQueueClaimStatus::Ready,
        )?;
        claims.push(claim);
    }
    claims.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    let mut view = WorkflowQueueReadyView {
        claims,
        blocked: readiness.blocked,
        expired_claims: readiness.expired_claims,
        ready_digest: String::new(),
    };
    view.ready_digest = view.digest();
    view.validate()?;
    Ok(view)
}

/// 由 transaction gate 在交接给既有 ControlPlane 命令路径前核对 lease 的 owner/fence/epoch/status/effect/expiry。
/// 调用底层 Domain `is_dispatchable()`；不读 EventStore，也不执行 effect。当前没有发现独立的生产调用点。
pub fn validate_workflow_queue_dispatch(
    lease: &WorkflowQueueLease,
    owner_id: &str,
    fence_token: u64,
    authority_epoch: u64,
    now_unix_ms: u64,
) -> Result<(), String> {
    lease.is_dispatchable(owner_id, fence_token, authority_epoch, now_unix_ms)
}

/// 校验从 ReadyView/WorkPacket 到 Lease 的完整 scheduler handoff tuple。
///
/// 【调用者】现有 ControlPlane 命令路由收到 scheduler 递交的 packet 前；目前仓库只找到测试直接调用。
/// 【输入】ready view、claim、packet/parent 快照、lease、owner/fence/authority 和当前 Unix 毫秒。
/// 【输出】全部绑定一致且仍可 dispatch 时返回 `Ok(())`；否则返回具体拒绝原因，由上层停止交接。
/// 【核心顺序】先校验 ready/claim 和重算 packet binding，再要求 claim 在 ready snapshot 中逐字段相同，确认
/// lease 对应 item/claim，限制租约不能越过 packet deadline，最后检查 owner/fence/epoch/expiry 和 effect 状态。
/// 【为什么要逐层核对】排队后 packet、权限代次、所有者或 fencing token 可能变化；只检查“曾经 ready”会让旧
/// worker 或过期快照绕过最新状态。
/// 【边界】此 gate 本身不是存储事务/原子 CAS；调用者要在适当的存储边界保护检查与后续交接，且需保证输入快照可信。
#[allow(clippy::too_many_arguments)]
pub fn validate_workflow_queue_transaction(
    ready: &WorkflowQueueReadyView,
    claim: &WorkflowQueueClaimContract,
    packet: &WorkPacket,
    parent: &WorkPacket,
    lease: &WorkflowQueueLease,
    owner_id: &str,
    fence_token: u64,
    authority_epoch: u64,
    now_unix_ms: u64,
) -> Result<(), String> {
    ready.validate()?;
    claim.validate()?;
    claim.validate_packet_binding(packet, parent)?;
    if !claim.is_ready_packet() {
        return Err("workflow_queue_claim_not_ready".to_owned());
    }
    let listed = ready
        .claims
        .iter()
        .find(|candidate| candidate.item_id == claim.item_id)
        .ok_or_else(|| "workflow_queue_ready_snapshot_mismatch".to_owned())?;
    if listed != claim {
        return Err("workflow_queue_ready_snapshot_mismatch".to_owned());
    }
    if lease.item_id != claim.item_id || lease.claim_digest != claim.claim_digest {
        return Err("workflow_queue_lease_claim_mismatch".to_owned());
    }
    if packet
        .deadline_unix_ms
        .is_some_and(|deadline| lease.expires_at_unix_ms > deadline)
    {
        return Err("workflow_queue_deadline_exceeded".to_owned());
    }
    validate_workflow_queue_dispatch(lease, owner_id, fence_token, authority_epoch, now_unix_ms)
}

/// 判断 lease 或 effect 是否处于 Unknown 恢复状态。
/// status/effect 任一为 ResultUnknown 都返回 true；Unknown 不能按普通过期项重新排队。
pub fn workflow_queue_requires_recovery(lease: &WorkflowQueueLease) -> bool {
    lease.status == WorkflowQueueLeaseStatus::ResultUnknown
        || lease.effect_state == WorkflowQueueEffectState::ResultUnknown
}
