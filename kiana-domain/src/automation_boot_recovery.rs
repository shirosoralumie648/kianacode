//! AUT-21 启动恢复状态事实的领域 DTO 与一致性校验。
//!
//! # 在系统中的位置
//!
//! `DaemonHost` 启动路径观测 EventStore 游标、authority epoch、租约和 journal 健康
//!     -> 构造 `AutomationBootRecoveryFact`
//!     -> 本文件检查游标、generation、authority epoch、Unknown 数、索引标记、租约、journal、
//!        typed reconcile case、状态组合和摘要
//!     -> 返回成功或稳定拒绝码
//!
//! 本文件没有 I/O，也不启动 worker。它只回答「这份恢复事实自洽吗、这些恢复前置条件是否仍
//! 未清除」。真正决定能否 dispatch 的是 daemon 的显式 resume 屏障，而不是本 DTO。
//!
//! # 证据边界
//!
//! `recovery_digest` 是无密钥内容指纹，不证明 fact 来自当前 EventLog 或可信恢复流程；通过
//! `validate()` 只表示字段之间的约束成立。

use crate::journal::JOURNAL_HEADER_SCHEMA;
use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 启动恢复状态 fact 的固定序列化 schema 名称。
pub const AUTOMATION_BOOT_RECOVERY_SCHEMA: &str = "kiana.automation-boot-recovery.v1";

/// 单个启动恢复 fact 允许携带的 typed reconcile case 上限。
///
/// 上限存在的目的不是性能，而是让「用超大列表淹没拒绝路径」不能成为一种绕过方式；超出上限
/// 的事实直接以 `automation_boot_recovery_reconcile_case_limit` 拒绝。
pub const MAX_AUTOMATION_RECONCILE_CASES: usize = 256;

/// 启动恢复期间对外报告的阶段标签。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationBootRecoveryState {
    /// 启动路径已经重建 projection，但显式 resume 之前 dispatch 仍被 fenced。
    ///
    /// 这是**干净重启的默认状态**：`Ready` 不是重启的默认值。本 DTO 不执行重建，重建由
    /// `kiana-daemon` 的启动方法调用 `ProjectionDriver::rebuild` 完成。
    Rebuilding,
    /// 存在待对账 Unknown / stale index / 未结租约，必须先建立并结清 reconcile case。
    ReconcileRequired,
    /// 所有恢复屏障均已清除；只有显式 operator resume 才能产出该状态。
    Ready,
    /// 恢复被阻断（journal/schema 损坏等）；该状态保留证据，但永不等于可 dispatch。
    Blocked,
}

/// journal 报告的健康输入；把「schema 不一致」与「读写损坏」分开。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationBootJournal {
    /// adapter 报告的 journal header schema，正常值为 [`JOURNAL_HEADER_SCHEMA`]。
    pub schema: String,
    /// adapter 自报的 healthy 标志；`false` 表示读写边界已损坏。
    pub healthy: bool,
}

impl AutomationBootJournal {
    /// 构造「健康且 schema 与当前 writer 一致」的输入。
    pub fn healthy() -> Self {
        Self {
            schema: JOURNAL_HEADER_SCHEMA.to_owned(),
            healthy: true,
        }
    }

    /// 只检查形状：schema 非空。健康/schema 匹配由 fact 的拒绝码分开表达。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim().is_empty() || self.schema.len() > 128 {
            return Err("automation_boot_recovery_journal_invalid");
        }
        Ok(())
    }
}

/// 启动时观测到的实例租约。
///
/// 无租约用 `lease_id = None` 表达；有租约时 `observed_at_unix_ms >= expires_at_unix_ms`
/// 即视为过期（stale）。未过期租约同样不允许 `Ready`：另一个进程仍持有恢复写权。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationBootLease {
    /// 观测到的租约 ID；`None` 表示没有租约。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    /// 租约到期时间（unix ms）；无租约时为 0。
    pub expires_at_unix_ms: u64,
    /// 观测该租约的时间（unix ms）；无租约时为 0。
    pub observed_at_unix_ms: u64,
}

impl AutomationBootLease {
    /// 构造「没有租约」的输入。
    pub fn absent() -> Self {
        Self {
            lease_id: None,
            expires_at_unix_ms: 0,
            observed_at_unix_ms: 0,
        }
    }

    /// 租约是否已过期；无租约不算 stale。
    pub fn is_stale(&self) -> bool {
        self.lease_id.is_some() && self.observed_at_unix_ms >= self.expires_at_unix_ms
    }

    /// 只检查形状，不判断是否 stale。
    pub fn validate(&self) -> Result<(), &'static str> {
        match &self.lease_id {
            Some(lease_id)
                if lease_id.trim().is_empty()
                    || lease_id.len() > 256
                    || self.expires_at_unix_ms == 0
                    || self.observed_at_unix_ms == 0 =>
            {
                Err("automation_boot_recovery_lease_invalid")
            }
            None if self.expires_at_unix_ms != 0 || self.observed_at_unix_ms != 0 => {
                Err("automation_boot_recovery_lease_invalid")
            }
            _ => Ok(()),
        }
    }
}

/// 一个待对账的恢复项。
///
/// 取代早期自由字符串 `reconcile_refs`：每个 case 必须绑定它被发现时的 source cursor、
/// projection generation、authority epoch 以及归属的 Unknown 数，因此「列一个空引用」不再
/// 能伪造对账进度。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationReconcileCase {
    /// 发现该 case 时的 source cursor；必须非零。
    pub source_cursor: u64,
    /// 需要重建/对账的 projection generation；必须非零。
    pub projection_generation: u64,
    /// 授权该未结工作的 authority epoch；必须非零。
    pub authority_epoch: u64,
    /// 归属本 case 的 Unknown 效果数；必须非零（无 Unknown 的项用 stale_index 表达）。
    pub pending_unknown_count: u32,
}

impl AutomationReconcileCase {
    /// 校验 case 自身的字段范围。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.source_cursor == 0
            || self.projection_generation == 0
            || self.authority_epoch == 0
            || self.pending_unknown_count == 0
        {
            return Err("automation_boot_recovery_reconcile_case_invalid");
        }
        Ok(())
    }
}

/// 把事件源和恢复投影的进度、代次及待对账状态放在一起的可序列化 fact。
///
/// `deny_unknown_fields` 保证 JSON 中多出的字段被拒绝而不是静默忽略。各字段仍由构造者提供；
/// 本结构不连接 EventStore，也不能据此查出计数或租约是否与真实运行状态相符。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationBootRecoveryFact {
    /// 必须与 `AUTOMATION_BOOT_RECOVERY_SCHEMA` 完全一致。
    pub schema: String,
    /// 调用方提供的 source 游标；必须非零。
    pub source_cursor: u64,
    /// 投影已处理到的游标；不能大于 source_cursor，`Ready` 时必须相等。
    pub projection_cursor: u64,
    /// 投影重建代次；必须非零。
    pub projection_generation: u64,
    /// fact 声明所依据的授权 epoch；必须非零，且 `Ready` 时必须与观测 epoch 相等。
    pub authority_epoch: u64,
    /// 启动路径观测到的当前授权 epoch；用于识别旧 epoch 快照。
    pub observed_authority_epoch: u64,
    /// 待明确处理的 Unknown 效果总数；`Ready` 必须为零。
    pub pending_unknown_count: u32,
    /// 调用方声明索引是否过期；`Ready` 必须为 false。
    pub stale_index: bool,
    /// 启动时观测到的实例租约。
    pub lease: AutomationBootLease,
    /// adapter 报告的 journal schema 与健康状态。
    pub journal: AutomationBootJournal,
    /// 调用方声明的恢复阶段。
    pub state: AutomationBootRecoveryState,
    /// 待结清的 typed reconcile case；`Ready` 必须为空。
    pub reconcile_cases: Vec<AutomationReconcileCase>,
    /// 对其余 fact 字段计算的 SHA-256 内容摘要，不是签名或来源证明。
    pub recovery_digest: String,
}

impl AutomationBootRecoveryFact {
    /// 重新计算并写入 `recovery_digest`，返回自身。
    ///
    /// 构造器/调用方在改完字段后必须调用它；`validate()` 会拒绝摘要与内容不一致的 fact。
    pub fn sealed(mut self) -> Self {
        self.recovery_digest = self.digest();
        self
    }

    /// 校验恢复 fact 的结构、恢复屏障和自摘要，不执行启动恢复。
    ///
    /// 【拒绝顺序】结构问题（schema、游标、代次、journal/lease 形状、case 形状与总量、
    /// 摘要）先于状态谓词。因此一个字段坏掉的 fact 不会因为 `state = Blocked` 而被接受。
    ///
    /// 【Blocked 的证据例外】`Blocked` 事实允许携带损坏的 journal / 过期租约 / 旧 epoch，
    /// 因为它记录的就是「启动被什么挡住」；但这些证据永远不能变成 `Ready`。
    ///
    /// 【Ready 的完整屏障】projection 追平 source、零 Unknown、零未结 case、非 stale index、
    /// 无租约、journal healthy 且 schema 匹配、authority epoch 追平观测 epoch。任何一项不满足
    /// 都返回各自稳定的拒绝码。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_BOOT_RECOVERY_SCHEMA
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
            || self.projection_generation == 0
            || self.authority_epoch == 0
            || self.observed_authority_epoch == 0
        {
            return Err("automation_boot_recovery_invalid");
        }
        self.journal.validate()?;
        self.lease.validate()?;
        if self.reconcile_cases.len() > MAX_AUTOMATION_RECONCILE_CASES {
            return Err("automation_boot_recovery_reconcile_case_limit");
        }
        let mut case_unknown_total: u64 = 0;
        for case in &self.reconcile_cases {
            case.validate()?;
            if case.source_cursor > self.source_cursor
                || case.projection_generation > self.projection_generation
                || case.authority_epoch > self.authority_epoch
            {
                return Err("automation_boot_recovery_reconcile_case_future");
            }
            case_unknown_total =
                case_unknown_total.saturating_add(u64::from(case.pending_unknown_count));
        }
        if case_unknown_total != u64::from(self.pending_unknown_count) {
            return Err("automation_boot_recovery_reconcile_case_total_mismatch");
        }
        if !valid_digest(&self.recovery_digest) || self.recovery_digest != self.digest() {
            return Err("automation_boot_recovery_invalid");
        }
        // Blocked 事实是证据容器：允许携带损坏的 journal / 旧 epoch / 未结租约。
        if matches!(self.state, AutomationBootRecoveryState::Blocked) {
            return Ok(());
        }
        if self.journal.schema != JOURNAL_HEADER_SCHEMA {
            return Err("automation_boot_recovery_journal_schema_mismatch");
        }
        if !self.journal.healthy {
            return Err("automation_boot_recovery_journal_corrupt");
        }
        match self.state {
            AutomationBootRecoveryState::Ready => {
                // 顺序即优先级：先证明投影追平，再拒绝未结工作，最后拒绝陈旧授权/租约。
                if self.projection_cursor != self.source_cursor {
                    return Err("automation_boot_recovery_projection_lagging");
                }
                if self.pending_unknown_count > 0 {
                    return Err("automation_boot_recovery_unknown_pending");
                }
                if !self.reconcile_cases.is_empty() {
                    return Err("automation_boot_recovery_reconcile_case_open");
                }
                if self.stale_index {
                    return Err("automation_boot_recovery_index_stale");
                }
                if self.lease.is_stale() {
                    return Err("automation_boot_recovery_stale_lease");
                }
                if self.lease.lease_id.is_some() {
                    return Err("automation_boot_recovery_lease_active");
                }
                if self.authority_epoch < self.observed_authority_epoch {
                    return Err("automation_boot_recovery_authority_epoch_stale");
                }
                if self.authority_epoch > self.observed_authority_epoch {
                    return Err("automation_boot_recovery_authority_epoch_ahead");
                }
            }
            AutomationBootRecoveryState::ReconcileRequired => {
                // ReconcileRequired 必须真的有一项待对账证据，否则状态本身在说谎。
                if self.pending_unknown_count == 0
                    && !self.stale_index
                    && self.reconcile_cases.is_empty()
                    && self.lease.lease_id.is_none()
                {
                    return Err("automation_boot_recovery_invalid");
                }
            }
            AutomationBootRecoveryState::Rebuilding => {}
            AutomationBootRecoveryState::Blocked => unreachable!("blocked returned above"),
        }
        Ok(())
    }

    /// 对除 `recovery_digest` 外的所有字段计算规范 JSON SHA-256 内容摘要。
    ///
    /// digest 用来发现字段与自带指纹不一致；没有签名密钥时它不证明来源。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "authority_epoch": self.authority_epoch,
            "observed_authority_epoch": self.observed_authority_epoch,
            "pending_unknown_count": self.pending_unknown_count,
            "stale_index": self.stale_index,
            "lease": self.lease,
            "journal": self.journal,
            "state": self.state,
            "reconcile_cases": self.reconcile_cases,
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
