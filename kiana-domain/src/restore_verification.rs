//! PD-23 恢复校验事实（fact）的领域数据合同。
//!
//! 本文件定义恢复阶段用来汇总“新实例是否与备份一致、旧写入者是否被写入围栏（fence）、
//! 外部副作用是否已核对”的结构和局部规则。典型数据关系如下：
//!
//! ```text
//! SnapshotManifest + 新实例/投影（projection）/围栏/副作用观察值
//!     ↓
//! 调用方构造 RestoreVerificationFact 并计算 verification_digest（验证摘要）
//!     ↓
//! RestoreVerificationFact::validate() 检查字段、自摘要和状态条件
//!     ↓
//! Daemon verify_restore() 再将关键身份与 SnapshotManifest 逐项绑定
//!     ↓
//! 后续恢复激活/就绪门（本文件本身不执行）
//! ```
//!
//! 当前 workspace 没有生产代码构造或消费该 fact；可见引用来自 Domain fixture 和 Daemon
//! 的公开校验函数。这里描述的是值合同，不代表 projection 已重建、ACL 已读取、外部 effect
//! 已从权威系统核实，或实例已经激活。
//!
//! ⚠ 所有“已围栏”“已对账”“默认暂停”等字段都是调用方提供的观察值。摘要（digest）只能检测载荷
//! 与自身摘要是否一致，不认证是谁提供了这些值，也不代替真实租约、存储或 effect adapter。
//! 未知外部效果不能因为有摘要就被当成成功。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RESTORE_VERIFICATION_SCHEMA: &str = "kiana.restore-verification.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 恢复校验汇总结果的三种状态。
///
/// 状态不会自行推进：调用方根据观察事实填写后，由 [`RestoreVerificationFact::validate`]
/// 检查该状态与计数、证据引用和 reason 是否相容。
pub enum RestoreVerificationState {
    /// 恢复条件明确不满足；必须给出非 `ok` 的原因。
    Blocked,
    /// 仍有审批、租约、触发器或未知 effect 等待人工/外部核对，必须附 reconciliation 引用。
    NeedsReconciliation,
    /// 所有验证条件都已由调用方报告满足；仍然只是 fact 状态，不等于激活或物理证明。
    Verified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次恢复验证的完整输入/输出事实。
///
/// 它将快照身份与恢复后的投影、epoch、资源计数和围栏/对账声明放在一起，并用
/// `verification_digest` 发现字段被意外改动。Daemon `verify_restore()` 会把 manifest
/// digest、owner、store、旧实例、源游标和旧 data epoch 绑定回原快照；本结构本身不查
/// EventStore、授权存储、触发器目录或外部副作用系统。
pub struct RestoreVerificationFact {
    /// 固定 wire schema 名称；读取 API 据此区分合同版本。
    pub schema: String,
    /// 本 fact 声称验证的是哪份 snapshot manifest，由摘要值关联而不是内嵌整份 manifest。
    pub snapshot_manifest_digest: String,
    /// 恢复所属的项目/owner 范围，应与快照清单相同。
    pub owner_scope: String,
    /// 恢复使用的存储域身份，应与快照清单相同。
    pub store_id: String,
    /// 被替换的旧实例 ID；None 表示没有旧实例可围栏，但 Verified 状态不允许缺失。
    pub previous_instance_id: Option<String>,
    /// 恢复后新实例的 ID；必须是有效文本，且不能与 previous_instance_id 相同。
    pub restored_instance_id: String,
    /// 快照的源游标；恢复校验要求大于 0，并应与 manifest 的 source_cursor 一致。
    pub source_cursor: u64,
    /// 新实例投影重建到的位置；不能超过 source_cursor，Verified 时必须追平它。
    pub projection_cursor: u64,
    /// 新投影的摘要格式值；此处不读取投影内容来重算它。
    pub projection_digest: String,
    /// 恢复后使用的数据 epoch；必须大于 0，且有 previous_data_epoch 时必须严格增加。
    pub data_epoch: u64,
    /// 被替换实例的数据 epoch；Daemon 以它和 manifest.data_epoch 做绑定。
    pub previous_data_epoch: Option<u64>,
    /// 恢复后使用的授权 epoch；必须大于 0，且有 previous_authority_epoch 时必须严格增加。
    pub authority_epoch: u64,
    /// 被替换实例的授权 epoch；只做本 fact 的单调性检查，不由 Daemon manifest verifier 绑定。
    pub previous_authority_epoch: Option<u64>,
    /// 调用方报告已核对的 artifact 引用；必须非空、唯一，最多 256 项。
    pub artifact_refs: Vec<String>,
    /// 恢复时仍待处理的审批数量；Verified 状态要求为 0。
    pub pending_approval_count: u32,
    /// 恢复时仍有效的旧租约数量；Verified 状态要求为 0。
    pub active_lease_count: u32,
    /// 恢复时发现的 trigger 数量；Verified 状态要求为 0。
    pub trigger_count: u32,
    /// 尚未确定结果的外部副作用数量；Verified 状态要求为 0。
    pub unknown_effect_count: u32,
    /// 调用方声明旧实例写入权已围栏；有 previous_instance_id 时 validator 要求为 true。
    pub old_instance_fenced: bool,
    /// 调用方声明旧授权写入权已围栏；有 previous_instance_id 时 validator 要求为 true。
    pub old_authority_fenced: bool,
    /// 调用方声明相关外部 effect 已完成核对；若为 true，unknown_effect_count 必须为 0。
    pub external_effects_reconciled: bool,
    /// 调用方声明新实例默认保持暂停；所有可接受状态都要求为 true。
    pub default_paused: bool,
    /// 待对账/已对账过程的证据引用；最多 64 项且不能重复。
    pub reconciliation_refs: Vec<String>,
    /// 事实当前声明的恢复状态；必须与计数、围栏和证据字段相容。
    pub state: RestoreVerificationState,
    /// 人类可读的状态原因；Verified 必须恰为 `ok`，其他约束按状态校验。
    pub reason: String,
    /// 对本 fact 其他字段计算的 `sha256:` JSON 摘要；不是签名或来源认证。
    pub verification_digest: String,
}

impl RestoreVerificationFact {
    /// 检查 fact 的结构、自摘要、围栏声明和状态条件是否互相一致。
    ///
    /// 【调用者】Daemon 的 `verify_restore()` 在绑定 manifest 之前调用；测试也会直接调用。
    /// 【输入/输出】只读取 `self`，成功返回 `Ok(())`，失败返回稳定错误码字符串。
    /// 【副作用】无 I/O、无事件写入、无租约操作、无 trigger/effect 执行或激活。
    ///
    /// 【核心流程】先检查 schema、字段格式/数量、epoch 单调性、默认暂停和摘要；再检查
    /// 旧实例围栏与 unknown-effect 计数关系；最后按 Blocked、NeedsReconciliation、Verified
    /// 分别检查业务状态约束。先发生的错误会决定最终错误码。
    ///
    /// 【设计原因】恢复需要同时保留“格式是否合法”和“状态是否可接受”的边界；一个摘要
    /// 正确的 DTO 仍可能处于 Blocked 或 NeedsReconciliation，不能只因自摘要通过就启动实例。
    pub fn validate(&self) -> Result<(), &'static str> {
        // 第一阶段拒绝错误 schema、不可绑定的标识、越界数据、重复引用和格式错误/不匹配的摘要。
        if self.schema != RESTORE_VERIFICATION_SCHEMA
            || !valid_digest(&self.snapshot_manifest_digest)
            || !valid_text(&self.owner_scope)
            || !valid_text(&self.store_id)
            || self
                .previous_instance_id
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_text(&self.restored_instance_id)
            || self
                .previous_instance_id
                .as_deref()
                .is_some_and(|value| value == self.restored_instance_id)
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
            || !valid_digest(&self.projection_digest)
            || self.data_epoch == 0
            || self
                .previous_data_epoch
                .is_some_and(|value| value >= self.data_epoch)
            || self.authority_epoch == 0
            || self
                .previous_authority_epoch
                .is_some_and(|value| value >= self.authority_epoch)
            || self.artifact_refs.is_empty()
            || self.artifact_refs.len() > 256
            || self.artifact_refs.iter().any(|value| !valid_text(value))
            || has_duplicates(&self.artifact_refs)
            || !self.default_paused
            || self
                .reconciliation_refs
                .iter()
                .any(|value| !valid_text(value))
            || self.reconciliation_refs.len() > 64
            || has_duplicates(&self.reconciliation_refs)
            || !valid_text(&self.reason)
            || !valid_digest(&self.verification_digest)
            || self.verification_digest != self.digest()
        {
            return Err("restore_verification_fact_invalid");
        }
        // 只有声明存在旧实例时，围栏字段才必须为真；字段仍来自调用方，不是租约系统回执。
        if self.previous_instance_id.is_some()
            && (!self.old_instance_fenced || !self.old_authority_fenced)
        {
            return Err("restore_old_instance_not_fenced");
        }
        // 不能一边声称所有 effect 已对账，一边还保留结果未知的 effect。
        if self.external_effects_reconciled && self.unknown_effect_count != 0 {
            return Err("restore_unknown_effect_reconciled_mismatch");
        }
        // 同一份字段结构能表达三类状态；下面分别拒绝自相矛盾的状态组合。
        match self.state {
            RestoreVerificationState::Blocked => {
                if self.reason == "ok" {
                    return Err("restore_blocked_reason_invalid");
                }
            }
            RestoreVerificationState::NeedsReconciliation => {
                if self.pending_approval_count == 0
                    && self.active_lease_count == 0
                    && self.trigger_count == 0
                    && self.unknown_effect_count == 0
                {
                    return Err("restore_reconciliation_not_needed");
                }
                if self.reconciliation_refs.is_empty() || self.external_effects_reconciled {
                    return Err("restore_reconciliation_evidence_invalid");
                }
            }
            RestoreVerificationState::Verified => {
                if self.previous_instance_id.is_none()
                    || self.previous_data_epoch.is_none()
                    || self.previous_authority_epoch.is_none()
                    || self.projection_cursor != self.source_cursor
                    || self.pending_approval_count != 0
                    || self.active_lease_count != 0
                    || self.trigger_count != 0
                    || self.unknown_effect_count != 0
                    || !self.old_instance_fenced
                    || !self.old_authority_fenced
                    || !self.external_effects_reconciled
                    || !self.reconciliation_refs.is_empty()
                    || self.reason != "ok"
                {
                    return Err("restore_verification_not_ready");
                }
            }
        }
        Ok(())
    }

    /// 计算 fact 自身字段的确定性摘要；不包含 `verification_digest` 字段本身。
    ///
    /// 这个 SHA-256 值用于发现序列化载荷被改动以及供 Daemon 绑定快照，不带密钥，因此
    /// 不能证明 fact 由受信任的恢复进程、EventStore 或租约服务生成。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "snapshot_manifest_digest": self.snapshot_manifest_digest,
            "owner_scope": self.owner_scope,
            "store_id": self.store_id,
            "previous_instance_id": self.previous_instance_id,
            "restored_instance_id": self.restored_instance_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_digest": self.projection_digest,
            "data_epoch": self.data_epoch,
            "previous_data_epoch": self.previous_data_epoch,
            "authority_epoch": self.authority_epoch,
            "previous_authority_epoch": self.previous_authority_epoch,
            "artifact_refs": self.artifact_refs,
            "pending_approval_count": self.pending_approval_count,
            "active_lease_count": self.active_lease_count,
            "trigger_count": self.trigger_count,
            "unknown_effect_count": self.unknown_effect_count,
            "old_instance_fenced": self.old_instance_fenced,
            "old_authority_fenced": self.old_authority_fenced,
            "external_effects_reconciled": self.external_effects_reconciled,
            "default_paused": self.default_paused,
            "reconciliation_refs": self.reconciliation_refs,
            "state": self.state,
            "reason": self.reason,
        }))
    }
}

/// 对外提供与 `fact.validate()` 相同的结构化检查入口。
///
/// 调用者通常可直接使用方法；这个自由函数让模块/API 调用点也能通过稳定名称复用同一套
/// Domain 规则。它不会额外查存储或绑定 snapshot manifest。
pub fn validate_restore_verification(fact: &RestoreVerificationFact) -> Result<(), &'static str> {
    fact.validate()
}

/// 检查需进入摘要的普通文本字段是否非空、长度不超过 512 字节且不含 NUL/换行符。
///
/// 长度使用 Rust `str::len()` 的 UTF-8 字节数，不是字符数。该限制用于避免控制字符和
/// 超长标识进入日志/摘要；它不验证文本是否对应真实项目、实例或 artifact。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

/// 检查摘要是否为固定 `sha256:` 前缀和 64 位十六进制文本。
///
/// 十六进制大小写都接受；这里只检验形状，真实摘要是否匹配由调用者与 `digest()` 比较。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// 用有序集合检查字符串切片中是否有完全相同的重复值。
///
/// 这里只比较字符串本身，不做路径、ID 或大小写归一化。
fn has_duplicates(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}
