//! 统一 shutdown（关闭）阶段回执与最终状态的 Domain 合同。
//!
//! 【预期系统位置与数据流】
//! ```text
//! 上层关闭协调器逐步取消/暂停/围栏/排空/flush/关闭
//!     -> 收集 ShutdownPhaseEvidence 和各阶段观察值
//!     -> ShutdownInput（调用方提供的关闭快照）
//!     -> 本文件校验快照并按优先级生成 ShutdownReport
//!     -> 上层根据 Stopped / NeedsRecovery / Unknown 决定后续恢复或报告
//! ```
//!
//! 本文件只消费已经整理好的布尔值、计数、时间和阶段回执，不会调用 cancellation、停止 scheduler/process、排空
//! runner/tool、flush/close EventStore 或写入 shutdown receipt。缺少/失败回执、Unknown 和 late result 会阻止 reducer
//! 给出 `Stopped`，但这只是值对象合同，不能证明真实资源已经关闭或回执来自可信 adapter。
//!
//! Serde 的 `Serialize`/`Deserialize` 用于 DTO 与 JSON 等外部格式互转；`deny_unknown_fields` 拒绝当前版本未声明的输入字段。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 单个 shutdown 阶段证据的 wire schema 名称。
pub const SHUTDOWN_PHASE_EVIDENCE_SCHEMA: &str = "kiana.shutdown-phase-evidence.v1";
/// shutdown input 的 wire schema 名称。
pub const SHUTDOWN_INPUT_SCHEMA: &str = "kiana.shutdown-input.v1";
/// 最终 shutdown report 的 wire schema 名称。
pub const SHUTDOWN_REPORT_SCHEMA: &str = "kiana.shutdown-report.v1";
/// 本文件 DTO 的当前 schema 版本。
pub const SHUTDOWN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 固定阶段序列的长度；新增阶段时需同步更新 enum、`ALL` 与 wire 合同。
pub const MAX_SHUTDOWN_PHASES: usize = 8;

/// 关闭协调过程中的有序阶段；派生 `Ord` 后的枚举顺序也用于校验输入排序。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownPhase {
    /// 停止接收新的工作。
    IntakePause,
    /// 请求取消正在运行的工作。
    Cancellation,
    /// 防止新的调度执行越过关闭边界。
    SchedulerFence,
    /// 等待 Runner 中已有执行排空。
    RunnerDrain,
    /// 等待工具调用、子进程或其他 capability 排空。
    ToolDrain,
    /// 确认 EventStore 的待写事实已经 flush。
    EventStoreFlush,
    /// 确认 ArtifactStore 的待写内容已经 flush。
    ArtifactFlush,
    /// 最后关闭 EventStore；此前的 flush 确认不能被这一步替代。
    EventStoreClose,
}

impl ShutdownPhase {
    /// 关闭报告必须覆盖的完整顺序。该常量是 reducer 的检查合同，不会自行执行这些阶段。
    pub const ALL: [Self; MAX_SHUTDOWN_PHASES] = [
        Self::IntakePause,
        Self::Cancellation,
        Self::SchedulerFence,
        Self::RunnerDrain,
        Self::ToolDrain,
        Self::EventStoreFlush,
        Self::ArtifactFlush,
        Self::EventStoreClose,
    ];
}

/// 单个阶段的回执状态；`Confirmed` 是调用方提供的声明，Domain 不验证底层系统事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownAckStatus {
    /// 阶段尚未完成或尚未收到最终回执；最终报告需要恢复。
    Pending,
    /// adapter/协调器声明阶段已确认完成。
    Confirmed,
    /// 阶段执行明确失败。
    Failed,
    /// 无法确定阶段是否完成；最终状态保留 Unknown。
    Unknown,
}

/// 汇总多个阶段和未解决工作后的关闭分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownStatus {
    /// 所有阶段已确认、边界布尔值全为 true、无未解决工作且尚未超过 deadline。
    Stopped,
    /// 有阶段缺失/失败、边界未确认、仍有工作或关闭 deadline 已过，需要补充恢复处理。
    NeedsRecovery,
    /// 至少一个阶段回执本身为 Unknown，不能可靠判断关闭结果。
    Unknown,
}

/// 一个 shutdown 阶段的调用方回执值。
///
/// `detail_digest` 指向阶段细节的内容摘要，`source_cursor` 标识其供给源位置；本类型只检查它们的非零/摘要格式，
/// 不读取真实日志、flush 记录或资源状态，也不保证 cursor 连续、递增或来自同一事实源。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownPhaseEvidence {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// 回执合同版本。
    pub version: SchemaVersion,
    /// 这条回执声明对应的 shutdown 阶段。
    pub phase: ShutdownPhase,
    /// Pending/Confirmed/Failed/Unknown 分类；只有 Unknown 和 Pending/Failed 会阻止正常 Stopped 路径。
    pub status: ShutdownAckStatus,
    /// 回执来源的 cursor；只要求非零，不与其他阶段做游标顺序/连续性比较。
    pub source_cursor: u64,
    /// 阶段细节或底层报告的摘要引用；只校验字符串形状，不回读其内容。
    pub detail_digest: String,
    /// 非空原因文本；最多 256 UTF-8 字节，主要供诊断/恢复消费方识别。
    pub reason: String,
    /// 除自身外其余回执字段的 SHA-256 内容摘要，不是 adapter 签名。
    pub evidence_digest: String,
}

impl ShutdownPhaseEvidence {
    /// 为一个阶段创建带摘要的回执值并运行自校验。
    ///
    /// 【调用者】预期由实际 shutdown 协调器在收到阶段结果后创建；当前 Domain 测试可直接构造，未发现生产调用链。
    /// 【输入/输出】输入阶段、ack 状态、来源 cursor、细节摘要和原因文本；成功返回自洽 DTO，字段/摘要不合法返回稳定 `String` 错误。
    /// 【副作用/限制】不执行阶段、不查 EventStore、不验证 source cursor 或底层效果，只分配内存值并计算摘要。
    pub fn new(
        phase: ShutdownPhase,
        status: ShutdownAckStatus,
        source_cursor: u64,
        detail_digest: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: SHUTDOWN_PHASE_EVIDENCE_SCHEMA.to_owned(),
            version: SHUTDOWN_VERSION,
            phase,
            status,
            source_cursor,
            detail_digest: detail_digest.into(),
            reason: reason.into(),
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    /// 检查回执 schema、版本、非零 source cursor、原因文本长度和摘要自洽性。
    ///
    /// `detail_digest` 通过只表示其外形合法；无密钥 `evidence_digest` 不能证明阶段真的完成。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHUTDOWN_PHASE_EVIDENCE_SCHEMA
            || self.version != SHUTDOWN_VERSION
            || self.source_cursor == 0
            || self.reason.trim().is_empty()
            || self.reason.len() > 256
        {
            return Err("shutdown_phase_evidence_header_invalid".to_owned());
        }
        valid_digest(&self.detail_digest, "shutdown_phase_detail_digest")?;
        valid_digest(&self.evidence_digest, "shutdown_phase_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("shutdown_phase_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 schema/version、phase/status、cursor、细节摘要和原因文本计算内容摘要，不含自身字段。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "phase": self.phase,
            "status": self.status,
            "source_cursor": self.source_cursor,
            "detail_digest": self.detail_digest,
            "reason": self.reason,
        }))
    }
}

/// 由关闭协调器提供给 reducer 的关闭输入快照。
///
/// 字段中既有阶段回执列表，也有对应边界的布尔值和未解决工作计数；本对象没有机制证明两组数据来自同一观察，
/// 所以上游必须保证其来源和时点一致。`input_digest` 只绑定当前 DTO 内容。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownInput {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// input 合同版本。
    pub version: SchemaVersion,
    /// 调用方提供的当前 Unix 毫秒时间；不由 Domain 读取系统时钟。
    pub now_unix_ms: u64,
    /// 关闭协调器使用的绝对 Unix 毫秒期限；过期判断使用 `now >= deadline`。
    pub deadline_unix_ms: u64,
    /// 上游是否已发起取消请求；只作结果检查，不会在此调用 cancel。
    pub cancellation_requested: bool,
    /// 上游是否已停止接收新工作；本值不会暂停实际 intake。
    pub intake_paused: bool,
    /// 上游是否已设置 scheduler fence，避免新调度越过关闭边界。
    pub scheduler_fenced: bool,
    /// 上游是否确认 Runner 中已有工作已排空。
    pub runner_drained: bool,
    /// 上游是否确认工具/capability 工作已排空。
    pub tools_drained: bool,
    /// 上游是否确认 EventStore 内容已 flush。
    pub event_store_flushed: bool,
    /// 上游是否确认 ArtifactStore 内容已 flush。
    pub artifact_flushed: bool,
    /// 上游是否确认 EventStore 已关闭；此项须与先前 flush 分开确认。
    pub event_store_closed: bool,
    /// 调用方统计的仍在执行工作数；大于零会导致 NeedsRecovery。
    pub active_work_count: u64,
    /// 调用方统计的效果结果未知数；大于零表示需要 reconciliation。
    pub unknown_effect_count: u64,
    /// 调用方统计的迟到结果数；用于避免把迟到写入/回执忽略后宣称 Stopped。
    pub late_result_count: u64,
    /// 阶段回执列表，按 `ShutdownPhase` 严格递增且最多 8 条；缺少阶段会进入恢复路径。
    pub phases: Vec<ShutdownPhaseEvidence>,
    /// 除自身摘要外所有输入字段的 SHA-256 内容摘要。
    pub input_digest: String,
}

impl ShutdownInput {
    /// 校验 input header、时间字段、阶段顺序/数量、嵌套回执和自摘要。
    ///
    /// 【调用者】`ShutdownReport::evaluate()` 在汇总阶段前调用，Core/Daemon 路由间接经过此检查。
    /// 【输入/输出】借用调用方提供的关闭快照；非法 schema、零时间、阶段未排序/重复/超限、回执错误或摘要不匹配时返回稳定字符串。
    /// 【重要限制】允许 `now_unix_ms >= deadline_unix_ms`，过期本身由 reducer 产出 NeedsRecovery；不会验证时钟、ack 来源、
    /// 阶段 cursor、布尔值与 evidence 列表是否相互对应，也不会确认这些事实最新。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHUTDOWN_INPUT_SCHEMA
            || self.version != SHUTDOWN_VERSION
            || self.now_unix_ms == 0
            || self.deadline_unix_ms == 0
            || self.phases.len() > MAX_SHUTDOWN_PHASES
            || self
                .phases
                .windows(2)
                .any(|pair| pair[0].phase >= pair[1].phase)
        {
            return Err("shutdown_input_header_invalid".to_owned());
        }
        // 上面的相邻严格递增检查已经拒绝相邻重复；任意重复 phase 在递增序列中也必然造成某处非递增。
        // 因此当前执行顺序下，下面的专用 shutdown_phase_duplicate 错误不会被重复 phase 触发。
        let mut phases = BTreeSet::new();
        for phase in &self.phases {
            phase.validate()?;
            if !phases.insert(phase.phase) {
                return Err("shutdown_phase_duplicate".to_owned());
            }
        }
        valid_digest(&self.input_digest, "shutdown_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("shutdown_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将关闭边界布尔值、计数、时间和有序阶段回执一起纳入 input digest。
    ///
    /// 摘要能检测未重新计算摘要的意外修改；能够构造 DTO 的调用方仍可同步修改字段和摘要，因此它不是来源认证或 durable receipt。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "now_unix_ms": self.now_unix_ms,
            "deadline_unix_ms": self.deadline_unix_ms,
            "cancellation_requested": self.cancellation_requested,
            "intake_paused": self.intake_paused,
            "scheduler_fenced": self.scheduler_fenced,
            "runner_drained": self.runner_drained,
            "tools_drained": self.tools_drained,
            "event_store_flushed": self.event_store_flushed,
            "artifact_flushed": self.artifact_flushed,
            "event_store_closed": self.event_store_closed,
            "active_work_count": self.active_work_count,
            "unknown_effect_count": self.unknown_effect_count,
            "late_result_count": self.late_result_count,
            "phases": self.phases,
        }))
    }
}

/// 关闭 reducer 的汇总报告。
///
/// `Stopped` 是本模块按 input 合同推导出的状态值，不是操作系统、EventStore 或 ArtifactStore 返回的独立证明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownReport {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// report 合同版本。
    pub version: SchemaVersion,
    /// 产生报告的 input 摘要。
    pub input_digest: String,
    /// Stopped/NeedsRecovery/Unknown 分类。
    pub status: ShutdownStatus,
    /// active work、unknown effect 和 late result 三个计数的饱和加和；来源计数若有重叠，本模块不会去重。
    pub unresolved_work_count: u64,
    /// 稳定原因码；不等同于最终用户可读错误文本。
    pub reason: String,
    /// 建议由上层采取的恢复动作标识；本 reducer 不执行它。
    pub remediation: String,
    /// 除自身外所有报告字段的 SHA-256 内容摘要。
    pub report_digest: String,
}

impl ShutdownReport {
    /// 校验 input 并生成统一 shutdown 汇总报告。
    ///
    /// 【调用者】Core `evaluate_shutdown()` 与 DaemonHost 路由静态委托到这里；当前未发现生产代码调用宿主方法。
    /// 【核心流程】校验 input → 构造 phase 覆盖/状态索引 → 饱和汇总未解决计数 → 按 Unknown、ack 缺失/失败、边界未确认、
    /// 未解决工作/deadline、全部成功的顺序选择最终结果 → 计算 report digest → `validate_against()` 重算绑定。
    /// 【输出/失败】有效 input 会返回报告；Unknown/NeedsRecovery 是正常 `Ok(report)` 状态，只有 DTO 合同错误返回 `Err(String)`。
    /// 【副作用】纯计算，不取消、不 drain、不 flush/close、不持久化，也不启动重试。
    pub fn evaluate(input: &ShutdownInput) -> Result<Self, String> {
        input.validate()?;
        let report = build_report(input);
        report.validate_against(input)?;
        Ok(report)
    }

    /// 只验证 report 自身 header、原因文本和摘要，不知道 report 应对应哪份 input。
    ///
    /// 要确认状态、计数、原因和 remediation 是按某份快照算出的，必须再调用 `validate_against(input)`。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHUTDOWN_REPORT_SCHEMA
            || self.version != SHUTDOWN_VERSION
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
        {
            return Err("shutdown_report_header_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "shutdown_report_input_digest")?;
        valid_digest(&self.report_digest, "shutdown_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("shutdown_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 通过同一 input 重建预期 report，并逐字段要求当前 report 完全一致。
    ///
    /// 格式、摘要或派生值不匹配时返回稳定 `String` 错误；这仍只验证两个 DTO 之间的自洽关系，不验证底层关闭效果。
    pub fn validate_against(&self, input: &ShutdownInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        if self != &build_report(input) {
            return Err("shutdown_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 schema/version、input digest、status、计数和原因字段计算摘要，不包含自身摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "status": self.status,
            "unresolved_work_count": self.unresolved_work_count,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// 从已通过 `validate()` 的 input 推导报告并填入稳定原因与 remediation。
///
/// 此函数由 `evaluate()` 与 `validate_against()` 调用；调用它之前必须完成 input 校验，否则阶段完整性和摘要假设不成立。
fn build_report(input: &ShutdownInput) -> ShutdownReport {
    // 固定序列中第一个没有回执的阶段；只要缺一个阶段，就不能生成 Stopped。
    let missing_phase = ShutdownPhase::ALL
        .iter()
        .find(|phase| !input.phases.iter().any(|fact| fact.phase == **phase));
    // Unknown 优先于其他失败：只要某阶段最终状态不可知，总体也不能降格成普通 NeedsRecovery。
    let unknown_phase = input
        .phases
        .iter()
        .find(|fact| fact.status == ShutdownAckStatus::Unknown);
    // Pending 表示还没有最终 ack，Failed 表示阶段失败；二者都需要恢复/补收回执。
    let failed_phase = input.phases.iter().find(|fact| {
        matches!(
            fact.status,
            ShutdownAckStatus::Failed | ShutdownAckStatus::Pending
        )
    });
    // 饱和加法避免异常大的计数发生 u64 溢出后回绕成 0，误判为没有未解决工作。
    // 这里按计数相加，不知道三个计数是否指向同一个底层工作，因此也不做实体去重。
    let unresolved = input
        .active_work_count
        .saturating_add(input.unknown_effect_count)
        .saturating_add(input.late_result_count);
    // 与维护窗口一致采用包含边界：到达 deadline 的那一刻即已超时。
    // 两个时刻都是调用方提供的值，本模块不验证系统时钟或期限来源。
    let deadline_expired = input.now_unix_ms >= input.deadline_unix_ms;
    // 分支顺序是对诊断分类的优先级：Unknown ack > 缺失/失败 ack > 布尔边界未确认 > 未解决/超时 > Stopped。
    // 因此多个问题同时存在时，只返回优先级最高的一个 reason。
    let (status, reason, remediation) = if unknown_phase.is_some() {
        (
            ShutdownStatus::Unknown,
            "shutdown_phase_unknown".to_owned(),
            "reconcile_shutdown_phase_before_restart".to_owned(),
        )
    } else if missing_phase.is_some() || failed_phase.is_some() {
        (
            ShutdownStatus::NeedsRecovery,
            "shutdown_ack_missing_or_failed".to_owned(),
            "collect_all_shutdown_acknowledgements".to_owned(),
        )
    } else if !input.cancellation_requested
        || !input.intake_paused
        || !input.scheduler_fenced
        || !input.runner_drained
        || !input.tools_drained
        || !input.event_store_flushed
        || !input.artifact_flushed
        || !input.event_store_closed
    {
        (
            ShutdownStatus::NeedsRecovery,
            "shutdown_boundary_unconfirmed".to_owned(),
            "complete_cancellation_drain_and_flush_ack".to_owned(),
        )
    } else if unresolved > 0 || deadline_expired {
        (
            ShutdownStatus::NeedsRecovery,
            if deadline_expired {
                "shutdown_deadline_expired".to_owned()
            } else {
                "shutdown_work_unresolved".to_owned()
            },
            "reconcile_inflight_or_unknown_work".to_owned(),
        )
    } else {
        (ShutdownStatus::Stopped, "ok".to_owned(), "none".to_owned())
    };
    let mut report = ShutdownReport {
        schema: SHUTDOWN_REPORT_SCHEMA.to_owned(),
        version: SHUTDOWN_VERSION,
        input_digest: input.input_digest.clone(),
        status,
        unresolved_work_count: unresolved,
        reason,
        remediation,
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report
}

/// 检查 `sha256:` 加 64 位十六进制字符的格式；不会读取被引用的阶段证据。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
