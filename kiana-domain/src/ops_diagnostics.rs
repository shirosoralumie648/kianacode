//! 运维状态（status）、诊断（doctor）和预检（preflight）的只读报告合同，属于 `kiana-domain`。
//!
//! 【预期系统位置与数据流】
//! ```text
//! 外部事实汇总者提供 config/startup/health/observability 四轴状态
//!     -> OpsDiagnosticsInput + OpsDiagnosticFact
//!     -> 本文件校验顺序、游标、脱敏文本和摘要
//!     -> OpsDiagnosticsReport::evaluate() 归并状态/退出码/复现提示
//!     -> Core -> DaemonHost 的只读路由
//!     -> 未来 CLI/Workbench/Web 等入口渲染或消费报告
//! ```
//!
//! 当前没有生产代码采集/组装这些事实或调用宿主路由；“server-owned facts”是预期上游责任，不是 DTO 能证明的来源。
//! 本模块不读配置文件/环境变量/EventLog，不探测 provider，不修复事实，不执行 remediation，也不修改准入状态。
//! 报告中的退出码只是数值字段，不会在这里调用 `std::process::exit`。缺失和未知会保留在结果中，避免被伪装为健康。
//!
//! Serde 将带版本 DTO 与 JSON 等外部格式互转：序列化把 Rust 值写成 JSON，反序列化把 JSON 读回 Rust 值。
//! `deny_unknown_fields` 会拒绝当前结构未声明的字段，避免新旧版本对额外数据产生不同解释。

use crate::{
    json_digest, redact_text, scan_secret_sentinels, QualityReproductionCommand, SchemaVersion,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 运维诊断 input 的稳定 wire schema 名称。
pub const OPS_DIAGNOSTICS_INPUT_SCHEMA: &str = "kiana.ops-diagnostics-input.v1";
/// 单个诊断事实的稳定 wire schema 名称。
pub const OPS_DIAGNOSTIC_FACT_SCHEMA: &str = "kiana.ops-diagnostic-fact.v1";
/// 聚合报告的稳定 wire schema 名称。
pub const OPS_DIAGNOSTICS_REPORT_SCHEMA: &str = "kiana.ops-diagnostics-report.v1";
/// 本模块 DTO 当前使用的 schema 版本。
pub const OPS_DIAGNOSTICS_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 当前 input 必须有四轴事实，所以这个上限暂不影响有效输入数量；为上层结构提供有限边界。
pub const OPS_DIAGNOSTICS_MAX_FACTS: usize = 8;
/// 调用方最多可随 input 提供的限制说明条数。
pub const OPS_DIAGNOSTICS_MAX_LIMITATIONS: usize = 16;
/// reason/remediation/limitation 等解释文本的最大 UTF-8 字节数。
pub const OPS_DIAGNOSTICS_MAX_TEXT: usize = 512;

/// 报告的使用模式；当前三个模式共用同一组 facts 和状态归并规则。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsDiagnosticsMode {
    /// 汇总当前运行状态；此处只决定报告显示的模式名。
    Status,
    /// 执行深度诊断的命令名称；当前 reducer 与另外两种模式使用相同事实和规则。
    Doctor,
    /// 执行操作前检查的命令名称；当前 reducer 与另外两种模式使用相同事实和规则。
    Preflight,
}

impl OpsDiagnosticsMode {
    /// 返回稳定的小写 wire/命令参数名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Doctor => "doctor",
            Self::Preflight => "preflight",
        }
    }
}

/// 四类运维诊断事实的固定轴；声明顺序也是 input 要求的顺序。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsDiagnosticAxis {
    /// 配置是否可用。
    Config,
    /// 进程启动阶段是否成功。
    Startup,
    /// 服务健康状况。
    Health,
    /// 可观测性组件是否可用。
    Observability,
}

impl OpsDiagnosticAxis {
    /// input 必须恰好包含的四轴顺序，供上游装配和校验器共同使用。
    pub const ALL: [Self; 4] = [
        Self::Config,
        Self::Startup,
        Self::Health,
        Self::Observability,
    ];

    /// 为 input 的固定顺序校验返回 1..=4 的序号；该序号不是数据库 ID。
    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Config => 1,
            Self::Startup => 2,
            Self::Health => 3,
            Self::Observability => 4,
        }
    }

    /// 返回稳定的小写轴名，供人类渲染使用。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Startup => "startup",
            Self::Health => "health",
            Self::Observability => "observability",
        }
    }
}

/// 单个轴上的状态；Missing 与 Unknown 分开，以区分“没有证据”和“证据存在但无法判定”。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsAxisStatus {
    /// 事实声明该轴当前可用，并按合同提供证据摘要。
    Ready,
    /// 仍可工作，但存在需要运维人员关注的问题。
    Degraded,
    /// 事实声明该轴被阻断，或证据缺失使其按聚合规则视为阻断。
    Blocked,
    /// 已有事实无法确定该轴状态；聚合时优先于其他状态，防止未知被当成健康。
    Unknown,
    /// 当前没有该轴的证据；与 Unknown 不同，Missing 表示证据不存在而非证据含糊。
    Missing,
}

impl OpsAxisStatus {
    /// 返回稳定的小写状态文本。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
            Self::Missing => "missing",
        }
    }
}

/// 四轴归并后的总状态；派生优先级由 `derive_status()` 决定。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsDiagnosticsStatus {
    /// 四轴均没有 Degraded、Blocked、Unknown 或 Missing。
    Ready,
    /// 至少一轴降级，且没有更高优先级的 Unknown/Blocked/Missing。
    Degraded,
    /// 至少一轴 Blocked 或 Missing，且没有 Unknown。
    Blocked,
    /// 至少一轴 Unknown；它是聚合优先级最高的结果。
    Unknown,
}

impl OpsDiagnosticsStatus {
    /// 返回稳定的小写状态文本。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    /// 返回报告约定的数值退出码；该方法只返回数字，不会设置当前进程退出状态。
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::Ready => 0,
            Self::Degraded => 10,
            Self::Blocked => 20,
            Self::Unknown => 30,
        }
    }

    /// 只有全部归并为 Ready 才视为健康；Degraded 也返回 false。
    pub const fn is_healthy(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// 某一个诊断轴上的单条事实声明。
///
/// `evidence_digest` 为空允许 Missing 事实使用；它只检查摘要字符串格式，不回读 evidence 对象，也不认证来源。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsDiagnosticFact {
    /// 固定 fact schema 名称。
    pub schema: String,
    /// fact schema 版本。
    pub version: SchemaVersion,
    /// 本事实覆盖 config/startup/health/observability 中的哪个轴。
    pub axis: OpsDiagnosticAxis,
    /// Ready/Degraded/Blocked/Unknown/Missing 分类。
    pub status: OpsAxisStatus,
    /// 上游事实源 cursor；必须非零，并需与同一 input 的 cursor 完全一致。
    pub source_cursor: u64,
    /// 可选的证据摘要引用；Ready 必须提供，Missing 必须为空，其他非 Ready 状态允许有或没有。
    pub evidence_digest: Option<String>,
    /// 稳定原因文本；必须通过路径、secret marker 和 redaction 一致性检查。
    pub reason: String,
    /// 建议处理方式文本；本模块只展示，不执行 remediation。
    pub remediation: String,
    /// 除自身字段外本 fact 的 SHA-256 内容摘要。
    pub fact_digest: String,
}

impl OpsDiagnosticFact {
    /// 创建一条指定 axis/status 的 fact，填入 schema/version、计算摘要并校验状态字段组合。
    ///
    /// 【调用者】预期由上游配置/启动/健康/观测事实汇总器创建；当前生产源码没有调用者，Domain tests 直接构造 fixture。
    /// 【输入/输出】调用方提供轴、状态、source cursor、可选 evidence digest、reason 和 remediation；合法时返回 DTO，
    /// 非法状态组合、文本、游标或摘要时返回稳定 `String` 错误。
    /// 【副作用/限制】只创建内存对象；不加载证据、不确认 facts 属于该轴，也不运行 remediation。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        axis: OpsDiagnosticAxis,
        status: OpsAxisStatus,
        source_cursor: u64,
        evidence_digest: Option<String>,
        reason: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: OPS_DIAGNOSTIC_FACT_SCHEMA.to_owned(),
            version: OPS_DIAGNOSTICS_VERSION,
            axis,
            status,
            source_cursor,
            evidence_digest,
            reason: reason.into(),
            remediation: remediation.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    /// 校验 fact header、可选 evidence digest、安全文本、status 与 evidence/reason/remediation 的组合及自摘要。
    ///
    /// Ready 必须有 evidence 且使用 `ok`/`none`；Missing 必须没有 evidence；其他非 Ready 状态只要求非默认解释。
    /// 通过不代表 evidence 对象存在或 status 是从真实系统状态计算的。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_DIAGNOSTIC_FACT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.source_cursor == 0
        {
            return Err("ops_diagnostic_fact_header_invalid".to_owned());
        }
        if let Some(digest) = &self.evidence_digest {
            valid_digest(digest, "ops_diagnostic_evidence_digest")?;
        }
        safe_text(&self.reason, "ops_diagnostic_reason")?;
        safe_text(&self.remediation, "ops_diagnostic_remediation")?;
        match self.status {
            OpsAxisStatus::Ready => {
                if self.evidence_digest.is_none()
                    || self.reason != "ok"
                    || self.remediation != "none"
                {
                    return Err("ops_diagnostic_ready_evidence_invalid".to_owned());
                }
            }
            OpsAxisStatus::Degraded | OpsAxisStatus::Blocked | OpsAxisStatus::Unknown => {
                if self.reason == "ok" || self.remediation == "none" {
                    return Err("ops_diagnostic_non_ready_explanation_missing".to_owned());
                }
            }
            OpsAxisStatus::Missing => {
                if self.evidence_digest.is_some()
                    || self.reason == "ok"
                    || self.remediation == "none"
                {
                    return Err("ops_diagnostic_missing_evidence_invalid".to_owned());
                }
            }
        }
        valid_digest(&self.fact_digest, "ops_diagnostic_fact_digest")?;
        if self.fact_digest != self.digest() {
            return Err("ops_diagnostic_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 axis/status、cursor、证据摘要和解释文本计算内容摘要，不包含 `fact_digest` 自身。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "axis": self.axis,
            "status": self.status,
            "source_cursor": self.source_cursor,
            "evidence_digest": self.evidence_digest,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// 汇总一次 status/doctor/preflight 检查所需的调用方事实。
///
/// 【系统位置】这是事实生产者交给 Domain reducer 的输入合同。当前仓库没有生产事实采集器；上游必须先组装四轴 facts，
/// 再交给 DaemonHost 路由，经 Core 转到 `OpsDiagnosticsReport::evaluate()`。
///
/// 【数据流】调用方给出模式、共享 cursor、观察时间、四条有序 fact 和限制说明 → 本结构计算自摘要并校验 → reducer 生成 report。
/// `observed_at_ms` 只被检查为非零并纳入 input digest；它没有与系统时钟或 EventLog 时间比较。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsDiagnosticsInput {
    /// 稳定的 input wire schema 名称；反序列化后必须与当前版本常量完全相同。
    pub schema: String,
    /// DTO 的 schema 版本，防止不同合同版本被当成同一种输入。
    pub version: SchemaVersion,
    /// 报告请求的命令语义；当前只用于绑定/回显和生成复现命令，不切换不同诊断算法。
    pub mode: OpsDiagnosticsMode,
    /// 四条事实共同引用的非零来源游标；本结构只做数值相等校验，不查询该游标对应的事件。
    pub source_cursor: u64,
    /// 调用方声称的观察时间（Unix 毫秒语义）；这里只要求非零并进入摘要，不验证时钟来源或新鲜度。
    pub observed_at_ms: u64,
    /// 恰好四条事实，顺序固定为 Config、Startup、Health、Observability；构造器不会替调用方排序。
    pub facts: Vec<OpsDiagnosticFact>,
    /// 调用方附带的限制说明，最多 16 条；报告会保留这些文字并另加两条固定限制。
    pub limitations: Vec<String>,
    /// 对除本字段外 input 内容计算的 SHA-256 摘要；用于发现内容变化，不是签名或来源认证。
    pub input_digest: String,
}

impl OpsDiagnosticsInput {
    /// 组装 input，先按传入顺序写入字段，再计算摘要并调用 `validate()`。
    ///
    /// 【调用者】预期由配置、启动、健康和观测事实汇总器调用；当前没有生产调用者，已有 Domain 测试直接构造 fixture。
    /// 【输入/输出】接收模式、共享 cursor、观察时间、facts 和限制说明；合法时返回可交给 Core/DaemonHost 的 DTO。
    /// 【失败情况】任何 header/时间/cursor/fact 顺序/文本/摘要不符合合同都会返回稳定 `String` 错误；不会自动补轴或排序。
    /// 【副作用与设计原因】只分配内存并算摘要。要求调用方保留固定轴顺序，可以让输入和报告稳定比较、稳定序列化。
    pub fn new(
        mode: OpsDiagnosticsMode,
        source_cursor: u64,
        observed_at_ms: u64,
        facts: Vec<OpsDiagnosticFact>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut input = Self {
            schema: OPS_DIAGNOSTICS_INPUT_SCHEMA.to_owned(),
            version: OPS_DIAGNOSTICS_VERSION,
            mode,
            source_cursor,
            observed_at_ms,
            facts,
            limitations,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate()?;
        Ok(input)
    }

    /// 验证 input 的 wire 版本、四轴完整性、固定顺序、共享 cursor、说明文本和自摘要。
    ///
    /// 【核心流程】先拒绝错误 header/零时间/数量边界；逐条验证 fact 后检查每条 fact 的 cursor 与 input 相同、轴序号递增且不重复；
    /// 再验证轴集合、限制文本和 input digest。
    /// 【限制】成功仅说明 DTO 内部满足结构合同。cursor、时间和 evidence digest 都没有向 EventLog、时钟或证据存储回查。
    /// 【失败处理】返回首个检测到的结构化错误字符串；Core 原样传播，Daemon adapter 再包装成 `PortError::Failed`。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_DIAGNOSTICS_INPUT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.source_cursor == 0
            || self.observed_at_ms == 0
            || self.facts.len() != OpsDiagnosticAxis::ALL.len()
            || self.facts.len() > OPS_DIAGNOSTICS_MAX_FACTS
            || self.limitations.len() > OPS_DIAGNOSTICS_MAX_LIMITATIONS
        {
            return Err("ops_diagnostics_input_header_invalid".to_owned());
        }
        let mut axes = BTreeSet::new();
        // expected_ordinal 表示当前期望的轴位置（1 到 4），不是事实数量或持久化 ID。
        // 使用显式序号而非只比较集合，确保反序列化得到的 input 也必须采用稳定顺序。
        let mut expected_ordinal = 1;
        for fact in &self.facts {
            fact.validate()?;
            if fact.source_cursor != self.source_cursor
                || fact.axis.ordinal() != expected_ordinal
                || !axes.insert(fact.axis)
            {
                return Err("ops_diagnostics_fact_order_or_cursor_invalid".to_owned());
            }
            expected_ordinal += 1;
        }
        if axes.len() != OpsDiagnosticAxis::ALL.len() {
            return Err("ops_diagnostics_axis_set_invalid".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "ops_diagnostics_limitation")?;
        }
        valid_digest(&self.input_digest, "ops_diagnostics_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("ops_diagnostics_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 input 的业务字段计算摘要，不包含 `input_digest` 自身。
    ///
    /// 观察时间和 limitations 都参与摘要，因此它们在构造后发生变化会导致 `validate()` 失败。
    /// 这是无密钥 SHA-256 内容指纹；能发现意外篡改，但不能证明谁生成了这些事实。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "mode": self.mode,
            "source_cursor": self.source_cursor,
            "observed_at_ms": self.observed_at_ms,
            "facts": self.facts,
            "limitations": self.limitations,
        }))
    }
}

/// 把四轴输入事实归并为可供运维入口展示的只读报告。
///
/// 【系统位置】Domain reducer 的输出 DTO，经 Core 和 DaemonHost 路由返回给未来的 CLI/Workbench/Web 消费者。
/// 当前仓库有路由定义，但没有生产 facts 组装器或入口消费者；生成报告不代表真实诊断已经执行。
///
/// 【重要绑定】`validate_against()` 会绑定模式、cursor、facts 和派生状态，但不保存 input digest/观察时间；
/// `validate_digest_only()` 面向无 input 的渲染，只检查报告自洽，不足以重新验证四轴顺序及 cursor 关系。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsDiagnosticsReport {
    /// 稳定的 report wire schema 名称。
    pub schema: String,
    /// DTO 的 schema 版本。
    pub version: SchemaVersion,
    /// 从输入回显的命令模式；不表示不同模式跑了不同检查。
    pub mode: OpsDiagnosticsMode,
    /// 由四轴状态按固定优先级归并得到的整体状态。
    pub status: OpsDiagnosticsStatus,
    /// 仅当整体状态为 Ready 时为 true，与 status 保持派生一致。
    pub healthy: bool,
    /// 与整体状态对应的约定退出码（Ready=0、Degraded=10、Blocked=20、Unknown=30）；只是报告数据。
    pub exit_code: i32,
    /// 从输入回显的事实 cursor；只表示 DTO 内部引用值，不证明事实已持久化。
    pub source_cursor: u64,
    /// 从输入复制的四轴事实；报告会展示 reason/remediation，不会据此执行操作。
    pub diagnostics: Vec<OpsDiagnosticFact>,
    /// 输入限制说明加上 reducer 固定附加的两条只读/证据来源限制。
    pub limitations: Vec<String>,
    /// 固定生成的脱敏复现命令描述；render 时只把它写进报告，不会执行命令。
    pub reproduction: QualityReproductionCommand,
    /// 对报告业务字段的无密钥 SHA-256 摘要，不包括自身。
    pub report_digest: String,
}

impl OpsDiagnosticsReport {
    /// 校验 input 并生成只读聚合报告。
    ///
    /// 【调用者】经 `kiana_core::evaluate_ops_diagnostics()` 和 Daemon adapter 的静态路由调用；目前没有已发现的生产事实生产者。
    /// 【输入/输出】读取完整 `OpsDiagnosticsInput`，成功时返回复制 facts、整体状态、固定退出码、限制和命令描述的 report。
    /// 【核心流程】验证 input → 按 Unknown、Blocked/Missing、Degraded、Ready 的优先级归并 → 附加两条限制 → 生成复现命令描述 →
    /// 计算 report digest → 用 `validate_against()` 校验输出与 input 的绑定。
    /// 【副作用/失败】没有文件、网络、进程或 EventLog I/O。输入无效或固定命令合同校验失败时返回 `String` 错误；不会重试或修复事实。
    /// 【设计原因】将诊断状态归并保持为纯函数，便于不同入口共用相同规则；代价是事实采集和真实性必须由上游另行负责。
    pub fn evaluate(input: &OpsDiagnosticsInput) -> Result<Self, String> {
        input.validate()?;
        let status = derive_status(&input.facts);
        let mut limitations = input.limitations.clone();
        limitations.push("read_only_diagnostics_no_fact_mutation".to_owned());
        limitations.push("adapter_supplied_evidence_source_only".to_owned());
        let reproduction = reproduction_command(input.mode)?;
        let mut report = Self {
            schema: OPS_DIAGNOSTICS_REPORT_SCHEMA.to_owned(),
            version: OPS_DIAGNOSTICS_VERSION,
            mode: input.mode,
            status,
            healthy: status.is_healthy(),
            exit_code: status.exit_code(),
            source_cursor: input.source_cursor,
            diagnostics: input.facts.clone(),
            limitations,
            reproduction,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(input)?;
        Ok(report)
    }

    /// 检查已生成的 report 是否仍绑定给定 input，并验证 report digest。
    ///
    /// 【调用者】`evaluate()` 在返回前调用；Core 的 `validate_ops_diagnostics()` 也可以把它暴露给上层复核。
    /// 【绑定内容】校验 input、模式、cursor、facts、派生 status/healthy/exit_code、复现命令和 report 自摘要。
    /// 【容易误读的限制】limitations 只比较数量（输入条数 + 2），不逐项比较输入文字和两条固定文字；report 也没有保存 input digest 或
    /// observed_at_ms，因此两者不受此处绑定。SHA-256 未加密钥，能重算摘要的人可以构造另一份自洽 report。
    /// 【失败处理】返回 report binding、文本或 digest 错误；本函数不记录日志、不重试，也不改变外部系统状态。
    pub fn validate_against(&self, input: &OpsDiagnosticsInput) -> Result<(), String> {
        input.validate()?;
        let expected_reproduction = reproduction_command(self.mode)?;
        if self.schema != OPS_DIAGNOSTICS_REPORT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.mode != input.mode
            || self.source_cursor != input.source_cursor
            || self.diagnostics != input.facts
            || self.status != derive_status(&input.facts)
            || self.healthy != self.status.is_healthy()
            || self.exit_code != self.status.exit_code()
            || self.limitations.len() != input.limitations.len() + 2
            || self.reproduction != expected_reproduction
        {
            return Err("ops_diagnostics_report_binding_invalid".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "ops_diagnostics_report_limitation")?;
        }
        if self.report_digest != self.digest() {
            return Err("ops_diagnostics_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 report 中除 `report_digest` 外的业务字段计算内容摘要。
    ///
    /// input 的 `observed_at_ms` 和 `input_digest` 不在 report 字段中，因此不会被此摘要覆盖；report digest 也不是带密钥签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "mode": self.mode,
            "status": self.status,
            "healthy": self.healthy,
            "exit_code": self.exit_code,
            "source_cursor": self.source_cursor,
            "diagnostics": self.diagnostics,
            "limitations": self.limitations,
            "reproduction": self.reproduction,
        }))
    }

    /// 校验报告当前的自洽性后序列化为格式化 JSON。
    ///
    /// 【调用者】预期由 CLI/UI 等展示层调用；当前生产代码尚未发现此方法的调用者。
    /// 【输入/输出】使用 Serde 把 report DTO 编码成多行 JSON 字符串。
    /// 【失败情况】先运行不带 input 的 `validate_digest_only()`；报告自身校验失败或 JSON 编码失败时返回稳定错误字符串。
    /// 【限制】这里无法重新执行 `validate_against(input)`，所以它不能证明 report 来源于哪一份 input。
    pub fn render_json(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "ops_diagnostics_json_encode_failed".to_owned())
    }

    /// 将报告状态、各轴解释、复现命令和限制说明拼成供人阅读的文本。
    ///
    /// 【调用者】预期由终端或 UI 展示层调用；当前生产代码尚未发现此方法的调用者。
    /// 【核心流程】先执行与 JSON 渲染相同的无 input 自校验，再按字段顺序格式化每个 fact，最后渲染固定命令描述并合并限制。
    /// 【失败情况】report/fact 自校验失败，或复现命令对象拒绝渲染时返回错误；没有命令执行、网络请求或 remediation 副作用。
    pub fn render_human(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        let mut lines = vec![
            format!("mode: {}", self.mode.as_str()),
            format!("status: {}", self.status.as_str()),
            format!("healthy: {}", self.healthy),
            format!("exit_code: {}", self.exit_code),
            format!("source_cursor: {}", self.source_cursor),
        ];
        for fact in &self.diagnostics {
            lines.push(format!(
                "{}: {} ({}) remediation={}",
                fact.axis.as_str(),
                fact.status.as_str(),
                fact.reason,
                fact.remediation
            ));
        }
        lines.push(format!(
            "reproduction: {}",
            self.reproduction
                .render()
                .map_err(|_| "ops_diagnostics_reproduction_render_failed".to_owned())?
        ));
        lines.push(format!("limitations: {}", self.limitations.join("; ")));
        Ok(lines.join("\n"))
    }

    /// 供渲染器使用的有限自校验；调用点没有原始 input 可供比较。
    ///
    /// 检查 schema/version、facts 数量、派生状态/布尔/退出码、命令描述、每条 fact、限制文本和摘要。
    /// ⚠ 不要把这个结果当作 input 绑定证明：它没有检查四轴各自唯一且有序，也没有检查每条 fact 的 cursor 与 report cursor 相等。
    /// 若外部能重算无密钥 report digest，形状仍符合这里所检查条件的报告可以通过渲染。
    fn validate_digest_only(&self) -> Result<(), String> {
        let expected_reproduction = reproduction_command(self.mode)?;
        if self.schema != OPS_DIAGNOSTICS_REPORT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.diagnostics.len() != OpsDiagnosticAxis::ALL.len()
            || self.status != derive_status(&self.diagnostics)
            || self.healthy != self.status.is_healthy()
            || self.exit_code != self.status.exit_code()
            || self.reproduction != expected_reproduction
        {
            return Err("ops_diagnostics_report_shape_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "ops_diagnostics_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("ops_diagnostics_report_digest_mismatch".to_owned());
        }
        for fact in &self.diagnostics {
            fact.validate()?;
        }
        for limitation in &self.limitations {
            safe_text(limitation, "ops_diagnostics_report_limitation")?;
        }
        self.reproduction
            .validate()
            .map_err(|_| "ops_diagnostics_reproduction_invalid".to_owned())
    }
}

/// 用固定优先级把四轴 fact 转换成一个整体状态。
///
/// Unknown > Blocked/Missing > Degraded > Ready。未知优先于已知阻断，是因为只要有一轴无法判断，
/// 就不能向调用方声称整体状态已被充分确定；Missing 视作整体 Blocked，避免缺少证据时得到健康结论。
/// 调用前通常已由 input 校验保证恰好四轴；本私有函数本身不检查数量或轴唯一性。
fn derive_status(facts: &[OpsDiagnosticFact]) -> OpsDiagnosticsStatus {
    if facts
        .iter()
        .any(|fact| fact.status == OpsAxisStatus::Unknown)
    {
        OpsDiagnosticsStatus::Unknown
    } else if facts
        .iter()
        .any(|fact| matches!(fact.status, OpsAxisStatus::Blocked | OpsAxisStatus::Missing))
    {
        OpsDiagnosticsStatus::Blocked
    } else if facts
        .iter()
        .any(|fact| fact.status == OpsAxisStatus::Degraded)
    {
        OpsDiagnosticsStatus::Degraded
    } else {
        OpsDiagnosticsStatus::Ready
    }
}

/// 生成报告中展示的固定复现命令对象，不启动子进程。
///
/// 命令程序固定为 `kiana`，参数来自 mode 并带 `--json`，环境变量列表为空，工作目录使用脱敏占位值。
/// `QualityReproductionCommand::validate()` 负责验证其命令合同和 digest；这里失败会向 evaluate/渲染调用者传播。
fn reproduction_command(mode: OpsDiagnosticsMode) -> Result<QualityReproductionCommand, String> {
    let mut command = QualityReproductionCommand {
        schema: crate::QUALITY_REPRODUCTION_SCHEMA.to_owned(),
        program: "kiana".to_owned(),
        args: vec![
            "ops".to_owned(),
            mode.as_str().to_owned(),
            "--json".to_owned(),
        ],
        env_keys: Vec::new(),
        cwd: "[REDACTED_PATH]".to_owned(),
        command_digest: String::new(),
    };
    command.command_digest = command.canonical_digest();
    command.validate()?;
    Ok(command)
}

/// 对面向报告的用户可见文本做有限的路径/秘密标记检查。
///
/// 先限制非空和 UTF-8 字节长度，再拦截控制换行、常见绝对路径/路径穿越形式，要求通用 redactor 不再改变字符串，
/// 最后使用 Receipt 通道扫描秘密哨兵。该检查只能约束文本外形，不能证明上游事实真实，也不是任意秘密检测器。
/// `field` 只用于构造稳定错误码后缀，不参与文本检查。
fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > OPS_DIAGNOSTICS_MAX_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("file://")
        || value.split_whitespace().any(|token| {
            token.starts_with('/')
                || token.starts_with('\\')
                || (token.len() >= 3
                    && token.as_bytes()[0].is_ascii_alphabetic()
                    && token.as_bytes()[1] == b':'
                    && matches!(token.as_bytes()[2], b'/' | b'\\'))
        })
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

/// 检查摘要是否符合 `sha256:` 加 64 位十六进制字符的外形。
///
/// 本函数不重算摘要、不验证输入来源，也不提供签名认证；实际内容比较由各 DTO 的 `validate()` 完成。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
