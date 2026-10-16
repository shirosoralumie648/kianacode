//! EQ-46 版本分桶 drift（指标偏移）度量和告警证据的数据合同。
//!
//! 上游先产生只读 [`DriftReport`]，再由 [`DriftEvaluationInput`] 携带目标摘要、阈值、来源游标和
//! 调用方提供的 route/grant 前后摘要交给 [`evaluate_drift()`]。本文件据此计算每个版本桶的错误率、
//! 平均耗时和裁决；超阈值时返回 `drift.alerted` 事件形状，但不负责把事件写入事实日志：
//!
//! ```text
//! 已有 DriftReport + 阈值/目标/游标/前后摘要
//!     -> validate 输入合同
//!     -> DriftBucket 转为有界 DriftMetricBucket
//!     -> Healthy / NeedsReview / Alerted
//!     -> metrics；必要时再产生 DriftAlert 和 DriftAlertEvent
//!     -> Core 适配器返回 append-ready 值
//! ```
//!
//! 本文件是纯计算层，不读取实时监控流、不查询目标/route/grant 的权威存储、不持久化告警，也不执行
//! 路由或权限变更。摘要相等只验证调用者交来的字符串相等，不能证明这些摘要确实来自同一权威快照。
//! 因此返回 `Alerted` 表示输入数据按当前阈值计算超限，不表示已通知用户或线上质量已经恶化。

use crate::{json_digest, DriftAlertId, DriftReport};
use serde::{Deserialize, Serialize};

pub const DRIFT_INPUT_SCHEMA: &str = "kiana.quality-drift-input.v1";
pub const DRIFT_METRICS_SCHEMA: &str = "kiana.quality-drift-metrics.v1";
pub const DRIFT_ALERT_SCHEMA: &str = "kiana.quality-drift-alert.v1";
pub const DRIFT_ALERT_EVENT_SCHEMA: &str = "kiana.drift-alerted-event.v1";
pub const DRIFT_ALERT_EVENT_KIND: &str = "drift.alerted";
/// `DriftMetrics::validate()` 接受的版本桶上限；仅有此 validator 不会自动约束 evaluator 的返回值。
const MAX_BUCKETS: usize = 256;
/// 文本标签与原因字段允许的 UTF-8 字节数上限，不是字符数量。
const MAX_TEXT_BYTES: usize = 512;

/// 评估对当前输入给出的三种证据结论。
///
/// `NeedsReview` 表示样本未达到最低数量，不能据此宣称健康或发出 breach 告警；
/// `Alerted` 是阈值计算结果；`Healthy` 只表示已评估的桶未超过提供的限值。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftVerdict {
    /// 已检查的样本均达到最低数量，且没有指标严格超过阈值。
    Healthy,
    /// 已检查的样本达到最低数量，并找到至少一个严格超过阈值的桶。
    Alerted,
    /// 在遍历中发现至少一个桶样本不足；此结果不附带告警事件。
    NeedsReview,
}

/// 由调用方提供的一组评估阈值及其内容摘要。
///
/// 【系统位置】被 `DriftEvaluationInput` 带入 Domain reducer；此类型没有从配置库加载或签名的能力。
/// 【下游】决定每桶最少观测数、错误率上限和平均耗时上限。
/// 【设计边界】`Option::None` 表示不启用该指标的上限；两项都为空会被拒绝，避免“没有任何判定条件”
/// 却产出健康结论。摘要用来发现内容字段与 `threshold_digest` 不一致，不负责认证阈值发布者。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftThreshold {
    /// 阈值合同版本。
    pub schema: String,
    /// 每个版本桶必须至少有多少条观测，才具备给出 Healthy/Alerted 的最低证据量。
    pub minimum_samples: u64,
    /// 可选错误率上限，单位为千分率：1000 表示 100%，例如 250 表示 25%。比较使用严格大于。
    pub max_error_rate_milli: Option<u64>,
    /// 可选平均耗时上限，单位为毫秒；比较使用严格大于。
    pub max_mean_elapsed_ms: Option<u64>,
    /// 对 schema 和三个阈值字段计算的 SHA-256 内容摘要。
    pub threshold_digest: String,
}

impl DriftThreshold {
    /// 验证阈值范围、至少存在一个限制条件，并核对阈值内容摘要。
    ///
    /// 【调用者】`DriftEvaluationInput::validate()` 在计算指标前调用。
    /// 【成功条件】schema 正确；minimum_samples 在 1 到 1,000,000 之间；错误率限制若存在则不超过
    /// 1,000 milli；至少提供错误率或耗时限制之一；摘要等于当前字段重算值。
    /// 【失败】字段或摘要不匹配返回 `drift_threshold_invalid`；两种限制都缺失返回
    /// `drift_threshold_empty`，外层输入校验会将二者归一为 `drift_input_invalid`。
    /// 【固定范围】合同代码将样本数最大值限制为 1,000,000；错误率以千分率表示，所以 1,000
    /// 对应 100% 的最大有效值。耗时上限没有相同的固定最大值。
    /// 【边界】自摘要无密钥，不证明阈值由管理员批准或当前仍然有效。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != "kiana.quality-drift-threshold.v1"
            || self.minimum_samples == 0
            || self.minimum_samples > 1_000_000
            || self.max_error_rate_milli.is_some_and(|value| value > 1_000)
            || self.threshold_digest != self.digest()
        {
            return Err("drift_threshold_invalid");
        }
        if self.max_error_rate_milli.is_none() && self.max_mean_elapsed_ms.is_none() {
            return Err("drift_threshold_empty");
        }
        Ok(())
    }

    /// 对 schema 和阈值字段生成确定性内容摘要。
    ///
    /// 【调用者】阈值装配端在填入 `threshold_digest` 前调用，校验器随后独立重算比较。
    /// 【输入/输出】读取 schema、minimum_samples、两个可选限制，返回 `json_digest()` 字符串。
    /// 【边界】摘要值本身不包含在摘要输入中，且不是签名；修改字段后旧摘要会不匹配。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "minimum_samples": self.minimum_samples,
            "max_error_rate_milli": self.max_error_rate_milli,
            "max_mean_elapsed_ms": self.max_mean_elapsed_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftMetricBucket {
    /// 源 `DriftReport` 的版本 key；输出桶必须按此值严格升序排列且不重复。
    pub version_key: String,
    /// 纳入计算的 turns 数量；0 个样本不构成有效指标桶。
    pub sample_count: u64,
    /// 被标记为错误的 turn 数量，不能大于样本数。
    pub error_count: u64,
    /// 桶内所有 turn 的耗时毫秒总和。
    pub elapsed_ms: u64,
    /// 错误率千分值，按整数除法向下取整计算。
    pub error_rate_milli: u64,
    /// 平均耗时毫秒，按整数除法向下取整计算。
    pub mean_elapsed_ms: u64,
}

impl DriftMetricBucket {
    /// 验证派生指标的范围和计数关系，并重算错误率/平均耗时以发现字段不一致。
    ///
    /// 【调用者】`DriftMetrics::validate()` 和私有转换函数 `from_report()`。
    /// 【成功条件】key 有界且无空白，样本数非零，错误数不超样本数，两个派生指标与整数除法重算值相同。
    /// 【失败】任何不一致返回 `drift_metric_bucket_invalid`，整份 DriftMetrics 会被拒绝。
    /// 【容易误改】错误率是 `error_count * 1000 / sample_count` 的整数向下取整；不能改为浮点四舍五入，
    /// 否则阈值边界和摘要都可能变化。
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.version_key)
            || self.sample_count == 0
            || self.error_count > self.sample_count
            || self.error_rate_milli > 1_000
            || self.mean_elapsed_ms > self.elapsed_ms
            || self.error_rate_milli != self.error_count.saturating_mul(1_000) / self.sample_count
            || self.mean_elapsed_ms != self.elapsed_ms / self.sample_count
        {
            return Err("drift_metric_bucket_invalid");
        }
        Ok(())
    }

    /// 从基础版本桶投影出评估用计数和派生指标。
    ///
    /// 【调用者】`evaluate_drift()` 为报告中的每个版本 key 调用。
    /// 【输入】key 与已记录的 DriftBucket；先要求源桶自身合法，再按 turns 计算错误率和平均耗时。
    /// 【输出】有效指标桶；空桶通过源报告验证后仍会在这里因 `sample_count == 0` 被拒绝。
    /// 【失败】源桶验证错误会向上传播；派生值不满足本类型不变量时返回指标桶错误。
    /// `max(1)` 防止除零，但不会让零样本变成合法样本，后续 `validate()` 仍会拒绝。
    /// 【数值边界】错误数乘 1,000 前使用饱和乘法，极端 u64 值下可能压平精确比率；正常样本量下按
    /// milli 整数比例计算。
    fn from_report(version_key: &str, bucket: &crate::DriftBucket) -> Result<Self, &'static str> {
        bucket.validate()?;
        let result = Self {
            version_key: version_key.to_owned(),
            sample_count: bucket.turns,
            error_count: bucket.errors,
            elapsed_ms: bucket.elapsed_ms,
            error_rate_milli: bucket.errors.saturating_mul(1_000) / bucket.turns.max(1),
            mean_elapsed_ms: bucket.elapsed_ms / bucket.turns.max(1),
        };
        result.validate()?;
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftMetrics {
    /// 指标 wire schema。
    pub schema: String,
    /// 本次评估所针对项目/目标的内容摘要。
    pub target_digest: String,
    /// 评估输入声明的来源游标；合同只要求大于零，不会连接数据源验证游标对应内容。
    pub source_cursor: u64,
    /// 本次采用的阈值摘要，便于报告追溯限值版本。
    pub threshold_digest: String,
    /// 按版本 key 严格升序排列的指标桶。
    pub buckets: Vec<DriftMetricBucket>,
    /// 汇总 verdict；不等同于完整质量评审或业务结果。
    pub verdict: DriftVerdict,
    /// 对除本字段外的指标内容计算的摘要。
    pub metrics_digest: String,
}

impl DriftMetrics {
    /// 计算指标对象的内容摘要，摘要字段自身不参与输入。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "target_digest": self.target_digest,
            "source_cursor": self.source_cursor,
            "threshold_digest": self.threshold_digest,
            "buckets": self.buckets,
            "verdict": self.verdict,
        }))
    }

    /// 验证指标 schema、target/cursor/threshold、桶顺序及自摘要。
    ///
    /// 【调用者】告警组装和测试在消费或传递指标结果前调用。
    /// 【成功条件】至少一个、至多 256 个桶；桶按 version_key 严格升序，因此也不能重复；每个桶自身
    /// 有效；摘要与除摘要字段以外的当前内容一致。
    /// 【失败】任一条件失败统一返回 `drift_metrics_invalid`。
    /// 【设计原因】有序且唯一的桶列表让序列化和摘要稳定，并避免同一版本被重复算作两个桶。
    /// 【边界】source_cursor 只检查非零；target digest、桶数据与游标的对应关系由调用方负责，摘要无密钥。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_METRICS_SCHEMA
            || !valid_digest(&self.target_digest)
            || self.source_cursor == 0
            || !valid_digest(&self.threshold_digest)
            || self.buckets.is_empty()
            || self.buckets.len() > MAX_BUCKETS
            || self
                .buckets
                .windows(2)
                .any(|pair| pair[0].version_key >= pair[1].version_key)
            || self.buckets.iter().any(|bucket| bucket.validate().is_err())
            || !valid_digest(&self.metrics_digest)
            || self.metrics_digest != self.digest()
        {
            return Err("drift_metrics_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftAlert {
    /// 告警 payload schema。
    pub schema: String,
    /// 调用方分配的告警身份；Domain 只拒绝 nil，不负责生成或持久化身份。
    pub alert_id: DriftAlertId,
    /// 所属目标的摘要，与本次 DriftMetrics 绑定。
    pub target_digest: String,
    /// 触发告警的指标摘要。
    pub metrics_digest: String,
    /// 做出判定时采用的阈值摘要。
    pub threshold_digest: String,
    /// 发生超限的单个版本桶 key。
    pub version_key: String,
    /// 面向下游的稳定机器可读原因标签，不是自然语言诊断。
    pub reason: String,
    /// 被选中桶的样本数。
    pub sample_count: u64,
    /// 评估输入中 route 的前置摘要。
    pub route_digest_before: String,
    /// 评估输入中 route 的后置摘要；本合同要求与 before 相同。
    pub route_digest_after: String,
    /// 评估输入中 grant 的前置摘要。
    pub grant_digest_before: String,
    /// 评估输入中 grant 的后置摘要；本合同要求与 before 相同。
    pub grant_digest_after: String,
    /// 必须为 false，表示生成证据时没有应用 authority 变更。
    pub authority_changes_applied: bool,
    /// 对除本字段外的告警内容计算的摘要。
    pub alert_digest: String,
}

impl DriftAlert {
    /// 计算告警所有业务字段的内容摘要，摘要字段自身不参与输入。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "alert_id": self.alert_id,
            "target_digest": self.target_digest,
            "metrics_digest": self.metrics_digest,
            "threshold_digest": self.threshold_digest,
            "version_key": self.version_key,
            "reason": self.reason,
            "sample_count": self.sample_count,
            "route_digest_before": self.route_digest_before,
            "route_digest_after": self.route_digest_after,
            "grant_digest_before": self.grant_digest_before,
            "grant_digest_after": self.grant_digest_after,
            "authority_changes_applied": self.authority_changes_applied,
        }))
    }

    /// 验证告警字段形状、route/grant 前后相等、无 authority 变更及自摘要。
    ///
    /// 【调用者】`DriftAlertEvent::validate()` 与 evaluator 生成事件时调用。
    /// 【成功条件】schema/id/digest/文本有效、样本数非零，且 route/grant 前后摘要相同，
    /// `authority_changes_applied` 为 false，告警摘要可重算。
    /// 【失败】统一返回 `drift_alert_invalid`；事件校验会将其转成事件级错误。
    /// 【重要边界】此 DTO 不携带完整阈值或指标桶，无法重放并证明样本确实超限；无密钥摘要也不能认证
    /// 告警来源。它只验证内容自洽及“不声称变更”的边界。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_ALERT_SCHEMA
            || self.alert_id.as_uuid().is_nil()
            || !valid_digest(&self.target_digest)
            || !valid_digest(&self.metrics_digest)
            || !valid_digest(&self.threshold_digest)
            || !valid_text(&self.version_key)
            || !valid_text(&self.reason)
            || self.sample_count == 0
            || !valid_digest(&self.route_digest_before)
            || self.route_digest_before != self.route_digest_after
            || !valid_digest(&self.grant_digest_before)
            || self.grant_digest_before != self.grant_digest_after
            || self.authority_changes_applied
            || !valid_digest(&self.alert_digest)
            || self.alert_digest != self.digest()
        {
            return Err("drift_alert_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftAlertEvent {
    /// 事件 payload schema。
    pub schema: String,
    /// 必须等于固定事件种类 `drift.alerted`。
    pub kind: String,
    /// 评估所使用的来源游标，只校验非零。
    pub source_cursor: u64,
    /// 告警详情；其验证也会在事件验证时执行。
    pub alert: DriftAlert,
    /// 对 schema、kind、游标和嵌套 alert 计算的事件摘要。
    pub event_digest: String,
}

impl DriftAlertEvent {
    /// 计算事件内容摘要，排除自身的 `event_digest` 字段。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "source_cursor": self.source_cursor,
            "alert": self.alert,
        }))
    }

    /// 验证事件类型、游标、嵌套告警和事件摘要。
    ///
    /// 【调用者】`evaluate_drift()` 在准备返回 breach 事件前；下游也可在接收 payload 后复验。
    /// 【失败】schema/kind/游标、嵌套告警或自摘要不符时返回 `drift_alert_event_invalid`。
    /// 【副作用】纯内存检查，不会把事件追加到任何日志，也不会向用户发送通知。
    /// 【边界】内容摘要不是签名；持有者可以重建摘要，因此外部仍需用可信事件写入边界确认来源。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_ALERT_EVENT_SCHEMA
            || self.kind != DRIFT_ALERT_EVENT_KIND
            || self.source_cursor == 0
            || self.alert.validate().is_err()
            || !valid_digest(&self.event_digest)
            || self.event_digest != self.digest()
        {
            return Err("drift_alert_event_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftEvaluationInput {
    /// 输入合同 schema。
    pub schema: String,
    /// 被评估目标的摘要标签。
    pub target_digest: String,
    /// 上游版本治理查询计算出的只读分桶报告。
    pub report: DriftReport,
    /// 本次要应用的阈值内容。
    pub threshold: DriftThreshold,
    /// 本次评估前观察到的 route 摘要。
    pub route_digest_before: String,
    /// 本次评估后观察到的 route 摘要；要求与 before 相同，防止把路由变化包装成普通 drift 告警。
    pub route_digest_after: String,
    /// 本次评估前观察到的 grant 摘要。
    pub grant_digest_before: String,
    /// 本次评估后观察到的 grant 摘要；要求与 before 相同。
    pub grant_digest_after: String,
    /// 调用方声明的输入数据源游标；当前合同只要求非零。
    pub source_cursor: u64,
    /// 调用方为可能产生的告警预先提供的 ID；即使最终没有告警也必须是非 nil。
    pub alert_id: DriftAlertId,
}

impl DriftEvaluationInput {
    /// 在计算前校验输入各子合同及 route/grant 未变化条件。
    ///
    /// 【调用者】`evaluate_drift()` 的第一步。
    /// 【成功条件】schema、摘要形状、嵌套 report/threshold、非零来源游标和非 nil alert ID 均有效，
    /// 并且 route/grant 的 before 与 after 分别相等。
    /// 【失败】所有子合同错误和摘要不一致统一返回 `drift_input_invalid`，避免调用者越过前置 fence。
    /// 【重要边界】这里比较的是调用方提供的快照标签，不会读取当前 route/grant 权威状态，也不验证
    /// target/source_cursor 与 report 真正对应；因此调用端必须从可信同一时点构造这些输入。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_INPUT_SCHEMA
            || !valid_digest(&self.target_digest)
            || self.report.validate().is_err()
            || self.threshold.validate().is_err()
            || !valid_digest(&self.route_digest_before)
            || self.route_digest_before != self.route_digest_after
            || !valid_digest(&self.grant_digest_before)
            || self.grant_digest_before != self.grant_digest_after
            || self.source_cursor == 0
            || self.alert_id.as_uuid().is_nil()
        {
            return Err("drift_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriftEvaluation {
    /// 本次生成并带有自摘要的指标结果。
    pub metrics: DriftMetrics,
    /// 若 verdict 为 Alerted，则包含对应告警；其它 verdict 由 evaluator 返回 None。
    pub alert: Option<DriftAlert>,
    /// 若 verdict 为 Alerted，则包含待由上层决定如何持久化的事件形状；Domain 不负责追加。
    pub event: Option<DriftAlertEvent>,
}

/// 根据一份只读 drift 报告和显式阈值计算指标及可选告警证据。
///
/// 【调用者】Core `evaluate_quality_drift()` 适配器；仓内搜索目前只发现该适配器被测试直接调用，
/// 没有生产命令/后台监控调用点为它提供输入。
/// 【输入】经 `DriftEvaluationInput::validate()` 通过的报告、阈值、目标/来源标签、前后摘要和告警 ID。
/// 【输出】指标总是产生；结果可能是 Healthy、NeedsReview 或 Alerted。Alerted 时同时返回告警和
/// `drift.alerted` 事件载荷，但不写入持久日志。
/// 【失败】输入无效返回 `drift_input_invalid`；空报告返回 `drift_samples_missing`；桶换算或生成事件
/// 校验失败时传播对应稳定错误码。Core 适配器只原样返回，不做重试或状态变更。
/// 【核心流程】验证输入 → 按有序版本 key 把 DriftBucket 换成指标桶 → 检查样本量和阈值 → 生成
/// metrics digest → 若 breach 则再生成 alert/event digest 并校验。
/// 【阈值含义】错误率与平均耗时都使用严格 `>`；刚好等于上限不算超限。error rate 单位为千分率，
/// 平均耗时以整数毫秒向下取整。
/// 【容易误改/当前发现】循环在遇到第一个 breach 桶后提前 `break`。`insufficient` 只记录已经遍历过
/// 的桶，因此若 BTreeMap 顺序靠后的桶样本不足，它可能没机会将最终结果改成 NeedsReview；同一组桶的
/// verdict 会受 version key 顺序影响。此问题仅记录，不在注释任务中修改。
/// 【另一个合同缺口】`DriftMetrics::validate()` 限制最多 256 个桶，但本函数只校验每个单独指标桶，
/// 没有校验最终 metrics，也没有对输入 report 的桶数限额；超过 256 桶时可能成功返回一个自己的
/// validator 会拒绝的 DriftMetrics。
/// 【副作用/边界】纯函数，告警事件只是 append-ready 证据；是否持久化、谁发布事件、如何通知不在本模块。
pub fn evaluate_drift(input: &DriftEvaluationInput) -> Result<DriftEvaluation, &'static str> {
    input.validate()?;
    // DriftReport 用 BTreeMap 保存版本桶，因此这里得到的 Vec 依照 version key 排序；下游校验要求
    // 严格升序，同时这样能让指标摘要在相同输入下稳定。
    let buckets = input
        .report
        .buckets
        .iter()
        .map(|(key, bucket)| DriftMetricBucket::from_report(key, bucket))
        .collect::<Result<Vec<_>, _>>()?;
    if buckets.is_empty() {
        // 合法的空 DriftReport 可以表示“当前无样本”，但评估不能把它误报为 Healthy。
        return Err("drift_samples_missing");
    }
    let mut metrics = DriftMetrics {
        schema: DRIFT_METRICS_SCHEMA.to_owned(),
        target_digest: input.target_digest.clone(),
        source_cursor: input.source_cursor,
        threshold_digest: input.threshold.threshold_digest.clone(),
        buckets,
        verdict: DriftVerdict::Healthy,
        metrics_digest: String::new(),
    };
    let mut alert_bucket = None;
    let mut insufficient = false;
    for bucket in &metrics.buckets {
        // 样本不足时先记下需要复核。注意这个标志只覆盖实际遍历到的桶。
        if bucket.sample_count < input.threshold.minimum_samples {
            insufficient = true;
        }
        if input
            .threshold
            .max_error_rate_milli
            .is_some_and(|limit| bucket.error_rate_milli > limit)
            || input
                .threshold
                .max_mean_elapsed_ms
                .is_some_and(|limit| bucket.mean_elapsed_ms > limit)
        {
            // 只保留按 version key 排序遇到的第一个超限桶。此处提前结束扫描是后续
            // NeedsReview 顺序相关边界的来源：更靠后的桶样本数不会再被检查。
            alert_bucket = Some(bucket);
            break;
        }
    }
    if insufficient {
        // 只要提前结束前已观察到样本不足，review 优先于 alert；没有被遍历到的桶不参与此判断。
        metrics.verdict = DriftVerdict::NeedsReview;
        metrics.metrics_digest = metrics.digest();
        return Ok(DriftEvaluation {
            metrics,
            alert: None,
            event: None,
        });
    }
    if let Some(bucket) = alert_bucket {
        // 只为选出的首个 breach 桶生成一份证据。相同输入若有多个 breach，也不会逐桶产生多个事件。
        metrics.verdict = DriftVerdict::Alerted;
        metrics.metrics_digest = metrics.digest();
        let mut alert = DriftAlert {
            schema: DRIFT_ALERT_SCHEMA.to_owned(),
            alert_id: input.alert_id,
            target_digest: input.target_digest.clone(),
            metrics_digest: metrics.metrics_digest.clone(),
            threshold_digest: input.threshold.threshold_digest.clone(),
            version_key: bucket.version_key.clone(),
            reason: "drift_threshold_exceeded".to_owned(),
            sample_count: bucket.sample_count,
            route_digest_before: input.route_digest_before.clone(),
            route_digest_after: input.route_digest_after.clone(),
            grant_digest_before: input.grant_digest_before.clone(),
            grant_digest_after: input.grant_digest_after.clone(),
            authority_changes_applied: false,
            alert_digest: String::new(),
        };
        alert.alert_digest = alert.digest();
        let mut event = DriftAlertEvent {
            schema: DRIFT_ALERT_EVENT_SCHEMA.to_owned(),
            kind: DRIFT_ALERT_EVENT_KIND.to_owned(),
            source_cursor: input.source_cursor,
            alert,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        // 生成阶段也走公开验证合同，避免返回字段之间已不一致的 payload。
        event.validate()?;
        return Ok(DriftEvaluation {
            metrics,
            alert: Some(event.alert.clone()),
            event: Some(event),
        });
    }
    metrics.verdict = DriftVerdict::Healthy;
    metrics.metrics_digest = metrics.digest();
    Ok(DriftEvaluation {
        metrics,
        alert: None,
        event: None,
    })
}

/// 检查版本 key、原因等单行文本是否非空、无任意空白/NUL 且不超过 512 个 UTF-8 字节。
///
/// 【调用者】指标桶与告警 validator。禁止内部空白使这些值保持为紧凑标签，而不是自由文本内容。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 检查摘要外形是否为固定前缀加 64 个十六进制字符。
///
/// 【调用者】阈值输入、metrics 和 alert validator。
/// 【重要边界】只验证字符串外形；不重新访问原始内容，也不认证摘要由谁计算。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
