//! 投影滞后视图与结果未知 mutation 恢复边界的领域合同。
//!
//! 【系统位置与数据流】
//! ```text
//! EventLog/投影读取者提供 source cursor、projection cursor、generation、epoch 和原因
//!     -> ProjectionLagView::new()
//!     -> 本文件校验游标关系并派生 CaughtUp/Pending/Unknown
//!     -> AuditProjectorFreshness、receipt/Inspector 等消费者拒绝使用不一致读模型
//! ```
//! 本文件只校验调用者已汇总的字段，不访问 EventLog、检查点或时钟。`UnknownMutationReconciliation` 是另一条防重复恢复合同：
//! 如果外部 mutation 的提交结果不确定，后续核查必须沿用原 mutation ID 和原 request digest，不能换新 ID 再发一次。
//! 当前仓内没有该 reconciliation DTO 的生产调用者；`reconcile()` 只比较给定 ID/digest，不查询外部 mutation 是否成功。
//!
//! 这些摘要是无密钥内容指纹，不代表事件来源或 mutation 结果已得到认证。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 投影滞后视图的稳定 wire schema 名称。
pub const PROJECTION_LAG_VIEW_SCHEMA: &str = "kiana.projection-lag-view.v1";
/// 结果未知 mutation 恢复合同的稳定 wire schema 名称。
pub const UNKNOWN_MUTATION_RECONCILIATION_SCHEMA: &str = "kiana.unknown-mutation-reconciliation.v1";
/// 两类恢复 DTO 当前使用的 schema 版本。
pub const PROJECTION_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 限制必填标识/说明文本的非空、字节长度和 NUL 字符；调用点再按字段需要检查更具体的规则。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查 `sha256:` 前缀和 64 位十六进制摘要外形，不读取摘要对应的源对象。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 投影相对 source cursor 的状态；由 cursor 是否缺失/落后推导，不由调用方任意指定。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionLagStatus {
    /// 投影 cursor 与 source cursor 相同。
    CaughtUp,
    /// 投影已有 cursor，但落后于 source cursor。
    Pending,
    /// 没有投影 cursor，无法确认从哪里继续或是否已处理任何事件。
    Unknown,
}

/// 一个投影读取视图相对输入源的游标、代数和数据代际快照。
///
/// 它帮助调用方决定读模型能否当作当前答案使用；不是 checkpoint、事件事实，也不负责触发重建。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionLagView {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// 可兼容演进的 DTO 版本。
    pub version: SchemaVersion,
    /// 调用方观察到的权威输入末端游标，必须非零。
    pub source_cursor: u64,
    /// 投影已经处理到的末端游标；None 表示没有可用的进度证据。
    #[serde(default)]
    pub projection_cursor: Option<u64>,
    /// 调用方观察到的投影构建代数，必须非零；本 DTO 不查询 generation store。
    pub projection_generation: u64,
    /// 数据治理/快照代际，必须非零；此处不校验其权威来源。
    pub data_epoch: u64,
    /// 由 cursor 与 pending 字段组合派生的三态结果。
    pub status: ProjectionLagStatus,
    /// None cursor 或 cursor 落后时为 true，表示不能视作已追平。
    pub projection_pending: bool,
    /// Pending/Unknown 时解释原因；CaughtUp 状态禁止带 reason。
    pub reason: Option<String>,
    /// 对不含自身的视图字段计算的无密钥 SHA-256 摘要。
    pub view_digest: String,
}

impl ProjectionLagView {
    /// 从调用方采集的游标/代数事实派生状态，计算摘要并执行完整 DTO 校验。
    ///
    /// 【调用者】可见构造点包括 `kiana-query::audit_projector::derive_lag_view()` 与 `kiana-core::receipts::projection_lag_from_events()`；
    /// `ContextMemoryInspectorSnapshot` 也会接收该 view。它们传入的都是调用方派生字段，不代表均已接通权威 EventLog 路径。
    /// 【输入/输出】接收 source/projection cursor、generation、data epoch 和可选原因；成功返回状态不可独立改写的 view。
    /// 【核心规则】source cursor、generation、epoch 必须非零；projection cursor 不能为零或大于 source；缺失/落后时要求原因，追平时禁止原因。
    /// 【失败情况】返回 cursor、原因、状态不匹配或摘要错误；不会自动等待投影追平，也不会发起 rebuild。
    /// 【边界】所有输入仍是调用方声明；校验通过不证明这些值来自当前 EventLog 或同一事务快照。
    pub fn new(
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        data_epoch: u64,
        reason: Option<String>,
    ) -> Result<Self, String> {
        if source_cursor == 0
            || projection_cursor == Some(0)
            || projection_cursor.is_some_and(|cursor| cursor > source_cursor)
            || projection_generation == 0
            || data_epoch == 0
        {
            return Err("projection_lag_cursor_invalid".to_owned());
        }
        let projection_pending = projection_cursor.is_none_or(|cursor| cursor < source_cursor);
        let status = if projection_cursor.is_none() {
            ProjectionLagStatus::Unknown
        } else if projection_pending {
            ProjectionLagStatus::Pending
        } else {
            ProjectionLagStatus::CaughtUp
        };
        if projection_pending && reason.as_deref().is_none_or(str::is_empty) {
            return Err("projection_lag_reason_required".to_owned());
        }
        if !projection_pending && reason.is_some() {
            return Err("projection_lag_caught_up_reason_forbidden".to_owned());
        }
        let mut view = Self {
            schema: PROJECTION_LAG_VIEW_SCHEMA.to_owned(),
            version: PROJECTION_RECOVERY_VERSION,
            source_cursor,
            projection_cursor,
            projection_generation,
            data_epoch,
            status,
            projection_pending,
            reason,
            view_digest: String::new(),
        };
        view.view_digest = view.digest();
        view.validate()?;
        Ok(view)
    }

    /// 重算 cursor/pending/status 关系并检查文本和自摘要，拒绝字段相互矛盾的 view。
    ///
    /// 兼容版本按 `is_compatible_with()` 判断；reason 最多 256 UTF-8 字节。这里不回查来源存储或当前 projection generation。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECTION_LAG_VIEW_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECTION_RECOVERY_VERSION)
            || self.source_cursor == 0
            || self.projection_cursor == Some(0)
            || self
                .projection_cursor
                .is_some_and(|cursor| cursor > self.source_cursor)
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.projection_pending
                != self
                    .projection_cursor
                    .is_none_or(|cursor| cursor < self.source_cursor)
        {
            return Err("projection_lag_view_invalid".to_owned());
        }
        if self.projection_pending && self.reason.as_deref().is_none_or(str::is_empty) {
            return Err("projection_lag_reason_required".to_owned());
        }
        if !self.projection_pending && self.reason.is_some() {
            return Err("projection_lag_caught_up_reason_forbidden".to_owned());
        }
        if self.status
            != if self.projection_cursor.is_none() {
                ProjectionLagStatus::Unknown
            } else if self.projection_pending {
                ProjectionLagStatus::Pending
            } else {
                ProjectionLagStatus::CaughtUp
            }
        {
            return Err("projection_lag_status_mismatch".to_owned());
        }
        if let Some(reason) = &self.reason {
            required(reason, "projection_lag_reason", 256)?;
        }
        digest(&self.view_digest, "projection_lag_view_digest")?;
        if self.view_digest != self.digest() {
            return Err("projection_lag_view_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 判断当前 view 是否结构有效且 cursor 已追平。
    ///
    /// 任何非法 DTO 都保守返回 false；true 只表示游标合同满足，不代表投影内容摘要与权威源一致。
    pub fn read_consistent(&self) -> bool {
        self.validate().is_ok() && !self.projection_pending
    }

    /// 对 view 除 `view_digest` 外的全部字段计算 SHA-256 内容指纹。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "status": self.status,
            "projection_pending": self.projection_pending,
            "reason": self.reason,
        }))
    }
}

/// 当前支持的未知 mutation 结果状态；只有 ResultUnknown 是一种不能自动新发请求的未决状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownMutationState {
    /// 请求可能已提交但响应/回执无法确认；须使用原 mutation ID 和 request digest 继续核查。
    ResultUnknown,
}

/// 绑定原 mutation 身份的“结果未知”恢复合同。
///
/// 该 DTO 禁止用新 mutation ID 重新发起同一意图，避免把网络超时误当作从未执行。
/// 它不保存请求正文、不查询幂等记录，也不推断原 mutation 最终是否成功。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownMutationReconciliation {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// 可兼容演进的 DTO 版本。
    pub version: SchemaVersion,
    /// 原请求使用的 mutation/idempotency key；恢复查询不能替换它。
    pub mutation_id: String,
    /// 原请求的内容摘要；必须原样带回以防同一 ID 被错误复用于另一请求。
    pub request_digest: String,
    /// 当前固定为 ResultUnknown，直到外部核查流程解决状态。
    pub state: UnknownMutationState,
    /// 当前固定为 false，明确不允许用新 ID 发起替代 mutation。
    pub retry_with_new_id_allowed: bool,
    /// 对其余合同字段计算的无密钥 SHA-256 内容摘要。
    pub reconciliation_digest: String,
}

impl UnknownMutationReconciliation {
    /// 创建一条要求沿原 ID 和原 request digest 核查的未知结果合同。
    ///
    /// 【调用者】检测到操作可能提交但结果不明的恢复流程；当前仓内没有生产调用点，Domain tests 构造此 DTO。
    /// 【输入/输出】接收原 mutation ID 和请求摘要，固定写入 `ResultUnknown`、禁止新 ID 重试，再计算并校验摘要。
    /// 【失败情况】ID 空/超长、请求摘要外形不正确或自摘要校验失败时返回稳定错误。
    /// 【副作用/限制】只构造 DTO；不查询幂等存储，不确认请求已落盘，也不执行原请求。
    pub fn new(
        mutation_id: impl Into<String>,
        request_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut reconciliation = Self {
            schema: UNKNOWN_MUTATION_RECONCILIATION_SCHEMA.to_owned(),
            version: PROJECTION_RECOVERY_VERSION,
            mutation_id: mutation_id.into(),
            request_digest: request_digest.into(),
            state: UnknownMutationState::ResultUnknown,
            retry_with_new_id_allowed: false,
            reconciliation_digest: String::new(),
        };
        reconciliation.reconciliation_digest = reconciliation.digest();
        reconciliation.validate()?;
        Ok(reconciliation)
    }

    /// 验证未知状态合同、原 ID、request digest 和自摘要。
    ///
    /// 此校验不能把 ResultUnknown 变成成功/失败结论；它只保证调用者没有把恢复合同改成另一 mutation。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UNKNOWN_MUTATION_RECONCILIATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECTION_RECOVERY_VERSION)
            || self.state != UnknownMutationState::ResultUnknown
            || self.retry_with_new_id_allowed
        {
            return Err("unknown_mutation_reconciliation_invalid".to_owned());
        }
        required(&self.mutation_id, "unknown_mutation_id", 256)?;
        digest(&self.request_digest, "unknown_mutation_request_digest")?;
        digest(
            &self.reconciliation_digest,
            "unknown_mutation_reconciliation_digest",
        )?;
        if self.reconciliation_digest != self.digest() {
            return Err("unknown_mutation_reconciliation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 确认恢复查询仍使用原 mutation ID 与原 request digest。
    ///
    /// 【输入/输出】参数应来自准备恢复/查询的调用；两者都精确相等时返回 `Ok(())`。
    /// 【失败情况】换了 ID 返回 `unknown_mutation_new_id_forbidden`，请求摘要不同返回 `unknown_mutation_request_digest_mismatch`；
    /// 合同自身非法时先传播 `validate()` 错误。
    /// 【重要限制】匹配成功不表示外部 mutation 已完成核查，也不表示可以重放；真正查状态需要后续幂等/事件权威组件。
    pub fn reconcile(&self, mutation_id: &str, request_digest: &str) -> Result<(), String> {
        self.validate()?;
        if mutation_id != self.mutation_id {
            return Err("unknown_mutation_new_id_forbidden".to_owned());
        }
        if request_digest != self.request_digest {
            return Err("unknown_mutation_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对恢复合同除 `reconciliation_digest` 外的全部字段计算无密钥 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "mutation_id": self.mutation_id,
            "request_digest": self.request_digest,
            "state": self.state,
            "retry_with_new_id_allowed": self.retry_with_new_id_allowed,
        }))
    }
}
