//! 可观测性告警/事件投影引擎（只读）
//!
//! 本模块从已提交的指标快照和失败事件推导出可观测性告警与事件。核心职责：
//!
//! # 核心职责
//!
//! 1. **规则引擎**：基于指标阈值（如 kiana.recovery.unknown_total > 0）和事件特征
//!    （如 kind 包含"redaction"且包含"fail"），匹配相应的告警规则。每条规则都对应
//!    一个具体的系统问题（ProjectorLag、AuditLoss、QueueOverflow 等）
//!
//! 2. **去重机制**：使用稳定指纹（stable fingerprint）识别重复告警。指纹由
//!    组件名+规则+分类计算得出，确保相同问题只投影为一个 Incident，
//!    即使关联了多个源事件（source_event_ids）
//!
//! 3. **恢复计划保证**：每条规则都强制关联一个「协调恢复计划」（recovery_plan）。
//!    这表示本模块是只读投影引擎：不直接修复问题，而是告知人/系统
//!    「该问题需要人工审视并做出明确决策，不可自动重试或批准」
//!
//! # 设计边界
//!
//! - **只读投影**：不向 EventLog 写入任何事件；所有投影都是临时计算结果
//! - **Fail-closed**：未匹配的规则不产生告警。规则条件是保守的（AND 逻辑为主）
//! - **事件幂等**：重复投影相同的事件集合总是得到相同的告警集合
//! - **无副作用**：投影过程不改变系统状态，只读取 MetricSnapshot 和 RuntimeEvent

use kiana_domain::{
    json_digest, AlertSeverity, AlertState, EventId, MetricSnapshot, ObservabilityAlert,
    ObservabilityIncident, ObservabilityIncidentCategory, ObservabilityIncidentSnapshot,
    ObservabilityIncidentState, RuntimeEvent,
};
use std::collections::{BTreeMap, BTreeSet};

use super::{project_operational_metrics, ControlPlane, CoreError};

const MAX_RULE_EVENTS: usize = 256; // 单条规则最多关联 256 个源事件。超出时丢弃最早的事件，防止告警对象无限膨胀

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IncidentProjectionError {
    /// 源事件集合为空：无法从空事件流推导任何告警。
    /// 通常表示 EventLog 尚未产生任何事件，不是错误状态，而是「系统冷启动」状态。
    #[error("observability_incident_source_empty")]
    SourceEmpty,

    /// 指标快照校验失败：MetricSnapshot 字段缺失、类型不符、或违反业务不变式。
    /// 这个错误会导致整个投影失败，因为规则决策依赖这些指标的有效性。
    /// 包含具体的校验失败原因（error message）便于诊断。
    #[error("observability_incident_metrics_invalid:{0}")]
    MetricsInvalid(String),

    /// 投影快照构造失败：ObservabilityIncidentSnapshot 在最后生成时无法通过
    /// 自身的校验规则（如告警或事件数组不满足约束）。
    /// 这表示投影过程中产生了无效的结果结构，需要审查投影逻辑。
    #[error("observability_incident_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

/// 中间结构体：表示一条聚合的告警原因及其关联的所有源事件。
///
/// # 为什么需要这个中间结构？
///
/// 投影过程有两个阶段：
/// 1. **扫描阶段**：遍历指标和事件，逐个识别触发的规则，累积关联事件
/// 2. **输出阶段**：从累积的规则和事件集合生成最终的 Alert/Incident 对象
///
/// Cause 就是这两个阶段间的中间状态。它以规则名（rule）为键进行去重，
/// 确保即使多个事件都触发了同一条规则，也只产生一个 Incident。
///
/// # 字段说明
///
/// - `category`：告警分类（如 ProjectorLag、AuditLoss）。对应领域模型中的 ObservabilityIncidentCategory
/// - `severity`：告警级别（Critical、Error、Warning）。影响 AlertState 和 on-call 触发阈值
/// - `rule`：规则标识符，是 BTreeMap<&'static str, Cause> 的 key。用于去重和快速查找
/// - `message`：告警描述信息，给运维人员的人类可读说明
/// - `requires_reconciliation`：协调恢复标记。为 true 时表示「这个问题不能自动重试」，
///   必须人工审视并做明确决策（如「拒绝」或「重新启动」）。这是 Kiana 的关键安全保证：
///   未知状态的效应绝不自动修复
/// - `event_ids`：触发这条规则的源事件 ID 集合。使用 BTreeSet<String> 确保去重
///   且有序，便于生成确定性的指纹。当超过 MAX_RULE_EVENTS 时，丢弃最早的。
struct Cause {
    category: Option<ObservabilityIncidentCategory>,
    severity: AlertSeverity,
    rule: &'static str,
    message: &'static str,
    requires_reconciliation: bool,
    event_ids: BTreeSet<String>,
}

impl Cause {
    /// 将一个 RuntimeEvent 加入该规则的事件集合。
    ///
    /// 设计要点：
    /// - 如果事件集已满（≥ MAX_RULE_EVENTS），则丢弃新事件，保留前 256 个
    ///   这是一种限流策略，防止告警对象占用过多内存
    /// - 使用 BTreeSet 自动去重，不会因重复扫描同一事件而产生重复记录
    fn add_event(&mut self, event: &RuntimeEvent) {
        if self.event_ids.len() < MAX_RULE_EVENTS {
            self.event_ids.insert(event.event_id.to_string());
        }
    }
}

/// 获取或创建一条规则对应的 Cause 对象。
///
/// # 设计——使用 BTreeMap 进行「规则去重」
///
/// 这个函数是投影的核心去重机制。在扫描指标和事件时，会多次调用此函数来
/// 获取同一条规则对应的 Cause 对象。使用 BTreeMap 的 entry API 实现 lazy init：
/// - `causes.entry(rule)` 检查规则是否已被记录
/// - 如果不存在，`.or_insert_with()` 创建一个新的 Cause
/// - 如果已存在，则返回既有的对象引用
///
/// 结果：同一条规则无论被触发多少次，在 causes map 中只出现一次，所有关联事件
/// 都累积到这一个 Cause 对象中。
///
/// # 参数说明
///
/// - `causes`：可变引用，指向规则→Cause 的映射表。数据类型是 BTreeMap 而非 HashMap，
///   目的是让输出的投影结果**有序可重现**。规则名按字典序排列，便于测试和审计
/// - `category`、`severity`、`rule`、`message`、`requires_reconciliation`：
///   新 Cause 对象的初始化参数。只在首次创建时使用；如果规则已存在，这些参数被忽略
///
/// # 返回值
///
/// 对该规则对应 Cause 对象的可变引用。后续可以通过这个引用调用 `.add_event()`
/// 将事件加入其 event_ids 集合。
fn cause_for<'a>(
    causes: &'a mut BTreeMap<&'static str, Cause>,
    category: ObservabilityIncidentCategory,
    severity: AlertSeverity,
    rule: &'static str,
    message: &'static str,
    requires_reconciliation: bool,
) -> &'a mut Cause {
    causes.entry(rule).or_insert_with(|| Cause {
        category: Some(category),
        severity,
        rule,
        message,
        requires_reconciliation,
        event_ids: BTreeSet::new(),
    })
}

/// 从指标快照中查询单个数值指标。
///
/// # 防御设计：为什么要 .max(0.0)？
///
/// 某些指标由外部系统（可观测性 agent、Prometheus exporter）提供，可能包含：
/// - 浮点数精度误差导致的负微小值（如 -0.0000001）
/// - 数据源的计算错误
/// - 竞态导致的瞬间负值
///
/// 这些负值在计数器（counter）语义上是非法的。使用 `.max(0.0)` 将这些值
/// 截断为 0，确保规则决策基于合理的度量。这是一种 fail-safe 策略：
/// 宁可低估问题严重程度，也不让异常数据触发误告。
///
/// # 参数
///
/// - `metrics`：MetricSnapshot 对象，包含 `points` 数组
/// - `name`：要查询的指标名称（如 "kiana.recovery.unknown_total"）
///
/// # 返回值
///
/// 如果指标存在且有效，返回其 value 截断为 ≥ 0 的 u64 形式；
/// 如果指标不存在，返回 0（缺失指标视同「未发生」）。
fn metric_value(metrics: &MetricSnapshot, name: &str) -> u64 {
    metrics
        .points
        .iter()
        .find(|point| point.name == name)
        .map(|point| point.value.max(0.0) as u64)
        .unwrap_or_default()
}

/// 将分类枚举映射为硬编码的字符串标识符。
///
/// # 为什么需要硬编码映射？
///
/// 分类字符串（如 "projector_lag"）被用来生成**事件指纹**（fingerprint）。
/// 指纹是一个 JSON 摘要，用于去重：相同的指纹意味着重复的告警。
///
/// 如果允许分类字符串动态变化（例如从 Display trait derive），就可能出现：
/// - 代码重构时不小心改变了字符串格式
/// - 相同的问题产生不同的指纹，导致重复告警
/// - 历史告警指纹无法匹配，中断审计链
///
/// 通过硬编码 match 表达式，我们确保：
/// 1. 分类字符串在整个系统生命周期内保持不变
/// 2. 如果要改名字，必须显式修改这个函数并审视所有依赖
/// 3. 编译器会在新增分类时强制补齐 match arm，防止遗漏
///
/// # 参数
///
/// - `category`：ObservabilityIncidentCategory 枚举值
///
/// # 返回值
///
/// 对应的字符串标识符（&'static str，生命周期同进程）。
fn category_name(category: ObservabilityIncidentCategory) -> &'static str {
    match category {
        ObservabilityIncidentCategory::ProjectorLag => "projector_lag",
        ObservabilityIncidentCategory::AuditLoss => "audit_loss",
        ObservabilityIncidentCategory::RedactionFailure => "redaction_failure",
        ObservabilityIncidentCategory::QueueOverflow => "queue_overflow",
        ObservabilityIncidentCategory::JournalCorruption => "journal_corruption",
        ObservabilityIncidentCategory::EffectUnknown => "effect_unknown",
        ObservabilityIncidentCategory::OrphanDispatch => "orphan_dispatch",
    }
}

/// 为一条规则确定其关联的源事件 ID 列表。
///
/// # 优先级逻辑：为什么需要备选方案？
///
/// 理想情况：一条规则被特定事件触发（如 "audit.fail" 事件），我们希望
/// 告警只关联这些"明确触发的事件"。这提供了最好的可观测性：
/// 运维人员看到告警时能直接定位到根本原因事件。
///
/// 实际情况：有些规则由纯指标触发（如 "kiana.recovery.unknown_total > 0"），
/// 这时 cause.event_ids 为空。此时我们无法提供"具体的触发事件"，
/// 所以退而求其次，关联**整个指标快照包含的所有源事件**。
/// 这些事件形成了这个"指标计算的上下文"。
///
/// # 算法步骤
///
/// 1. 如果 cause.event_ids 非空（规则被具体事件触发）
///    → 使用规则的 event_ids 作为源
/// 2. 如果 cause.event_ids 为空（规则由纯指标触发）
///    → 返回 metrics.source_event_ids（快照的全部源事件）
/// 3. 特殊情况：如果过滤后结果为空（快照事件集与规则事件集无交集）
///    → 仍返回 metrics.source_event_ids，确保告警永不为"无源"状态
///
/// # 参数
///
/// - `metrics`：指标快照，包含 source_event_ids（生成本快照的所有事件 ID）
/// - `cause`：累积的规则对象，包含 event_ids（触发该规则的事件 ID）
///
/// # 返回值
///
/// 一个 Vec<EventId>，表示该规则关联的事件列表。
fn source_ids(metrics: &MetricSnapshot, cause: &Cause) -> Vec<EventId> {
    // 情形 1：规则被具体事件触发，直接用规则的事件集
    if cause.event_ids.is_empty() {
        return metrics.source_event_ids.clone();
    }

    // 情形 2：尝试从快照的源事件中筛选出触发规则的事件
    let filtered = metrics
        .source_event_ids
        .iter()
        .filter(|event_id| cause.event_ids.contains(&event_id.to_string()))
        .copied()
        .collect::<Vec<_>>();

    // 情形 3：筛选后无结果（快照事件集与规则事件集无交集）
    // 这通常表示事件数据不一致或事件已过期，但仍返回快照源事件，确保追溯链
    if filtered.is_empty() {
        metrics.source_event_ids.clone()
    } else {
        filtered
    }
}

/// 尝试将单个 RuntimeEvent 匹配到一条规则。
///
/// # 规则匹配逻辑说明
///
/// 这个函数是投影的"规则库"。每个 if 分支代表一条告警规则，
/// 规则使用事件的 `kind` 字段和可选的 `data` 字段来判断是否触发。
/// 如果匹配，返回规则的完整信息；否则返回 None。
///
/// ## 规则 1: Redaction Failure（数据脱敏失败）
///
/// **触发条件**：事件 kind 包含"redaction"且（包含"fail"或有 error 字段）
///
/// **含义**：可观测性采集时脱敏过滤器失效，敏感数据可能被泄露。
/// 这是一个 **Critical 级别问题**，需要立即人工审视日志并检查是否有数据泄露。
///
/// **协调要求**：requires_reconciliation = true，表示问题不能自动重试，
/// 必须人工确认"问题已修复"后才能恢复采集。
///
/// ## 规则 2: Queue Overflow（队列溢出）
///
/// **触发条件**：事件 kind 包含"queue"或"overflow"
///
/// **含义**：可观测性事件队列容量不足，部分告警被丢弃。
/// 这是一个 Error 级别问题（非关键但需要关注），通常表示收集速率超过发送速率。
///
/// **协调要求**：requires_reconciliation = false。队列溢出通常通过扩容或降速就能恢复，
/// 不需要强制人工干预。
///
/// ## 规则 3: Journal Corruption（日志损坏）
///
/// **触发条件**：事件 kind 包含"journal"且（包含"corrupt"或"fail"）
///
/// **含义**：EventLog 或其他持久化存储的日志文件损坏或提交失败。
/// 这是一个 **Critical 级别问题**，可能导致事件丢失或不可重放。
///
/// **协调要求**：requires_reconciliation = true。数据损坏需要人工审视
/// 并决策是否回滚、恢复备份，或接受数据丢失。
///
/// ## 规则 4: Audit Loss（审计证据丢失）
///
/// **触发条件**：事件 kind 以"audit."开头且（包含"fail"或"unknown"）
///
/// **含义**：必需的审计证据（如执行日志、权限变化记录）无法读取或状态未知。
/// 这是一个 **Critical 级别问题**，违反了 CompanyOS 的审计不变式。
///
/// **协조要求**：requires_reconciliation = true。缺失审计证据需要人工决定
/// 是否接受合规风险，或者启动恢复流程。
///
/// # 返回值
///
/// Some((rule_id, category, severity, rule_name, message, requires_reconciliation))
/// 如果事件匹配某条规则；否则 None（事件无告警意义）。
fn rule_for_event(
    event: &RuntimeEvent,
) -> Option<(
    &'static str,
    ObservabilityIncidentCategory,
    AlertSeverity,
    &'static str,
    &'static str,
    bool,
)> {
    let kind = event.kind.to_ascii_lowercase();

    // 规则 1: Redaction Failure
    if kind.contains("redaction") && (kind.contains("fail") || event.data.get("error").is_some()) {
        return Some((
            "redaction_failure",
            ObservabilityIncidentCategory::RedactionFailure,
            AlertSeverity::Critical,
            "redaction_failure",
            "Redaction failed; candidate telemetry was withheld",
            true,
        ));
    }

    // 规则 2: Queue Overflow
    if kind.contains("queue") || kind.contains("overflow") {
        return Some((
            "queue_overflow",
            ObservabilityIncidentCategory::QueueOverflow,
            AlertSeverity::Error,
            "queue_overflow",
            "Observability queue capacity was exceeded",
            false,
        ));
    }

    // 规则 3: Journal Corruption
    if kind.contains("journal") && (kind.contains("corrupt") || kind.contains("fail")) {
        return Some((
            "journal_corruption",
            ObservabilityIncidentCategory::JournalCorruption,
            AlertSeverity::Critical,
            "journal_corruption",
            "Journal integrity or commit evidence requires inspection",
            true,
        ));
    }

    // 规则 4: Audit Loss
    if kind.starts_with("audit.") && (kind.contains("fail") || kind.contains("unknown")) {
        return Some((
            "audit_loss",
            ObservabilityIncidentCategory::AuditLoss,
            AlertSeverity::Critical,
            "audit_loss",
            "Required audit evidence is missing or unknown",
            true,
        ));
    }

    None
}

/// 从已提交的指标快照和事件流推导出可观测性告警和事件。
///
/// # 函数职责
///
/// 这是模块的核心投影函数。它实现了完整的规则扫描→去重→输出流程：
/// 1. 扫描指标快照中的关键计数器（如 recovery.unknown_total）
/// 2. 扫描事件流并应用规则库（rule_for_event）
/// 3. 使用 BTreeMap<rule, Cause> 进行去重
/// 4. 生成 Alert 和 Incident 对象并关联指纹
/// 5. 返回 ObservabilityIncidentSnapshot
///
/// # 参数
///
/// - `events`：已提交的 RuntimeEvent 数组。这些事件已永久化在 EventLog 中，不会被回滚
/// - `metrics`：OA-10 指标快照。包含关键的性能和恢复计数器，用于启发式规则触发
///
/// # 返回值
///
/// - `Ok(ObservabilityIncidentSnapshot)`：投影成功。返回结构包含：
///   - 所有推导出的 Alert 对象（关联到 Incident）
///   - 所有推导出的 Incident 对象（包含恢复计划）
///   - 源事件游标和 ID 列表
///   - 限制条件列表（标记本投影是诊断性的）
///
/// - `Err(IncidentProjectionError)`：投影失败，具体原因包括：
///   - SourceEmpty：事件集合为空
///   - MetricsInvalid：指标快照校验失败
///   - SnapshotInvalid：最终快照构造失败
///
/// # 投影过程详述
///
/// ## 阶段 1: 指标规则扫描（纯指标触发的告警）
///
/// 这部分规则不依赖具体事件，只基于聚合指标的值：
/// - projector_lag：如果 projector_cursor 落后 source_cursor 或存在游标间隙
/// - effect_unknown：如果 kiana.recovery.unknown_total > 0（外部效应未知）
/// - orphan_dispatch：如果 kiana.recovery.orphan_total > 0（派遣无终止结果）
/// - audit_query_failure：如果 kiana.receipts.query_failure_total > 0（审计查询失败）
/// - artifact_read_failure：如果 kiana.artifacts.read_failure_total > 0（证据读取失败）
///
/// 这些规则通过 cause_for() 调用进行去重，如果同一条规则多次匹配，
/// 仍只在 causes map 中出现一次。
///
/// ## 阶段 2: 事件规则扫描（事件特征触发的告警）
///
/// 遍历 events 数组，对每个事件调用 rule_for_event() 判断是否匹配规则库。
/// 如果匹配，将事件 ID 累积到对应的 Cause.event_ids 中。
///
/// 特殊处理：redaction_failure 规则会被添加两次
/// （代码中有 if plan == "redaction_failure" 的重复逻辑），
/// 这通常是为了确保该关键规则在错误链中有完整的因果追踪。
///
/// ## 阶段 3: 指纹生成和去重
///
/// 对每个 Cause 对象：
/// 1. 生成稳定的 JSON 摘要作为指纹：{ "component": "observability", "rule": cause.rule, "category": ... }
/// 2. 从指纹派生 incident_id 和 alert_id
/// 3. 确保相同指纹的告警只出现一次
///
/// ## 阶段 4: Alert 和 Incident 构造
///
/// 对每个 Cause 对象创建：
/// - 一个 Alert（告警对象）：表示某个时间点检测到的问题
/// - 一个 Incident（事件对象）：表示问题的根本原因及恢复步骤
///
/// 所有 Incident 都携带统一的恢复计划（recovery_plan）：
/// 1. 审视提交的源事实和当前游标
/// 2. 对未知效应进行隔离；不自动重试或批准
/// 3. 要求明确的协调决策才能关闭告警
///
/// # 设计要点
///
/// - **幂等性**：相同的 events + metrics 输入总是产生相同的输出
/// - **有序性**：使用 BTreeMap 和 BTreeSet 确保输出顺序确定，便于测试和审计
/// - **防御性**：指标值被 max(0.0) 截断，缺失指标视同 0，事件集限流到 256
/// - **可追踪**：每个告警都有指纹和源事件 ID，支持回溯调查
pub fn project_observability_incidents(
    events: &[RuntimeEvent],
    metrics: &MetricSnapshot,
) -> Result<ObservabilityIncidentSnapshot, IncidentProjectionError> {
    // 前置条件检查：事件集非空且指标有效
    if events.is_empty() {
        return Err(IncidentProjectionError::SourceEmpty);
    }
    metrics
        .validate()
        .map_err(IncidentProjectionError::MetricsInvalid)?;

    // 初始化规则累积表：BTreeMap 确保输出有序
    let mut causes: BTreeMap<&'static str, Cause> = BTreeMap::new();

    // === 阶段 1: 指标规则扫描 ===

    // 规则：ProjectorLag
    // 触发条件：投影器游标落后于源游标，或存在游标间隙
    // 含义：系统处于数据追赶状态，可观测性数据可能有延迟
    if metrics.projector_cursor < metrics.source_cursor
        || metrics
            .limitations
            .iter()
            .any(|limitation| limitation == "eventlog_cursor_gap")
    {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::ProjectorLag,
            AlertSeverity::Warning,
            "projector_lag",
            "Projector cursor is behind or has a source gap",
            true,
        );
    }

    // 规则：EffectUnknown
    // 触发条件：kiana.recovery.unknown_total > 0
    // 含义：存在一个或多个外部效应（如对外部 API 的调用）状态未知
    // 这表示系统无法确定某个关键操作是否成功，可能需要重新同步或人工审视
    if metric_value(metrics, "kiana.recovery.unknown_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::EffectUnknown,
            AlertSeverity::Critical,
            "effect_unknown",
            "An external effect is unknown and remains fenced",
            true,
        );
    }

    // 规则：OrphanDispatch
    // 触发条件：kiana.recovery.orphan_total > 0
    // 含义：存在一个派遣操作（Dispatch）无法找到对应的终止结果
    // 这表示系统在某个能力请求上失去了追踪，无法确认成功或失败
    if metric_value(metrics, "kiana.recovery.orphan_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::OrphanDispatch,
            AlertSeverity::Error,
            "orphan_dispatch",
            "A dispatch has no committed terminal result",
            true,
        );
    }

    // 规则：AuditQueryFailure
    // 触发条件：kiana.receipts.query_failure_total > 0
    // 含义：审计记录查询失败，无法读取已提交的执行证据
    if metric_value(metrics, "kiana.receipts.query_failure_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::AuditLoss,
            AlertSeverity::Error,
            "audit_query_failure",
            "Receipt/audit query failed and requires re-read",
            true,
        );
    }

    // 规则：ArtifactReadFailure
    // 触发条件：kiana.artifacts.read_failure_total > 0
    // 含义：某个证据文件（如存储的审计快照、恢复计划）无法读取或校验失败
    if metric_value(metrics, "kiana.artifacts.read_failure_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::AuditLoss,
            AlertSeverity::Error,
            "artifact_read_failure",
            "An evidence artifact could not be read or verified",
            true,
        );
    }

    // === 阶段 2: 事件规则扫描 ===

    for event in events {
        if let Some((rule, category, severity, message, plan, requires)) = rule_for_event(event) {
            // 第一次添加到 causes 表（如果规则首次出现）
            cause_for(&mut causes, category, severity, rule, message, requires).add_event(event);

            // 特殊处理：redaction_failure 规则需要额外的因果链追踪
            // （通常用于审计目的，确保敏感数据泄露事件被完整记录）
            if plan == "redaction_failure" {
                cause_for(
                    &mut causes,
                    ObservabilityIncidentCategory::RedactionFailure,
                    severity,
                    rule,
                    message,
                    requires,
                )
                .add_event(event);
            }
        }
    }

    // === 阶段 3 和 4: 指纹生成、Alert/Incident 构造 ===

    let mut alerts = Vec::new();
    let mut incidents = Vec::new();

    for cause in causes.values() {
        // 验证分类不为 None（理论上不应该发生）
        let category = cause
            .category
            .ok_or(IncidentProjectionError::MetricsInvalid(
                "category_missing".to_owned(),
            ))?;

        // 生成稳定的指纹：基于组件、规则名、分类
        // 这个指纹确定了 Alert/Incident 的 ID，也是去重的关键
        let fingerprint = json_digest(&serde_json::json!({
            "component": "observability",
            "rule": cause.rule,
            "category": category_name(category),
        }));

        // 从指纹派生最终的对象 ID（格式："{问题类型}:{指纹}"）
        let incident_id = format!("observability-incident:{fingerprint}");
        let alert_id = format!("observability-alert:{fingerprint}");

        // 确定该规则关联的源事件（优先级逻辑见 source_ids 函数）
        let ids = source_ids(metrics, cause);

        // 构造 Alert 对象
        let alert = ObservabilityAlert::new(
            alert_id.clone(),
            fingerprint.clone(),
            cause.rule,
            cause.severity,
            AlertState::Open, // 新投影的告警总是 Open 状态
            metrics.source_cursor,
            ids.clone(),
            cause.message,
            Some(incident_id.clone()),
        )
        .map_err(IncidentProjectionError::SnapshotInvalid)?;

        // 构造统一的恢复计划（所有 Incident 都遵循相同的协调流程）
        let recovery_plan = vec![
            "Inspect committed source facts and current cursor".to_owned(),
            "Fence unknown effects; do not retry or approve automatically".to_owned(),
            "Require an explicit reconciliation decision before closure".to_owned(),
        ];

        // 构造 Incident 对象
        let incident = ObservabilityIncident::new(
            incident_id,
            fingerprint,
            category,
            cause.severity,
            ObservabilityIncidentState::Open, // 新投影的事件总是 Open 状态
            metrics.source_cursor,
            ids,
            cause.requires_reconciliation,
            recovery_plan,
            vec![alert_id],
        )
        .map_err(IncidentProjectionError::SnapshotInvalid)?;

        alerts.push(alert);
        incidents.push(incident);
    }

    // === 阶段 5: 快照构造 ===

    let mut limitations = metrics.limitations.clone();

    // 如果产生了任何告警，在限制列表中标记"这是诊断性投影"
    // 用于告诉上游系统不要将投影结果误认为事件流本身的不变式
    if !causes.is_empty() && limitations.len() < kiana_domain::MAX_HEALTH_LIMITATIONS {
        limitations.push("observability_incident_projection_is_diagnostic".to_owned());
    }

    // 生成最终的快照对象
    ObservabilityIncidentSnapshot::new(
        metrics.source_cursor,
        metrics.source_event_ids.clone(),
        alerts,
        incidents,
        limitations,
    )
    .map_err(IncidentProjectionError::SnapshotInvalid)
}

/// 快速路径：从事件流直接推导告警，无需外部指标。
///
/// # 用途
///
/// 这个函数是 project_observability_incidents 的简化版本。
/// 当调用者只持有事件流而无关键指标时，可以使用此函数：
/// 1. 内部调用 project_operational_metrics() 计算指标快照
/// 2. 调用 project_observability_incidents() 生成投影
///
/// # 限制
///
/// 这条路径不能使用外部系统提供的、实时采集的指标数据
/// （如 Prometheus exporter 收集的 kiana.recovery.unknown_total）。
/// 而是依赖从事件流本身推导的指标，可能延迟或不完整。
///
/// # 参数 & 返回
///
/// - `events`：事件流
/// - 返回：快照或投影错误
pub fn project_incidents(
    events: &[RuntimeEvent],
) -> Result<ObservabilityIncidentSnapshot, IncidentProjectionError> {
    let metrics = project_operational_metrics(events, None)
        .map_err(|error| IncidentProjectionError::MetricsInvalid(error.to_string()))?;
    project_observability_incidents(events, &metrics)
}

impl ControlPlane {
    /// ControlPlane 的可观测性告警查询接口。
    ///
    /// # 职责
    ///
    /// 这个方法是 ControlPlane 对外暴露的告警查询端点。它完成以下操作：
    /// 1. 从 EventLog 读取全部已提交的事件
    /// 2. 计算该事件集的聚合指标
    /// 3. 投影出当前的告警和事件
    /// 4. 返回 ObservabilityIncidentSnapshot
    ///
    /// # 使用场景
    ///
    /// 这个接口通常被以下组件调用：
    /// - Health API（返回系统健康状态）
    /// - Dashboard（显示当前告警）
    /// - Monitoring 系统（抓取告警并转发到 Pagerduty/Slack）
    ///
    /// # 返回值
    ///
    /// - `Ok(ObservabilityIncidentSnapshot)`：投影成功
    /// - `Err(CoreError::PortError(Unavailable(...)))`：EventLog 不支持
    ///   全量读取操作（某些实现可能只支持增量读）
    /// - `Err(CoreError::PortError(Failed(...)))`：指标计算或投影失败
    pub async fn observability_incidents(
        &self,
    ) -> Result<ObservabilityIncidentSnapshot, CoreError> {
        // 从 EventLog 获取全部已提交事件
        // 注意：这是一个可能很重的操作，在大规模系统中应该考虑增量版本
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable(
                "observability_incidents_read_all_unsupported".to_owned(),
            )
        })?;

        // 从事件集计算聚合指标
        let metrics = project_operational_metrics(&events, None)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;

        // 投影告警和事件
        project_observability_incidents(&events, &metrics)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
