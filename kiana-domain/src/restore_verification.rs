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
//! 当前 workspace 的生产消费入口是 `kiana-daemon::DaemonHost::restore_gate` /
//! `restore_gate_after_restart` / `admit_restored_command`：它们组合本文件的值合同、Daemon
//! 的 manifest/artifact 绑定、Core 的 effect claim 交叉核对与 DEP-23 激活就绪门。这里仍是
//! 值合同，不代表 projection 已重建、ACL 已读取、外部 effect 已从权威系统核实，或实例
//! 已经激活；读取备份根、写文件、取锁、围栏进程和激活 root 都不在本模块。
//!
//! ⚠ 所有“已围栏”“已对账”“默认暂停”等字段都是调用方提供的观察值。摘要（digest）只能检测载荷
//! 与自身摘要是否一致，不认证是谁提供了这些值，也不代替真实租约、存储或 effect adapter。
//! 未知外部效果不能因为有摘要就被当成成功。

use crate::{json_digest, EffectObservationState, SnapshotFileSeal, SnapshotManifest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

pub const RESTORE_EFFECT_CLAIM_SCHEMA: &str = "kiana.restore-effect-claim.v1";
/// 单条恢复 fact 可携带的对账证据上限。
pub const MAX_RESTORE_EFFECT_CLAIMS: usize = 64;
/// 恢复事实中 artifact 引用的固定前缀。
pub const RESTORE_ARTIFACT_REF_PREFIX: &str = "artifact:";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 恢复 fact 采纳的外部 effect 处置，只允许既有 reconciliation receipt 已经决定过的四种。
///
/// 该枚举刻意不复用 Core 的 receipt 类型：Domain 不能依赖 Core，但字段语义与
/// `kiana-core::EffectResolution` 一一对应；Core 的 `restore_effect_claim_from_receipt`
/// 是唯一合法的派生入口。
pub enum RestoreEffectResolution {
    /// 外部系统确认了结果。
    Reconciled,
    /// 已证明没有发生，因此可以重发。
    RetryWithoutEffect,
    /// 永远不会发生。
    Abandoned,
    /// 可能发生过，已执行补偿。
    Compensated,
}

impl RestoreEffectResolution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reconciled => "reconciled",
            Self::RetryWithoutEffect => "retry_without_effect",
            Self::Abandoned => "abandoned",
            Self::Compensated => "compensated",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一条来自外部 effect reconciliation receipt 的恢复证据。
///
/// 这里只保存摘要与身份，不保存 provider payload。`observed_state` 必须与真实
/// `EffectObservation` 的状态一致；`claim_digest` 只能证明字段自洽，不能证明 receipt
/// 存在——后者由 Core 的 `verify_restore_effect_claims` 对传入 receipt 重新派生后比对。
pub struct RestoreExternalEffectClaim {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// 该处置所依据的 `EffectObservation.observation_digest`。
    pub observation_digest: String,
    /// 观察到的真实状态；`Unknown` 永远不能出现在对账证据里。
    pub observed_state: EffectObservationState,
    /// receipt 已决定的处置。
    pub resolution: RestoreEffectResolution,
    /// 派生该 claim 的 `EffectReconciliationReceipt.receipt_digest`。
    pub receipt_digest: String,
    /// 授权该次对账的审批引用。
    pub approval_ref: String,
    /// 对账发生时的 authority epoch。
    pub authority_epoch: u64,
    /// 对本结构其他字段的 `sha256:` JSON 摘要。
    pub claim_digest: String,
}

impl RestoreExternalEffectClaim {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        observation_digest: impl Into<String>,
        observed_state: EffectObservationState,
        resolution: RestoreEffectResolution,
        receipt_digest: impl Into<String>,
        approval_ref: impl Into<String>,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut claim = Self {
            schema: RESTORE_EFFECT_CLAIM_SCHEMA.to_owned(),
            observation_digest: observation_digest.into(),
            observed_state,
            resolution,
            receipt_digest: receipt_digest.into(),
            approval_ref: approval_ref.into(),
            authority_epoch,
            claim_digest: String::new(),
        };
        claim.claim_digest = claim.digest();
        claim.validate().map_err(str::to_owned)?;
        Ok(claim)
    }

    /// 重新计算自身摘要；仅供已经有意修改字段的夹具使用。
    pub fn recompute_digest(&mut self) {
        self.claim_digest = self.digest();
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "observation_digest": self.observation_digest,
            "observed_state": self.observed_state,
            "resolution": self.resolution,
            "receipt_digest": self.receipt_digest,
            "approval_ref": self.approval_ref,
            "authority_epoch": self.authority_epoch,
        }))
    }

    /// 检查字段格式以及处置与观察状态是否相容。
    ///
    /// 【核心拒绝】`Unknown` 不是任何一种对账结论：无论调用方给出哪种 resolution，
    /// 都返回 `restore_effect_claim_unknown`。这是「Unknown 不能被声明为已对账」的第一道门。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != RESTORE_EFFECT_CLAIM_SCHEMA
            || !valid_digest(&self.observation_digest)
            || !valid_digest(&self.receipt_digest)
            || !valid_text(&self.approval_ref)
            || self.authority_epoch == 0
            || !valid_digest(&self.claim_digest)
            || self.claim_digest != self.digest()
        {
            return Err("restore_effect_claim_invalid");
        }
        if self.observed_state == EffectObservationState::Unknown {
            return Err("restore_effect_claim_unknown");
        }
        match self.resolution {
            RestoreEffectResolution::Reconciled => {}
            RestoreEffectResolution::RetryWithoutEffect | RestoreEffectResolution::Abandoned => {
                if self.observed_state != EffectObservationState::NoEffect {
                    return Err("restore_effect_claim_resolution_state_mismatch");
                }
            }
            RestoreEffectResolution::Compensated => {
                if self.observed_state == EffectObservationState::NoEffect {
                    return Err("restore_effect_claim_resolution_state_mismatch");
                }
            }
        }
        Ok(())
    }
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
    /// 调用方报告的 artifact 引用；必须非空、唯一、最多 256 项，且每条必须是
    /// `artifact:<relative_path>#<sha256>`。Daemon `verify_restore()` 会把它逐条绑定到
    /// `SnapshotManifest.files` 的 `content_hash`：缺失、未知或哈希不符都被拒绝。
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
    /// 调用方提交的外部副作用对账证据；每条都必须由既有 effect reconciliation receipt 派生。
    /// `Unknown` 观察值不能出现在这里，且 `unknown_effect_count` 必须为 0 才可能 Verified。
    pub external_effect_claims: Vec<RestoreExternalEffectClaim>,
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
        // 对账证据本身必须合法、有界，且每条对应不同的 observation。Unknown 在
        // `claim.validate()` 里已被拒绝：摘要正确不能把未知 effect 变成已对账。
        if self.external_effect_claims.len() > MAX_RESTORE_EFFECT_CLAIMS {
            return Err("restore_effect_claims_unbounded");
        }
        let mut claimed_observations = BTreeSet::new();
        for claim in &self.external_effect_claims {
            claim.validate()?;
            if !claimed_observations.insert(claim.observation_digest.as_str()) {
                return Err("restore_effect_claim_duplicate");
            }
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
                if self.reconciliation_refs.is_empty() || !self.external_effect_claims.is_empty() {
                    return Err("restore_reconciliation_evidence_invalid");
                }
            }
            RestoreVerificationState::Verified => {
                // 注意：这里只检查 fact 的自洽条件。projector/ACL 读模型本身不由本模块读取或
                // 重建；`projection_cursor == source_cursor` 与 `projection_digest` 是调用方
                // 观察值。真正的读模型比对属于 PD-29 的 storage health 与后续 projector 适配器，
                // 本切片的 limitation 在 docs/roadmap/pd23-restore-baseline.md 中显式记录。
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
                    || !self.reconciliation_refs.is_empty()
                    || self.reason != "ok"
                {
                    return Err("restore_verification_not_ready");
                }
            }
        }
        Ok(())
    }

    /// 重新计算并写入自身摘要。
    ///
    /// 仅供已经有意修改字段的派生器/夹具使用。摘要不带密钥，`recompute_digest()` 只让
    /// 载荷重新自洽，不证明字段来自可信恢复进程。
    pub fn recompute_digest(&mut self) {
        self.verification_digest = self.digest();
    }

    /// 激活前的就绪门：只有 `Verified` 的 fact 才能进入恢复激活。
    ///
    /// 【调用者】Daemon 的恢复校验/激活入口。`validate()` 仍允许表达 Blocked 与
    /// NeedsReconciliation（它们需要被记录、被人工对账），但结构合法不等于可以启动实例。
    pub fn require_verified(&self) -> Result<(), &'static str> {
        if self.state != RestoreVerificationState::Verified {
            return Err("restore_verification_not_verified");
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
            "external_effect_claims": self.external_effect_claims,
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

/// 激活前就绪门的自由函数形式；与 [`RestoreVerificationFact::require_verified`] 相同。
pub fn require_verified_restore_verification(
    fact: &RestoreVerificationFact,
) -> Result<(), &'static str> {
    fact.require_verified()
}

/// 清单文件在恢复事实中的规范 artifact 引用：`artifact:<relative_path>#<content_hash>`。
pub fn restore_artifact_ref(file: &SnapshotFileSeal) -> String {
    format!(
        "{RESTORE_ARTIFACT_REF_PREFIX}{}#{}",
        file.relative_path, file.content_hash
    )
}

/// 将 fact 的 artifact 引用绑定到已封存清单的真实文件哈希。
///
/// 【调用者】Daemon `verify_restore()`。`RestoreVerificationFact::validate()` 只能检查
/// 「非空、唯一、长度」，它不知道清单里有哪些文件；这里补上真正的比对：
/// - 引用形状必须是 `artifact:<relative_path>#sha256:<64 hex>`；
/// - 每个引用都必须指向清单中的文件，且哈希与 `SnapshotFileSeal.content_hash` 相同；
/// - 清单中的每个文件都必须至少被一个引用覆盖。
///
/// 【边界】只比较调用方提供的元数据，不读取文件字节；文件内容是否真的匹配哈希仍由
/// 备份/恢复 adapter 负责。
pub fn bind_restore_artifact_refs(
    manifest: &SnapshotManifest,
    artifact_refs: &[String],
) -> Result<(), &'static str> {
    let mut expected: BTreeMap<&str, &str> = BTreeMap::new();
    for file in &manifest.files {
        expected.insert(file.relative_path.as_str(), file.content_hash.as_str());
    }
    let mut covered: BTreeSet<&str> = BTreeSet::new();
    for reference in artifact_refs {
        let Some(rest) = reference.strip_prefix(RESTORE_ARTIFACT_REF_PREFIX) else {
            return Err("restore_artifact_ref_invalid");
        };
        let Some((path, hash)) = rest.rsplit_once('#') else {
            return Err("restore_artifact_ref_invalid");
        };
        if path.is_empty() || !valid_digest(hash) {
            return Err("restore_artifact_ref_invalid");
        }
        let Some(expected_hash) = expected.get(path) else {
            return Err("restore_artifact_ref_unknown");
        };
        if *expected_hash != hash {
            return Err("restore_artifact_ref_hash_mismatch");
        }
        covered.insert(path);
    }
    if expected.keys().any(|path| !covered.contains(*path)) {
        return Err("restore_artifact_ref_missing");
    }
    Ok(())
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
