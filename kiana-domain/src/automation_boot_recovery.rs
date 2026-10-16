//! AUT-21 启动恢复状态事实的领域 DTO 与局部一致性校验。
//!
//! # 在系统中的位置
//!
//! 启动恢复编排器（当前未找到生产调用者）构造 `AutomationBootRecoveryFact`
//!     -> 本文件检查游标、generation、authority epoch、Unknown 数、索引标记、状态组合和摘要
//!     -> 返回成功或 `automation_boot_recovery_invalid`
//!
//! AUT-21 的产品目标还包括重建投影和索引、枚举待 reconcile 项并追加恢复事件；本文件没有这些 I/O，
//! 也不启动 worker。Core 文件只是本 validator 的只读转发层，所以该类型存在不代表真实 boot recovery
//! 已接入产品。`recovery_digest` 是无密钥内容指纹，不证明 fact 来自当前 EventLog 或可信恢复流程。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 启动恢复状态 fact 的固定序列化 schema 名称。
pub const AUTOMATION_BOOT_RECOVERY_SCHEMA: &str = "kiana.automation-boot-recovery.v1";

/// 启动恢复期间对外报告的阶段标签。
///
/// 这些枚举值本身不会触发重建、对账或阻断；当前 validator 只对其中两种状态应用局部字段约束。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationBootRecoveryState {
    /// 声明恢复流程正在重建 projection；本 DTO 不执行重建。
    Rebuilding,
    /// 声明存在待对账 Unknown 或 stale index；本 DTO 不创建 reconcile case。
    ReconcileRequired,
    /// 声明系统已就绪；实现目前只排除 Unknown 和 stale index，没有要求投影游标追平 source。
    Ready,
    /// 声明恢复被阻断；validator 当前没有为此状态强制更细的原因或字段组合。
    Blocked,
}

/// 把事件源和恢复投影的进度、代次及待对账状态放在一起的可序列化 fact。
///
/// `deny_unknown_fields` 是 Serde 的反序列化规则：JSON 带有结构体没有定义的额外字段时会失败，而不是
/// 悄悄忽略。各游标、代次、数量、标志和引用仍由 fact 的构造者提供；本结构不连接 EventStore，也不
/// 能据此查出计数或引用是否与真实运行状态相符。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationBootRecoveryFact {
    /// 必须与 `AUTOMATION_BOOT_RECOVERY_SCHEMA` 完全一致。
    pub schema: String,
    /// 调用方提供的 source 游标；必须非零，但本 DTO 没有定义其具体来源或全局/单流含义。
    pub source_cursor: u64,
    /// 投影已处理到的游标；不能大于 source_cursor，但当前并未要求 Ready 时相等。
    pub projection_cursor: u64,
    /// 投影重建代次；必须非零，但不查询存储中的当前 generation。
    pub projection_generation: u64,
    /// fact 声明所依据的授权 epoch；必须非零，但不读取当前 authority。
    pub authority_epoch: u64,
    /// 待明确处理的 Unknown 效果数；Ready 必须为零。
    pub pending_unknown_count: u32,
    /// 调用方声明索引是否过期；Ready 必须为 false，但这里不查询索引。
    pub stale_index: bool,
    /// 调用方声明的恢复阶段；具体状态约束由 validate 的 match-like 条件实现。
    pub state: AutomationBootRecoveryState,
    /// 调用方提供的 reconcile 引用；每项只要求 trim 后非空，不检查存在性、唯一性或数量匹配。
    pub reconcile_refs: Vec<String>,
    /// 对其余 fact 字段计算的 SHA-256 内容摘要，不是签名或来源证明。
    pub recovery_digest: String,
}

impl AutomationBootRecoveryFact {
    /// 校验恢复 fact 的基本范围、部分状态组合和自摘要，不执行启动恢复。
    ///
    /// 【调用者/输入】Core facade `validate_boot_recovery()` 会转调此方法；目前没有在生产启动路径中发现
    /// 调用。输入是上游组装的整份快照。
    ///
    /// 【校验顺序与边界】要求 schema 正确、source_cursor 非零、projection_cursor 不超过 source、
    /// generation/authority epoch 非零、引用字符串非空、摘要格式与重算摘要一致。Ready 仅拒绝
    /// `pending_unknown_count > 0` 或 stale index；ReconcileRequired 仅在 Unknown 为零且 index 不 stale
    /// 时拒绝；Rebuilding/Blocked 没有额外状态约束。
    ///
    /// 【输出/失败】满足本地字段规则返回 `Ok(())`，任意失败统一为 `automation_boot_recovery_invalid`。
    /// 不记录日志、不重试、不写恢复事件、不改变投影/worker 状态；调用方决定错误如何呈现。
    ///
    /// ⚠ 文档 AUT-21 baseline 声明 Ready 要求 projection 已追平 source cursor，但当前条件没有检查相等。
    /// 因此带有落后游标、零 Unknown、非 stale index 且摘要正确的 Ready fact 可通过这里；这项差异仅记录，
    /// 不在注释任务中改变 validator 或状态账本。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_BOOT_RECOVERY_SCHEMA
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
            || self.projection_generation == 0
            || self.authority_epoch == 0
            || self.reconcile_refs.iter().any(|v| v.trim().is_empty())
            || matches!(self.state, AutomationBootRecoveryState::Ready)
                && (self.pending_unknown_count > 0 || self.stale_index)
            || matches!(self.state, AutomationBootRecoveryState::ReconcileRequired)
                && self.pending_unknown_count == 0
                && !self.stale_index
            || !valid_digest(&self.recovery_digest)
            || self.recovery_digest != self.digest()
        {
            return Err("automation_boot_recovery_invalid");
        }
        Ok(())
    }

    /// 对除 recovery_digest 外的所有字段计算规范 JSON SHA-256 内容摘要。
    ///
    /// digest 用来发现字段与自带指纹不一致；由于没有签名密钥且没有读取可信数据源，匹配不等于 fact
    /// 已由真实恢复过程产生。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "authority_epoch": self.authority_epoch,
            "pending_unknown_count": self.pending_unknown_count,
            "stale_index": self.stale_index,
            "state": self.state,
            "reconcile_refs": self.reconcile_refs,
        }))
    }
}

/// 领域层公开 validator 入口；只转调 fact 自身规则，不连接持久化或恢复执行器。
pub fn validate_automation_boot_recovery(
    fact: &AutomationBootRecoveryFact,
) -> Result<(), &'static str> {
    fact.validate()
}

/// 要求固定 `sha256:` 前缀和 64 位 ASCII 十六进制字符；不验证外部数据或签名。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
